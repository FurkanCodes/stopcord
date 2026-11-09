//! Path handling shared by the backends.

use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::policy::Policy;

/// Parts of a main checkout's `.git` that a linked worktree must write to. Everything else there,
/// `hooks` and `config` above all, stays read-only: those run or configure code outside the sandbox.
pub(crate) const GIT_WRITES: &[&str] = &[
    "objects",
    "refs",
    "logs",
    "packed-refs",
    "packed-refs.lock",
    "packed-refs.new",
    "worktrees",
    "ORIG_HEAD",
    "FETCH_HEAD",
    "index",
    "COMMIT_EDITMSG",
    "lfs",
    "rr-cache",
];

/// Files in `.git/worktrees/<name>` that say where the repository is. Git in that worktree obeys them,
/// including git run later outside the sandbox, so editing one would redirect it to planted hooks.
pub(crate) const GIT_POINTERS: &[&str] = &["commondir", "gitdir", "config.worktree"];

/// The path with `.` and `..` removed and symlinks resolved, as far as it exists. A part that doesn't
/// exist yet is kept as written, so a folder a tool creates later is still covered.
pub(crate) fn resolve(path: &Path) -> PathBuf {
    let path = normalize(path);
    let mut existing = path.as_path();
    let mut missing: Vec<OsString> = Vec::new();
    loop {
        if let Ok(mut real) = fs::canonicalize(existing) {
            real.extend(missing.iter().rev());
            return real;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name.to_os_string());
                existing = parent;
            }
            _ => return path,
        }
    }
}

/// The path with its parent resolved and its last part left alone, so it names a symlink rather than
/// what the symlink points at.
pub(crate) fn resolve_parent(path: &Path) -> PathBuf {
    let path = normalize(path);
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => resolve(parent).join(name),
        _ => path,
    }
}

/// The paths git's control files live at for this policy that a sandboxed process must not change.
/// Directories and files named here need not exist; the backends deal with that.
pub(crate) fn protected_git_paths(policy: &Policy) -> Vec<PathBuf> {
    let dot_git = policy.worktree.join(".git");
    let mut paths = vec![dot_git.join("hooks"), dot_git.join("config")];
    if let Some(common) = &policy.git_common_dir {
        paths.push(common.join("hooks"));
        paths.push(common.join("config"));
    }
    paths
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_keeps_the_part_that_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().canonicalize().unwrap();
        assert_eq!(resolve(&dir.path().join("a/b")), real.join("a/b"));
    }

    #[test]
    fn resolve_removes_dots() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().canonicalize().unwrap();
        assert_eq!(resolve(&dir.path().join("a/../b/./c")), real.join("b/c"));
    }

    #[test]
    fn resolve_follows_symlinks_but_resolve_parent_stops_at_the_last() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::create_dir(root.join("real")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
        assert_eq!(resolve(&root.join("link")), root.join("real"));
        assert_eq!(resolve_parent(&root.join("link")), root.join("link"));
    }

    #[test]
    fn a_dangling_symlink_stays_where_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::os::unix::fs::symlink(root.join("nowhere"), root.join("dangling")).unwrap();
        assert_eq!(resolve(&root.join("dangling")), root.join("dangling"));
    }
}
