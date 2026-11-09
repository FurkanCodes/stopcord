//! Getting a commit ready for a gate.
//!
//! A gate tests what the agent committed, in a folder the agent can't touch.
//! An agent can weaken a test and pass honestly, so files the line protects come
//! from the base branch instead of from the agent's commit. A test the agent
//! changed is listed at handoff, whatever the result.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::worktree::git;

/// Matches a path against a pattern: `*` stays within a folder, `**` crosses them.
pub fn matches(pattern: &str, path: &str) -> bool {
    fn seg(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => seg(&p[1..], s) || (!s.is_empty() && seg(p, &s[1..])),
            (Some(a), Some(b)) if a == b => seg(&p[1..], &s[1..]),
            _ => false,
        }
    }
    fn walk(p: &[&str], s: &[&str]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(&"**"), _) => walk(&p[1..], s) || (!s.is_empty() && walk(p, &s[1..])),
            (Some(a), Some(b)) if seg(a.as_bytes(), b.as_bytes()) => walk(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let p: Vec<&str> = pattern.trim_start_matches("./").split('/').collect();
    let s: Vec<&str> = path.split('/').collect();
    walk(&p, &s)
}

fn matching<'a>(patterns: &[String], files: impl Iterator<Item = &'a str>) -> Vec<String> {
    files
        .filter(|f| patterns.iter().any(|p| matches(p, f)))
        .map(String::from)
        .collect()
}

/// Protected files the agent changed or deleted compared with `base`.
pub fn changed_protected(worktree: &Path, base: &str, patterns: &[String]) -> Result<Vec<String>> {
    if patterns.is_empty() {
        return Ok(Vec::new());
    }
    let names = git(worktree, &["diff", "--name-only", "--no-renames", base, "HEAD"])?;
    Ok(matching(patterns, names.lines()))
}

/// Copies what git ignores, such as `node_modules`, so checks don't have to reinstall.
/// On APFS and reflink filesystems this costs almost nothing.
fn copy_ignored(from: &Path, to: &Path) -> Result<()> {
    let list = git(from, &["ls-files", "--others", "--ignored", "--exclude-standard", "--directory"])?;
    for entry in list.lines() {
        let entry = entry.trim_end_matches('/');
        if entry.is_empty() || entry == ".git" {
            continue;
        }
        let target = to.join(entry);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut cp = Command::new("cp");
        if cfg!(target_os = "macos") {
            cp.args(["-cRP"]);
        } else {
            cp.args(["-RP", "--reflink=auto"]);
        }
        // A file that can't be copied only costs the check a slower start, or fails it visibly.
        let _ = cp.arg(from.join(entry)).arg(&target).output();
    }
    Ok(())
}

/// A clean copy of the agent's commit for checks to run in, removed when dropped.
pub struct Snapshot {
    pub dir: PathBuf,
    project: PathBuf,
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = git(&self.project, &["worktree", "remove", "--force", &self.dir.display().to_string()]);
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = git(&self.project, &["worktree", "prune"]);
    }
}

/// Builds the snapshot. Fails, with a sentence an agent can act on, when there's nothing to test.
pub fn snapshot(project: &Path, worktree: &Path, base: &str, protect: &[String], into: &Path) -> Result<Snapshot> {
    let dirty = git(worktree, &["status", "--porcelain", "--untracked-files=normal"])?;
    if !dirty.trim().is_empty() {
        let files: Vec<&str> = dirty.lines().take(10).collect();
        bail!("There are uncommitted changes, and a gate only tests commits. Commit them:\n{}", files.join("\n"));
    }
    let head = git(worktree, &["rev-parse", "HEAD"])?;
    if !base.is_empty() && git(worktree, &["rev-parse", base])? == head {
        bail!("Nothing has been committed since this work started, so there's nothing to test.");
    }
    if let Some(parent) = into.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = git(project, &["worktree", "remove", "--force", &into.display().to_string()]);
    let _ = std::fs::remove_dir_all(into);
    git(project, &["worktree", "add", "--quiet", "--detach", &into.display().to_string(), &head])
        .with_context(|| "couldn't make a copy of the commit to test")?;
    let snap = Snapshot {
        dir: into.to_path_buf(),
        project: project.to_path_buf(),
    };
    copy_ignored(worktree, into)?;
    if !protect.is_empty() && !base.is_empty() {
        let tracked = git(into, &["ls-tree", "-r", "--name-only", base])?;
        let files = matching(protect, tracked.lines());
        if !files.is_empty() {
            let mut args = vec!["checkout", "--quiet", base, "--"];
            args.extend(files.iter().map(String::as_str));
            git(into, &args).with_context(|| "couldn't put the protected files back")?;
        }
    }
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]).unwrap();
        git(p, &["config", "user.email", "t@t"]).unwrap();
        git(p, &["config", "user.name", "t"]).unwrap();
        std::fs::create_dir_all(p.join("tests")).unwrap();
        std::fs::write(p.join("tests/a.test"), "strict").unwrap();
        std::fs::write(p.join("tests/b.test"), "keep").unwrap();
        std::fs::write(p.join("lib.txt"), "v1").unwrap();
        std::fs::write(p.join(".gitignore"), "deps/\n").unwrap();
        git(p, &["add", "-A"]).unwrap();
        git(p, &["commit", "-qm", "base"]).unwrap();
        dir
    }

    /// A linked worktree on branch `w`, as an agent would have.
    fn agent(project: &Path, parent: &Path) -> PathBuf {
        let wt = parent.join("wt");
        git(project, &["worktree", "add", "-q", "-b", "w", &wt.display().to_string()]).unwrap();
        wt
    }

    fn commit(wt: &Path) {
        git(wt, &["add", "-A"]).unwrap();
        git(wt, &["commit", "-qm", "work"]).unwrap();
    }

    #[test]
    fn patterns() {
        assert!(matches("tests/**", "tests/a/b.rs"));
        assert!(matches("tests/**", "tests/a.rs"));
        assert!(matches("**/*.test.ts", "src/x/y.test.ts"));
        assert!(matches("**/*.test.ts", "y.test.ts"));
        assert!(matches("*.spec.js", "a.spec.js"));
        assert!(!matches("*.spec.js", "src/a.spec.js"));
        assert!(!matches("tests/**", "src/tests.rs"));
        assert!(matches("./tests/*", "tests/a"));
    }

    #[test]
    fn a_weakened_test_is_put_back_and_the_agents_code_stays() {
        let (r, out) = (repo(), tempfile::tempdir().unwrap());
        let base = git(r.path(), &["rev-parse", "HEAD"]).unwrap();
        let wt = agent(r.path(), out.path());
        std::fs::write(wt.join("tests/a.test"), "weakened").unwrap();
        std::fs::remove_file(wt.join("tests/b.test")).unwrap();
        std::fs::write(wt.join("tests/new.test"), "added").unwrap();
        std::fs::write(wt.join("lib.txt"), "v2").unwrap();
        commit(&wt);

        let patterns = vec!["tests/**".to_string()];
        let snap = snapshot(r.path(), &wt, &base, &patterns, &out.path().join("gate")).unwrap();
        let read = |n: &str| std::fs::read_to_string(snap.dir.join(n)).unwrap();
        assert_eq!(read("tests/a.test"), "strict");
        assert_eq!(read("tests/b.test"), "keep");
        assert_eq!(read("tests/new.test"), "added");
        assert_eq!(read("lib.txt"), "v2");
        // The agent's own folder is untouched.
        assert_eq!(std::fs::read_to_string(wt.join("tests/a.test")).unwrap(), "weakened");

        let mut changed = changed_protected(&wt, &base, &patterns).unwrap();
        changed.sort();
        assert_eq!(changed, ["tests/a.test", "tests/b.test", "tests/new.test"]);
    }

    #[test]
    fn what_git_ignores_comes_along() {
        let (r, out) = (repo(), tempfile::tempdir().unwrap());
        let base = git(r.path(), &["rev-parse", "HEAD"]).unwrap();
        let wt = agent(r.path(), out.path());
        std::fs::create_dir_all(wt.join("deps/x")).unwrap();
        std::fs::write(wt.join("deps/x/mod"), "installed").unwrap();
        std::fs::write(wt.join("lib.txt"), "v2").unwrap();
        commit(&wt);
        let snap = snapshot(r.path(), &wt, &base, &[], &out.path().join("gate")).unwrap();
        assert_eq!(std::fs::read_to_string(snap.dir.join("deps/x/mod")).unwrap(), "installed");
    }

    #[test]
    fn uncommitted_work_is_sent_back() {
        let (r, out) = (repo(), tempfile::tempdir().unwrap());
        let base = git(r.path(), &["rev-parse", "HEAD"]).unwrap();
        let wt = agent(r.path(), out.path());
        std::fs::write(wt.join("lib.txt"), "v2").unwrap();
        commit(&wt);
        std::fs::write(wt.join("lib.txt"), "v3").unwrap();
        let e = snapshot(r.path(), &wt, &base, &[], &out.path().join("gate")).err().unwrap();
        assert!(e.to_string().contains("uncommitted"), "{e}");
    }

    #[test]
    fn nothing_committed_is_sent_back() {
        let (r, out) = (repo(), tempfile::tempdir().unwrap());
        let base = git(r.path(), &["rev-parse", "HEAD"]).unwrap();
        let wt = agent(r.path(), out.path());
        let e = snapshot(r.path(), &wt, &base, &[], &out.path().join("gate")).err().unwrap();
        assert!(e.to_string().contains("Nothing has been committed"), "{e}");
    }

    #[test]
    fn the_snapshot_cleans_up_after_itself() {
        let (r, out) = (repo(), tempfile::tempdir().unwrap());
        let base = git(r.path(), &["rev-parse", "HEAD"]).unwrap();
        let wt = agent(r.path(), out.path());
        std::fs::write(wt.join("lib.txt"), "v2").unwrap();
        commit(&wt);
        let dir = out.path().join("gate");
        drop(snapshot(r.path(), &wt, &base, &[], &dir).unwrap());
        assert!(!dir.exists());
        assert!(!git(r.path(), &["worktree", "list"]).unwrap().contains("/gate"));
    }
}
