//! Git worktrees, so each terminal can work on its own branch in its own folder.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, bail};

use crate::factories::slug;

/// Git as the engine runs it, outside any sandbox, often in a folder an agent could write to. The
/// repo's own hooks and fsmonitor would run code the agent may have planted there, and a replace ref
/// it made would change what a commit holds, so none of them apply to Stopcord's own git.
pub fn git_command(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C")
        .arg(dir)
        .args(["-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false"])
        .env("GIT_NO_REPLACE_OBJECTS", "1");
    c
}

/// The repo's own value for a config key, which [`git_command`]'s overrides would hide. Reading config
/// runs no hooks.
pub fn config(dir: &Path, key: &str) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(["config", key]).output().ok()?;
    let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !value.is_empty()).then_some(value)
}

/// Runs git in `dir` and returns its trimmed output, or its error message.
pub fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = git_command(dir).args(args).output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        // Some failures, such as a merge that conflicts, are told on stdout only.
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        bail!("{}", if err.is_empty() { String::from_utf8_lossy(&out.stdout).trim().to_string() } else { err })
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

/// Fails unless `dir` has `branch` checked out. An agent can move its worktree's HEAD, and a commit the
/// engine makes there would then land on whatever branch the agent pointed it at.
fn on_branch(dir: &Path, branch: &str) -> Result<()> {
    let head = git(dir, &["symbolic-ref", "-q", "HEAD"]).unwrap_or_default();
    if head != format!("refs/heads/{branch}") {
        let now = head.strip_prefix("refs/heads/").map_or_else(|| "no branch".to_string(), |b| format!("branch {b}"));
        bail!("{} is on {now}, not {branch}, so Stopcord didn't commit there", dir.display());
    }
    Ok(())
}

/// Commits just these files, skipping hooks because the engine is doing its own bookkeeping.
/// Returns whether there was anything to commit.
pub fn commit_files(dir: &Path, branch: &str, files: &[String], message: &str) -> Result<bool> {
    on_branch(dir, branch)?;
    let names: Vec<&str> = files.iter().map(String::as_str).collect();
    git(dir, &[&["add", "--"], names.as_slice()].concat())?;
    if git(dir, &[&["diff", "--cached", "--quiet", "--"], names.as_slice()].concat()).is_ok() {
        return Ok(false);
    }
    git(dir, &[&["commit", "--quiet", "--no-verify", "-m", message, "--"], names.as_slice()].concat())?;
    Ok(true)
}

/// Takes files the line only kept for its own stages, such as PLAN.md, back out of a branch before
/// it's merged, so they never reach `target`: put back as `target` has them, or removed. Commits
/// that on the branch, and says which files it touched.
pub fn leave_out(dir: &Path, branch: &str, target: &str, files: &[String], message: &str) -> Result<Vec<String>> {
    on_branch(dir, branch)?;
    let mut touched = Vec::new();
    for file in files {
        if git(dir, &["cat-file", "-e", &format!("HEAD:{file}")]).is_err() {
            continue;
        }
        let there = git(dir, &["cat-file", "-e", &format!("{target}:{file}")]).is_ok();
        if there && git(dir, &["diff", "--quiet", target, "HEAD", "--", file]).is_ok() {
            continue;
        }
        if there {
            git(dir, &["checkout", target, "--", file])?;
        } else {
            git(dir, &["rm", "-q", "--", file])?;
        }
        touched.push(file.clone());
    }
    if !touched.is_empty() {
        git(dir, &["commit", "-q", "--no-verify", "-m", message])?;
    }
    Ok(touched)
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
    fn the_plan_stays_out_of_a_merge_and_the_work_stays_in() {
        let project = repo();
        let holder = tempfile::tempdir().unwrap();
        let wt = ensure(project.path(), &holder.path().join("trees"), "t-1").unwrap();
        std::fs::write(wt.join("PLAN.md"), "the plan").unwrap();
        std::fs::write(wt.join("work.txt"), "the work").unwrap();
        git(&wt, &["add", "-A"]).unwrap();
        git(&wt, &["commit", "-qm", "plan and work"]).unwrap();
        let main = git(project.path(), &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(leave_out(&wt, "t-1", &main, &["PLAN.md".into()], "leave it out").unwrap(), ["PLAN.md"]);
        assert!(!wt.join("PLAN.md").exists() && wt.join("work.txt").exists());
        assert!(leave_out(&wt, "t-1", &main, &["PLAN.md".into()], "again").unwrap().is_empty(), "nothing left to take out");

        // When the project already has one, the branch gets the project's copy back.
        std::fs::write(project.path().join("PLAN.md"), "an old plan").unwrap();
        git(project.path(), &["add", "-A"]).unwrap();
        git(project.path(), &["commit", "-qm", "an old plan"]).unwrap();
        let main = git(project.path(), &["rev-parse", "HEAD"]).unwrap();
        std::fs::write(wt.join("PLAN.md"), "a new plan").unwrap();
        git(&wt, &["add", "-A"]).unwrap();
        git(&wt, &["commit", "-qm", "new plan"]).unwrap();
        leave_out(&wt, "t-1", &main, &["PLAN.md".into()], "leave it out").unwrap();
        assert_eq!(std::fs::read_to_string(wt.join("PLAN.md")).unwrap(), "an old plan");
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
        assert!(commit_files(&real, "t-1", &files, "plan for T-1").unwrap());
        assert_eq!(git(&real, &["log", "-1", "--format=%s", "--name-only"]).unwrap().lines().filter(|l| !l.is_empty()).collect::<Vec<_>>(), ["plan for T-1", "PLAN.md"]);
        assert!(!commit_files(&real, "t-1", &files, "again").unwrap(), "nothing new, so no empty commit");
    }

    /// Git as an agent would run it: plain, with the repo's own hooks and replace refs in force.
    fn plain_git(dir: &Path, args: &[&str]) -> std::process::Output {
        Command::new("git").arg("-C").arg(dir).args(args).output().unwrap()
    }

    #[test]
    fn the_engines_git_runs_no_hook_an_agent_planted() {
        let project = repo();
        let holder = tempfile::tempdir().unwrap();
        let marker = holder.path().join("hook-ran");
        git(project.path(), &["config", "core.hooksPath", ".husky"]).unwrap();
        let wt = ensure(project.path(), &holder.path().join("trees"), "t-1").unwrap();
        // The agent plants hooks on its branch, where a relative hooks path finds them when git runs in its folder.
        std::fs::create_dir_all(wt.join(".husky")).unwrap();
        for hook in ["post-commit", "post-checkout", "post-merge", "pre-commit"] {
            let path = wt.join(".husky").join(hook);
            std::fs::write(&path, format!("#!/bin/sh\necho {hook} >> '{}'\n", marker.display())).unwrap();
            std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        }
        std::fs::write(wt.join("PLAN.md"), "the plan").unwrap();
        git(&wt, &["add", "-A"]).unwrap();
        git(&wt, &["commit", "-qm", "plan and hooks"]).unwrap();
        assert!(!marker.exists(), "committing through the engine's git ran {:?}", std::fs::read_to_string(&marker));

        let main = git(project.path(), &["rev-parse", "HEAD"]).unwrap();
        leave_out(&wt, "t-1", &main, &["PLAN.md".into()], "leave it out").unwrap();
        git(project.path(), &["merge", "--no-ff", "--no-edit", "t-1"]).unwrap();
        assert!(!marker.exists(), "the engine ran a planted hook: {:?}", std::fs::read_to_string(&marker));

        // The hooks are live: git run the agent's way in its folder does run them.
        assert!(plain_git(&wt, &["commit", "-q", "--allow-empty", "-m", "x"]).status.success());
        assert!(marker.exists(), "control: the planted post-commit should run for plain git");
    }

    #[test]
    fn the_engine_wont_commit_where_an_agent_moved_head() {
        let project = repo();
        let holder = tempfile::tempdir().unwrap();
        git(project.path(), &["branch", "other"]).unwrap();
        let other = git(project.path(), &["rev-parse", "other"]).unwrap();
        let wt = ensure(project.path(), &holder.path().join("trees"), "t-1").unwrap();
        std::fs::write(wt.join("PLAN.md"), "the plan").unwrap();
        git(&wt, &["add", "-A"]).unwrap();
        git(&wt, &["commit", "-qm", "plan"]).unwrap();
        // The agent points its worktree at another branch, keeping the plan as a change there.
        let admin = PathBuf::from(git(&wt, &["rev-parse", "--absolute-git-dir"]).unwrap());
        std::fs::write(admin.join("HEAD"), "ref: refs/heads/other\n").unwrap();
        let main = git(project.path(), &["rev-parse", "main"]).unwrap();

        let err = leave_out(&wt, "t-1", &main, &["PLAN.md".into()], "leave it out").unwrap_err();
        assert!(format!("{err:#}").contains("not t-1"), "{err:#}");
        let err = commit_files(&wt, "t-1", &["PLAN.md".into()], "plan").unwrap_err();
        assert!(format!("{err:#}").contains("on branch other"), "{err:#}");
        assert_eq!(git(project.path(), &["rev-parse", "other"]).unwrap(), other, "other branch untouched");
    }

    #[test]
    fn the_engines_git_ignores_replace_refs() {
        let project = repo();
        let p = project.path();
        let first = git(p, &["rev-parse", "HEAD"]).unwrap();
        git(p, &["commit", "-q", "--allow-empty", "-m", "something else"]).unwrap();
        let other = git(p, &["rev-parse", "HEAD"]).unwrap();
        git(p, &["reset", "-q", "--hard", &first]).unwrap();
        // An agent can write refs, so it could make `first` read as a different commit.
        assert!(plain_git(p, &["replace", &first, &other]).status.success());
        let plain = String::from_utf8_lossy(&plain_git(p, &["log", "-1", "--format=%s", &first]).stdout).trim().to_string();
        assert_eq!(plain, "something else", "control: plain git follows the replace ref");
        assert_eq!(git(p, &["log", "-1", "--format=%s", &first]).unwrap(), "first");
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
