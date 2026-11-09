//! Real `sandbox-exec` runs. Everything lives in a temporary folder under the user's home, because
//! `/tmp` is writable by design, and every path the sandbox is tested against is inside that folder.
#![cfg(target_os = "macos")]

use std::fs;
use std::io::Read;
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::thread;

use sawtooth_sandbox::{Filter, Network, Policy, wrap};

struct Fix {
    _dir: tempfile::TempDir,
    root: PathBuf,
    policy: Policy,
    generated: tempfile::TempDir,
}

fn fix() -> Fix {
    let home = std::env::var("HOME").expect("HOME");
    let dir = tempfile::Builder::new()
        .prefix(".sbx-test-")
        .tempdir_in(home)
        .unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir(root.join("wt")).unwrap();
    fs::create_dir(root.join("home")).unwrap();
    let policy = Policy::new(root.join("wt"), root.join("home"), "/bin/false".into());
    Fix {
        _dir: dir,
        root,
        policy,
        generated: tempfile::tempdir().unwrap(),
    }
}

impl Fix {
    fn run_in(&self, cwd: &Path, script: &str) -> Output {
        let argv = vec!["/bin/sh".to_string(), "-c".to_string(), script.to_string()];
        let wrapped = wrap(&self.policy, &argv, self.generated.path()).unwrap();
        Command::new(&wrapped.argv[0])
            .args(&wrapped.argv[1..])
            .envs(wrapped.env.iter().map(|(k, v)| (k, v)))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .current_dir(cwd)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn run(&self, script: &str) -> Output {
        self.run_in(&self.policy.worktree, script)
    }

    /// A linked worktree of a fresh repository, the way Sawtooth makes one. Returns (worktree, common dir).
    fn linked_worktree(&mut self) -> (PathBuf, PathBuf) {
        let proj = self.root.join("proj");
        let wt = self.root.join("linked");
        fs::create_dir(&proj).unwrap();
        git(&proj, &["init", "-q", "-b", "main"]);
        git(&proj, &["commit", "-q", "--allow-empty", "-m", "first"]);
        git(
            &proj,
            &["worktree", "add", "-q", "-b", "w", wt.to_str().unwrap()],
        );
        let common = proj.join(".git");
        self.policy.worktree = wt.clone();
        self.policy.git_common_dir = Some(common.clone());
        (wt, common)
    }
}

/// Git outside the sandbox, with the user's own configuration and hooks kept out of it.
fn git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args([
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(cwd)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn text(output: &Output) -> String {
    format!(
        "status {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_ok(output: &Output) {
    assert!(output.status.success(), "{}", text(output));
}

fn assert_denied(output: &Output) {
    assert!(
        !output.status.success(),
        "should have failed: {}",
        text(output)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Operation not permitted"),
        "{}",
        text(output)
    );
}

/// Accepts and immediately closes every connection on a Unix socket.
fn unix_server(path: &Path) {
    let listener = UnixListener::bind(path).unwrap();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            drop(stream);
        }
    });
}

/// Accepts and immediately closes every connection on a loopback port, and returns the port.
fn tcp_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            drop(stream);
        }
    });
    port
}

fn filtered(policy: &mut Policy, port: u16) {
    policy.network = Network::Filter {
        port,
        socket: "/nonexistent-filter.sock".into(),
    };
}

#[test]
fn writing_inside_the_worktree_works() {
    let f = fix();
    let target = f.policy.worktree.join("made-here");
    assert_ok(&f.run(&format!("echo x > '{}'", target.display())));
    assert!(target.exists());
}

#[test]
fn writing_to_a_sibling_folder_is_denied() {
    let f = fix();
    let sibling = f.root.join("sibling");
    fs::create_dir(&sibling).unwrap();
    let target = sibling.join("escaped");
    assert_denied(&f.run(&format!("echo x > '{}'", target.display())));
    assert!(!target.exists());
}

#[test]
fn writing_to_the_home_folder_is_denied() {
    let f = fix();
    let target = f.root.join("home/escaped");
    assert_denied(&f.run(&format!("echo x > '{}'", target.display())));
    assert!(!target.exists());
}

#[test]
fn a_denied_read_is_denied() {
    let mut f = fix();
    let key = f.root.join("home/.ssh/key");
    fs::create_dir(f.root.join("home/.ssh")).unwrap();
    fs::write(&key, "secret").unwrap();
    f.policy.deny_read = vec![f.root.join("home/.ssh")];
    let output = f.run(&format!("cat '{}'", key.display()));
    assert_denied(&output);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("secret"));
}

#[test]
fn a_denied_read_cannot_be_reached_through_different_case() {
    let mut f = fix();
    fs::create_dir(f.root.join("home/.ssh")).unwrap();
    fs::write(f.root.join("home/.ssh/key"), "secret").unwrap();
    f.policy.deny_read = vec![f.root.join("home/.ssh")];
    let shouted = f.root.join("home/.SSH/key");
    let output = f.run(&format!("cat '{}'", shouted.display()));
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("secret"),
        "{}",
        text(&output)
    );
    // Default macOS volumes ignore case, so this is a real attempt there and must be a real denial.
    if shouted.exists() {
        assert_denied(&output);
    }
}

#[test]
fn a_read_only_path_inside_a_writable_folder_stays_read_only() {
    let mut f = fix();
    let state = f.root.join("home/.state");
    fs::create_dir(&state).unwrap();
    let settings = state.join("settings.json");
    fs::write(&settings, "{}").unwrap();
    f.policy.write.push(state.clone());
    f.policy.read_only.push(settings.clone());
    assert_denied(&f.run(&format!("echo x > '{}'", settings.display())));
    assert_eq!(fs::read_to_string(&settings).unwrap(), "{}");
    assert_ok(&f.run(&format!("echo x > '{}/other'", state.display())));
}

#[test]
fn a_read_only_symlink_cannot_be_replaced() {
    let mut f = fix();
    let state = f.root.join("home/.state");
    fs::create_dir(&state).unwrap();
    let real = f.root.join("dotfiles-settings");
    fs::write(&real, "{}").unwrap();
    let link = state.join("settings.json");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    f.policy.write.push(state.clone());
    f.policy.read_only.push(link.clone());
    assert_denied(&f.run(&format!("rm '{}'", link.display())));
    assert!(link.is_symlink());
}

#[test]
fn git_hooks_and_config_are_denied_in_a_linked_worktree() {
    let mut f = fix();
    let (wt, common) = f.linked_worktree();
    let hook = common.join("hooks/pre-commit");
    assert_denied(&f.run_in(&wt, &format!("echo x > '{}'", hook.display())));
    assert!(!hook.exists());
    let config = common.join("config");
    let before = fs::read_to_string(&config).unwrap();
    assert_denied(&f.run_in(&wt, &format!("echo x >> '{}'", config.display())));
    assert_eq!(fs::read_to_string(&config).unwrap(), before);
}

#[test]
fn git_pointers_cannot_be_rewritten_in_a_linked_worktree() {
    let mut f = fix();
    let (wt, common) = f.linked_worktree();
    let dot_git = wt.join(".git");
    let before = fs::read_to_string(&dot_git).unwrap();
    assert_denied(&f.run_in(&wt, &format!("echo gitdir: /x > '{}'", dot_git.display())));
    assert_eq!(fs::read_to_string(&dot_git).unwrap(), before);
    let commondir = common.join("worktrees/linked/commondir");
    let before = fs::read_to_string(&commondir).unwrap();
    assert_denied(&f.run_in(&wt, &format!("echo /x > '{}'", commondir.display())));
    assert_eq!(fs::read_to_string(&commondir).unwrap(), before);
}

#[test]
fn git_commit_works_in_a_linked_worktree() {
    let mut f = fix();
    let (wt, common) = f.linked_worktree();
    let script = "echo hello > file && git add file \
        && git -c user.email=t@t -c user.name=t commit -q -m inside \
        && git log --oneline -1";
    let output = f.run_in(&wt, script);
    assert_ok(&output);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("inside"),
        "{}",
        text(&output)
    );
    // The commit landed in the main checkout's object store, on the worktree's branch.
    let log = Command::new("git")
        .args(["log", "--format=%s", "-1", "w"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(common.parent().unwrap())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "inside");
}

#[test]
fn a_masked_unix_socket_is_denied() {
    let mut f = fix();
    let socket = f.root.join("d.sock");
    unix_server(&socket);
    filtered(&mut f.policy, tcp_server());
    f.policy.masked_sockets.push(socket.clone());
    let output = f.run(&format!(
        "/usr/bin/nc -U -w 1 '{}' </dev/null",
        socket.display()
    ));
    assert!(!output.status.success(), "{}", text(&output));
}

#[test]
fn an_unlisted_unix_socket_is_denied() {
    let f = fix();
    let socket = f.root.join("other.sock");
    unix_server(&socket);
    let output = f.run(&format!(
        "/usr/bin/nc -U -w 1 '{}' </dev/null",
        socket.display()
    ));
    assert!(!output.status.success(), "{}", text(&output));
}

#[test]
fn the_report_socket_can_be_reached() {
    let mut f = fix();
    let socket = f.root.join("r.sock");
    unix_server(&socket);
    f.policy.report_socket = Some(socket.clone());
    assert_ok(&f.run(&format!(
        "/usr/bin/nc -U -w 1 '{}' </dev/null",
        socket.display()
    )));
}

#[test]
fn a_denied_port_is_denied_while_another_loopback_port_works() {
    let mut f = fix();
    let denied = tcp_server();
    let open = tcp_server();
    filtered(&mut f.policy, open);
    f.policy.deny_ports.push(denied);
    let output = f.run(&format!("/usr/bin/nc -z -w 2 127.0.0.1 {denied}"));
    assert!(!output.status.success(), "{}", text(&output));
    assert_ok(&f.run(&format!("/usr/bin/nc -z -w 2 127.0.0.1 {open}")));
}

#[test]
fn network_off_blocks_loopback_too() {
    let f = fix();
    let port = tcp_server();
    let output = f.run(&format!("/usr/bin/nc -z -w 2 127.0.0.1 {port}"));
    assert!(!output.status.success(), "{}", text(&output));
}

// Both commands are harmless if the sandbox fails: `open -h` prints usage, and this script only returns 1.
#[test]
fn open_and_osascript_are_denied() {
    let f = fix();
    assert_denied(&f.run("/usr/bin/open -h"));
    assert_denied(&f.run("/usr/bin/osascript -e 'return 1'"));
}

#[test]
fn traffic_goes_through_the_filter_with_the_wrapped_environment() {
    let mut f = fix();
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    thread::spawn(move || {
        for mut stream in upstream.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let _ = std::io::Write::write_all(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nthrough",
            );
        }
    });
    let filter = Filter::start(
        vec!["localhost".to_string()],
        f.root.join("f.sock"),
        Arc::new(|_| {}),
    )
    .unwrap();
    f.policy.network = Network::Filter {
        port: filter.port(),
        socket: filter.socket().to_path_buf(),
    };

    // NO_PROXY is meant for loopback, so curl is told to use the proxy for localhost anyway.
    let allowed = f.run(&format!(
        "/usr/bin/curl -sS -m 5 --noproxy '' http://localhost:{upstream_port}/"
    ));
    assert_ok(&allowed);
    assert_eq!(
        String::from_utf8_lossy(&allowed.stdout),
        "through",
        "{}",
        text(&allowed)
    );

    let blocked =
        f.run("/usr/bin/curl -sS -m 5 -o /dev/null -w '%{http_code}' http://blocked.invalid/");
    assert_eq!(
        String::from_utf8_lossy(&blocked.stdout),
        "403",
        "{}",
        text(&blocked)
    );

    let tunnel = f.run("/usr/bin/curl -sS -m 5 https://blocked.invalid/");
    assert!(!tunnel.status.success(), "{}", text(&tunnel));
    assert!(
        String::from_utf8_lossy(&tunnel.stderr).contains("403"),
        "{}",
        text(&tunnel)
    );

    filter.stop();
}
