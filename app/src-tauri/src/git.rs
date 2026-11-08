//! Reads which branch a folder is on, straight from `.git`, without running git.

use std::path::{Path, PathBuf};

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

#[cfg(test)]
mod tests {
    use super::*;

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
