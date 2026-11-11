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
    /// Answer it outside a sandbox too. Off by default: the question exists to stop a repo's
    /// own hooks from running unseen. Inside a sandbox it's always answered, as those hooks are confined.
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
    /// Sites its own background chatter uses, like update checks. Still blocked and logged,
    /// but not worth stopping a person for.
    pub quiet_domains: Vec<String>,
    /// Added to its start command inside a sandbox: a sandbox can't run inside another, and with
    /// the OS holding the boundary, a prompt for every edit would only stall unattended work.
    /// `{project}` becomes the project's folder.
    pub sandboxed_args: String,
    /// Flags that turn on the harness's own sandbox, for the harness level. Empty means none Sawtooth knows.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub own_sandbox_args: String,
    /// How to run a single prompt and exit, with `{prompt}` where the prompt goes and `{model}` where
    /// the model flag goes, if the start command names a model. For judges and suggestions.
    pub one_shot: String,
    /// The flag that picks a model, such as `--model`. Empty means Sawtooth doesn't know one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub model_flag: String,
    /// Models to offer for it. Any other name can still be typed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    /// How the start command says how hard to think, with `{effort}` for the level, such as
    /// `--effort {effort}`. Empty means Sawtooth doesn't know one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub effort_arg: String,
    /// Effort levels to offer. Any other can still be typed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub efforts: Vec<String>,
    pub trust: Option<Trust>,
    /// Environment variables it can log in with, for a VM, where its login on the Mac isn't there.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub login_env: Vec<String>,
    /// Where a person gets one of those.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub login_hint: String,
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
            quiet_domains: strings(&["formulae.brew.sh"]),
            // Not bypass mode: that shows a confirmation we'd have to edit its settings to skip.
            sandboxed_args: "--permission-mode acceptEdits --allowedTools Bash".into(),
            own_sandbox_args: r#"--permission-mode acceptEdits --settings '{"sandbox":{"enabled":true,"autoAllowBashIfSandboxed":true}}'"#.into(),
            one_shot: "claude {model} -p {prompt}".into(),
            model_flag: "--model".into(),
            models: strings(&["opus", "sonnet", "haiku"]),
            effort_arg: "--effort {effort}".into(),
            efforts: strings(&["low", "medium", "high", "xhigh", "max"]),
            login_env: strings(&["CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_API_KEY"]),
            login_hint: "a token from `claude setup-token` on your mac, or an API key".into(),
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
            quiet_domains: Vec::new(),
            // Its background server sits outside the sandbox, so reaching it would be a way out.
            // Trust is set for this run only: its config is read-only in here, so a saved answer would fail.
            // A whole table, because Codex splits a dotted key at every dot, including those in the path.
            sandboxed_args: r#"--sandbox danger-full-access --ask-for-approval never --no-daemon -c 'projects={"{project}"={trust_level="trusted"}}'"#.into(),
            own_sandbox_args: r#"--sandbox workspace-write --ask-for-approval never -c 'projects={"{project}"={trust_level="trusted"}}'"#.into(),
            one_shot: "codex exec {model} --skip-git-repo-check --sandbox danger-full-access {prompt}".into(),
            model_flag: "--model".into(),
            models: Vec::new(),
            effort_arg: "-c model_reasoning_effort={effort}".into(),
            efforts: strings(&["low", "medium", "high"]),
            login_env: strings(&["OPENAI_API_KEY", "CODEX_API_KEY"]),
            login_hint: "an OpenAI API key".into(),
            trust: Some(Trust {
                contains: "Trust this folder?".into(),
                // The menu starts on "Trust and continue".
                answer: "\r".into(),
                auto: false,
            }),
        },
        Harness {
            id: "gemini".into(),
            commands: strings(&["gemini"]),
            write: strings(&[".gemini"]),
            write_prefix: Vec::new(),
            read_only: strings(&[".gemini/settings.json"]),
            domains: strings(&["googleapis.com", "google.com", "gstatic.com"]),
            quiet_domains: Vec::new(),
            sandboxed_args: "--yolo".into(),
            own_sandbox_args: String::new(),
            one_shot: "gemini {model} -p {prompt}".into(),
            model_flag: "--model".into(),
            models: Vec::new(),
            effort_arg: String::new(),
            efforts: Vec::new(),
            login_env: strings(&["GEMINI_API_KEY"]),
            login_hint: "a Gemini API key from Google AI Studio".into(),
            trust: None,
        },
        Harness {
            id: "opencode".into(),
            commands: strings(&["opencode"]),
            write: strings(&[".opencode", ".local/share/opencode", ".local/state/opencode", ".cache/opencode", ".config/opencode"]),
            write_prefix: Vec::new(),
            read_only: strings(&[".config/opencode/opencode.json"]),
            domains: strings(&["opencode.ai", "models.dev"]),
            quiet_domains: Vec::new(),
            sandboxed_args: "--auto".into(),
            own_sandbox_args: String::new(),
            one_shot: "opencode run {model} {prompt}".into(),
            model_flag: "--model".into(),
            models: Vec::new(),
            effort_arg: String::new(),
            efforts: Vec::new(),
            login_env: strings(&["OPENROUTER_API_KEY", "ANTHROPIC_API_KEY", "OPENAI_API_KEY"]),
            login_hint: "the API key of the provider you use with it".into(),
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

/// The model a start command names with the harness's flag, as in `claude --model haiku` or `--model=haiku`.
pub fn model_of(harness: Option<&Harness>, command: &str) -> Option<String> {
    let flag = harness.map(|h| h.model_flag.as_str()).filter(|f| !f.is_empty())?;
    let mut words = command.split_whitespace();
    while let Some(w) = words.next() {
        if w == flag {
            return words.next().map(|m| m.trim_matches(|c| c == '\'' || c == '"').to_string()).filter(|m| !m.is_empty());
        }
        if let Some(m) = w.strip_prefix(flag).and_then(|r| r.strip_prefix('=')) {
            return Some(m.trim_matches(|c| c == '\'' || c == '"').to_string()).filter(|m| !m.is_empty());
        }
    }
    None
}

/// The words of `command` that set its effort, by the harness's `effort_arg`, and the level in them.
fn effort_words(harness: &Harness, command: &str) -> Option<(Vec<String>, String)> {
    let shape: Vec<&str> = harness.effort_arg.split_whitespace().collect();
    let at = shape.iter().position(|w| w.contains("{effort}"))?;
    let (before, after) = shape[at].split_once("{effort}")?;
    let words: Vec<&str> = command.split_whitespace().collect();
    (0..words.len().saturating_sub(shape.len() - 1)).find_map(|i| {
        let here = &words[i..i + shape.len()];
        let fits = shape.iter().zip(here).enumerate().all(|(j, (s, w))| if j == at { w.starts_with(before) && w.ends_with(after) && w.len() > before.len() + after.len() } else { s == w });
        fits.then(|| {
            let level = here[at][before.len()..here[at].len() - after.len()].trim_matches(|c| c == '\'' || c == '"').to_string();
            (here.iter().map(|w| w.to_string()).collect(), level)
        })
    })
}

/// The one-shot command for a prompt, keeping the program, the model and the effort the start command picked.
pub fn one_shot_line(harness: &Harness, command: &str, quoted_prompt: &str) -> String {
    let picked = model_of(Some(harness), command).map_or(String::new(), |m| format!("{} {}", harness.model_flag, crate::shell::quote(&m)));
    let effort = effort_words(harness, command).map_or(String::new(), |(words, _)| words.join(" "));
    let model = [picked, effort].into_iter().filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    // The model goes in before the prompt, so a prompt that happens to say {model} stays as it is.
    let line = if model.is_empty() { harness.one_shot.replace(" {model}", "").replace("{model}", "") } else { harness.one_shot.replace("{model}", &model) };
    // `/opt/bin/claude` asks that claude, not whichever one is first on the PATH.
    let program = command.split_whitespace().next().unwrap_or_default();
    let line = match line.split_once(' ') {
        Some((first, rest)) if program != first && Path::new(program).file_name().is_some_and(|n| n == first) => format!("{program} {rest}"),
        _ => line,
    };
    line.replace("{prompt}", quoted_prompt)
}

/// Whether a terminal really started inside a sandbox, going by the command the daemon ran.
/// A harness's own command is one shell string, so mentioning a sandbox doesn't count.
pub fn in_sandbox(terminal: &serde_json::Value) -> bool {
    terminal["argv"].as_array().is_some_and(|argv| {
        argv.iter().any(|a| matches!(a.as_str(), Some("/usr/bin/sandbox-exec" | "bwrap")))
    })
}

/// Whether a blocked site is only this harness talking to itself.
pub fn is_quiet(harness: Option<&Harness>, host: &str) -> bool {
    harness.is_some_and(|h| matches(&h.quiet_domains, host))
}

/// Whether `host` is one of `sites` or under one of them.
pub fn matches(sites: &[String], host: &str) -> bool {
    sites.iter().any(|d| host == d || host.ends_with(&format!(".{d}")))
}

/// The start command for the harness level, with the harness's own sandbox turned on. None when
/// Sawtooth doesn't know how to turn it on for this harness.
pub fn own_sandbox_command(harness: Option<&Harness>, command: &str, project: &str) -> Option<String> {
    let h = harness.filter(|h| !h.own_sandbox_args.trim().is_empty())?;
    let args = h.own_sandbox_args.replace("{project}", &project.replace('\'', r"'\''"));
    Some(if command.contains(&args) { command.to_string() } else { format!("{command} {args}") })
}

/// The start command to use inside a sandbox.
pub fn sandboxed_command(harness: Option<&Harness>, command: &str, project: &str) -> String {
    let args = harness.map_or(String::new(), |h| h.sandboxed_args.replace("{project}", &project.replace('\'', r"'\''")));
    if args.is_empty() || command.contains(&args) {
        command.to_string()
    } else {
        format!("{command} {args}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_harness_level_turns_on_only_sandboxes_it_knows() {
        let all = builtin();
        let find = |id: &str| all.iter().find(|h| h.id == id);
        let claude = own_sandbox_command(find("claude"), "claude", "/p").unwrap();
        assert!(claude.contains(r#""sandbox":{"enabled":true"#), "{claude}");
        let codex = own_sandbox_command(find("codex"), "codex", "/my 'p").unwrap();
        assert!(codex.contains("--sandbox workspace-write") && codex.contains(r"/my '\''p"), "{codex}");
        assert!(own_sandbox_command(find("gemini"), "gemini", "/p").is_none());
        assert!(own_sandbox_command(None, "anything", "/p").is_none());
    }

    #[test]
    fn a_model_is_read_from_the_command_and_kept_when_asking_once() {
        let all = builtin();
        let claude = find(&all, "claude").unwrap();
        assert_eq!(model_of(Some(claude), "claude --model haiku").as_deref(), Some("haiku"));
        assert_eq!(model_of(Some(claude), "/bin/claude --model=opus --verbose").as_deref(), Some("opus"));
        assert_eq!(model_of(Some(claude), "claude"), None);
        assert_eq!(model_of(None, "aider --model x"), None);
        assert_eq!(one_shot_line(claude, "claude --model haiku", "'hi'"), "claude --model 'haiku' -p 'hi'");
        assert_eq!(one_shot_line(claude, "claude", "'two\n  lines'"), "claude -p 'two\n  lines'");
        assert_eq!(one_shot_line(claude, "/opt/bin/claude --model haiku", "'hi'"), "/opt/bin/claude --model 'haiku' -p 'hi'");
        let codex = find(&all, "codex").unwrap();
        assert!(one_shot_line(codex, "codex --model gpt-5", "'hi'").starts_with("codex exec --model 'gpt-5' --skip-git-repo-check"));
    }

    #[test]
    fn a_site_matches_itself_and_what_is_under_it() {
        let sites = vec!["github.com".to_string()];
        assert!(matches(&sites, "github.com") && matches(&sites, "api.github.com"));
        assert!(!matches(&sites, "notgithub.com"));
    }

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
        let once = sandboxed_command(codex, "codex --model x", "/p");
        assert!(once.starts_with("codex --model x --sandbox danger-full-access --ask-for-approval never --no-daemon"), "{once}");
        assert!(once.contains(r#"'projects={"/p"={trust_level="trusted"}}'"#), "{once}");
        assert_eq!(sandboxed_command(codex, &once, "/p"), once);
                assert_eq!(sandboxed_command(None, "aider", "/p"), "aider");
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
    fn a_one_shot_keeps_the_effort_its_start_command_asked_for() {
        let all = builtin();
        let claude = find(&all, "claude").unwrap();
        let codex = find(&all, "codex").unwrap();
        let level = |h: &Harness, c: &str| effort_words(h, c).map(|(_, l)| l);
        assert_eq!(level(claude, "claude --model opus --effort high").as_deref(), Some("high"));
        assert_eq!(level(codex, "codex -c model_reasoning_effort=low --model gpt-5").as_deref(), Some("low"));
        assert_eq!(level(claude, "claude --model opus"), None);
        assert_eq!(one_shot_line(claude, "claude --model opus --effort max", "'hi'"), "claude --model 'opus' --effort max -p 'hi'");
        assert_eq!(one_shot_line(codex, "codex -c model_reasoning_effort=high", "'hi'"), "codex exec -c model_reasoning_effort=high --skip-git-repo-check --sandbox danger-full-access 'hi'");
        assert_eq!(one_shot_line(claude, "claude", "'hi'"), "claude -p 'hi'");
    }

    #[test]
    fn trusting_a_folder_is_never_automatic_out_of_the_box() {
        assert!(builtin().iter().filter_map(|h| h.trust.as_ref()).all(|t| !t.auto));
        assert!(in_sandbox(&serde_json::json!({ "argv": ["/usr/bin/sandbox-exec", "-f", "p", "--", "zsh"] })));
        assert!(!in_sandbox(&serde_json::json!({ "argv": ["/bin/zsh", "-l", "-i", "-c", "bwrap x; /usr/bin/sandbox-exec y"] })));
        assert!(!in_sandbox(&serde_json::json!({})));
    }
}
