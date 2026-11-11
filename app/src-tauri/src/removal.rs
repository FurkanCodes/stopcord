//! Taking a factory out of Sawtooth, as far as the person asks: what it left in the project
//! (worktrees and branches), its keys, and its own files. The project folder and its commits stay.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::factories::Factory;
use crate::worktree::git;

/// A worktree the factory made for a terminal.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub dir: String,
    pub branch: String,
    /// It has changes nobody committed.
    pub dirty: bool,
}

/// A branch the factory made in the project.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub name: String,
    /// Everything on it is already in what the project has checked out, so deleting it loses nothing.
    pub merged: bool,
}

/// Everything removing a factory could take with it, so a person can choose.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub worktrees: Vec<Worktree>,
    /// Throwaway copies stages worked in.
    pub copies: usize,
    pub branches: Vec<Branch>,
    /// Keys kept for its sources, notifications and sandboxes. Names only.
    pub keys: Vec<String>,
    /// Copies of it kept from an earlier remove.
    pub kept: usize,
}

/// What the person chose to take with it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Choices {
    pub worktrees: bool,
    pub branches: Vec<String>,
    pub keys: bool,
    /// Keep the factory's files under `removed/`, as a plain remove does.
    pub keep_files: bool,
}

fn key_prefixes(id: &str) -> [String; 3] {
    [format!("source.{id}."), format!("report.{id}."), format!("vm.{id}.")]
}

/// The factory's keys, by name.
pub fn keys(home: &Path, id: &str) -> Vec<String> {
    let prefixes = key_prefixes(id);
    let Ok(entries) = std::fs::read_dir(home.join("secrets")) else { return Vec::new() };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| prefixes.iter().any(|p| n.starts_with(p.as_str())))
        .collect();
    out.sort();
    out
}

/// Copies kept from earlier removes of a factory with this id.
fn kept(home: &Path, id: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(home.join("removed")) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.file_name().to_str().and_then(|n| n.strip_prefix(&format!("{id}-"))).is_some_and(|stamp| stamp.chars().all(|c| c.is_ascii_digit())))
        .map(|e| e.path())
        .collect()
}

/// The worktrees under `dir`, each on its branch.
fn worktrees(dir: &Path) -> Vec<Worktree> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Worktree> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(".git").is_file())
        .map(|p| Worktree {
            branch: git(&p, &["branch", "--show-current"]).unwrap_or_default(),
            dirty: git(&p, &["status", "--porcelain"]).map(|s| !s.is_empty()).unwrap_or(false),
            dir: p.display().to_string(),
        })
        .collect();
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out
}

/// What removing `factory` could take, with its worktrees under `trees`.
pub fn preview(home: &Path, factory: &Factory, trees: &Path) -> Preview {
    let project = Path::new(&factory.path);
    let worktrees = worktrees(trees);
    let copies = std::fs::read_dir(trees.join(".copies")).map(|e| e.flatten().count()).unwrap_or(0);
    // The branches it made: its worktrees' and its tickets'. Never the one the project is on.
    let current = git(project, &["branch", "--show-current"]).unwrap_or_default();
    let mut names: Vec<String> = worktrees.iter().map(|w| w.branch.clone()).chain(factory.work.iter().map(|w| w.branch.clone())).filter(|b| !b.is_empty() && *b != current).collect();
    names.sort();
    names.dedup();
    let branches = names
        .into_iter()
        .filter(|b| git(project, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{b}")]).is_ok())
        .map(|name| Branch { merged: git(project, &["merge-base", "--is-ancestor", &name, "HEAD"]).is_ok(), name })
        .collect();
    Preview { worktrees, copies, branches, keys: keys(home, &factory.id), kept: kept(home, &factory.id).len() }
}

/// Clears what the person chose, after the factory itself is out. Says what couldn't be done;
/// the rest still happens.
pub fn clear(home: &Path, factory: &Factory, trees: &Path, moved_to: &Path, choices: &Choices) -> Vec<String> {
    let project = Path::new(&factory.path);
    let mut problems = Vec::new();
    if choices.worktrees {
        for w in worktrees(trees) {
            if let Err(e) = git(project, &["worktree", "remove", "--force", &w.dir]) {
                problems.push(format!("the worktree for {} stayed: {e}", w.branch));
            }
        }
        if trees.exists() {
            if let Err(e) = std::fs::remove_dir_all(trees) {
                problems.push(format!("{} stayed: {e}", trees.display()));
            }
        }
        let _ = git(project, &["worktree", "prune"]);
    }
    for b in &choices.branches {
        if let Err(e) = git(project, &["branch", "-D", b]) {
            problems.push(format!("the branch {b} stayed: {e}"));
        }
    }
    if choices.keys {
        for name in keys(home, &factory.id) {
            let _ = std::fs::remove_file(home.join("secrets").join(name));
        }
    }
    if !choices.keep_files {
        let mut gone = kept(home, &factory.id);
        if !gone.iter().any(|d| d == moved_to) {
            gone.push(moved_to.to_path_buf());
        }
        for dir in gone {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                problems.push(format!("{} stayed: {e}", dir.display()));
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::WorkItem;

    #[test]
    fn removing_everything_takes_its_worktrees_branches_keys_and_files_and_leaves_the_rest() {
        let project = crate::worktree::tests::repo();
        let home = tempfile::tempdir().unwrap();
        let trees = home.path().join("worktrees/f");
        let done = crate::worktree::ensure(project.path(), &trees, "t-1").unwrap();
        let open = crate::worktree::ensure(project.path(), &trees, "t-2").unwrap();
        std::fs::write(open.join("wip.txt"), "x").unwrap();
        git(&open, &["add", "-A"]).unwrap();
        git(&open, &["commit", "-qm", "wip"]).unwrap();
        std::fs::write(open.join("loose.txt"), "x").unwrap();
        let _ = done;
        let secrets = home.path().join("secrets");
        std::fs::create_dir_all(&secrets).unwrap();
        for n in ["source.f.gh.TOKEN", "report.f.slack.HOOK", "source.fx.gh.TOKEN", "connection.sentry.TOKEN"] {
            std::fs::write(secrets.join(n), "v").unwrap();
        }
        let mut factory = Factory { id: "f".into(), path: project.path().display().to_string(), ..Factory::default() };
        factory.work.push(WorkItem { branch: "t-3".into(), ..WorkItem::default() });

        let p = preview(home.path(), &factory, &trees);
        assert_eq!(p.worktrees.iter().map(|w| (w.branch.as_str(), w.dirty)).collect::<Vec<_>>(), [("t-1", false), ("t-2", true)]);
        assert_eq!(p.branches, [Branch { name: "t-1".into(), merged: true }, Branch { name: "t-2".into(), merged: false }], "t-3 was never made");
        assert_eq!(p.keys, ["report.f.slack.HOOK", "source.f.gh.TOKEN"], "only its own keys");

        let moved = home.path().join("removed/f-123");
        std::fs::create_dir_all(&moved).unwrap();
        let choices = Choices { worktrees: true, branches: vec!["t-1".into(), "t-2".into()], keys: true, keep_files: false };
        let problems = clear(home.path(), &factory, &trees, &moved, &choices);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(!trees.exists() && !moved.exists());
        assert!(git(project.path(), &["worktree", "list"]).unwrap().lines().count() == 1);
        assert!(git(project.path(), &["branch", "--list", "t-*"]).unwrap().is_empty());
        let left: Vec<String> = std::fs::read_dir(&secrets).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
        assert_eq!(left.len(), 2, "another factory's key and the shared sign-in stay: {left:?}");
    }

    #[test]
    fn a_plain_remove_keeps_everything_but_the_factory() {
        let project = crate::worktree::tests::repo();
        let home = tempfile::tempdir().unwrap();
        let trees = home.path().join("worktrees/f");
        crate::worktree::ensure(project.path(), &trees, "t-1").unwrap();
        let factory = Factory { id: "f".into(), path: project.path().display().to_string(), ..Factory::default() };
        let moved = home.path().join("removed/f-1");
        std::fs::create_dir_all(&moved).unwrap();
        assert!(clear(home.path(), &factory, &trees, &moved, &Choices { keep_files: true, ..Choices::default() }).is_empty());
        assert!(trees.exists() && moved.exists());
        assert!(!git(project.path(), &["branch", "--list", "t-1"]).unwrap().is_empty());
    }
}
