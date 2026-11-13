//! Turns a policy into bubblewrap arguments.
//!
//! bwrap applies its operations in order, so a mount that sits under another has to come after it.
//! Writes come first, then read-only paths, then read denials, then masks: the later group wins where
//! paths nest, which matches Seatbelt's deny-after-allow behaviour.

use std::fs;
use std::path::{Path, PathBuf};

use crate::paths::{GIT_POINTERS, GIT_WRITES, protected_git_paths, resolve};
use crate::policy::{Network, Policy};

/// Where the filter's Unix socket appears inside the sandbox.
pub(crate) const FILTER_SOCKET: &str = "/run/stopcord-filter.sock";

/// Renders the bubblewrap arguments for a policy, without the `bwrap` program name or the command.
/// Paths that don't exist are skipped, because bwrap fails on a missing bind source.
pub fn bwrap_args(policy: &Policy) -> Vec<String> {
    let mut args = Args::default();
    args.extend(&["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]);
    args.extend(&["--tmpfs", "/tmp", "--tmpfs", "/run"]);
    if policy.network != Network::Open {
        args.extend(&["--unshare-net"]);
    }
    // No `--new-session`: these are interactive terminals, and a new session detaches the process from
    // its controlling terminal, which breaks job control. The usual reason for the flag is TIOCSTI
    // injection into the parent shell, but the daemon's pty has no parent shell to inject into.
    args.extend(&[
        "--unshare-pid",
        "--unshare-ipc",
        "--unshare-uts",
        "--die-with-parent",
    ]);

    // The tmpfs mounts above would hide a forwarder that lives under them, as an AppImage does.
    let program = resolve(&policy.program);
    if program.exists() && (under_tmpfs(&policy.program) || under_tmpfs(&program)) {
        args.bind("--ro-bind", &program);
    }

    for path in by_depth(existing(write_paths(policy))) {
        args.bind("--bind", &path);
    }
    for path in by_depth(existing(read_only_paths(policy))) {
        args.bind("--ro-bind", &path);
    }
    for path in by_depth(existing(
        policy.deny_read.iter().map(|p| resolve(p)).collect(),
    )) {
        if path.is_dir() {
            args.extend(&["--tmpfs"]);
            args.path(&path);
        } else {
            args.mask(&path);
        }
    }
    // A read-only bind doesn't stop a connect() to a Unix socket on it, so sockets are covered by path.
    for path in existing(policy.masked_sockets.iter().map(|p| resolve(p)).collect()) {
        args.mask(&path);
    }
    if let Some(report) = &policy.report_socket {
        let report = resolve(report);
        if report.exists() {
            args.bind("--bind", &report);
        }
    }
    if let Network::Filter { socket, .. } = &policy.network {
        let socket = resolve(socket);
        if socket.exists() {
            args.extend(&["--ro-bind"]);
            args.path(&socket);
            args.extend(&[FILTER_SOCKET]);
        }
    }
    let worktree = resolve(&policy.worktree);
    if worktree.exists() {
        args.extend(&["--chdir"]);
        args.path(&worktree);
    }
    args.0
}

fn write_paths(policy: &Policy) -> Vec<PathBuf> {
    let mut paths = vec![resolve(&policy.worktree)];
    if let Some(common) = &policy.git_common_dir {
        let common = resolve(common);
        paths.extend(GIT_WRITES.iter().map(|g| common.join(g)));
    }
    paths.extend(policy.write.iter().map(|p| resolve(p)));
    // `/tmp` is already a private tmpfs. Binding it would hand the sandbox the real one.
    paths.retain(|p| p != Path::new("/tmp"));
    paths
}

fn read_only_paths(policy: &Policy) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = policy.read_only.iter().map(|p| resolve(p)).collect();
    paths.extend(protected_git_paths(policy).iter().map(|p| resolve(p)));
    // `.git` in a linked worktree is a file naming the repository. A real `.git` directory has to stay
    // writable; its hooks and config are covered above.
    let dot_git = resolve(&policy.worktree).join(".git");
    if dot_git.is_file() {
        paths.push(dot_git);
    }
    if let Some(common) = &policy.git_common_dir {
        paths.extend(git_pointer_files(&resolve(common)));
    }
    paths
}

/// The files that say where each linked worktree's repository is. Unlike Seatbelt's regex, a bind can
/// only cover what exists now, so a worktree added inside the sandbox is not covered.
fn git_pointer_files(common: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(common.join("worktrees")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .flat_map(|e| GIT_POINTERS.iter().map(move |n| e.path().join(n)))
        .collect()
}

fn existing(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.into_iter().filter(|p| p.exists()).collect()
}

/// Shallowest first and without repeats, so a mount under another always comes after it.
fn by_depth(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort_by_key(|p| p.components().count());
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));
    paths
}

fn under_tmpfs(path: &Path) -> bool {
    path.starts_with("/tmp") || path.starts_with("/run")
}

#[derive(Default)]
struct Args(Vec<String>);

impl Args {
    fn extend(&mut self, items: &[&str]) {
        self.0.extend(items.iter().map(|s| s.to_string()));
    }

    fn path(&mut self, path: &Path) {
        self.0.push(path.to_string_lossy().into_owned());
    }

    fn bind(&mut self, flag: &str, path: &Path) {
        self.extend(&[flag]);
        self.path(path);
        self.path(path);
    }

    /// Covers a file or socket with an empty read-only file.
    fn mask(&mut self, path: &Path) {
        self.extend(&["--ro-bind", "/dev/null"]);
        self.path(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Network;
    use std::fs;
    use std::os::unix::net::UnixListener;
    use std::path::{Path, PathBuf};

    struct Fix {
        _dir: tempfile::TempDir,
        root: PathBuf,
    }

    fn fix() -> Fix {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("wt")).unwrap();
        fs::create_dir_all(root.join("home")).unwrap();
        Fix { _dir: dir, root }
    }

    fn policy(f: &Fix) -> Policy {
        Policy::new(f.root.join("wt"), f.root.join("home"), "/bin/app".into())
    }

    fn s(p: &Path) -> String {
        p.to_str().unwrap().to_string()
    }

    /// Where `seq` first appears as consecutive arguments.
    fn at(args: &[String], seq: &[&str]) -> Option<usize> {
        args.windows(seq.len())
            .position(|w| w.iter().map(String::as_str).eq(seq.iter().copied()))
    }

    fn must(args: &[String], seq: &[&str]) -> usize {
        at(args, seq).unwrap_or_else(|| panic!("{seq:?} not in {args:?}"))
    }

    fn bind_at(args: &[String], flag: &str, p: &Path) -> usize {
        must(args, &[flag, &s(p), &s(p)])
    }

    #[test]
    fn base_flags_come_first_in_order() {
        let f = fix();
        let a = bwrap_args(&policy(&f));
        let expected = [
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--tmpfs",
            "/tmp",
            "--tmpfs",
            "/run",
        ];
        assert_eq!(&a[..expected.len()], &expected);
        for flag in [
            "--unshare-net",
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--die-with-parent",
        ] {
            assert!(a.iter().any(|x| x == flag), "missing {flag}");
        }
    }

    #[test]
    fn never_starts_a_new_session() {
        let f = fix();
        let mut p = policy(&f);
        for net in [Network::Off, Network::Open] {
            p.network = net;
            assert!(!bwrap_args(&p).iter().any(|x| x == "--new-session"));
        }
    }

    #[test]
    fn open_network_keeps_the_host_network_namespace() {
        let f = fix();
        let mut p = policy(&f);
        p.network = Network::Open;
        let a = bwrap_args(&p);
        assert!(!a.iter().any(|x| x == "--unshare-net"));
        assert!(a.iter().any(|x| x == "--unshare-pid"));
    }

    #[test]
    fn worktree_is_bound_and_is_the_working_directory() {
        let f = fix();
        let a = bwrap_args(&policy(&f));
        let wt = f.root.join("wt");
        bind_at(&a, "--bind", &wt);
        assert_eq!(&a[a.len() - 2..], ["--chdir".to_string(), s(&wt)]);
    }

    #[test]
    fn missing_paths_are_skipped() {
        let f = fix();
        let mut p = policy(&f);
        let gone = f.root.join("gone");
        p.write.push(gone.clone());
        p.read_only.push(gone.join("x"));
        p.deny_read.push(gone.join("y"));
        p.masked_sockets.push(gone.join("d.sock"));
        p.report_socket = Some(gone.join("r.sock"));
        p.git_common_dir = Some(gone.join(".git"));
        let a = bwrap_args(&p);
        assert!(!a.iter().any(|x| x.contains("/gone")), "{a:?}");
    }

    #[test]
    fn a_bind_under_another_bind_comes_later_whatever_the_list_order() {
        let f = fix();
        let mut p = policy(&f);
        let outer = f.root.join("outer");
        let inner = outer.join("inner");
        fs::create_dir_all(&inner).unwrap();
        p.write = vec![inner.clone(), outer.clone()];
        let a = bwrap_args(&p);
        assert!(bind_at(&a, "--bind", &outer) < bind_at(&a, "--bind", &inner));
    }

    #[test]
    fn read_only_paths_come_after_the_writes_they_sit_under() {
        let f = fix();
        let mut p = policy(&f);
        let state = f.root.join("home/.claude");
        let settings = state.join("settings.json");
        fs::create_dir_all(&state).unwrap();
        fs::write(&settings, "{}").unwrap();
        p.write.push(state.clone());
        p.read_only.push(settings.clone());
        let a = bwrap_args(&p);
        assert!(bind_at(&a, "--bind", &state) < bind_at(&a, "--ro-bind", &settings));
    }

    #[test]
    fn deny_read_uses_tmpfs_for_directories_and_dev_null_for_files() {
        let f = fix();
        let mut p = policy(&f);
        let dir = f.root.join("home/.ssh");
        let file = f.root.join("home/.netrc");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&file, "x").unwrap();
        p.deny_read = vec![dir.clone(), file.clone()];
        let a = bwrap_args(&p);
        must(&a, &["--tmpfs", &s(&dir)]);
        must(&a, &["--ro-bind", "/dev/null", &s(&file)]);
    }

    #[test]
    fn deny_read_wins_over_a_write_under_it() {
        let f = fix();
        let mut p = policy(&f);
        let dir = f.root.join("home/.ssh");
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        p.deny_read = vec![dir.clone()];
        p.write.push(sub.clone());
        let a = bwrap_args(&p);
        assert!(bind_at(&a, "--bind", &sub) < must(&a, &["--tmpfs", &s(&dir)]));
    }

    #[test]
    fn masked_sockets_are_covered_and_the_report_socket_is_bound_after() {
        let f = fix();
        let mut p = policy(&f);
        let d = f.root.join("d.sock");
        let r = f.root.join("r.sock");
        let _l1 = UnixListener::bind(&d).unwrap();
        let _l2 = UnixListener::bind(&r).unwrap();
        p.masked_sockets.push(d.clone());
        p.report_socket = Some(r.clone());
        let a = bwrap_args(&p);
        let mask = must(&a, &["--ro-bind", "/dev/null", &s(&d)]);
        assert!(mask < bind_at(&a, "--bind", &r));
    }

    #[test]
    fn filter_socket_is_mounted_at_the_fixed_path_after_run_is_replaced() {
        let f = fix();
        let mut p = policy(&f);
        let sock = f.root.join("f.sock");
        let _l = UnixListener::bind(&sock).unwrap();
        p.network = Network::Filter {
            port: 18080,
            socket: sock.clone(),
        };
        let a = bwrap_args(&p);
        let tmpfs_run = must(&a, &["--tmpfs", "/run"]);
        let bound = must(&a, &["--ro-bind", &s(&sock), "/run/stopcord-filter.sock"]);
        assert!(tmpfs_run < bound);
        assert!(a.iter().any(|x| x == "--unshare-net"));
    }

    #[test]
    fn real_tmp_is_never_bound_over_the_private_tmpfs() {
        let f = fix();
        let a = bwrap_args(&policy(&f));
        assert!(at(&a, &["--bind", "/tmp", "/tmp"]).is_none(), "{a:?}");
    }

    #[test]
    fn git_common_dir_binds_objects_and_keeps_hooks_and_config_read_only() {
        let f = fix();
        let mut p = policy(&f);
        let c = f.root.join("proj/.git");
        for d in ["objects", "refs", "logs", "worktrees/w", "hooks"] {
            fs::create_dir_all(c.join(d)).unwrap();
        }
        fs::write(c.join("config"), "").unwrap();
        fs::write(c.join("worktrees/w/commondir"), "../..").unwrap();
        p.git_common_dir = Some(c.clone());
        let a = bwrap_args(&p);
        for d in ["objects", "refs", "logs", "worktrees"] {
            bind_at(&a, "--bind", &c.join(d));
        }
        assert!(at(&a, &["--bind", &s(&c.join("hooks")), &s(&c.join("hooks"))]).is_none());
        assert!(
            at(
                &a,
                &["--bind", &s(&c.join("config")), &s(&c.join("config"))]
            )
            .is_none()
        );
        let pointer = bind_at(&a, "--ro-bind", &c.join("worktrees/w/commondir"));
        assert!(bind_at(&a, "--bind", &c.join("worktrees")) < pointer);
    }

    #[test]
    fn worktree_git_pointer_file_is_read_only_but_a_git_directory_is_not() {
        let f = fix();
        let mut p = policy(&f);
        let dotgit = f.root.join("wt/.git");
        fs::write(&dotgit, "gitdir: x").unwrap();
        let a = bwrap_args(&p);
        bind_at(&a, "--ro-bind", &dotgit);

        fs::remove_file(&dotgit).unwrap();
        fs::create_dir_all(dotgit.join("hooks")).unwrap();
        fs::write(dotgit.join("config"), "").unwrap();
        p.deny_read.clear();
        let a = bwrap_args(&p);
        assert!(
            at(&a, &["--ro-bind", &s(&dotgit), &s(&dotgit)]).is_none(),
            "{a:?}"
        );
        bind_at(&a, "--ro-bind", &dotgit.join("hooks"));
        bind_at(&a, "--ro-bind", &dotgit.join("config"));
    }

    #[test]
    fn only_paths_under_tmp_and_run_count_as_hidden_by_the_tmpfs_mounts() {
        assert!(under_tmpfs(Path::new("/tmp/.mount_x/app")));
        assert!(under_tmpfs(Path::new("/run/user/1000/app")));
        assert!(!under_tmpfs(Path::new("/tmpfoo/app")));
        assert!(!under_tmpfs(Path::new("/usr/bin/app")));
    }

    #[test]
    fn the_forwarder_program_survives_a_tmp_location() {
        let f = fix();
        let mut p = policy(&f);
        p.program = "/tmp/.mount_x/app".into();
        let _ = bwrap_args(&p); // Doesn't exist here; only must not panic.

        // A program outside the tmpfs mounts needs no bind of its own.
        p.program = "/bin/sh".into();
        let a = bwrap_args(&p);
        assert!(at(&a, &["--ro-bind", "/bin/sh", "/bin/sh"]).is_none());
    }
}
