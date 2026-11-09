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
    // Different branches can make the same folder name, so a folder only counts if it's on this branch.
    let base = slug(branch);
    let mut n = 1;
    let dir = loop {
        let dir = if n == 1 {
            worktrees_dir.join(&base)
        } else {
            worktrees_dir.join(format!("{base}-{n}"))
        };
        if !dir.join(".git").exists() {
            break dir;
        }
        if git(&dir, &["branch", "--show-current"]).is_ok_and(|b| b == branch) {
            return Ok(dir);
        }
        n += 1;
    };
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

/// Makes `into` a standalone clone of `branch`, for a stage that mustn't touch the real work.
/// Being its own repository, nothing done in it can move the real branch, and with no remote
/// it has nowhere to push. Ignored files such as dependencies come across from `source`.
pub fn throwaway_copy(project: &Path, source: &Path, branch: &str, into: &Path) -> Result<()> {
    let _ = std::fs::remove_dir_all(into);
    if let Some(parent) = into.parent() {
        std::fs::create_dir_all(parent)?;
    }
    git(project, &["clone", "--quiet", "--shared", "--no-checkout", ".", &into.display().to_string()])?;
    git(into, &["fetch", "--quiet", "--update-head-ok", "origin", "+refs/heads/*:refs/heads/*"])?;
    git(into, &["checkout", "--quiet", branch])?;
    git(into, &["remote", "remove", "origin"])?;
    crate::gate::copy_ignored(source, into)
}

/// Brings a copy up to date with the real work for a stage's next round, dropping whatever
/// the last round left. The folder stays, because a terminal is open in it.
pub fn refresh_copy(copy: &Path, source: &Path, branch: &str) -> Result<()> {
    git(copy, &["fetch", "--quiet", &source.display().to_string(), "HEAD"])?;
    git(copy, &["checkout", "--quiet", "--force", "-B", branch, "FETCH_HEAD"])?;
    git(copy, &["clean", "-fdq"])?;
    Ok(())
}

/// Copies the kept files from a copy into the real worktree, and says which came back.
pub fn take_kept(copy: &Path, real: &Path, keep: &[String]) -> Result<Vec<String>> {
    let mut taken = Vec::new();
    for name in keep {
        let from = copy.join(name);
        // A link could point anywhere, so only a plain file comes back.
        if !std::fs::symlink_metadata(&from).is_ok_and(|m| m.is_file()) {
            continue;
        }
        let to = real.join(name);
        if std::fs::symlink_metadata(&to).is_ok_and(|m| m.file_type().is_symlink()) {
            bail!("{name} is a link in the worktree, so it wasn't overwritten");
        }
        std::fs::copy(&from, &to)?;
        taken.push(name.clone());
    }
    Ok(taken)
}

/// Commits just these files, skipping hooks because the engine is doing its own bookkeeping.
/// Returns whether there was anything to commit.
pub fn commit_files(dir: &Path, files: &[String], message: &str) -> Result<bool> {
    let names: Vec<&str> = files.iter().map(String::as_str).collect();
    git(dir, &[&["add", "--"], names.as_slice()].concat())?;
    if git(dir, &[&["diff", "--cached", "--quiet", "--"], names.as_slice()].concat()).is_ok() {
        return Ok(false);
    }
    git(dir, &[&["commit", "--quiet", "--no-verify", "-m", message, "--"], names.as_slice()].concat())?;
    Ok(true)
}

/// Removes a finished worktree, but only when nothing can be lost: it's still on
/// `branch`, it has no uncommitted or untracked files, and the branch is merged
/// into what the project has checked out. The branch itself stays. Returns
/// whether it was removed.
pub fn remove_if_merged(project: &Path, dir: &Path, branch: &str) -> Result<bool> {
    if !dir.join(".git").exists() || git(dir, &["branch", "--show-current"])? != branch {
        return Ok(false);
    }
    if !git(dir, &["status", "--porcelain"])?.is_empty() {
        return Ok(false);
    }
    if git(project, &["merge-base", "--is-ancestor", branch, "HEAD"]).is_err() {
        return Ok(false);
    }
    git(project, &["worktree", "remove", &dir.display().to_string()])?;
    Ok(true)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    fn copy_setup() -> (tempfile::TempDir, tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let project = repo();
        let holder = tempfile::tempdir().unwrap();
        let real = ensure(project.path(), &holder.path().join("trees"), "t-1").unwrap();
        let copy = holder.path().join("copies/plan");
        throwaway_copy(project.path(), &real, "t-1", &copy).unwrap();
        (project, holder, real, copy)
    }

    #[test]
    fn nothing_done_in_a_copy_reaches_the_real_branch() {
        let (project, _holder, real, copy) = copy_setup();
        let tip = git(&real, &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(git(&copy, &["branch", "--show-current"]).unwrap(), "t-1");
        assert_eq!(git(&copy, &["rev-parse", "main"]).unwrap(), tip, "the base must be there to compare with");
        std::fs::write(copy.join("evil.txt"), "x").unwrap();
        git(&copy, &["add", "-A"]).unwrap();
        git(&copy, &["commit", "-qm", "too far"]).unwrap();
        git(&copy, &["branch", "-f", "main", "HEAD"]).unwrap();
        assert!(git(&copy, &["remote"]).unwrap().is_empty(), "a copy has nowhere to push");
        assert_eq!(git(&real, &["rev-parse", "HEAD"]).unwrap(), tip);
        assert_eq!(git(project.path(), &["rev-parse", "main"]).unwrap(), tip);
        assert!(!real.join("evil.txt").exists());
    }

    #[test]
    fn refreshing_a_copy_takes_the_new_work_and_drops_the_old_mess() {
        let (_project, _holder, real, copy) = copy_setup();
        std::fs::write(copy.join("junk.txt"), "x").unwrap();
        git(&copy, &["add", "-A"]).unwrap();
        git(&copy, &["commit", "-qm", "junk"]).unwrap();
        std::fs::write(copy.join("untracked.txt"), "x").unwrap();
        git(&copy, &["checkout", "-qb", "elsewhere"]).unwrap();
        std::fs::write(real.join("new.txt"), "x").unwrap();
        git(&real, &["add", "-A"]).unwrap();
        git(&real, &["commit", "-qm", "new work"]).unwrap();
        refresh_copy(&copy, &real, "t-1").unwrap();
        assert_eq!(git(&copy, &["rev-parse", "HEAD"]).unwrap(), git(&real, &["rev-parse", "HEAD"]).unwrap());
        assert_eq!(git(&copy, &["branch", "--show-current"]).unwrap(), "t-1");
        assert!(copy.join("new.txt").exists());
        assert!(!copy.join("junk.txt").exists());
        assert!(!copy.join("untracked.txt").exists());
    }

    #[test]
    fn only_kept_plain_files_come_back() {
        let (_project, _holder, real, copy) = copy_setup();
        std::fs::write(copy.join("PLAN.md"), "the plan").unwrap();
        std::fs::write(copy.join("other.txt"), "x").unwrap();
        std::os::unix::fs::symlink("/etc/hosts", copy.join("LINK.md")).unwrap();
        let keep = ["PLAN.md".to_string(), "LINK.md".to_string(), "MISSING.md".to_string()];
        assert_eq!(take_kept(&copy, &real, &keep).unwrap(), ["PLAN.md"]);
        assert_eq!(std::fs::read_to_string(real.join("PLAN.md")).unwrap(), "the plan");
        assert!(!real.join("other.txt").exists());
        assert!(!real.join("LINK.md").exists());
    }

    #[test]
    fn a_kept_file_is_never_written_through_a_link() {
        let (_project, holder, real, copy) = copy_setup();
        let outside = holder.path().join("outside.txt");
        std::fs::write(&outside, "safe").unwrap();
        std::os::unix::fs::symlink(&outside, real.join("PLAN.md")).unwrap();
        std::fs::write(copy.join("PLAN.md"), "overwrite").unwrap();
        assert!(take_kept(&copy, &real, &["PLAN.md".to_string()]).is_err());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "safe");
    }

    #[test]
    fn committing_kept_files_leaves_other_changes_alone() {
        let (_project, _holder, real, _copy) = copy_setup();
        std::fs::write(real.join("PLAN.md"), "the plan").unwrap();
        std::fs::write(real.join("other.txt"), "x").unwrap();
        git(&real, &["add", "other.txt"]).unwrap();
        let files = ["PLAN.md".to_string()];
        assert!(commit_files(&real, &files, "plan for T-1").unwrap());
        assert_eq!(git(&real, &["log", "-1", "--format=%s", "--name-only"]).unwrap().lines().filter(|l| !l.is_empty()).collect::<Vec<_>>(), ["plan for T-1", "PLAN.md"]);
        assert!(!commit_files(&real, &files, "again").unwrap(), "nothing new, so no empty commit");
    }

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
    fn branches_with_the_same_slug_get_their_own_folders() {
        let repo = repo();
        let trees = tempfile::tempdir().unwrap();
        let a = ensure(repo.path(), trees.path(), "feature/foo").unwrap();
        let b = ensure(repo.path(), trees.path(), "feature-foo").unwrap();
        assert_ne!(a, b);
        assert_eq!(git(&a, &["branch", "--show-current"]).unwrap(), "feature/foo");
        assert_eq!(git(&b, &["branch", "--show-current"]).unwrap(), "feature-foo");
        assert_eq!(ensure(repo.path(), trees.path(), "feature-foo").unwrap(), b);
        assert_eq!(ensure(repo.path(), trees.path(), "feature/foo").unwrap(), a);
    }

    fn commit_in(dir: &Path, file: &str) {
        std::fs::write(dir.join(file), "x").unwrap();
        git(dir, &["add", "."]).unwrap();
        git(dir, &["commit", "-q", "-m", file]).unwrap();
    }

    #[test]
    fn a_merged_clean_worktree_is_removed_but_its_branch_stays() {
        let repo = repo();
        let trees = tempfile::tempdir().unwrap();
        let dir = ensure(repo.path(), trees.path(), "done").unwrap();
        commit_in(&dir, "a.txt");
        git(repo.path(), &["merge", "-q", "done"]).unwrap();
        assert!(remove_if_merged(repo.path(), &dir, "done").unwrap());
        assert!(!dir.exists());
        assert!(git(repo.path(), &["show-ref", "--verify", "refs/heads/done"]).is_ok());
    }

    #[test]
    fn unmerged_work_keeps_its_worktree() {
        let repo = repo();
        let trees = tempfile::tempdir().unwrap();
        let dir = ensure(repo.path(), trees.path(), "wip").unwrap();
        commit_in(&dir, "a.txt");
        assert!(!remove_if_merged(repo.path(), &dir, "wip").unwrap());
        assert!(dir.join("a.txt").exists());
    }

    #[test]
    fn uncommitted_work_keeps_its_worktree_even_when_merged() {
        let repo = repo();
        let trees = tempfile::tempdir().unwrap();
        let dir = ensure(repo.path(), trees.path(), "dirty").unwrap();
        std::fs::write(dir.join("notes.txt"), "unsaved").unwrap();
        assert!(!remove_if_merged(repo.path(), &dir, "dirty").unwrap());
        assert!(dir.join("notes.txt").exists());
    }

    #[test]
    fn a_worktree_moved_to_another_branch_is_left_alone() {
        let repo = repo();
        let trees = tempfile::tempdir().unwrap();
        let dir = ensure(repo.path(), trees.path(), "one").unwrap();
        git(&dir, &["checkout", "-q", "-b", "two"]).unwrap();
        assert!(!remove_if_merged(repo.path(), &dir, "one").unwrap());
        assert!(dir.exists());
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
