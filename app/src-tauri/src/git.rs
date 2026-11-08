//! Reads which branch a folder is on, straight from `.git`, without running git,
//! and what a terminal's checkout changed, with git.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

/// The branch checked out in the repository that holds `dir`, or the short
/// commit id when HEAD is detached. `None` outside a repository.
pub fn branch(dir: &Path) -> Option<String> {
    let gitdir = dir.ancestors().find_map(git_dir)?;
    let head = std::fs::read_to_string(gitdir.join("HEAD")).ok()?;
    let head = head.trim();
    Some(match head.strip_prefix("ref: refs/heads/") {
        Some(branch) => branch.to_string(),
        None => head.chars().take(7).collect(),
    })
}

/// `.git` is a folder in a normal checkout, and a file naming the real one in a worktree.
fn git_dir(dir: &Path) -> Option<PathBuf> {
    let git = dir.join(".git");
    if git.is_dir() {
        return Some(git);
    }
    let text = std::fs::read_to_string(&git).ok()?;
    let target = text.trim().strip_prefix("gitdir:")?.trim();
    Some(dir.join(target))
}

/// One file a checkout changed, with lines added and taken out. Binary files count 0.
#[derive(Debug, Serialize, PartialEq)]
pub struct FileChange {
    pub path: String,
    pub added: u32,
    pub removed: u32,
}

/// What a checkout did since it left `base`: its commits, and every changed file,
/// committed or not, untracked files included.
#[derive(Debug, Serialize, PartialEq)]
pub struct Changes {
    pub commits: u32,
    pub files: Vec<FileChange>,
}

fn run(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The changes in `dir` since it split from the checkout in `base`. When `dir` is the
/// base itself, that's only what isn't committed yet. `None` outside git.
pub fn changes(dir: &Path, base: &Path) -> Option<Changes> {
    let head = run(dir, &["rev-parse", "HEAD"])?;
    let head = head.trim();
    let same = dir.canonicalize().ok() == base.canonicalize().ok();
    let from = if same {
        head.to_string()
    } else {
        let base_head = run(base, &["rev-parse", "HEAD"]).unwrap_or_else(|| head.to_string());
        run(dir, &["merge-base", head, base_head.trim()])
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| head.to_string())
    };
    let commits = if from == head {
        0
    } else {
        run(dir, &["rev-list", "--count", &format!("{from}..{head}")])
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    };

    let mut files = Vec::new();
    for line in run(dir, &["diff", "--numstat", "--no-renames", &from])
        .unwrap_or_default()
        .lines()
    {
        let mut cols = line.splitn(3, '\t');
        let (Some(a), Some(r), Some(path)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        files.push(FileChange {
            path: path.to_string(),
            added: a.parse().unwrap_or(0),
            removed: r.parse().unwrap_or(0),
        });
    }
    for path in run(dir, &["ls-files", "--others", "--exclude-standard"])
        .unwrap_or_default()
        .lines()
        .filter(|p| !p.is_empty())
    {
        let full = dir.join(path);
        // A big or binary file still counts as changed; its lines don't matter here.
        let added = std::fs::metadata(&full)
            .ok()
            .filter(|m| m.is_file() && m.len() < 1_000_000)
            .and_then(|_| std::fs::read_to_string(&full).ok())
            .map(|t| t.lines().count() as u32)
            .unwrap_or(0);
        files.push(FileChange { path: path.to_string(), added, removed: 0 });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Some(Changes { commits, files })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_in(dir: &Path, args: &[&str]) {
        let ok = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stderr));
    }

    #[test]
    fn changes_since_a_worktree_left_main() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git_in(&repo, &["init", "-q", "-b", "main"]);
        git_in(&repo, &["config", "user.email", "t@example.invalid"]);
        git_in(&repo, &["config", "user.name", "t"]);
        std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
        git_in(&repo, &["add", "-A"]);
        git_in(&repo, &["commit", "-q", "-m", "start"]);

        let tree = root.path().join("tree");
        git_in(&repo, &["worktree", "add", "-q", "-b", "t1", tree.to_str().unwrap()]);
        std::fs::write(tree.join("a.txt"), "one\n2\nthree\n").unwrap();
        git_in(&tree, &["commit", "-q", "-am", "edit"]);
        std::fs::write(tree.join("new.txt"), "x\ny\n").unwrap();
        // main moving on doesn't count as the worktree's change.
        std::fs::write(repo.join("main.txt"), "m\n").unwrap();
        git_in(&repo, &["add", "-A"]);
        git_in(&repo, &["commit", "-q", "-m", "main"]);

        let c = changes(&tree, &repo).unwrap();
        assert_eq!(c.commits, 1);
        assert_eq!(
            c.files,
            vec![
                FileChange { path: "a.txt".into(), added: 2, removed: 1 },
                FileChange { path: "new.txt".into(), added: 2, removed: 0 },
            ]
        );
        assert_eq!(changes(&repo, &repo).unwrap(), Changes { commits: 0, files: vec![] });
        assert_eq!(changes(&root.path().join("nowhere"), &repo), None);
    }

    #[test]
    fn a_branch_from_any_folder_inside() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".git")).unwrap();
        std::fs::write(
            repo.path().join(".git/HEAD"),
            "ref: refs/heads/feature/gst\n",
        )
        .unwrap();
        let deep = repo.path().join("src/billing");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(branch(&deep).as_deref(), Some("feature/gst"));
    }

    #[test]
    fn a_worktree_and_a_detached_head() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("main/.git/worktrees/lin-92");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("HEAD"), "3f9c2a1b8d7e6f\n").unwrap();
        let tree = root.path().join("lin-92");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(tree.join(".git"), format!("gitdir: {}\n", real.display())).unwrap();
        assert_eq!(branch(&tree).as_deref(), Some("3f9c2a1"));
    }

    #[test]
    fn nothing_outside_a_repository() {
        let plain = tempfile::tempdir().unwrap();
        assert_eq!(branch(plain.path()), None);
    }
}
