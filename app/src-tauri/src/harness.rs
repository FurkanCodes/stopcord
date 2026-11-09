//! What Sawtooth needs to know about each harness to confine it: where it keeps
//! its state, which sites it talks to, and what to do about its own sandbox.
//!
//! These are data, so harnesses differ only here. `~/.sawtooth/harnesses/<id>.yaml`
//! replaces a built-in profile or adds a new one. A harness with no profile still
//! runs: it just has no extras, and whatever it needs shows up as blocked in NEEDS.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Trust {
    /// Text on the screen that means the harness is asking whether to trust the folder.
    pub contains: String,
    /// What to type to say yes. Enter (`\r`) is sent separately, after the menu has moved.
    pub answer: String,
    /// Answer it without asking the person. Off by default: the question exists to
    /// stop a repo's own hooks from running unseen.
    pub auto: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Harness {
    pub id: String,
    /// The program names that mean this harness.
    pub commands: Vec<String>,
    /// Folders and files under the home folder it writes its own state to.
    pub write: Vec<String>,
    /// Files it may write if their name starts with one of these, for saves through temp files.
    pub write_prefix: Vec<String>,
    /// Its settings, kept read-only so an agent can't plant hooks for a later unconfined session.
    pub read_only: Vec<String>,
    /// Sites it needs.
    pub domains: Vec<String>,
    /// Added to its start command inside a sandbox, because a sandbox can't run inside another.
    pub sandboxed_args: String,
    /// How to run a single prompt and exit, with `{prompt}` where the prompt goes. For judges and suggestions.
    pub one_shot: String,
    pub trust: Option<Trust>,
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

pub fn builtin() -> Vec<Harness> {
    vec![
        Harness {
            id: "claude".into(),
            commands: strings(&["claude"]),
            write: strings(&[
                ".claude",
                ".claude.json",
                ".claude.json.lock",
                ".claude.json.backup",
                ".local/state/claude",
                ".local/share/claude",
                ".cache/claude",
            ]),
            write_prefix: strings(&[".claude.json"]),
            read_only: strings(&[".claude/settings.json", ".claude/settings.local.json", ".claude/hooks"]),
            domains: strings(&["anthropic.com", "claude.ai", "claude.com", "datadoghq.com", "statsig.com", "sentry.io"]),
            sandboxed_args: String::new(),
            one_shot: "claude -p {prompt}".into(),
            trust: Some(Trust {
                contains: "Yes, I trust this folder".into(),
                // The menu starts on "No, exit", so Enter alone would refuse.
                answer: "\u{1b}[B\r".into(),
                auto: false,
            }),
        },
        Harness {
            id: "codex".into(),
            commands: strings(&["codex"]),
            write: strings(&[".codex"]),
            write_prefix: Vec::new(),
            read_only: strings(&[".codex/config.toml"]),
            domains: strings(&["openai.com", "chatgpt.com", "oaistatic.com"]),
            sandboxed_args: "--sandbox danger-full-access".into(),
            one_shot: "codex exec --skip-git-repo-check {prompt}".into(),
            trust: None,
        },
        Harness {
            id: "gemini".into(),
            commands: strings(&["gemini"]),
            write: strings(&[".gemini"]),
            write_prefix: Vec::new(),
            read_only: strings(&[".gemini/settings.json"]),
            domains: strings(&["googleapis.com", "google.com", "gstatic.com"]),
            sandboxed_args: String::new(),
            one_shot: "gemini -p {prompt}".into(),
            trust: None,
        },
        Harness {
            id: "opencode".into(),
            commands: strings(&["opencode"]),
            write: strings(&[".opencode", ".local/share/opencode", ".local/state/opencode", ".cache/opencode", ".config/opencode"]),
            write_prefix: Vec::new(),
            read_only: strings(&[".config/opencode/opencode.json"]),
            domains: strings(&["opencode.ai", "models.dev"]),
            sandboxed_args: String::new(),
            one_shot: "opencode run {prompt}".into(),
            trust: None,
        },
    ]
}

/// The built-ins with `dir`'s files on top. Returns what couldn't be read, so it can be shown.
pub fn load(dir: &Path) -> (Vec<Harness>, Vec<String>) {
    let mut all = builtin();
    let mut problems = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (all, problems);
    };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "yaml"))
        .collect();
    files.sort();
    for file in files {
        match read(&file) {
            Ok(h) => {
                all.retain(|b| b.id != h.id);
                all.push(h);
            }
            Err(e) => problems.push(format!("{e:#}")),
        }
    }
    (all, problems)
}

fn read(file: &Path) -> Result<Harness> {
    let text = std::fs::read_to_string(file).with_context(|| format!("couldn't read {}", file.display()))?;
    let mut h: Harness = serde_yaml_ng::from_str(&text).with_context(|| format!("{} isn't valid", file.display()))?;
    if h.id.is_empty() {
        h.id = file.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    }
    if h.commands.is_empty() {
        h.commands = vec![h.id.clone()];
    }
    Ok(h)
}

/// Which harness a start command runs, going by its program name.
pub fn find<'a>(all: &'a [Harness], command: &str) -> Option<&'a Harness> {
    let program = command
        .split_whitespace()
        .find(|w| !w.contains('='))?
        .rsplit('/')
        .next()?;
    all.iter().find(|h| h.commands.iter().any(|c| c == program))
}

/// The start command to use inside a sandbox.
pub fn sandboxed_command(harness: Option<&Harness>, command: &str) -> String {
    match harness {
        Some(h) if !h.sandboxed_args.is_empty() && !command.contains(&h.sandboxed_args) => {
            format!("{command} {}", h.sandboxed_args)
        }
        _ => command.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_finds_its_harness_however_it_is_written() {
        let all = builtin();
        assert_eq!(find(&all, "claude").unwrap().id, "claude");
        assert_eq!(find(&all, "/opt/bin/codex --model x").unwrap().id, "codex");
        assert_eq!(find(&all, "FOO=1 opencode").unwrap().id, "opencode");
        assert!(find(&all, "aider").is_none());
        assert!(find(&all, "").is_none());
    }

    #[test]
    fn a_harness_with_its_own_sandbox_has_it_turned_off_once() {
        let all = builtin();
        let codex = find(&all, "codex");
        let once = sandboxed_command(codex, "codex --model x");
        assert_eq!(once, "codex --model x --sandbox danger-full-access");
        assert_eq!(sandboxed_command(codex, &once), once);
        assert_eq!(sandboxed_command(find(&all, "claude"), "claude"), "claude");
        assert_eq!(sandboxed_command(None, "aider"), "aider");
    }

    #[test]
    fn a_file_replaces_a_builtin_or_adds_a_new_one_and_a_bad_one_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("claude.yaml"), "domains: [only.example]\n").unwrap();
        std::fs::write(dir.path().join("aider.yaml"), "write: [.aider]\n").unwrap();
        std::fs::write(dir.path().join("bad.yaml"), "nonsense: [\n").unwrap();
        let (all, problems) = load(dir.path());
        assert_eq!(find(&all, "claude").unwrap().domains, ["only.example"]);
        assert_eq!(find(&all, "aider").unwrap().write, [".aider"]);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("bad.yaml"), "{problems:?}");
    }

    #[test]
    fn trusting_a_folder_is_never_automatic_out_of_the_box() {
        assert!(builtin().iter().filter_map(|h| h.trust.as_ref()).all(|t| !t.auto));
    }
}
