//! Which agent is running in a terminal, from its foreground process.

use sawtooth_detect::RuleBook;
use sawtooth_term::Process;

const RUNTIMES: &[&str] = &["node", "bun", "deno", "python", "python3"];
const SCRIPT_EXTENSIONS: &[&str] = &[".js", ".mjs", ".cjs", ".ts", ".py", ".exe"];

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn stem(name: &str) -> &str {
    SCRIPT_EXTENSIONS
        .iter()
        .find_map(|ext| name.strip_suffix(ext))
        .unwrap_or(name)
}

/// The names a process could be known by: its executable, its argv[0], and
/// for a runtime such as `node`, the script it runs and that script's package.
pub fn candidates(process: &Process) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |name: &str| {
        let name = stem(name).to_lowercase();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    };
    if let Some(argv0) = process.argv.first() {
        push(basename(argv0));
    }
    push(&process.name);

    let runtime = process
        .argv
        .first()
        .map(|a| basename(a))
        .unwrap_or(&process.name)
        .to_lowercase();
    if RUNTIMES.contains(&runtime.as_str())
        && let Some(script) = process.argv.iter().skip(1).find(|a| !a.starts_with('-'))
    {
        if let Some(after) = script.rsplit_once("node_modules/").map(|(_, rest)| rest) {
            // ".../node_modules/@google/gemini-cli/dist/index.js" → "gemini-cli"
            let mut parts = after.split('/');
            let package = match parts.next() {
                Some(scope) if scope.starts_with('@') => parts.next(),
                other => other,
            };
            if let Some(package) = package {
                push(package);
            }
        }
        push(basename(script));
    }
    out
}

/// The agent's rule-set id. `declared` is what the terminal was started as;
/// it counts only while that start command itself is in the foreground.
pub fn identify(
    rules: &RuleBook,
    foreground: Option<&Process>,
    own_pid: Option<u32>,
    declared: Option<&str>,
) -> Option<String> {
    if let Some(process) = foreground {
        for name in candidates(process) {
            if let Some(set) = rules.get(&name) {
                return Some(set.id().to_string());
            }
        }
    }
    let start_command_in_front = match (foreground, own_pid) {
        (Some(process), Some(pid)) => process.pid == pid,
        (None, _) => true,
        _ => false,
    };
    if start_command_in_front && let Some(set) = declared.and_then(|name| rules.get(name)) {
        return Some(set.id().to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, argv: &[&str]) -> Process {
        Process {
            pid: 10,
            name: name.into(),
            argv: argv.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn native_binaries_by_argv0() {
        let rules = RuleBook::bundled();
        // A versioned binary run through a `claude` symlink.
        let claude = p("2.1.30", &["claude"]);
        assert_eq!(
            identify(&rules, Some(&claude), None, None).as_deref(),
            Some("claude")
        );
        let codex = p("codex", &["/opt/homebrew/bin/codex", "--yolo"]);
        assert_eq!(
            identify(&rules, Some(&codex), None, None).as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn node_scripts_by_package() {
        let rules = RuleBook::bundled();
        let gemini = p(
            "node",
            &[
                "node",
                "/usr/local/lib/node_modules/@google/gemini-cli/dist/index.js",
            ],
        );
        assert_eq!(
            identify(&rules, Some(&gemini), None, None).as_deref(),
            Some("gemini")
        );
        let pi = p("node", &["node", "--no-warnings", "/Users/me/.npm/bin/pi"]);
        assert_eq!(
            identify(&rules, Some(&pi), None, None).as_deref(),
            Some("pi")
        );
    }

    #[test]
    fn declared_agent_only_while_its_command_is_in_front() {
        let rules = RuleBook::bundled();
        let wrapper = Process {
            pid: 42,
            name: "sh".into(),
            argv: vec!["sh".into()],
        };
        assert_eq!(
            identify(&rules, Some(&wrapper), Some(42), Some("claude")).as_deref(),
            Some("claude")
        );
        let other = Process {
            pid: 43,
            name: "ls".into(),
            argv: vec!["ls".into()],
        };
        assert_eq!(
            identify(&rules, Some(&other), Some(42), Some("claude")),
            None
        );
        assert_eq!(
            identify(&rules, Some(&p("zsh", &["-zsh"])), Some(7), None),
            None
        );
    }
}
