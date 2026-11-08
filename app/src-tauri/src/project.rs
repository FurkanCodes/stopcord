//! What a project folder looks like, for the New factory screen.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::git;
use crate::worktree::is_git;

pub const HARNESS_FILES: [&str; 3] = ["AGENTS.md", "CLAUDE.md", "GEMINI.md"];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub git: bool,
    pub branch: Option<String>,
    /// Which of AGENTS.md, CLAUDE.md and GEMINI.md the folder already has.
    pub harness_files: Vec<String>,
    /// How this project runs its tests, when that can be told from its files.
    pub test_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSuggestion {
    pub path: String,
    pub git: bool,
    pub branch: Option<String>,
    pub harness_files: Vec<String>,
    /// The factory's name, when the folder already is one.
    pub factory: Option<String>,
}

pub fn detect(path: &Path) -> ProjectInfo {
    let git = is_git(path);
    ProjectInfo {
        git,
        branch: if git { git::branch(path) } else { None },
        harness_files: harness_files(path),
        test_command: test_command(path),
    }
}

fn harness_files(path: &Path) -> Vec<String> {
    HARNESS_FILES
        .iter()
        .filter(|f| path.join(f).exists())
        .map(|f| f.to_string())
        .collect()
}

pub fn test_command(path: &Path) -> Option<String> {
    let has = |f: &str| path.join(f).exists();
    if let Ok(text) = std::fs::read_to_string(path.join("package.json"))
        && let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&text)
        && pkg["scripts"]["test"].is_string()
    {
        let runner = if has("pnpm-lock.yaml") {
            "pnpm"
        } else if has("yarn.lock") {
            "yarn"
        } else if has("bun.lockb") || has("bun.lock") {
            "bun"
        } else {
            "npm"
        };
        return Some(format!("{runner} test"));
    }
    if has("Cargo.toml") {
        return Some("cargo test".into());
    }
    if has("go.mod") {
        return Some("go test ./...".into());
    }
    if has("pytest.ini") || has("pyproject.toml") {
        return Some("pytest".into());
    }
    if let Ok(text) = std::fs::read_to_string(path.join("Makefile"))
        && text.lines().any(|l| l.starts_with("test:"))
    {
        return Some("make test".into());
    }
    None
}

/// Project folders in the usual places, newest first. `factories` are the
/// folders that already are factories, with their names.
pub fn nearby(home: &Path, factories: &[(String, String)]) -> Vec<FolderSuggestion> {
    let roots = [
        "projects",
        "work",
        "code",
        "src",
        "dev",
        "Developer",
        "Documents/GitHub",
    ];
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(home.join(root)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let time = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            found.push((time, path.canonicalize().unwrap_or(path)));
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.dedup_by(|a, b| a.1 == b.1);
    found
        .into_iter()
        .take(12)
        .map(|(_, path)| {
            let git = path.join(".git").exists();
            let shown = path.display().to_string();
            FolderSuggestion {
                factory: factories
                    .iter()
                    .find(|(p, _)| *p == shown)
                    .map(|(_, n)| n.clone()),
                branch: if git { git::branch(&path) } else { None },
                harness_files: harness_files(&path),
                git,
                path: shown,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_test_commands() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(test_command(d.path()), None);
        std::fs::write(d.path().join("Makefile"), "build:\n\ttrue\ntest:\n\ttrue\n").unwrap();
        assert_eq!(test_command(d.path()).as_deref(), Some("make test"));
        std::fs::write(
            d.path().join("package.json"),
            r#"{"scripts":{"test":"vitest"}}"#,
        )
        .unwrap();
        std::fs::write(d.path().join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(test_command(d.path()).as_deref(), Some("pnpm test"));
    }

    #[test]
    fn detects_and_suggests() {
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join("projects");
        std::fs::create_dir_all(projects.join("a")).unwrap();
        std::fs::create_dir_all(projects.join(".hidden")).unwrap();
        std::fs::write(projects.join("a/CLAUDE.md"), "x").unwrap();
        std::fs::create_dir_all(projects.join("b/.git")).unwrap();
        std::fs::write(projects.join("b/.git/HEAD"), "ref: refs/heads/dev\n").unwrap();
        let b = projects
            .join("b")
            .canonicalize()
            .unwrap()
            .display()
            .to_string();
        let list = nearby(home.path(), &[(b.clone(), "bee".into())]);
        assert_eq!(list.len(), 2);
        let bee = list.iter().find(|s| s.path == b).unwrap();
        assert_eq!(
            (bee.git, bee.branch.as_deref(), bee.factory.as_deref()),
            (true, Some("dev"), Some("bee"))
        );
        let a = list.iter().find(|s| s.path != b).unwrap();
        assert_eq!(a.harness_files, ["CLAUDE.md"]);
        assert!(!detect(&projects.join("a")).git);
    }
}
