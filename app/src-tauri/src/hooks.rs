//! Checks that run on every commit or before a push, as git hooks.
//!
//! The hook is a tiny script that calls this app with `hook <factory> <name>`.
//! The app runs the factory's checks, shows their output (an agent committing
//! sees it in its terminal), writes the run log, and stops the commit when one fails.

use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

use crate::factories::{Factory, Kind, RunsOn, Store};
use crate::runs::{self, Run};
use crate::worktree::{common_dir, git, is_git};

const MARKER: &str = "# stopcord-hook";
/// The marker on hooks installed before the app was renamed from Sawtooth. They're still ours to update.
const OLD_MARKER: &str = "# sawtooth-hook";

/// The longest a check on a commit or push may run. Long enough for a full test
/// suite; short enough that a hung one can't hold an agent's commit forever.
const HOOK_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// How long a timed-out check gets to stop cleanly before it's killed.
const STOP_GRACE: Duration = Duration::from_secs(2);

/// The process group of the check running now, for `forward_signal`.
static RUNNING_GROUP: AtomicI32 = AtomicI32::new(0);

/// A check runs in its own process group, so that a timeout can stop all of it.
/// That also takes it out of the reach of a Ctrl-C at the terminal, so the hook
/// passes the signal on before dying itself.
extern "C" fn forward_signal(signal: libc::c_int) {
    let group = RUNNING_GROUP.load(Ordering::SeqCst);
    // SAFETY: kill, signal and raise are all async-signal-safe.
    unsafe {
        if group > 0 {
            libc::kill(-group, libc::SIGTERM);
        }
        libc::signal(signal, libc::SIG_DFL);
        libc::raise(signal);
    }
}

fn forward_signals() {
    let handler = forward_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: the handler does only async-signal-safe work.
        unsafe {
            libc::signal(signal, handler);
        }
    }
}

/// The hooks a factory can need, and which checks each one runs.
const HOOKS: [(&str, RunsOn); 2] = [("pre-commit", RunsOn::Commit), ("pre-push", RunsOn::Merge)];

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn script(factory: &Factory, program: &Path, home: &Path, hook: &str) -> String {
    format!(
        "#!/bin/sh\n{MARKER}\n# Runs the checks of the Stopcord factory \"{name}\". Stopcord rewrites this file;\n\
         # remove the factory's checks to remove it.\n\
         app={app}\n\
         if [ -x \"$app\" ]; then STOPCORD_HOME={home} exec \"$app\" hook {id} {hook}; fi\n\
         echo \"stopcord: the app isn't at $app any more, so the factory's checks were skipped.\" >&2\n\
         exit 0\n",
        name = factory.name.replace('\n', " "),
        app = quote(&program.display().to_string()),
        home = quote(&home.display().to_string()),
        id = quote(&factory.id),
    )
}

fn hooks_dir(project: &Path) -> Result<PathBuf, String> {
    if let Some(path) = crate::worktree::config(project, "core.hooksPath") {
        return Err(format!(
            "This repo keeps its git hooks in {path} (core.hooksPath), so Stopcord didn't add its checks there. \
             Call `stopcord-app hook <factory> pre-commit` from your own hook to run them."
        ));
    }
    common_dir(project)
        .map(|d| d.join("hooks"))
        .ok_or_else(|| "couldn't find the repo's git folder".to_string())
}

fn ours(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|t| t.lines().any(|l| l.trim() == MARKER || l.trim() == OLD_MARKER))
}

/// The factory a hook of ours belongs to.
fn owner(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let rest = text.split("exec \"$app\" hook '").nth(1)?;
    Some(rest.split('\'').next()?.to_string())
}

/// The name a hook of ours says it runs checks for.
fn owner_name(path: &Path) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| Some(t.split("Stopcord factory \"").nth(1)?.split('"').next()?.to_string()))
        .unwrap_or_else(|| "another factory".into())
}

/// The hook that keeps work in the factory's worktrees from moving the project's branch: only a merge,
/// which Stopcord makes in the project folder, may. Plain shell, so it holds even with the app gone.
const GUARD: &str = "reference-transaction";

fn guard_script(factory: &Factory, branch: &str, worktrees: &Path) -> String {
    format!(
        "#!/bin/sh\n{MARKER}\n# The Stopcord factory \"{name}\" ({id}): work in its worktrees can't move {branch}.\n\
         # {branch} changes by merging, in the project folder. Stopcord rewrites this file.\n\
         [ \"$1\" = prepared ] || exit 0\n\
         case \"$(pwd -P)/\" in {trees}/*) ;; *) exit 0 ;; esac\n\
         while read -r old new ref; do\n\
         \x20 if [ \"$ref\" = {target} ]; then\n\
         \x20   echo \"stopcord: {branch} only changes by merging it in Stopcord, not from a worktree.\" >&2\n\
         \x20   exit 1\n\
         \x20 fi\n\
         done\n\
         exit 0\n",
        name = factory.name.replace('\n', " "),
        id = quote(&factory.id),
        trees = quote(&worktrees.canonicalize().unwrap_or_else(|_| worktrees.to_path_buf()).display().to_string()),
        target = quote(&format!("refs/heads/{branch}")),
    )
}

/// Puts the branch guard in place, or says why it couldn't. A repo's own hook of that name stays.
fn install_guard(factory: &Factory, dir: &Path, home: &Path) -> Option<String> {
    let project = Path::new(&factory.path);
    let branch = git(project, &["branch", "--show-current"]).ok().filter(|b| !b.is_empty())?;
    let path = dir.join(GUARD);
    if path.exists() && !ours(&path) {
        return Some(format!("This repo already has a {GUARD} hook, so Stopcord couldn't add the one that stops agents moving {branch} from their worktrees."));
    }
    // Every factory's worktrees, so two factories in one repo share the one guard.
    let trees = home.join("worktrees");
    let _ = std::fs::create_dir_all(&trees);
    let write = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, guard_script(factory, &branch, &trees))).and_then(|_| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
    });
    write.err().map(|e| format!("couldn't write {}: {e}", path.display()))
}

/// Writes the hooks the factory's checks need and removes ours that it no longer needs.
/// Returns problems in words. Outside git there's nothing to hook, so checks on commit can't run.
pub fn install(factory: &Factory, program: &Path, home: &Path) -> Vec<String> {
    let project = Path::new(&factory.path);
    let needs = |on: RunsOn| {
        factory
            .parts
            .iter()
            .any(|p| p.kind == Kind::Check && p.runs_on == on && !p.command.trim().is_empty())
    };
    if !is_git(project) {
        return if needs(RunsOn::Commit) || needs(RunsOn::Merge) {
            vec![
                "This folder isn't a git repo, so checks on commit or before a push never run."
                    .into(),
            ]
        } else {
            Vec::new()
        };
    }
    let dir = match hooks_dir(project) {
        Ok(dir) => dir,
        Err(problem) => {
            return if needs(RunsOn::Commit) || needs(RunsOn::Merge) {
                vec![problem]
            } else {
                Vec::new()
            };
        }
    };
    let mut problems = Vec::new();
    for (hook, on) in HOOKS {
        let path = dir.join(hook);
        let exists = path.exists();
        // A hook of ours for a factory that's still around isn't ours to replace.
        let taken = exists
            && ours(&path)
            && owner(&path).is_some_and(|id| {
                id != factory.id
                    && (home.join("factories").join(&id).join("definition/factory.yaml").is_file()
                        || home.join("factories").join(&id).join("factory.json").is_file())
            });
        if needs(on) {
            if taken {
                problems.push(format!(
                    "The factory {} already runs its checks from this repo's {hook} hook, so this factory's checks on that step won't run. A repo can only run one factory's hooks.",
                    owner_name(&path)
                ));
                continue;
            }
            if exists && !ours(&path) {
                problems.push(format!(
                    "This repo already has a {hook} hook, so Stopcord didn't replace it. \
                     Add this line to {} to run the factory's checks too: \"{}\" hook {} {hook}",
                    path.display(),
                    program.display(),
                    factory.id
                ));
                continue;
            }
            let write = std::fs::create_dir_all(&dir)
                .and_then(|_| std::fs::write(&path, script(factory, program, home, hook)))
                .and_then(|_| {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                });
            if let Err(e) = write {
                problems.push(format!("couldn't write {}: {e}", path.display()));
            }
        } else if exists && ours(&path) && !taken {
            let _ = std::fs::remove_file(&path);
        }
    }
    problems.extend(install_guard(factory, &dir, home));
    problems
}

/// Removes the factory's hooks.
pub fn uninstall(factory: &Factory) {
    let project = Path::new(&factory.path);
    if let Ok(dir) = hooks_dir(project) {
        for hook in HOOKS.iter().map(|(h, _)| *h).chain([GUARD]) {
            let path = dir.join(hook);
            if ours(&path)
                && std::fs::read_to_string(&path).is_ok_and(|t| t.contains(&quote(&factory.id)))
            {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

/// `command` as the person's login shell runs it. An app opened from Finder has a
/// bare PATH without node, cargo and the rest; a commit hook doesn't need this,
/// because it runs inside the agent's terminal.
pub fn login_command(shell: &str, command: &str) -> String {
    format!("{} -l -c {}", quote(shell), quote(command))
}

/// What git sets while a hook runs, tying every git command under it to this repository.
const GIT_REPO_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_PREFIX",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_SHALLOW_FILE",
];

/// A check's own git commands must not land in the repository being committed to: a test
/// suite that makes a temporary repo would otherwise rewrite the real one. A check that reads
/// what's staged keeps them, because `git commit -a` stages into an index of its own.
fn clear_git_env(cmd: &mut Command, command: &str) {
    if command.contains("--cached") || command.contains("--staged") {
        return;
    }
    for name in GIT_REPO_ENV {
        cmd.env_remove(name);
    }
}

/// Runs a shell command in `cwd`, copying its output to `echo` as it comes.
/// Returns whether it passed and everything it printed. With a timeout, a
/// command still running then is stopped, along with everything it started,
/// and counts as failed.
pub fn run_command(
    command: &str,
    cwd: &Path,
    echo: &mut dyn Write,
    timeout: Option<std::time::Duration>,
) -> (bool, String) {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(format!("exec 2>&1; {command}"))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .process_group(0);
    clear_git_env(&mut cmd, command);
    let child = cmd.spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return (false, format!("couldn't run it: {e}")),
    };
    RUNNING_GROUP.store(child.id() as i32, Ordering::SeqCst);
    let mut out = child.stdout.take().unwrap();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let timed_out = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Some(timeout) = timeout {
        let (done, timed_out, pid) = (done.clone(), timed_out.clone(), child.id());
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + timeout;
            while !done.load(std::sync::atomic::Ordering::SeqCst) {
                if std::time::Instant::now() > deadline {
                    timed_out.store(true, std::sync::atomic::Ordering::SeqCst);
                    let group = pid as i32;
                    // SAFETY: only sends a signal to the group this function started.
                    unsafe { libc::kill(-group, libc::SIGTERM) };
                    let hard = std::time::Instant::now() + STOP_GRACE;
                    while std::time::Instant::now() < hard {
                        if done.load(std::sync::atomic::Ordering::SeqCst) {
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    // SAFETY: as above.
                    unsafe { libc::kill(-group, libc::SIGKILL) };
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        });
    }
    let mut all = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match out.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let _ = echo.write_all(&buf[..n]);
                all.extend_from_slice(&buf[..n]);
            }
        }
    }
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let mut text = String::from_utf8_lossy(&all).into_owned();
    if timed_out.load(std::sync::atomic::Ordering::SeqCst) {
        text.push_str("\n(stopped: it ran too long)\n");
        return (false, text);
    }
    (ok, text)
}

/// What `stopcord-app hook <id> <hook>` does. Returns the exit code for git.
pub fn run_hook(store: &Store, id: &str, hook: &str, cwd: &Path) -> i32 {
    let Some(on) = HOOKS.iter().find(|(h, _)| *h == hook).map(|(_, on)| *on) else {
        eprintln!("stopcord: there's no hook called {hook}");
        return 0;
    };
    let factory = match store.get(id) {
        Ok(f) => f,
        Err(_) => {
            eprintln!("stopcord: the factory {id} is gone, so no checks ran.");
            return 0;
        }
    };
    forward_signals();
    let terminal = std::env::var("STOPCORD_TERMINAL").ok();
    let branch = git(cwd, &["branch", "--show-current"]).unwrap_or_default();
    let what = if on == RunsOn::Commit {
        "commit"
    } else {
        "push"
    };
    let mut failed = Vec::new();
    let mut stderr = std::io::stderr();
    for part in factory
        .parts
        .iter()
        .filter(|p| p.kind == Kind::Check && p.runs_on == on && !p.command.trim().is_empty())
    {
        let _ = writeln!(stderr, "stopcord: {} ({})", part.name, part.command);
        let (ok, output) = run_command(&part.command, cwd, &mut stderr, Some(HOOK_TIMEOUT));
        let _ = writeln!(
            stderr,
            "stopcord: {} {}",
            part.name,
            if ok { "passed" } else { "failed" }
        );
        let on_branch = if branch.is_empty() {
            String::new()
        } else {
            format!(" on {branch}")
        };
        let text = format!(
            "{} {} for a {what}{on_branch}",
            part.name,
            if ok { "passed" } else { "failed" }
        );
        let run = Run::new("check", text)
            .ok(ok)
            .part(&part.id)
            .terminal(terminal.as_deref())
            .detail(&output);
        let _ = runs::append(store, id, &run);
        if !ok {
            failed.push(part.name.clone());
        }
    }
    if failed.is_empty() {
        0
    } else {
        let _ = writeln!(
            stderr,
            "stopcord: the {what} was stopped because {} failed. Fix what it says above, then {what} again.",
            failed.join(" and ")
        );
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::Part;
    use crate::worktree::tests::repo;

    #[test]
    fn a_login_shell_command_keeps_its_output_exit_code_and_quotes() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = login_command("/bin/sh", "echo 'it'\\''s'; exit 3");
        let (ok, text) = run_command(&cmd, dir.path(), &mut std::io::sink(), None);
        assert!(!ok);
        assert_eq!(text.trim(), "it's");
        let (ok, _) = run_command(&login_command("/bin/sh", "true"), dir.path(), &mut std::io::sink(), None);
        assert!(ok);
    }

    #[test]
    fn a_timeout_stops_everything_the_command_started() {
        let dir = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();
        let (ok, text) = run_command(
            "sleep 8 | cat",
            dir.path(),
            &mut std::io::sink(),
            Some(std::time::Duration::from_millis(200)),
        );
        assert!(!ok);
        assert!(text.contains("ran too long"), "{text}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "took {:?}, so the children were left running",
            started.elapsed()
        );
    }
    fn check(name: &str, command: &str, on: RunsOn) -> Part {
        Part {
            id: crate::factories::slug(name),
            kind: Kind::Check,
            name: name.into(),
            command: command.into(),
            runs_on: on,
            ..Part::default()
        }
    }

    fn factory(store: &Store, path: &Path, parts: Vec<Part>) -> Factory {
        store.create(path, "demo", parts).unwrap()
    }

    #[test]
    fn work_in_a_worktree_cant_move_the_projects_branch_but_a_merge_in_the_project_can() {
        let repo = repo();
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        let f = factory(&store, repo.path(), vec![]);
        assert!(install(&f, Path::new("/no/such/app"), home.path()).is_empty());
        let wt = crate::worktree::ensure(repo.path(), &store.worktrees_dir(&f.id), "t-1").unwrap();
        // The agent's git, unlike the engine's, runs the repo's hooks, so the guard applies to it.
        let agent = |args: &[&str]| -> Result<String, String> {
            let out = Command::new("git").arg("-C").arg(&wt).args(args).output().unwrap();
            if out.status.success() { Ok(String::new()) } else { Err(String::from_utf8_lossy(&out.stderr).into_owned()) }
        };
        std::fs::write(wt.join("work.txt"), "x").unwrap();
        agent(&["add", "-A"]).unwrap();
        agent(&["commit", "-qm", "work"]).expect("its own branch takes commits");
        let main = git(repo.path(), &["rev-parse", "main"]).unwrap();
        // Git refuses these itself while main is checked out in the project.
        assert!(agent(&["branch", "-f", "main", "HEAD"]).is_err());
        assert!(agent(&["push", ".", "HEAD:main"]).is_err());
        // This one gets past git's own checks, so the guard is what stops it.
        let e = agent(&["update-ref", "refs/heads/main", "HEAD"]).unwrap_err();
        assert!(e.contains("only changes by merging"), "{e}");
        assert_eq!(git(repo.path(), &["rev-parse", "main"]).unwrap(), main, "main didn't move");
        git(repo.path(), &["merge", "-q", "--no-ff", "--no-edit", "t-1"]).expect("a merge in the project still works");
        uninstall(&f);
        assert!(!common_dir(repo.path()).unwrap().join("hooks").join(GUARD).exists(), "removing the factory takes the guard");
    }

    #[test]
    fn two_factories_in_one_repo_leave_each_others_hooks_alone() {
        let repo = repo();
        let sub = repo.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        let program = Path::new("/no/such/app");
        let first = factory(&store, repo.path(), vec![check("Tests", "true", RunsOn::Commit)]);
        assert!(install(&first, program, home.path()).is_empty());
        let hook = common_dir(repo.path()).unwrap().join("hooks/pre-commit");

        let second = factory(&store, &sub, vec![check("Lint", "true", RunsOn::Commit)]);
        let problems = install(&second, program, home.path());
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains(&first.name), "{problems:?}");
        assert!(std::fs::read_to_string(&hook).unwrap().contains(&quote(&first.id)));

        // One with no checks at all mustn't clear the first one's hook either.
        let mut idle = second.clone();
        idle.parts.clear();
        assert!(install(&idle, program, home.path()).is_empty());
        assert!(hook.exists());

        // Once the first factory is gone, its hook is free to take.
        store.remove(&first.id).unwrap();
        assert!(install(&second, program, home.path()).is_empty());
        assert!(std::fs::read_to_string(&hook).unwrap().contains(&quote(&second.id)));
    }

    #[test]
    fn a_hook_installed_before_the_rename_is_still_ours() {
        let repo = repo();
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        let f = factory(&store, repo.path(), vec![check("Tests", "true", RunsOn::Commit)]);
        let hooks = common_dir(repo.path()).unwrap().join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(hooks.join("pre-commit"), format!("#!/bin/sh\n{OLD_MARKER}\nexit 0\n")).unwrap();
        assert!(ours(&hooks.join("pre-commit")));
        assert!(install(&f, Path::new("/no/such/app"), home.path()).is_empty(), "replaced, not reported as someone else's");
        let now = std::fs::read_to_string(hooks.join("pre-commit")).unwrap();
        assert!(now.contains(MARKER) && !now.contains(OLD_MARKER), "{now}");
    }

    #[test]
    fn installs_only_what_checks_need_and_keeps_foreign_hooks() {
        let repo = repo();
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        let mut f = factory(
            &store,
            repo.path(),
            vec![check("Tests", "true", RunsOn::Commit)],
        );
        let program = Path::new("/no/such/app");
        assert!(install(&f, program, home.path()).is_empty());
        let hooks = common_dir(repo.path()).unwrap().join("hooks");
        assert!(ours(&hooks.join("pre-commit")));
        assert!(!hooks.join("pre-push").exists());

        // A missing app lets the person's commit through with a warning. Their git runs the hook; the engine's wouldn't.
        std::fs::write(repo.path().join("a.txt"), "a").unwrap();
        git(repo.path(), &["add", "."]).unwrap();
        let commit = Command::new("git").arg("-C").arg(repo.path()).args(["commit", "-q", "-m", "a"]).output().unwrap();
        assert!(commit.status.success(), "{}", String::from_utf8_lossy(&commit.stderr));
        assert!(String::from_utf8_lossy(&commit.stderr).contains("the app isn't at"), "the hook ran and warned");

        std::fs::write(hooks.join("pre-push"), "#!/bin/sh\nexit 0\n").unwrap();
        f.parts = vec![check("Lint", "true", RunsOn::Merge)];
        let problems = install(&f, program, home.path());
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("already has a pre-push hook"));
        assert!(
            !hooks.join("pre-commit").exists(),
            "ours goes when no check needs it"
        );

        uninstall(&f);
        assert!(
            hooks.join("pre-push").exists(),
            "never removes someone else's hook"
        );
    }

    #[test]
    fn hooks_path_and_plain_folders() {
        let repo = repo();
        git(repo.path(), &["config", "core.hooksPath", ".husky"]).unwrap();
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        let f = factory(
            &store,
            repo.path(),
            vec![check("Tests", "true", RunsOn::Commit)],
        );
        let problems = install(&f, Path::new("/x"), home.path());
        assert!(problems[0].contains("core.hooksPath"), "{problems:?}");

        let plain = tempfile::tempdir().unwrap();
        let g = store
            .create(
                plain.path(),
                "plain",
                vec![check("Tests", "true", RunsOn::Commit)],
            )
            .unwrap();
        assert!(install(&g, Path::new("/x"), home.path())[0].contains("isn't a git repo"));
    }

    #[test]
    fn a_command_that_runs_too_long_fails() {
        let dir = tempfile::tempdir().unwrap();
        let (ok, out) = run_command(
            "sleep 5",
            dir.path(),
            &mut std::io::sink(),
            Some(std::time::Duration::from_millis(300)),
        );
        assert!(!ok);
        assert!(out.contains("too long"));
        let (ok, out) = run_command(
            "echo hi; echo err >&2",
            dir.path(),
            &mut std::io::sink(),
            None,
        );
        assert!(ok);
        assert_eq!(out, "hi\nerr\n");
    }

    #[test]
    fn a_check_cant_reach_the_repository_being_committed_to() {
        let mut tests = Command::new("sh");
        clear_git_env(&mut tests, "cargo test --workspace");
        assert!(tests.get_envs().any(|(k, v)| k == "GIT_DIR" && v.is_none()));
        assert!(tests.get_envs().any(|(k, v)| k == "GIT_INDEX_FILE" && v.is_none()));
        let mut staged = Command::new("sh");
        clear_git_env(&mut staged, "git diff --cached -U0 | grep -q secret");
        assert_eq!(staged.get_envs().count(), 0, "reading what's staged needs git's own view");
    }

    #[test]
    fn run_hook_logs_each_check_and_stops_on_failure() {
        let repo = repo();
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        let f = factory(
            &store,
            repo.path(),
            vec![
                check("Tests", "echo all good", RunsOn::Commit),
                check("No secrets", "echo found a key >&2; exit 1", RunsOn::Commit),
                check("Push only", "exit 1", RunsOn::Merge),
            ],
        );
        assert_eq!(run_hook(&store, &f.id, "pre-commit", repo.path()), 1);
        let log = runs::read(&store, &f.id, 10).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].ok, Some(false));
        assert_eq!(log[0].text, "No secrets failed for a commit on main");
        assert!(log[0].detail.as_deref().unwrap().contains("found a key"));
        assert_eq!(log[1].ok, Some(true));
        assert_eq!(run_hook(&store, &f.id, "pre-commit-nope", repo.path()), 0);
        assert_eq!(run_hook(&store, "gone", "pre-commit", repo.path()), 0);
    }
}
