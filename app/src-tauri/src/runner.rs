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

/// What to give the daemon's `terminal.start`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Started {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub confine: String,
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

/// Fills in a VM command. `{dir}` is the folder and `{cmd}` the command, already quoted for a shell.
pub fn vm_argv(template: &str, dir: &Path, command: &str, shell_path: &str) -> Vec<String> {
    let quoted = shell::quote(command);
    let line = template.replace("{dir}", &shell::quote(&dir.display().to_string())).replace("{cmd}", &quoted);
    vec![shell_path.to_string(), "-l".into(), "-c".into(), line]
}

/// Long enough for a cold install on a slow connection.
const SETUP_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const SETUP_DONE: &str = "sawtooth-setup-done";

fn setup_markers(cwd: &Path) -> Option<(PathBuf, PathBuf)> {
    let done = cwd.join(crate::worktree::git(cwd, &["rev-parse", "--git-path", SETUP_DONE]).ok()?);
    let failed = PathBuf::from(format!("{}.failed", done.display()));
    Some((done, failed))
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
pub fn wait_for_setup(runner: Option<&Runner>, cwd: &Path) -> Result<(), String> {
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

pub const DEFAULT_VM_COMMAND: &str = "limactl shell --workdir {dir} default bash -lc {cmd}";

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
    let setup = runner.map_or(&[][..], |r| r.setup_commands.as_slice());
    if !setup.is_empty()
        && let Some((_, failed)) = setup_markers(cwd)
    {
        // A failure left by an earlier try mustn't look like this one's.
        let _ = std::fs::remove_file(failed);
    }
    let run_command = command.map(|c| {
        let c = if contained { harness::sandboxed_command(harness, c, &factory.path) } else { c.to_string() };
        with_setup(setup, &c)
    });
    let argv = shell::argv(&shell_path, run_command.as_deref());
    let allow: Value = match runner {
        Some(r) if contained => json!(allowed_sites(r, harness)),
        _ => Value::Null,
    };
    let info = bridge.call("confine.prepare", json!({ "allow": allow }))?;
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
        (Level::Vm, Some(r)) => {
            let template = if r.vm_command.trim().is_empty() { DEFAULT_VM_COMMAND } else { r.vm_command.as_str() };
            let line = run_command.unwrap_or_else(|| "bash -l".to_string());
            started.argv = vm_argv(template, cwd, &line, &shell_path);
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

    #[test]
    fn a_vm_command_gets_the_folder_and_the_command_quoted() {
        let argv = vm_argv("vmrun {dir} -- {cmd}", Path::new("/a b"), "claude --x 'y'", "/bin/zsh");
        assert_eq!(argv[..3], ["/bin/zsh", "-l", "-c"]);
        assert_eq!(argv[3], r#"vmrun '/a b' -- 'claude --x '\''y'\'''"#);
    }
}
