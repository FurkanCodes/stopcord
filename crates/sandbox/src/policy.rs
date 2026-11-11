//! What a sandboxed terminal may touch.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// How a sandboxed process reaches the network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Network {
    /// No network at all.
    Off,
    /// Only through the local filter. On macOS the sandbox may reach this TCP port on loopback.
    /// On Linux the namespace is empty and `socket` is bind-mounted in; a forwarder inside listens on `port`.
    Filter { port: u16, socket: PathBuf },
    /// Unfiltered. Used by the plain worktree level only.
    Open,
}

/// What a sandboxed process may read.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Reads {
    /// Everything except `deny_read`.
    #[default]
    Everything,
    /// Only these paths, the paths it may write, and the system folders any program needs. macOS only for now.
    Listed(Vec<PathBuf>),
}

/// One terminal's confinement, independent of the backend that enforces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    pub worktree: PathBuf,
    /// `.git` of the main checkout, for a linked worktree. Only objects, refs, logs, worktrees, packed-refs,
    /// index and similar become writable. `hooks` and `config` stay read-only.
    pub git_common_dir: Option<PathBuf>,
    /// The project's own branch, such as `main`. Work in a worktree can't move it: only a merge,
    /// made outside the sandbox, does. Its ref and `packed-refs` stay read-only.
    #[serde(default)]
    pub protected_branch: Option<String>,
    pub home: PathBuf,
    /// Extra writable paths, such as a harness's own state folder.
    pub write: Vec<PathBuf>,
    /// Files that may be written if their path starts with one of these, for tools that
    /// save through temp files such as `~/.claude.json.tmp.123`. macOS only: a bind mount
    /// on Linux can't express a prefix.
    #[serde(default)]
    pub write_prefix: Vec<PathBuf>,
    /// Stay read-only even under a writable path, such as harness settings.
    pub read_only: Vec<PathBuf>,
    pub deny_read: Vec<PathBuf>,
    #[serde(default)]
    pub reads: Reads,
    /// Unix sockets to make unreachable by path (daemon control, Docker). Linux needs this; see the spike doc.
    pub masked_sockets: Vec<PathBuf>,
    /// The one Unix socket the agent may connect to, for reporting back.
    pub report_socket: Option<PathBuf>,
    pub network: Network,
    /// Local ports the agent must not reach (macOS only; loopback is otherwise open for test servers).
    pub deny_ports: Vec<u16>,
    /// Program that implements the Linux forwarder, called as `<program> sandbox-bridge <port> <socket> -- argv...`.
    pub program: PathBuf,
}

/// Writable under home for every terminal: package caches and the like.
const COMMON_WRITES: &[&str] = &[
    ".npm",
    ".cargo/registry",
    ".cargo/git",
    ".cache",
    "Library/Caches",
];

/// Unreadable under home for every terminal: credentials.
pub const COMMON_DENY_READS: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".config/gcloud",
    ".kube",
    ".docker/config.json",
    ".netrc",
    ".npmrc",
    ".sawtooth/secrets",
];

impl Policy {
    /// Common writes (~/.npm, ~/.cargo/registry, ~/.cargo/git, ~/.cache, ~/Library/Caches, tmp) and common denied
    /// reads (~/.ssh, ~/.aws, ~/.gnupg, ~/.config/gcloud, ~/.kube, ~/.docker/config.json, ~/.netrc, ~/.npmrc, ~/.sawtooth/secrets).
    ///
    /// Tmp covers `/tmp` and macOS's per-user temp folders under `/private/var/folders`. Both are listed
    /// on every OS: a backend skips what it doesn't have. The network starts `Off`.
    pub fn new(worktree: PathBuf, home: PathBuf, program: PathBuf) -> Policy {
        let under_home = |names: &[&str]| names.iter().map(|n| home.join(n)).collect::<Vec<_>>();
        let mut write = under_home(COMMON_WRITES);
        write.push(PathBuf::from("/tmp"));
        write.push(PathBuf::from("/private/var/folders"));
        let deny_read = under_home(COMMON_DENY_READS);
        Policy {
            worktree,
            git_common_dir: None,
            protected_branch: None,
            home,
            write,
            write_prefix: Vec::new(),
            read_only: Vec::new(),
            deny_read,
            reads: Reads::Everything,
            masked_sockets: Vec::new(),
            report_socket: None,
            network: Network::Off,
            deny_ports: Vec::new(),
            program,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_policy_has_the_common_writes_and_denied_reads() {
        let p = Policy::new("/w".into(), "/h".into(), "/bin/app".into());
        for w in [
            "/h/.npm",
            "/h/.cargo/registry",
            "/h/.cargo/git",
            "/h/.cache",
            "/h/Library/Caches",
            "/tmp",
        ] {
            assert!(p.write.contains(&PathBuf::from(w)), "missing write {w}");
        }
        for d in [
            "/h/.ssh",
            "/h/.aws",
            "/h/.gnupg",
            "/h/.config/gcloud",
            "/h/.kube",
            "/h/.docker/config.json",
            "/h/.netrc",
            "/h/.npmrc",
            "/h/.sawtooth/secrets",
        ] {
            assert!(p.deny_read.contains(&PathBuf::from(d)), "missing deny {d}");
        }
        assert_eq!(p.network, Network::Off);
    }
}
