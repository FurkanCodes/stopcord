//! Git worktrees, so each terminal can work on its own branch in its own folder.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, bail};

use crate::factories::slug;

/// Runs git in `dir` and returns its trimmed output, or its error message.
pub fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim())
    }
}

/// True when `path` is inside a git work tree.
pub fn is_git(path: &Path) -> bool {
    git(path, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|s| s == "true")
}

/// The repository's shared git folder: the same for the main checkout and every worktree.
pub fn common_dir(path: &Path) -> Option<PathBuf> {
    let out = git(path, &["rev-parse", "--git-common-dir"]).ok()?;
    let dir = PathBuf::from(out);
    let dir = if dir.is_absolute() {
        dir
    } else {
        path.join(dir)
    };
    dir.canonicalize().ok()
}

/// The folder a terminal on `branch` works in. Outside git, or with no branch,
/// that's the project itself. Otherwise a worktree under `worktrees_dir`,
/// made on first use and reused after.
pub fn ensure(project: &Path, worktrees_dir: &Path, branch: &str) -> Result<PathBuf> {
    let branch = branch.trim();
    if branch.is_empty() || !is_git(project) {
        return Ok(project.to_path_buf());
    }
    if git(project, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_err() {
        bail!(
            "This repo has no commits yet, so terminals can't get their own worktree. Make a first commit."
        );
    }
    if git(project, &["check-ref-format", "--branch", branch]).is_err() {
        bail!("{branch:?} can't be a branch name");
    }
    let dir = worktrees_dir.join(slug(branch));
    if dir.join(".git").exists() {
        return Ok(dir);
    }
    std::fs::create_dir_all(worktrees_dir)?;
    // A worktree folder that was deleted by hand leaves a record behind; clear it first.
    let _ = git(project, &["worktree", "prune"]);
    let exists = git(
        project,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok();
    let target = dir.display().to_string();
    let result = if exists {
        git(project, &["worktree", "add", &target, branch])
    } else {
        git(project, &["worktree", "add", "-b", branch, &target])
    };
    if let Err(e) = result {
        bail!("couldn't make a worktree for {branch}: {e}");
    }
    Ok(dir)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A repo with one commit, in a temp folder.
    pub fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.email", "t@example.com"],
            &["config", "user.name", "T"],
            &["config", "commit.gpgsign", "false"],
        ] {
            git(p, args).unwrap();
        }
        std::fs::write(p.join("README.md"), "hi\n").unwrap();
        git(p, &["add", "."]).unwrap();
        git(p, &["commit", "-q", "-m", "first"]).unwrap();
        dir
    }

    #[test]
    fn makes_and_reuses_a_worktree() {
        let repo = repo();
        let trees = tempfile::tempdir().unwrap();
        let project = repo.path().canonicalize().unwrap();
        let a = ensure(&project, trees.path(), "LIN-92/totals").unwrap();
        assert_eq!(a, trees.path().join("lin-92-totals"));
        assert_eq!(
            git(&a, &["branch", "--show-current"]).unwrap(),
            "LIN-92/totals"
        );
        assert_eq!(ensure(&project, trees.path(), "LIN-92/totals").unwrap(), a);
        assert_eq!(common_dir(&a), common_dir(&project));
        assert_eq!(ensure(&project, trees.path(), " ").unwrap(), project);
    }

    #[test]
    fn an_existing_branch_and_plain_folders() {
        let repo = repo();
        git(repo.path(), &["branch", "old"]).unwrap();
        let trees = tempfile::tempdir().unwrap();
        let dir = ensure(repo.path(), trees.path(), "old").unwrap();
        assert_eq!(git(&dir, &["branch", "--show-current"]).unwrap(), "old");

        let plain = tempfile::tempdir().unwrap();
        assert!(!is_git(plain.path()));
        assert_eq!(
            ensure(plain.path(), trees.path(), "x").unwrap(),
            plain.path()
        );
    }

    #[test]
    fn a_repo_with_no_commits() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]).unwrap();
        let trees = tempfile::tempdir().unwrap();
        let err = ensure(dir.path(), trees.path(), "x").unwrap_err();
        assert!(err.to_string().contains("no commits yet"), "{err}");
    }
}
