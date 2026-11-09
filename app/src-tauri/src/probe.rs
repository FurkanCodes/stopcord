//! `sawtooth-app probe <spec>`: run inside a runner's sandbox, it tries what the sandbox should
//! allow or stop and prints what happened. Nothing it tries leaves a trace: a file written by
//! mistake is removed, connections are closed at once, and `open -h` only prints its help.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// What the probe is told to try, and what the runner's settings say should happen.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Spec {
    /// Whether this runner is Sawtooth's own sandbox. Without one, everything is expected to work.
    pub contained: bool,
    /// Whether it reads only what's listed.
    pub listed: bool,
    /// A file outside the folder to try writing.
    pub outside: PathBuf,
    /// A folder outside the agent's own to try reading.
    pub sample: PathBuf,
    pub blocks: Vec<Block>,
    pub sockets: Vec<Socket>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Block {
    pub label: String,
    pub paths: Vec<PathBuf>,
    /// Whether this runner blocks it.
    pub blocked: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Socket {
    pub label: String,
    pub path: PathBuf,
}

/// One thing tried. `held` is whether it went the way the settings say; `None` when it couldn't be told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub want: String,
    pub got: String,
    pub held: Option<bool>,
}

/// The line the probe's result is printed on, so a login shell's own output can't be mistaken for it.
pub const MARK: &str = "SAWTOOTH-PROBE ";

enum Outcome {
    Allowed,
    Blocked,
    Unclear(String),
}

fn outcome<T>(result: std::io::Result<T>) -> Outcome {
    match result {
        Ok(_) => Outcome::Allowed,
        Err(e) if e.kind() == ErrorKind::PermissionDenied => Outcome::Blocked,
        Err(e) => Outcome::Unclear(e.to_string()),
    }
}

fn check(name: &str, should_block: bool, got: Outcome) -> Check {
    let want = if should_block { "blocked" } else { "allowed" };
    let (got, held) = match got {
        Outcome::Allowed => ("allowed".to_string(), Some(!should_block)),
        Outcome::Blocked => ("blocked".to_string(), Some(should_block)),
        Outcome::Unclear(why) => (format!("couldn't tell: {why}"), None),
    };
    Check { name: name.to_string(), want: want.to_string(), got, held }
}

/// Whether a path is on this machine. A sandbox that blocks a path can also refuse to say it's
/// there, so a refusal counts as there: skipping it would drop exactly the checks that matter.
fn present(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(e) => e.kind() == ErrorKind::PermissionDenied,
    }
}

fn try_read(path: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::read_dir(path).map(|_| ())
    } else {
        std::fs::File::open(path).map(|_| ())
    }
}

fn try_write(path: &Path) -> std::io::Result<()> {
    let written = std::fs::write(path, b"sawtooth probe");
    if written.is_ok() {
        let _ = std::fs::remove_file(path);
    }
    written
}

/// Asks the web filter for a site that's on no list. Anything but a 200 means it was refused.
fn through_filter(proxy: &str) -> Outcome {
    let addr = proxy.trim_end_matches('/').rsplit("//").next().unwrap_or(proxy);
    let Ok(mut stream) = TcpStream::connect(addr) else {
        return Outcome::Unclear(format!("couldn't reach the filter at {addr}"));
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let ask = "CONNECT sawtooth-probe.invalid:443 HTTP/1.1\r\nHost: sawtooth-probe.invalid:443\r\n\r\n";
    if stream.write_all(ask.as_bytes()).is_err() {
        return Outcome::Blocked;
    }
    let mut head = [0u8; 64];
    let n = stream.read(&mut head).unwrap_or(0);
    let reply = String::from_utf8_lossy(&head[..n]);
    if reply.split_whitespace().nth(1) == Some("200") { Outcome::Allowed } else { Outcome::Blocked }
}

/// Tries everything in `spec` from wherever it's running.
pub fn run(spec: &Spec) -> Vec<Check> {
    let sandboxed = spec.contained;
    let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut out = vec![
        check("writes in its own folder", false, outcome(try_write(&here.join(".sawtooth-probe")))),
        check("writes outside its folder", sandboxed, outcome(try_write(&spec.outside))),
    ];
    for block in &spec.blocks {
        // Only what's on this machine can be tried; a folder that isn't there proves nothing.
        let Some(path) = block.paths.iter().find(|p| present(p)) else {
            continue;
        };
        out.push(check(&format!("reads {}", block.label), sandboxed && block.blocked, outcome(try_read(path))));
    }
    if present(&spec.sample) {
        out.push(check("reads outside its folder", sandboxed && spec.listed, outcome(try_read(&spec.sample))));
    }
    for socket in &spec.sockets {
        if !present(&socket.path) {
            continue;
        }
        out.push(check(&format!("reaches {}", socket.label), sandboxed, outcome(UnixStream::connect(&socket.path))));
    }
    let launch = std::process::Command::new("/usr/bin/open").arg("-h").output();
    out.push(check("launches apps", sandboxed, outcome(launch)));
    let direct = TcpStream::connect_timeout(&SocketAddr::from(([1, 1, 1, 1], 443)), Duration::from_secs(3));
    out.push(check("goes around the web filter", sandboxed, outcome(direct)));
    if sandboxed {
        let proxy = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("https_proxy"));
        let got = match proxy {
            Ok(p) if !p.is_empty() => through_filter(&p),
            _ => Outcome::Unclear("no web filter was set up".into()),
        };
        out.push(check("reaches a site on no list", true, got));
    }
    out
}

pub fn main(args: &[String]) -> i32 {
    let Some(spec) = args.first().and_then(|a| serde_json::from_str::<Spec>(a).ok()) else {
        eprintln!("usage: sawtooth-app probe '<spec as json>'");
        return 2;
    };
    println!("{MARK}{}", serde_json::to_string(&run(&spec)).unwrap_or_default());
    0
}

/// Reads the probe's result out of everything it printed.
pub fn parse(output: &str) -> Option<Vec<Check>> {
    output.lines().rev().find_map(|l| l.strip_prefix(MARK)).and_then(|j| serde_json::from_str(j).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_sandbox_everything_is_expected_to_work() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("keys")).unwrap();
        let spec = Spec {
            outside: dir.path().join("outside.txt"),
            sample: dir.path().to_path_buf(),
            blocks: vec![Block { label: "keys".into(), paths: vec![dir.path().join("missing"), dir.path().join("keys")], blocked: true }],
            ..Spec::default()
        };
        let checks = run(&spec);
        let named = |n: &str| checks.iter().find(|c| c.name == n).unwrap_or_else(|| panic!("no {n} in {checks:?}"));
        assert_eq!(named("writes outside its folder").held, Some(true));
        assert!(!dir.path().join("outside.txt").exists(), "a probe leaves nothing behind");
        assert_eq!(named("reads keys").want, "allowed", "not sandboxed, so nothing is expected to be blocked");
        assert_eq!(named("reads keys").held, Some(true));
        assert_eq!(named("reads outside its folder").held, Some(true));
        assert!(checks.iter().all(|c| c.name != "reaches a site on no list"), "no filter to ask without a sandbox");
    }

    #[test]
    fn a_path_it_may_not_look_at_still_counts_as_there() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let hidden = present(&locked.join("key"));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(hidden, "a refusal to say means it's there");
        assert!(!present(&dir.path().join("missing")));
    }

    #[test]
    fn a_blocked_answer_that_comes_back_allowed_did_not_hold() {
        let c = check("reads keys", true, Outcome::Allowed);
        assert_eq!((c.want.as_str(), c.got.as_str(), c.held), ("blocked", "allowed", Some(false)));
        assert_eq!(check("x", true, Outcome::Unclear("no network".into())).held, None);
    }

    #[test]
    fn the_result_is_found_after_whatever_the_shell_printed() {
        let line = format!("motd\n{MARK}{}\n", serde_json::to_string(&vec![check("x", false, Outcome::Allowed)]).unwrap());
        assert_eq!(parse(&line).unwrap()[0].name, "x");
        assert!(parse("nothing here").is_none());
    }
}
