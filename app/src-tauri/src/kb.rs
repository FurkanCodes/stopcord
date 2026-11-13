//! Linking the knowledge base into a checkout, under the names harnesses read
//! at start: AGENTS.md, CLAUDE.md, GEMINI.md.
//!
//! A missing file becomes a symlink to the knowledge base, kept out of git with
//! the repo's `info/exclude`. A file the project already has gets one marked
//! pointer block at the end. `unlink` takes back exactly those two things.

use std::path::Path;

use crate::factories::{Factory, Kind};
use crate::worktree::{common_dir, git, is_git};

const MARKER: &str = "<!-- stopcord: knowledge base -->";
const POINTER: &str = "Also read the factory's knowledge base:";

/// The first text of a new factory's knowledge base.
pub fn seed(name: &str) -> String {
    format!(
        "# {name}\n\n## About this project\n- \n\n## How this factory works\n\
         - When you find something out about this project, add it under \"About this project\".\n\
         - Keep long notes in their own files and link them here.\n\
         - Put rules and facts in this file, not in your own memory.\n"
    )
}

const LIBRARY_START: &str = "<!-- stopcord: library -->";
const LIBRARY_END: &str = "<!-- /stopcord: library -->";

/// The markers as they were written before the app was renamed from Sawtooth, and what they are now.
const OLD_MARKERS: [(&str, &str); 3] = [
    ("<!-- sawtooth: knowledge base -->", MARKER),
    ("<!-- sawtooth: library -->", LIBRARY_START),
    ("<!-- /sawtooth: library -->", LIBRARY_END),
];

/// `text` with any marker from before the rename written the current way, so old blocks are found.
fn renamed(text: &str) -> String {
    OLD_MARKERS.iter().fold(text.to_string(), |t, (old, new)| t.replace(old, new))
}

/// `text` with its Library section matching the factory's folder parts: one line per
/// folder, so agents know what's there to read. Stopcord owns the lines between the markers.
pub fn with_library(text: &str, factory: &Factory) -> String {
    let current = renamed(text);
    let text = current.as_str();
    let folders: Vec<String> = factory
        .parts
        .iter()
        .filter(|p| p.kind == Kind::Folder && !p.folder.trim().is_empty())
        .map(|p| {
            let folder = p.folder.trim().trim_start_matches("./");
            format!("- `{folder}` ({}): read the files here when you need them.", p.name.trim())
        })
        .collect();
    let section = if folders.is_empty() {
        String::new()
    } else {
        format!("{LIBRARY_START}\n## Library\n{}\n{LIBRARY_END}\n", folders.join("\n"))
    };
    let (before, after) = match (text.find(LIBRARY_START), text.find(LIBRARY_END)) {
        (Some(a), Some(b)) if b > a => {
            let end = b + LIBRARY_END.len();
            let end = if text[end..].starts_with('\n') { end + 1 } else { end };
            (&text[..a], &text[end..])
        }
        _ => (text, ""),
    };
    if section.is_empty() {
        let mut out = format!("{before}{after}");
        if before.ends_with("\n\n") && after.is_empty() {
            out.truncate(out.trim_end_matches('\n').len());
            out.push('\n');
        }
        return out;
    }
    let mut out = before.to_string();
    if after.is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
    }
    out.push_str(&section);
    out.push_str(after);
    out
}

fn block(kb_file: &Path) -> String {
    format!("\n{MARKER}\n{POINTER} {}\n", kb_file.display())
}

fn is_ours(path: &Path, kb_file: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.file_type().is_symlink() {
        return false;
    }
    match (path.canonicalize(), kb_file.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => std::fs::read_link(path).is_ok_and(|t| t == kb_file),
    }
}

/// The pattern for `name` in `dir`, relative to the repo's top: what info/exclude needs.
fn exclude_pattern(dir: &Path, name: &str) -> String {
    let prefix = git(dir, &["rev-parse", "--show-prefix"]).unwrap_or_default();
    format!("/{prefix}{name}")
}

fn exclude(dir: &Path, name: &str) -> Result<(), String> {
    if !is_git(dir) {
        return Ok(());
    }
    let Some(common) = common_dir(dir) else {
        return Ok(());
    };
    let file = common.join("info").join("exclude");
    let pattern = exclude_pattern(dir, name);
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    if text.lines().any(|l| l.trim() == pattern) {
        return Ok(());
    }
    let mut next = text;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&pattern);
    next.push('\n');
    std::fs::create_dir_all(file.parent().unwrap())
        .and_then(|_| std::fs::write(&file, next))
        .map_err(|e| format!("couldn't keep {name} out of git: {e}"))
}

/// Links the knowledge base into `dir` as each of `names`. Returns what went wrong, in words.
pub fn link(kb_file: &Path, dir: &Path, names: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for name in names {
        let name = name.trim();
        if name.is_empty() || name.contains('/') {
            continue;
        }
        let path = dir.join(name);
        if is_ours(&path, kb_file) {
            continue;
        }
        let meta = std::fs::symlink_metadata(&path).ok();
        if meta.as_ref().is_some_and(|m| m.file_type().is_symlink()) {
            // Someone else's link. Writing through it would change the file it points at,
            // which may be another factory's knowledge base. Replace it only if it's broken.
            if path.exists() {
                let target = std::fs::read_link(&path).unwrap_or_default();
                problems.push(format!(
                    "{} links to {}, so it was left alone",
                    path.display(),
                    target.display()
                ));
                continue;
            }
            if let Err(e) = std::fs::remove_file(&path) {
                problems.push(format!("couldn't replace the broken link {}: {e}", path.display()));
                continue;
            }
        }
        let exists = std::fs::symlink_metadata(&path).is_ok();
        if !exists {
            if let Err(e) = std::os::unix::fs::symlink(kb_file, &path) {
                problems.push(format!("couldn't link {}: {e}", path.display()));
                continue;
            }
            if let Err(p) = exclude(dir, name) {
                problems.push(p);
            }
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(text) if renamed(&text).contains(MARKER) => {
                let current = renamed(&text);
                if current != text
                    && let Err(e) = std::fs::write(&path, current)
                {
                    problems.push(format!("couldn't update {}: {e}", path.display()));
                }
            }
            Ok(mut text) => {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&block(kb_file));
                if let Err(e) = std::fs::write(&path, text) {
                    problems.push(format!("couldn't add a line to {}: {e}", path.display()));
                }
            }
            Err(e) => problems.push(format!("couldn't read {}: {e}", path.display())),
        }
    }
    problems
}

/// Takes back what `link` did in `dir` for `names`.
pub fn unlink(kb_file: &Path, dir: &Path, names: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for name in names {
        let name = name.trim();
        if name.is_empty() || name.contains('/') {
            continue;
        }
        let path = dir.join(name);
        if is_ours(&path, kb_file) {
            if let Err(e) = std::fs::remove_file(&path) {
                problems.push(format!("couldn't remove {}: {e}", path.display()));
            }
            continue;
        }
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let text = renamed(&text);
        if !text.contains(MARKER) {
            continue;
        }
        let exact = block(kb_file);
        let next = if text.contains(&exact) {
            text.replacen(&exact, "", 1)
        } else {
            // The knowledge base moved since: drop the marker and the line after it.
            let mut out = Vec::new();
            let mut skip = false;
            for line in text.lines() {
                if skip {
                    skip = false;
                    if line.starts_with(POINTER) {
                        continue;
                    }
                }
                if line == MARKER {
                    skip = true;
                    if out.last() == Some(&"") {
                        out.pop();
                    }
                    continue;
                }
                out.push(line);
            }
            let mut joined = out.join("\n");
            if text.ends_with('\n') {
                joined.push('\n');
            }
            joined
        };
        if let Err(e) = std::fs::write(&path, next) {
            problems.push(format!("couldn't edit {}: {e}", path.display()));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::tests::repo;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_block_written_before_the_rename_is_still_found() {
        let repo = repo();
        let dir = repo.path();
        let home = tempfile::tempdir().unwrap();
        let kb = home.path().join("knowledge.md");
        std::fs::write(&kb, seed("demo")).unwrap();
        let old = format!("# Mine\n\n<!-- sawtooth: knowledge base -->\n{POINTER} {}\n", kb.display());
        std::fs::write(dir.join("CLAUDE.md"), &old).unwrap();

        assert!(link(&kb, dir, &names(&["CLAUDE.md"])).is_empty());
        let now = std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
        assert_eq!(now.matches(MARKER).count(), 1, "updated in place, not added twice: {now}");
        assert!(!now.contains("sawtooth"), "{now}");

        std::fs::write(dir.join("CLAUDE.md"), &old).unwrap();
        assert!(unlink(&kb, dir, &names(&["CLAUDE.md"])).is_empty());
        assert_eq!(std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap(), "# Mine\n");

        let library = "# KB\n\n<!-- sawtooth: library -->\n## Library\n- old\n<!-- /sawtooth: library -->\n";
        let f = crate::factories::Factory::default();
        assert_eq!(with_library(library, &f), "# KB\n");
    }

    #[test]
    fn links_missing_files_and_points_from_existing_ones() {
        let repo = repo();
        let dir = repo.path();
        let home = tempfile::tempdir().unwrap();
        let kb = home.path().join("knowledge.md");
        std::fs::write(&kb, seed("demo")).unwrap();
        std::fs::write(dir.join("CLAUDE.md"), "# Mine\nKeep this.").unwrap();

        let all = names(&["AGENTS.md", "CLAUDE.md"]);
        assert!(link(&kb, dir, &all).is_empty());
        assert!(link(&kb, dir, &all).is_empty(), "twice is the same as once");

        assert!(is_ours(&dir.join("AGENTS.md"), &kb));
        assert_eq!(
            std::fs::read_to_string(dir.join("AGENTS.md")).unwrap(),
            seed("demo")
        );
        let claude = std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
        assert_eq!(claude.matches(MARKER).count(), 1);
        assert!(claude.starts_with("# Mine\nKeep this.\n"));
        // The symlink stays out of git; CLAUDE.md shows as changed.
        let status = git(dir, &["status", "--porcelain"]).unwrap();
        assert!(!status.contains("AGENTS.md"), "{status}");
        assert!(status.contains("CLAUDE.md"), "{status}");

        assert!(unlink(&kb, dir, &all).is_empty());
        assert!(!dir.join("AGENTS.md").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap(),
            "# Mine\nKeep this.\n"
        );
    }

    #[test]
    fn unlink_after_the_knowledge_base_moved() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "a\n").unwrap();
        let old = dir.path().join("old.md");
        link(&old, dir.path(), &names(&["AGENTS.md"]));
        unlink(
            &dir.path().join("new.md"),
            dir.path(),
            &names(&["AGENTS.md"]),
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
            "a\n"
        );
    }

    #[test]
    fn never_writes_through_someone_elses_link() {
        let dir = tempfile::tempdir().unwrap();
        let other = dir.path().join("other-kb.md");
        std::fs::write(&other, "other factory\n").unwrap();
        std::os::unix::fs::symlink(&other, dir.path().join("AGENTS.md")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("gone.md"), dir.path().join("CLAUDE.md")).unwrap();
        let kb = dir.path().join("kb.md");
        std::fs::write(&kb, "mine\n").unwrap();
        let all = names(&["AGENTS.md", "CLAUDE.md"]);

        let problems = link(&kb, dir.path(), &all);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "other factory\n");
        assert!(is_ours(&dir.path().join("CLAUDE.md"), &kb), "a broken link is replaced");

        assert!(unlink(&kb, dir.path(), &all).is_empty());
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "other factory\n");
        assert!(dir.path().join("AGENTS.md").exists());
    }

    #[test]
    fn library_section_follows_folder_parts() {
        let mut f = Factory::default();
        let text = seed("demo");
        assert_eq!(with_library(&text, &f), text, "no folders, no section");
        f.parts.push(crate::factories::Part {
            kind: Kind::Folder,
            name: "Specs".into(),
            folder: "specs/".into(),
            ..Default::default()
        });
        let with = with_library(&text, &f);
        assert!(with.contains("- `specs/` (Specs)"), "{with}");
        assert_eq!(with_library(&with, &f), with, "twice is the same as once");
        let edited = format!("{with}- a fact added after\n");
        f.parts[0].folder = "docs/".into();
        let moved = with_library(&edited, &f);
        assert!(moved.contains("`docs/`") && !moved.contains("`specs/`"), "{moved}");
        assert!(moved.ends_with("- a fact added after\n"), "{moved}");
        f.parts.clear();
        assert_eq!(with_library(&with, &f), text);
    }

    #[test]
    fn in_a_subfolder_of_a_repo() {
        let repo = repo();
        let sub = repo.path().join("app");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("x.txt"), "x").unwrap();
        let kb = repo.path().join("../kb.md");
        assert!(link(&kb, &sub, &names(&["GEMINI.md"])).is_empty());
        let status = git(repo.path(), &["status", "--porcelain", "-uall"]).unwrap();
        assert!(!status.contains("GEMINI.md"), "{status}");
    }
}
