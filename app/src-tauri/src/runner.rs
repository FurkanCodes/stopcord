//! Turning a runner into what a terminal actually starts with.
//!
//! Every factory terminal gets a report socket. A contained one is also wrapped in
//! an OS sandbox with web traffic through a filter, and a VM one runs through the
//! person's VM command.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sawtooth_sandbox::{Network, Policy, wrap};
use serde_json::{Value, json};

use crate::bridge::{Bridge, Failure};
use crate::factories::{Factory, Level, Runner, Store};
use crate::harness::{self, Harness};
use crate::shell;
use crate::vm;

/// What to give the daemon's `terminal.start`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Started {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub confine: String,
    /// The harness, said outright when the daemon can't see it run, as in a VM.
    pub agent: Option<String>,
}

/// Whether a runner puts its agents in Sawtooth's VM, rather than a person's own VM command.
pub fn in_vm(runner: Option<&Runner>) -> bool {
    runner.is_some_and(|r| r.level == Level::Vm && r.vm_command.trim().is_empty())
}

/// The runner a terminal uses: the one asked for, else the factory's default.
pub fn pick<'a>(factory: &'a Factory, asked: Option<&str>) -> Option<&'a Runner> {
    let id = asked.filter(|a| !a.is_empty()).unwrap_or(&factory.defaults.runner);
    factory.runners.iter().find(|r| r.id == id)
}

/// Sites the filter starts out allowing.
pub fn allowed_sites(runner: &Runner, harness: Option<&Harness>) -> Vec<String> {
    let mut all: Vec<String> = harness.map(|h| h.domains.clone()).unwrap_or_default();
    all.extend(runner.network_allow.iter().cloned());
    all.sort();
    all.dedup();
    all
}

fn expand(home: &Path, path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(path),
    }
}

/// Sockets no agent may reach. A contained agent that reached the daemon could ask for an unconfined terminal.
fn masked_sockets(home: &Path, sawtooth_home: &Path, daemon_socket: &Path) -> Vec<PathBuf> {
    vec![
        daemon_socket.to_path_buf(),
        sawtooth_home.join("sawtooth.sock"),
        PathBuf::from("/var/run/docker.sock"),
        PathBuf::from("/run/docker.sock"),
        home.join(".docker/run/docker.sock"),
        home.join(".colima/default/docker.sock"),
        home.join(".lima/default/sock/docker.sock"),
    ]
}

/// A path agents can't read unless a runner turns the block off.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Block {
    pub id: String,
    pub label: String,
    pub paths: Vec<PathBuf>,
}

/// Every built-in read block, by the name a runner's `unblock` uses. Covers the sandbox crate's own list.
pub fn built_in_blocks(home: &Path, sawtooth_home: &Path) -> Vec<Block> {
    let block = |id: &str, label: &str, paths: &[&str]| Block {
        id: id.into(),
        label: label.into(),
        paths: paths.iter().map(|p| home.join(p)).collect(),
    };
    let mut secrets = block("secrets", "sawtooth's secrets", &[".sawtooth/secrets"]);
    if !secrets.paths.contains(&sawtooth_home.join("secrets")) {
        secrets.paths.push(sawtooth_home.join("secrets"));
    }
    vec![
        block("ssh", "ssh keys", &[".ssh"]),
        block("cloud", "cloud logins", &[".aws", ".config/gcloud", ".kube"]),
        block("gpg", "gpg keys", &[".gnupg"]),
        block("tokens", "registry tokens", &[".docker/config.json", ".netrc", ".npmrc"]),
        block("history", "shell history", &[".zsh_history", ".bash_history"]),
        secrets,
    ]
}

/// What no runner can open, and why. Opening any of these would let an agent start something outside.
pub const ESCAPES: &[(&str, &str)] = &[
    ("the daemon's socket", "an agent that reached it could ask for a terminal with no sandbox"),
    ("docker's socket", "a container started through it runs outside the sandbox"),
    ("launching apps", "an app macOS starts for an agent runs outside the sandbox"),
];

/// Readable under home when a runner reads only what's listed: shells, toolchains and git settings.
pub const LISTED_HOME_READS: &[&str] = &[
    ".zshrc", ".zprofile", ".zshenv", ".zlogin", ".oh-my-zsh", ".bashrc", ".bash_profile", ".profile",
    ".gitconfig", ".config/git", ".nvm", ".volta", ".bun", ".deno", ".cargo", ".rustup", ".local",
    ".pyenv", ".asdf", ".sdkman",
];

/// Everything the sandbox needs to know, as data, so it can be tested.
#[allow(clippy::too_many_arguments)]
pub fn policy_for(
    store: &Store,
    factory: &Factory,
    runner: &Runner,
    harness: Option<&Harness>,
    cwd: &Path,
    program: &Path,
    home: &Path,
    daemon_socket: &Path,
    network: Network,
    report: Option<PathBuf>,
) -> Policy {
    let mut p = Policy::new(cwd.to_path_buf(), home.to_path_buf(), program.to_path_buf());
    p.git_common_dir = crate::worktree::common_dir(cwd);
    // Whatever the project has checked out moves only by a merge, which Sawtooth makes outside the sandbox.
    p.protected_branch = crate::worktree::git(Path::new(&factory.path), &["branch", "--show-current"]).ok().filter(|b| !b.is_empty());
    if let Some(h) = harness {
        p.write.extend(h.write.iter().map(|w| home.join(w)));
        p.write_prefix.extend(h.write_prefix.iter().map(|w| home.join(w)));
        p.read_only.extend(h.read_only.iter().map(|w| home.join(w)));
    }
    p.write.extend(runner.write.iter().map(|w| expand(home, w)));
    // Agents may propose knowledge. What they write goes through the diff review.
    p.write.push(store.knowledge_file(&factory.id));
    p.deny_read.extend(runner.deny_read.iter().map(|w| expand(home, w)));
    p.deny_read.push(store.home().join("secrets"));
    // A shell's history often holds tokens typed on a command line.
    p.deny_read.extend([".zsh_history", ".bash_history"].map(|f| home.join(f)));
    let unblocked: Vec<PathBuf> = built_in_blocks(home, store.home())
        .into_iter()
        .filter(|b| runner.unblock.contains(&b.id))
        .flat_map(|b| b.paths)
        .collect();
    p.deny_read.retain(|d| !unblocked.contains(d));
    if runner.reads == crate::factories::ReadMode::Listed {
        let mut listed: Vec<PathBuf> = LISTED_HOME_READS.iter().map(|r| home.join(r)).collect();
        listed.push(store.home().join("bin"));
        listed.extend(runner.read.iter().map(|r| expand(home, r)));
        p.reads = sawtooth_sandbox::Reads::Listed(listed);
    }
    // Login shells rewrite their completion cache, and complain on every start if they can't.
    p.write_prefix.push(home.join(".zcompdump"));
    p.masked_sockets = masked_sockets(home, store.home(), daemon_socket);
    p.report_socket = report;
    p.network = network;
    p
}

fn fail(text: impl Into<String>) -> Failure {
    Failure::new("runner", text)
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// What a sandbox test found.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestReport {
    pub checks: Vec<crate::probe::Check>,
    /// Said instead of checks when this kind of runner can't be tested from here.
    pub note: String,
    pub at: u64,
}

/// Runs the probe under exactly the confinement an agent on this runner would get, in a scratch folder.
pub fn test(store: &Store, bridge: &Bridge, factory: &Factory, runner: &Runner, program: &Path) -> Result<TestReport, Failure> {
    use crate::probe;
    let at = crate::runs::now_ms();
    let note = |text: &str| Ok(TestReport { checks: Vec::new(), note: text.to_string(), at });
    match runner.level {
        Level::Harness => return note("A harness's own sandbox runs inside the harness, so Sawtooth can't test it from outside."),
        Level::Vm if !runner.vm_command.trim().is_empty() => return note("A VM started by your own command can't be tested from here."),
        Level::Vm => return test_vm(store, bridge, factory, runner, at),
        _ => {}
    }
    let home = home_dir();
    let dir = store.worktrees_dir(&factory.id).join(".probe");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| fail(format!("couldn't make a folder to test in: {e}")))?;
    let mut blocks: Vec<probe::Block> = built_in_blocks(&home, store.home())
        .into_iter()
        .map(|b| probe::Block { blocked: !runner.unblock.contains(&b.id), label: b.label, paths: b.paths })
        .collect();
    if !runner.deny_read.is_empty() {
        let paths = runner.deny_read.iter().map(|p| expand(&home, p)).collect();
        blocks.push(probe::Block { label: "your own blocks".into(), paths, blocked: true });
    }
    let daemon = sawtooth_daemon::paths::socket();
    let mut sockets = vec![
        probe::Socket { label: "the daemon".into(), path: daemon.clone() },
        probe::Socket { label: "the app".into(), path: store.home().join("sawtooth.sock") },
    ];
    // A machine can have several, often one linked to another, so each is named by where it is.
    let tilde = |p: &Path| p.strip_prefix(&home).map_or_else(|_| p.display().to_string(), |r| format!("~/{}", r.display()));
    sockets.extend(
        masked_sockets(&home, store.home(), &daemon)
            .into_iter()
            .skip(2)
            .map(|path| probe::Socket { label: format!("docker at {}", tilde(&path)), path }),
    );
    let spec = probe::Spec {
        contained: runner.level == Level::Contained,
        listed: runner.reads == crate::factories::ReadMode::Listed,
        outside: home.join(format!(".sawtooth-probe-{}", std::process::id())),
        sample: PathBuf::from(&factory.path),
        blocks,
        sockets,
    };
    let argv = vec![program.display().to_string(), "probe".into(), serde_json::to_string(&spec).unwrap_or_default()];
    let (argv, env, token) = if runner.level == Level::Contained {
        sawtooth_sandbox::available().map_err(|why| fail(format!("This machine can't make a sandbox: {why}")))?;
        let info = bridge.call("confine.prepare", json!({ "allow": allowed_sites(runner, None) }))?;
        let token = info["token"].as_str().unwrap_or_default().to_string();
        let network = Network::Filter {
            port: info["port"].as_u64().unwrap_or(0) as u16,
            socket: PathBuf::from(info["filter"].as_str().unwrap_or_default()),
        };
        let policy = policy_for(store, factory, runner, None, &dir, program, &home, &daemon, network, None);
        match wrap(&policy, &argv, &store.home().join("run")) {
            Ok(w) => (w.argv, w.env, token),
            Err(e) => {
                let _ = bridge.call("confine.release", json!({ "token": token }));
                return Err(fail(format!("{e:#}")));
            }
        }
    } else {
        (argv, Vec::new(), String::new())
    };
    let output = std::process::Command::new(&argv[0]).args(&argv[1..]).envs(env).current_dir(&dir).output();
    if !token.is_empty() {
        let _ = bridge.call("confine.release", json!({ "token": token }));
    }
    let _ = std::fs::remove_dir_all(&dir);
    let output = output.map_err(|e| fail(format!("couldn't start the test: {e}")))?;
    let checks = probe::parse(&String::from_utf8_lossy(&output.stdout)).ok_or_else(|| {
        fail(format!("the test didn't report back: {}", crate::runs::last_lines(&String::from_utf8_lossy(&output.stderr), 5)))
    })?;
    Ok(TestReport { checks, note: String::new(), at })
}

/// Runs the probe in a VM runner's container, on the network and relay an agent would get.
/// The probe also reaches a site the factory's harness needs, as an agent would.
fn test_vm(store: &Store, bridge: &Bridge, factory: &Factory, runner: &Runner, at: u64) -> Result<TestReport, Failure> {
    let docker = vm::available().map_err(fail)?;
    let tag = vm::ensure_image(&docker, store.home(), true).map_err(fail)?;
    vm::prune_old(&docker);
    vm::remove_orphaned_volumes(&docker);
    let (harnesses, _) = harness::load(&store.home().join("harnesses"));
    let harness = harness::find(&harnesses, &factory.defaults.harness);
    let site = harness.and_then(|h| h.domains.first()).map(|d| format!("api.{d}")).unwrap_or_default();
    let info = bridge.call("confine.prepare", json!({ "allow": allowed_sites(runner, harness), "vm": true }))?;
    let spec = vm_spec(store, &info, factory, Path::new(""), runner, String::new(), String::new(), Vec::new(), None, Vec::new());
    let image = if runner.vm_image.trim().is_empty() { tag } else { runner.vm_image.trim().to_string() };
    let checks = vm::probe(&docker, &spec, &image, &home_dir(), &site);
    let _ = bridge.call("confine.release", json!({ "token": spec.token }));
    Ok(TestReport { checks: checks.map_err(fail)?, note: String::new(), at })
}

/// A VM terminal's spec, from what the daemon made for it.
#[allow(clippy::too_many_arguments)]
fn vm_spec(store: &Store, info: &Value, factory: &Factory, dir: &Path, runner: &Runner, command: String, base: String, exclude: Vec<String>, knowledge: Option<PathBuf>, env: Vec<(String, String)>) -> vm::Spec {
    vm::Spec {
        home: vm::home_volume(store.home(), &factory.id, &runner.id),
        factory: factory.id.clone(),
        runner: runner.id.clone(),
        token: info["token"].as_str().unwrap_or_default().to_string(),
        dir: dir.to_path_buf(),
        store: store.home().to_path_buf(),
        image: runner.vm_image.trim().to_string(),
        command,
        setup: runner.setup_commands.clone(),
        base,
        exclude,
        throwaway: false,
        knowledge,
        filter_port: info["port"].as_u64().unwrap_or(0) as u16,
        filter_socket: PathBuf::from(info["filter"].as_str().unwrap_or_default()),
        report_port: info["reportPort"].as_u64().unwrap_or(0) as u16,
        report_socket: PathBuf::from(info["report"].as_str().unwrap_or_default()),
        key: info["key"].as_str().unwrap_or_default().to_string(),
        env,
        secrets: runner.secrets.clone(),
    }
}

/// Who commits in the VM: the same person as in the folder.
fn git_identity(dir: &Path) -> Vec<(String, String)> {
    let mut env = Vec::new();
    for (key, vars) in [("user.name", ["GIT_AUTHOR_NAME", "GIT_COMMITTER_NAME"]), ("user.email", ["GIT_AUTHOR_EMAIL", "GIT_COMMITTER_EMAIL"])] {
        if let Ok(value) = crate::worktree::git(dir, &["config", key]) {
            env.extend(vars.iter().map(|v| (v.to_string(), value.clone())));
        }
    }
    env
}

/// Whether the VM runner can work here, and what the window can offer to fix it.
fn docker_info() -> Value {
    let state = vm::docker_state();
    let (name, built) = match &state {
        vm::Docker::Ready(docker) => ("ready", vm::has_image(docker, &vm::image())),
        vm::Docker::Stopped(_) => ("stopped", false),
        vm::Docker::Missing => ("missing", false),
    };
    json!({
        "ok": name == "ready",
        "state": name,
        "why": vm::why(&state),
        "image": vm::LATEST,
        "built": built,
        "installPage": vm::install_page(),
        "homebrew": vm::homebrew().is_some(),
        "mac": cfg!(target_os = "macos"),
    })
}

/// What the settings screen shows that isn't in a factory: built-in blocks, locked escapes, what
/// listed reads always include, and the sites each harness needs.
pub fn info(store: &Store) -> Value {
    let home = home_dir();
    let tilde = |p: &Path| p.strip_prefix(&home).map_or_else(|_| p.display().to_string(), |r| format!("~/{}", r.display()));
    let (harnesses, _) = harness::load(&store.home().join("harnesses"));
    let blocks: Vec<Value> = built_in_blocks(&home, store.home())
        .iter()
        .map(|b| {
            let mut paths: Vec<String> = b.paths.iter().map(|p| tilde(p)).collect();
            paths.dedup();
            json!({ "id": b.id, "label": b.label, "paths": paths })
        })
        .collect();
    json!({
        "blocks": blocks,
        "escapes": ESCAPES.iter().map(|(what, why)| json!({ "what": what, "why": why })).collect::<Vec<_>>(),
        "alwaysReads": LISTED_HOME_READS.iter().map(|r| format!("~/{r}")).collect::<Vec<_>>(),
        "harnesses": harnesses
            .iter()
            .map(|h| json!({ "id": h.id, "sites": h.domains, "quiet": h.quiet_domains, "ownSandbox": !h.own_sandbox_args.trim().is_empty(), "loginEnv": h.login_env, "loginHint": h.login_hint }))
            .collect::<Vec<_>>(),
        "docker": docker_info(),
    })
}

/// Fills in a VM command. `{dir}` is the folder and `{cmd}` the command, already quoted for a shell.
pub fn vm_argv(template: &str, dir: &Path, command: &str, shell_path: &str) -> Vec<String> {
    let quoted = shell::quote(command);
    let line = template.replace("{dir}", &shell::quote(&dir.display().to_string())).replace("{cmd}", &quoted);
    vec![shell_path.to_string(), "-l".into(), "-c".into(), line]
}

/// Long enough for a cold install on a slow connection.
const SETUP_TIMEOUT: Duration = Duration::from_secs(20 * 60);
pub const SETUP_DONE: &str = "sawtooth-setup-done";

fn setup_markers(cwd: &Path) -> Option<(PathBuf, PathBuf)> {
    let done = cwd.join(crate::worktree::git(cwd, &["rev-parse", "--git-path", SETUP_DONE]).ok()?);
    let failed = PathBuf::from(format!("{}.failed", done.display()));
    Some((done, failed))
}

/// A copy of a set-up worktree has its dependencies already, so it needn't run setup again.
pub fn carry_setup(from: &Path, to: &Path) {
    if let (Some((done, _)), Some((target, _))) = (setup_markers(from), setup_markers(to))
        && done.exists()
    {
        let _ = std::fs::copy(done, target);
    }
}

/// Puts a runner's setup in front of the command, so it runs under the same confinement as the
/// agent: `npm ci` runs the repo's own scripts. Once per worktree, and no agent on a failed setup.
fn with_setup(setup: &[String], command: &str) -> String {
    if setup.is_empty() {
        return command.to_string();
    }
    let steps = setup.iter().map(|s| format!("({s})")).collect::<Vec<_>>().join(" && ");
    format!(
        r#"m="$(git rev-parse --git-path {SETUP_DONE})"; if [ -f "$m" ] || {{ {steps} && : > "$m"; }}; then {command}; else : > "$m.failed"; echo "Setup failed, so the agent did not start."; fi"#
    )
}

/// Waits for a just-started terminal's setup to finish, so typing can't time out behind a slow install.
/// In a VM it waits for the image, the copy and the setup, which `vm-run` does before the agent starts.
pub fn wait_for_setup(runner: Option<&Runner>, cwd: &Path) -> Result<(), String> {
    if in_vm(runner) {
        let (ready, failed) = vm::markers(cwd).ok_or("couldn't find the worktree's git folder")?;
        let start = Instant::now();
        while start.elapsed() < SETUP_TIMEOUT {
            if ready.exists() {
                return Ok(());
            }
            if failed.exists() {
                let why = std::fs::read_to_string(&failed).unwrap_or_default();
                return Err(format!("the VM didn't start: {}", why.trim()));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        return Err("the VM didn't start in 20 minutes".to_string());
    }
    if runner.is_none_or(|r| r.setup_commands.is_empty()) {
        return Ok(());
    }
    let (done, failed) = setup_markers(cwd).ok_or("couldn't find the worktree's git folder")?;
    let start = Instant::now();
    while start.elapsed() < SETUP_TIMEOUT {
        if done.exists() {
            return Ok(());
        }
        if failed.exists() {
            return Err("the runner's setup failed. Its terminal shows why.".to_string());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err("the runner's setup didn't finish in 20 minutes".to_string())
}

/// Prepares a terminal's start. Wraps it in its runner and makes its report socket.
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    store: &Store,
    bridge: &Bridge,
    factory: &Factory,
    runner: Option<&Runner>,
    command: Option<&str>,
    cwd: &Path,
    program: &Path,
) -> Result<Started, Failure> {
    let shell_path = shell::user_shell();
    let (harnesses, _) = harness::load(&store.home().join("harnesses"));
    let harness = command.and_then(|c| harness::find(&harnesses, c));
    let level = runner.map_or(Level::Worktree, |r| r.level);
    let contained = level == Level::Contained;
    if level == Level::Harness && command.is_some() && harness.is_none_or(|h| h.own_sandbox_args.trim().is_empty()) {
        let name = harness.map_or("This command", |h| h.id.as_str());
        return Err(fail(format!("{name} has no sandbox of its own that Sawtooth knows how to turn on. Use Sawtooth's sandbox, or none.")));
    }
    let setup = runner.map_or(&[][..], |r| r.setup_commands.as_slice());
    if !setup.is_empty()
        && let Some((_, failed)) = setup_markers(cwd)
    {
        // A failure left by an earlier try mustn't look like this one's.
        let _ = std::fs::remove_file(failed);
    }
    let run_command = command.map(|c| {
        // A throwaway copy is a repository of its own, so that's what a harness has to trust.
        let trusted = if cwd.join(".git").is_dir() { cwd.display().to_string() } else { factory.path.clone() };
        let c = match level {
            Level::Contained => harness::sandboxed_command(harness, c, &trusted),
            Level::Harness => harness::own_sandbox_command(harness, c, &trusted).unwrap_or_else(|| c.to_string()),
            _ => c.to_string(),
        };
        with_setup(setup, &c)
    });
    let argv = shell::argv(&shell_path, run_command.as_deref());
    let docker_vm = in_vm(runner);
    let docker = if docker_vm { Some(vm::available().map_err(fail)?) } else { None };
    let allow: Value = match runner {
        Some(r) if contained || docker_vm => json!(allowed_sites(r, harness)),
        _ => Value::Null,
    };
    let info = bridge.call("confine.prepare", json!({ "allow": allow, "vm": docker_vm }))?;
    let confine = info["token"].as_str().unwrap_or_default().to_string();
    let report = info["report"].as_str().map(PathBuf::from);
    let bin = store.home().join("bin");
    let mut started = Started {
        env: vec![
            ("SAWTOOTH_ST".to_string(), bin.join("st").display().to_string()),
        ],
        confine: confine.clone(),
        ..Started::default()
    };
    match (level, runner) {
        (Level::Contained, Some(r)) => {
            if let Err(why) = sawtooth_sandbox::available() {
                let _ = bridge.call("confine.release", json!({ "token": confine }));
                return Err(fail(format!("This factory's runner needs a sandbox and this machine can't make one: {why}")));
            }
            let network = Network::Filter {
                port: info["port"].as_u64().unwrap_or(0) as u16,
                socket: PathBuf::from(info["filter"].as_str().unwrap_or_default()),
            };
            let home = home_dir();
            let policy = policy_for(
                store,
                factory,
                r,
                harness,
                cwd,
                program,
                &home,
                &sawtooth_daemon::paths::socket(),
                network,
                report,
            );
            let dir = store.home().join("run");
            let wrapped = wrap(&policy, &argv, &dir).map_err(|e| {
                let _ = bridge.call("confine.release", json!({ "token": confine }));
                fail(format!("{e:#}"))
            })?;
            started.argv = wrapped.argv;
            started.env.extend(wrapped.env);
        }
        (Level::Vm, Some(r)) if docker.is_some() => {
            if let Some((ready, failed)) = vm::markers(cwd) {
                let _ = std::fs::remove_file(ready);
                let _ = std::fs::remove_file(failed);
            }
            if let Some((_, failed)) = setup_markers(cwd) {
                let _ = std::fs::remove_file(failed);
            }
            // Inside, the harness trusts the VM's copy of the folder, and its own sandbox is off as with ours.
            let command = command.map(|c| harness::sandboxed_command(harness, c, "/work")).unwrap_or_default();
            let base = crate::worktree::git(Path::new(&factory.path), &["branch", "--show-current"]).unwrap_or_default();
            let (exclude, knowledge) = match factory.knowledge() {
                Some(part) => {
                    let names = part.read_as.iter().map(|n| n.trim().to_string()).filter(|n| !n.is_empty() && !n.contains('/') && n != "." && n != "..").collect();
                    (names, Some(store.knowledge_file(&factory.id)))
                }
                None => (Vec::new(), None),
            };
            let mut spec = vm_spec(store, &info, factory, cwd, r, command, base, exclude, knowledge, git_identity(cwd));
            spec.throwaway = cwd.starts_with(store.worktrees_dir(&factory.id).join(".copies"));
            started.argv = vec![program.display().to_string(), "vm-run".into(), serde_json::to_string(&spec).unwrap_or_default()];
            started.env.push((vm::KEY.to_string(), spec.key.clone()));
            started.agent = harness.map(|h| h.id.clone());
        }
        (Level::Vm, Some(r)) => {
            let line = run_command.unwrap_or_else(|| "bash -l".to_string());
            started.argv = vm_argv(&r.vm_command, cwd, &line, &shell_path);
        }
        _ => started.argv = argv,
    }
    Ok(started)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::builtin;

    fn factory() -> Factory {
        Factory {
            id: "f".into(),
            defaults: crate::factories::Defaults {
                runner: "base".into(),
                ..Default::default()
            },
            runners: vec![
                Runner {
                    id: "base".into(),
                    level: Level::Contained,
                    network_allow: vec!["registry.npmjs.org".into(), "anthropic.com".into()],
                    write: vec!["~/.gradle".into()],
                    deny_read: vec!["~/.config/mine".into()],
                    ..Runner::default()
                },
                Runner {
                    id: "open".into(),
                    ..Runner::default()
                },
            ],
            ..Factory::default()
        }
    }

    #[test]
    fn an_agent_can_have_its_own_runner_or_use_the_default() {
        let f = factory();
        assert_eq!(pick(&f, None).unwrap().id, "base");
        assert_eq!(pick(&f, Some("")).unwrap().id, "base");
        assert_eq!(pick(&f, Some("open")).unwrap().id, "open");
        assert!(pick(&f, Some("ghost")).is_none());
    }

    #[test]
    fn sites_are_the_harness_and_the_runner_with_no_repeats() {
        let all = builtin();
        let h = harness::find(&all, "claude");
        let f = factory();
        let sites = allowed_sites(&f.runners[0], h);
        assert!(sites.contains(&"registry.npmjs.org".to_string()));
        assert_eq!(sites.iter().filter(|s| *s == "anthropic.com").count(), 1);
    }

    #[test]
    fn the_policy_blocks_secrets_and_every_escape_socket() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("st"));
        let f = factory();
        let all = builtin();
        let p = policy_for(
            &store,
            &f,
            &f.runners[0],
            harness::find(&all, "claude"),
            dir.path(),
            Path::new("/app"),
            Path::new("/Users/me"),
            Path::new("/Users/me/.sawtooth/sawtooth.sock"),
            Network::Off,
            Some(PathBuf::from("/r.sock")),
        );
        assert!(p.deny_read.contains(&store.home().join("secrets")));
        assert!(p.deny_read.contains(&PathBuf::from("/Users/me/.config/mine")));
        assert!(p.masked_sockets.contains(&PathBuf::from("/Users/me/.sawtooth/sawtooth.sock")));
        assert!(p.masked_sockets.contains(&PathBuf::from("/var/run/docker.sock")));
        assert!(p.write.contains(&PathBuf::from("/Users/me/.claude")));
        assert!(p.write.contains(&PathBuf::from("/Users/me/.gradle")));
        assert!(p.write.contains(&store.knowledge_file("f")));
        assert!(p.read_only.contains(&PathBuf::from("/Users/me/.claude/settings.json")));
        assert_eq!(p.report_socket, Some(PathBuf::from("/r.sock")));
    }

    fn policy_with(edit: impl FnOnce(&mut Runner)) -> (tempfile::TempDir, Store, sawtooth_sandbox::Policy) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("st"));
        let mut f = factory();
        edit(&mut f.runners[0]);
        let p = policy_for(&store, &f, &f.runners[0], None, dir.path(), Path::new("/app"), Path::new("/Users/me"), Path::new("/d.sock"), Network::Off, None);
        (dir, store, p)
    }

    #[test]
    fn every_built_in_block_is_on_until_a_runner_turns_it_off() {
        let (_d, store, on) = policy_with(|_| {});
        for block in built_in_blocks(Path::new("/Users/me"), store.home()) {
            for path in &block.paths {
                assert!(on.deny_read.contains(path), "{} isn't blocked by default", path.display());
            }
        }
        let (_d, _s, off) = policy_with(|r| r.unblock = vec!["ssh".into(), "history".into()]);
        assert!(!off.deny_read.contains(&PathBuf::from("/Users/me/.ssh")));
        assert!(!off.deny_read.contains(&PathBuf::from("/Users/me/.zsh_history")));
        assert!(off.deny_read.contains(&PathBuf::from("/Users/me/.aws")), "only what was named comes off");
        assert!(off.deny_read.contains(&PathBuf::from("/Users/me/.config/mine")), "a runner's own blocks stay");
        assert!(off.masked_sockets.contains(&PathBuf::from("/d.sock")), "escapes never come off");
    }

    #[test]
    fn the_crates_own_blocks_all_have_a_name() {
        let named: Vec<PathBuf> = built_in_blocks(Path::new("/h"), Path::new("/h/.sawtooth")).into_iter().flat_map(|b| b.paths).collect();
        for path in sawtooth_sandbox::COMMON_DENY_READS {
            assert!(named.contains(&Path::new("/h").join(path)), "{path} can't be turned off by name");
        }
    }

    #[test]
    fn listed_reads_add_the_tools_and_what_the_runner_lists() {
        let (_d, _s, everything) = policy_with(|_| {});
        assert_eq!(everything.reads, sawtooth_sandbox::Reads::Everything);
        let (_d, store, listed) = policy_with(|r| {
            r.reads = crate::factories::ReadMode::Listed;
            r.read = vec!["~/notes".into()];
        });
        let sawtooth_sandbox::Reads::Listed(paths) = listed.reads else { panic!("not listed") };
        assert!(paths.contains(&PathBuf::from("/Users/me/notes")));
        assert!(paths.contains(&PathBuf::from("/Users/me/.nvm")));
        assert!(paths.contains(&store.home().join("bin")), "st has to be readable to report");
    }

    #[test]
    fn a_vm_command_gets_the_folder_and_the_command_quoted() {
        let argv = vm_argv("vmrun {dir} -- {cmd}", Path::new("/a b"), "claude --x 'y'", "/bin/zsh");
        assert_eq!(argv[..3], ["/bin/zsh", "-l", "-c"]);
        assert_eq!(argv[3], r#"vmrun '/a b' -- 'claude --x '\''y'\'''"#);
    }
}
