//! Reads which branch a folder is on, straight from `.git`, without running git,
//! and what a terminal's checkout changed, with git.

use std::path::{Path, PathBuf};

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

/// One line of a diff, as a person reads it.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    /// "add", "del", "same", or "gap" between two hunks.
    pub kind: &'static str,
    /// Its number in the new file, for lines that are still there.
    pub line: Option<u64>,
    pub text: String,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FileDiff {
    pub path: String,
    pub lines: Vec<DiffLine>,
}

/// What a person needs on screen to decide about a piece of work.
#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    /// The planner's plan, when it wrote one.
    pub plan: Option<String>,
    pub files: Vec<FileDiff>,
    /// `git status` lines that a merge would refuse.
    pub uncommitted: Vec<String>,
    /// The branch a local merge goes into.
    pub target: String,
    /// The diff was too long to show whole.
    pub truncated: bool,
}

/// More than this and a diff isn't something to read in a pane.
const MAX_DIFF_LINES: usize = 4000;
const MAX_PLAN_BYTES: usize = 64 * 1024;

/// Turns `git diff` output into files and lines, stopping at `max` lines.
fn parse_diff(text: &str, max: usize) -> (Vec<FileDiff>, bool) {
    let mut files: Vec<FileDiff> = Vec::new();
    let (mut new_line, mut shown, mut in_hunk) = (0u64, 0usize, false);
    let strip = |p: &str| p.split('\t').next().unwrap_or(p).trim_matches('"').to_string();
    for raw in text.lines() {
        if raw.starts_with("diff --git ") {
            files.push(FileDiff { path: String::new(), lines: Vec::new() });
            in_hunk = false;
            continue;
        }
        let Some(file) = files.last_mut() else { continue };
        if let Some(rest) = raw.strip_prefix("@@ ") {
            new_line = rest.split(" +").nth(1).and_then(|s| s.split([',', ' ']).next()).and_then(|n| n.parse().ok()).unwrap_or(1);
            if in_hunk {
                file.lines.push(DiffLine { kind: "gap", line: None, text: String::new() });
            }
            in_hunk = true;
        } else if !in_hunk {
            if let Some(p) = raw.strip_prefix("+++ b/") {
                file.path = strip(p);
            } else if let Some(p) = raw.strip_prefix("--- a/") {
                file.path = strip(p);
            } else if raw.starts_with("Binary files") {
                let name = raw.rsplit(" b/").next().unwrap_or("").trim_end_matches(" differ");
                if file.path.is_empty() {
                    file.path = name.to_string();
                }
                file.lines.push(DiffLine { kind: "same", line: None, text: "binary file".to_string() });
            }
        } else if shown >= max {
            return (files, true);
        } else if let Some(t) = raw.strip_prefix('+') {
            file.lines.push(DiffLine { kind: "add", line: Some(new_line), text: t.to_string() });
            new_line += 1;
            shown += 1;
        } else if let Some(t) = raw.strip_prefix('-') {
            file.lines.push(DiffLine { kind: "del", line: None, text: t.to_string() });
            shown += 1;
        } else if let Some(t) = raw.strip_prefix(' ') {
            file.lines.push(DiffLine { kind: "same", line: Some(new_line), text: t.to_string() });
            new_line += 1;
            shown += 1;
        }
    }
    (files, false)
}

/// The plan and the diff of a worktree since `base`, and where a merge would go.
pub fn evidence(worktree: &Path, base: &str, project: &Path) -> Evidence {
    let plan = std::fs::read(worktree.join("PLAN.md")).ok().map(|b| {
        let end = b.len().min(MAX_PLAN_BYTES);
        String::from_utf8_lossy(&b[..end]).into_owned()
    });
    let text = run(worktree, &["diff", "--no-renames", "--no-color", "--no-ext-diff", base]).unwrap_or_default();
    let (files, truncated) = parse_diff(&text, MAX_DIFF_LINES);
    let uncommitted = run(worktree, &["status", "--porcelain", "--untracked-files=normal"])
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .collect();
    let target = run(project, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default().trim().to_string();
    Evidence { plan, files, uncommitted, target, truncated }
}

fn run(dir: &Path, args: &[&str]) -> Option<String> {
    let out = crate::worktree::git_command(dir).args(args).output().ok()?;
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

/// A file changed in a folder, for folders git doesn't track.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Touched {
    pub path: String,
    /// Milliseconds since 1970.
    pub modified: u64,
}

/// Folders that hold what tools make, not what people and agents write.
const SKIP: &[&str] = &["node_modules", "target", "dist", "build", "__pycache__", "venv", ".venv"];

/// Files under `root` changed at or after `since` (ms since 1970), newest first, at most 100.
/// Hidden files and folders and the folders in `SKIP` are left out; the walk stops after
/// 20 000 entries so a huge folder can't stall it.
pub fn touched(root: &Path, since: u64) -> Vec<Touched> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut seen = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            if seen > 20_000 {
                break;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || SKIP.contains(&name.as_ref()) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_millis() as u64);
                if modified >= since {
                    let path = entry.path();
                    let rel = path.strip_prefix(root).unwrap_or(&path);
                    out.push(Touched { path: rel.to_string_lossy().into_owned(), modified });
                }
            }
        }
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.path.cmp(&b.path)));
    out.truncate(100);
    out
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_diff_becomes_files_with_new_line_numbers() {
        let text = "diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n keep\n-old\n+new\n@@ -9 +9,2 @@ fn x\n nine\n+ten\ndiff --git a/gone.txt b/gone.txt\n--- a/gone.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-bye\n";
        let (files, cut) = parse_diff(text, 100);
        assert!(!cut);
        assert_eq!(files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["a.txt", "gone.txt"]);
        let a = &files[0].lines;
        assert_eq!((a[2].kind, a[2].line, a[2].text.as_str()), ("add", Some(2), "new"));
        assert_eq!(a[3].kind, "gap");
        assert_eq!((a[5].kind, a[5].line), ("add", Some(10)));
        assert_eq!(files[1].lines[0].kind, "del");
    }

    #[test]
    fn a_long_diff_stops_and_says_so() {
        let body: String = (0..50).map(|i| format!("+line {i}\n")).collect();
        let text = format!("diff --git a/a b/a\n--- /dev/null\n+++ b/a\n@@ -0,0 +1,50 @@\n{body}");
        let (files, cut) = parse_diff(&text, 10);
        assert!(cut);
        assert_eq!(files[0].lines.len(), 10);
    }

    #[test]
    fn touched_lists_new_files_and_skips_hidden_and_tool_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/x")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("old.txt"), "old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(root.join("notes/controls.md"), "keys").unwrap();
        std::fs::write(root.join("node_modules/x/y.js"), "x").unwrap();
        std::fs::write(root.join(".git/HEAD"), "x").unwrap();
        let paths: Vec<String> = touched(root, since).into_iter().map(|t| t.path).collect();
        assert_eq!(paths, vec!["notes/controls.md".to_string()]);
    }

    use super::*;
    use std::process::Command;

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
