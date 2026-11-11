//! Getting a project from GitHub, or any git host, so a new factory can start from a link.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Result, bail};

use crate::shell::quote;
use crate::tickets::run_in_login_shell;

/// What a pasted repo means: what to clone, and the folder name it gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    /// `owner/repo` when it's on GitHub, for `gh`, which signs in on its own.
    pub github: Option<String>,
    /// What `git clone` takes.
    pub url: String,
    pub name: String,
}

/// The person's home folder, where their projects are.
pub fn person_home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Reads `owner/repo`, a github.com address in any of its forms, or any other git URL.
pub fn parse(input: &str) -> Option<Repo> {
    let s = input.trim().trim_end_matches('/');
    if s.is_empty() || s.contains(char::is_whitespace) {
        return None;
    }
    let clean = |n: &str| n.trim_end_matches(".git").to_string();
    let ok = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    let github_path = s
        .strip_prefix("https://github.com/")
        .or_else(|| s.strip_prefix("http://github.com/"))
        .or_else(|| s.strip_prefix("github.com/"))
        .or_else(|| s.strip_prefix("git@github.com:"))
        .or_else(|| s.strip_prefix("ssh://git@github.com/"))
        .or_else(|| (!s.contains(':') && s.matches('/').count() == 1).then_some(s));
    if let Some(path) = github_path {
        // A link to a page inside the repo still means the repo.
        let mut parts = path.split('/');
        let (owner, repo) = (parts.next()?, clean(parts.next()?));
        if !ok(owner) || !ok(&repo) {
            return None;
        }
        return Some(Repo { github: Some(format!("{owner}/{repo}")), url: format!("https://github.com/{owner}/{repo}.git"), name: repo });
    }
    let looks_like_git = ["https://", "http://", "ssh://", "git@", "file://"].iter().any(|p| s.starts_with(p));
    let name = clean(s.rsplit(['/', ':']).next()?);
    (looks_like_git && ok(&name)).then(|| Repo { github: None, url: s.to_string(), name })
}

/// Where a clone goes: the first of the usual project folders that exists, else ~/projects.
pub fn parent(home: &Path) -> PathBuf {
    ["projects", "work", "code", "src", "dev", "Developer", "Documents/GitHub"]
        .iter()
        .map(|r| home.join(r))
        .find(|p| p.is_dir())
        .unwrap_or_else(|| home.join("projects"))
}

/// The repos the person's gh can see, newest first: their own and their organizations'.
pub fn mine(home: &Path) -> Vec<String> {
    let out = run_in_login_shell("gh repo list --limit 200 --json nameWithOwner --jq '.[].nameWithOwner'", home, &[], Duration::from_secs(20)).unwrap_or_default();
    out.lines().map(str::trim).filter(|l| parse(l).is_some_and(|r| r.github.is_some())).map(String::from).collect()
}

/// Clones `repo` into `parent`, through gh when it's on GitHub and gh is there, so private repos
/// work with the person's own sign-in. Returns the folder. A folder already there is never touched.
pub fn clone(repo: &Repo, parent: &Path, home: &Path) -> Result<PathBuf> {
    let into = parent.join(&repo.name);
    if into.exists() {
        bail!("{} is already there. Pick it from folders nearby, or move it first", into.display());
    }
    std::fs::create_dir_all(parent)?;
    let target = quote(&into.display().to_string());
    let command = match &repo.github {
        Some(name) => format!("if command -v gh >/dev/null 2>&1; then gh repo clone {} {target}; else git clone {} {target}; fi", quote(name), quote(&repo.url)),
        None => format!("git clone {} {target}", quote(&repo.url)),
    };
    if let Err(e) = run_in_login_shell(&command, home, &[], Duration::from_secs(600)) {
        // A clone that stopped halfway leaves a folder that isn't a project.
        let _ = std::fs::remove_dir_all(&into);
        bail!("couldn't clone {}: {}", repo.github.as_deref().unwrap_or(&repo.url), format!("{e:#}").replace("the pull command failed", "git said"));
    }
    Ok(into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pasted_repo_is_read_in_every_usual_form() {
        for s in ["acme/web", "github.com/acme/web", "https://github.com/acme/web", "https://github.com/acme/web.git", "https://github.com/acme/web/tree/main/src", "git@github.com:acme/web.git"] {
            let r = parse(s).unwrap_or_else(|| panic!("{s}"));
            assert_eq!((r.github.as_deref(), r.name.as_str(), r.url.as_str()), (Some("acme/web"), "web", "https://github.com/acme/web.git"), "{s}");
        }
        let lab = parse("https://gitlab.com/acme/api.git").unwrap();
        assert_eq!((lab.github, lab.name.as_str()), (None, "api"));
        for bad in ["", "not a repo", "acme", "acme/web; rm -rf ~", "../../x/y"] {
            assert!(parse(bad).is_none_or(|r| !r.name.contains(';')), "{bad}");
        }
        assert!(parse("acme/web; rm -rf ~").is_none());
    }

    #[test]
    fn a_clone_lands_next_to_the_other_projects_and_never_over_a_folder() {
        let source = crate::worktree::tests::repo();
        let home = tempfile::tempdir().unwrap();
        let repo = Repo { github: None, url: source.path().display().to_string(), name: "web".into() };
        let into = clone(&repo, &home.path().join("projects"), home.path()).unwrap();
        assert!(into.join(".git").exists());
        assert!(clone(&repo, &home.path().join("projects"), home.path()).unwrap_err().to_string().contains("already there"));
        let missing = Repo { github: None, url: home.path().join("nope").display().to_string(), name: "nope".into() };
        assert!(clone(&missing, &home.path().join("projects"), home.path()).is_err());
        assert!(!home.path().join("projects/nope").exists(), "a failed clone leaves nothing behind");
    }
}
