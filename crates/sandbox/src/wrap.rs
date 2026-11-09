//! Builds the command that runs argv inside the sandbox on this machine.

#[cfg(any(target_os = "linux", test))]
use anyhow::Context;
use anyhow::bail;
use std::path::Path;

use crate::policy::{Network, Policy};

/// A sandboxed command: run `argv` with `env` added to the environment.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Wrapped {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Whether this machine can run the backend (sandbox-exec or bwrap present), with a human-readable
/// reason if not. Runs a trivial sandbox rather than looking for the program, because a machine can
/// have the program and still forbid it: nesting on macOS, restricted user namespaces on Linux.
pub fn available() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        probe(
            "/usr/bin/sandbox-exec",
            &["-p", "(version 1)(allow default)", "--", "/usr/bin/true"],
        )
    }
    #[cfg(target_os = "linux")]
    {
        probe(
            "bwrap",
            &["--ro-bind", "/", "/", "--unshare-pid", "--", "true"],
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err("sandboxing needs macOS or Linux".to_string())
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn probe(program: &str, args: &[&str]) -> Result<(), String> {
    match std::process::Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(format!(
            "{program} can't start a sandbox here: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("{program} is not installed"))
        }
        Err(error) => Err(format!("{program}: {error}")),
    }
}

/// Wraps argv. `dir` is where to write generated files such as the .sb profile.
/// Env includes HTTPS_PROXY/HTTP_PROXY/https_proxy/http_proxy/ALL_PROXY/NO_PROXY when Network::Filter.
pub fn wrap(policy: &Policy, argv: &[String], dir: &Path) -> anyhow::Result<Wrapped> {
    validate(policy, argv)?;
    Ok(Wrapped {
        argv: command(policy, argv, dir)?,
        env: proxy_env(policy),
    })
}

/// Refuses a policy that would render into something other than what it says. Relative paths can't be
/// matched by Seatbelt, and the output formats are text, so a non-UTF-8 path could only be mangled.
fn validate(policy: &Policy, argv: &[String]) -> anyhow::Result<()> {
    if argv.is_empty() {
        bail!("nothing to run");
    }
    let mut paths: Vec<&Path> = vec![&policy.worktree, &policy.home, &policy.program];
    let lists = [
        &policy.write,
        &policy.read_only,
        &policy.deny_read,
        &policy.masked_sockets,
    ];
    paths.extend(
        lists
            .iter()
            .flat_map(|list| list.iter().map(|p| p.as_path())),
    );
    paths.extend(policy.git_common_dir.as_deref());
    paths.extend(policy.report_socket.as_deref());
    if let Network::Filter { socket, .. } = &policy.network {
        paths.push(socket);
    }
    for path in paths {
        if path.to_str().is_none() {
            bail!("path is not valid UTF-8: {}", path.display());
        }
        if !path.is_absolute() {
            bail!("path is not absolute: {}", path.display());
        }
    }
    if !policy.worktree.is_dir() {
        bail!("worktree does not exist: {}", policy.worktree.display());
    }
    Ok(())
}

/// Proxy settings for tools. NO_PROXY keeps local test servers off the proxy. Lower-case forms are set
/// as well because tools disagree about which they read.
fn proxy_env(policy: &Policy) -> Vec<(String, String)> {
    let Network::Filter { port, .. } = policy.network else {
        return Vec::new();
    };
    let url = format!("http://127.0.0.1:{port}");
    let mut env: Vec<(String, String)> = [
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "https_proxy",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ]
    .iter()
    .map(|name| (name.to_string(), url.clone()))
    .collect();
    for name in ["NO_PROXY", "no_proxy"] {
        env.push((name.to_string(), "localhost,127.0.0.1,::1".to_string()));
    }
    env
}

#[cfg(target_os = "macos")]
fn command(policy: &Policy, argv: &[String], dir: &Path) -> anyhow::Result<Vec<String>> {
    let profile = write_profile(&crate::seatbelt_profile(policy), dir)?;
    let mut command = vec![
        "/usr/bin/sandbox-exec".to_string(),
        "-f".to_string(),
        profile,
        "--".to_string(),
    ];
    command.extend_from_slice(argv);
    Ok(command)
}

#[cfg(target_os = "linux")]
fn command(policy: &Policy, argv: &[String], _dir: &Path) -> anyhow::Result<Vec<String>> {
    bwrap_command(policy, argv)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn command(_policy: &Policy, _argv: &[String], _dir: &Path) -> anyhow::Result<Vec<String>> {
    bail!("sandboxing needs macOS or Linux")
}

/// Writes the profile to a new file with a name of its own: a shared name could let one terminal's
/// sandbox start under another's profile.
#[cfg(target_os = "macos")]
fn write_profile(profile: &str, dir: &Path) -> anyhow::Result<String> {
    use anyhow::Context;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = format!(
        "sawtooth-{}-{nanos}-{}.sb",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let path = dir.join(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.write_all(profile.as_bytes())?;
    path.into_os_string()
        .into_string()
        .map_err(|p| anyhow::anyhow!("profile path is not valid UTF-8: {}", p.display()))
}

/// Run ahead of bwrap, so a terminal interrupt can't kill it.
#[cfg(any(target_os = "linux", test))]
const SHIELD: &str = r#"trap '' INT QUIT; exec "$@""#;

/// The full bubblewrap command line for `argv`.
///
/// Ctrl-C goes to the terminal's whole foreground process group, and that includes bwrap's own process
/// outside the namespace. If that process dies, `--die-with-parent` takes the sandbox down with it, so
/// a program that handles Ctrl-C would be killed instead. bwrap starts with the signals ignored, and
/// the forwarder, which runs inside as the command's parent, swaps that for handlers that exec resets,
/// so the command itself starts with normal signal behaviour. That is also why the forwarder runs for
/// every network mode; port 0 means there is nothing for it to forward.
#[cfg(any(target_os = "linux", test))]
fn bwrap_command(policy: &Policy, argv: &[String]) -> anyhow::Result<Vec<String>> {
    let mut command = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        SHIELD.to_string(),
        "sawtooth-sandbox".to_string(),
        "bwrap".to_string(),
    ];
    command.extend(crate::bwrap_args(policy));
    command.push("--".to_string());
    let (port, socket) = match &policy.network {
        Network::Filter { port, socket } => {
            // bwrap_args skips a missing source, which would leave a sandbox with no way out.
            std::fs::metadata(socket)
                .with_context(|| format!("filter socket {} is not there", socket.display()))?;
            (*port, crate::bwrap::FILTER_SOCKET)
        }
        _ => (0, "/dev/null"),
    };
    command.extend([
        policy.program.display().to_string(),
        "sandbox-bridge".to_string(),
        port.to_string(),
        socket.to_string(),
        "--".to_string(),
    ]);
    command.extend_from_slice(argv);
    Ok(command)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::bwrap::FILTER_SOCKET;
    use crate::policy::Network;
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn fixture() -> (tempfile::TempDir, Policy) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("wt")).unwrap();
        std::fs::create_dir(root.join("home")).unwrap();
        let policy = Policy::new(root.join("wt"), root.join("home"), "/bin/app".into());
        (dir, policy)
    }

    #[test]
    fn empty_argv_is_refused() {
        let (dir, policy) = fixture();
        assert!(wrap(&policy, &[], dir.path()).is_err());
    }

    #[test]
    fn a_missing_worktree_is_refused() {
        let (dir, mut policy) = fixture();
        policy.worktree.push("gone");
        assert!(wrap(&policy, &strings(&["true"]), dir.path()).is_err());
    }

    #[test]
    fn relative_paths_are_refused() {
        let (dir, mut policy) = fixture();
        policy.write.push(PathBuf::from("relative/path"));
        let error = wrap(&policy, &strings(&["true"]), dir.path()).unwrap_err();
        assert!(error.to_string().contains("relative/path"), "{error}");
    }

    #[test]
    fn paths_that_are_not_utf8_are_refused_rather_than_mangled() {
        let (dir, mut policy) = fixture();
        policy
            .deny_read
            .push(PathBuf::from(OsStr::from_bytes(b"/nowhere/\xff")));
        assert!(wrap(&policy, &strings(&["true"]), dir.path()).is_err());
    }

    #[test]
    fn filter_network_sets_every_proxy_variable() {
        let (_dir, mut policy) = fixture();
        policy.network = Network::Filter {
            port: 18080,
            socket: "/x.sock".into(),
        };
        let env = proxy_env(&policy);
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        for key in [
            "HTTPS_PROXY",
            "HTTP_PROXY",
            "https_proxy",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            assert_eq!(get(key), Some("http://127.0.0.1:18080"), "{key}");
        }
        for key in ["NO_PROXY", "no_proxy"] {
            let value = get(key).unwrap();
            assert!(
                value.contains("localhost") && value.contains("127.0.0.1"),
                "{key}: {value}"
            );
        }
    }

    #[test]
    fn other_networks_set_no_proxy_variables() {
        let (_dir, mut policy) = fixture();
        assert!(proxy_env(&policy).is_empty());
        policy.network = Network::Open;
        assert!(proxy_env(&policy).is_empty());
    }

    #[test]
    fn linux_command_ends_with_the_plain_argv_without_a_filter() {
        let (_dir, policy) = fixture();
        let argv = bwrap_command(&policy, &strings(&["agent", "--flag"])).unwrap();
        let expected = strings(&[
            "--",
            "/bin/app",
            "sandbox-bridge",
            "0",
            "/dev/null",
            "--",
            "agent",
            "--flag",
        ]);
        assert_eq!(&argv[argv.len() - expected.len()..], expected);
    }

    #[test]
    fn linux_command_shields_bwrap_from_terminal_interrupts() {
        let (_dir, policy) = fixture();
        let argv = bwrap_command(&policy, &strings(&["agent"])).unwrap();
        assert_eq!(argv[0], "/bin/sh");
        assert_eq!(argv[1], "-c");
        assert_eq!(argv[2], r#"trap '' INT QUIT; exec "$@""#);
        assert_eq!(argv[4], "bwrap");
    }

    #[test]
    fn linux_command_starts_the_forwarder_when_filtered() {
        let (dir, mut policy) = fixture();
        let socket = dir.path().join("f.sock");
        let _listener = UnixListener::bind(&socket).unwrap();
        policy.network = Network::Filter {
            port: 18080,
            socket,
        };
        let argv = bwrap_command(&policy, &strings(&["agent", "--flag"])).unwrap();
        let expected = strings(&[
            "--",
            "/bin/app",
            "sandbox-bridge",
            "18080",
            FILTER_SOCKET,
            "--",
            "agent",
            "--flag",
        ]);
        assert_eq!(&argv[argv.len() - expected.len()..], expected);
    }

    #[test]
    fn linux_command_refuses_a_missing_filter_socket() {
        let (dir, mut policy) = fixture();
        policy.network = Network::Filter {
            port: 1,
            socket: dir.path().join("gone.sock"),
        };
        assert!(bwrap_command(&policy, &strings(&["x"])).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_wrap_writes_a_private_profile_and_runs_through_it() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, policy) = fixture();
        let out = dir.path().join("generated");
        let first = wrap(&policy, &strings(&["agent", "-x"]), &out).unwrap();
        assert_eq!(first.argv[0], "/usr/bin/sandbox-exec");
        assert_eq!(first.argv[1], "-f");
        assert_eq!(&first.argv[first.argv.len() - 3..], ["--", "agent", "-x"]);
        let file = PathBuf::from(&first.argv[2]);
        assert!(file.starts_with(&out));
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            crate::seatbelt_profile(&policy)
        );
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let second = wrap(&policy, &strings(&["agent"]), &out).unwrap();
        assert_ne!(
            first.argv[2], second.argv[2],
            "two terminals must not share a profile file"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_machine_has_the_backend() {
        assert_eq!(available(), Ok(()));
    }
}
