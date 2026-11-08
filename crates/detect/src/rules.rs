//! Rule files: loading, checking and matching them.

use std::collections::BTreeMap;
use std::path::Path;

use regex::Regex;
use serde::Deserialize;

use crate::{Detection, Region, Screen, State};

const MAX_RULES: usize = 128;
const MAX_DEPTH: usize = 8;
const MAX_PATTERN_CHARS: usize = 512;

/// The rule files that ship with Sawtooth.
const BUNDLED: &[(&str, &str)] = &[
    ("claude", include_str!("../rules/claude.toml")),
    ("codex", include_str!("../rules/codex.toml")),
    ("gemini", include_str!("../rules/gemini.toml")),
    ("opencode", include_str!("../rules/opencode.toml")),
    ("pi", include_str!("../rules/pi.toml")),
];

// ---- The file format -------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDef {
    id: String,
    /// Process and command names that mean this agent.
    #[serde(default)]
    names: Vec<String>,
    // Provenance fields from herdr's format. Accepted, not used.
    #[serde(default)]
    version: Option<toml::Value>,
    #[serde(default)]
    min_engine_version: Option<u32>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    rules: Vec<RuleDef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StateDef {
    Idle,
    Working,
    Blocked,
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleDef {
    id: String,
    state: Option<StateDef>,
    #[serde(default)]
    priority: i32,
    #[serde(default = "whole_recent")]
    region: String,
    #[serde(default)]
    skip_state_update: bool,
    // herdr marks which rules show something on screen. Accepted, not used.
    #[serde(default)]
    visible_idle: bool,
    #[serde(default)]
    visible_blocker: bool,
    #[serde(default)]
    visible_working: bool,
    #[serde(default)]
    all: Vec<GateDef>,
    #[serde(default)]
    any: Vec<GateDef>,
    #[serde(default, rename = "not")]
    not_: Vec<GateDef>,
    #[serde(default)]
    contains: Vec<String>,
    #[serde(default)]
    regex: Vec<String>,
    #[serde(default)]
    line_regex: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GateDef {
    #[serde(default)]
    all: Vec<GateDef>,
    #[serde(default)]
    any: Vec<GateDef>,
    #[serde(default, rename = "not")]
    not_: Vec<GateDef>,
    #[serde(default)]
    contains: Vec<String>,
    #[serde(default)]
    regex: Vec<String>,
    #[serde(default)]
    line_regex: Vec<String>,
}

fn whole_recent() -> String {
    "whole_recent".into()
}

// ---- Compiled rules --------------------------------------------------------

/// A test on a piece of screen text. Every listed condition must hold:
/// each `contains` needle is present (ignoring case), each `regex` matches,
/// each `line_regex` matches some line, every `all` gate matches, at least one
/// `any` gate matches (when there are any), and no `not` gate matches.
#[derive(Debug)]
struct Gate {
    contains: Vec<String>,
    regex: Vec<Regex>,
    line_regex: Vec<Regex>,
    all: Vec<Gate>,
    any: Vec<Gate>,
    not_: Vec<Gate>,
}

impl Gate {
    fn compile(
        contains: &[String],
        regex: &[String],
        line_regex: &[String],
        all: &[GateDef],
        any: &[GateDef],
        not_: &[GateDef],
        depth: usize,
    ) -> Result<Self, String> {
        if depth > MAX_DEPTH {
            return Err(format!("gates nest deeper than {MAX_DEPTH}"));
        }
        let pattern = |p: &String| -> Result<Regex, String> {
            if p.chars().count() > MAX_PATTERN_CHARS {
                return Err(format!(
                    "a pattern is longer than {MAX_PATTERN_CHARS} characters"
                ));
            }
            Regex::new(p).map_err(|e| format!("bad regex {p:?}: {e}"))
        };
        let nested = |defs: &[GateDef]| -> Result<Vec<Gate>, String> {
            defs.iter()
                .map(|g| {
                    Gate::compile(
                        &g.contains,
                        &g.regex,
                        &g.line_regex,
                        &g.all,
                        &g.any,
                        &g.not_,
                        depth + 1,
                    )
                })
                .collect()
        };
        Ok(Gate {
            contains: contains.iter().map(|s| s.to_lowercase()).collect(),
            regex: regex.iter().map(pattern).collect::<Result<_, _>>()?,
            line_regex: line_regex.iter().map(pattern).collect::<Result<_, _>>()?,
            all: nested(all)?,
            any: nested(any)?,
            not_: nested(not_)?,
        })
    }

    fn is_empty(&self) -> bool {
        self.contains.is_empty()
            && self.regex.is_empty()
            && self.line_regex.is_empty()
            && self.all.is_empty()
            && self.any.is_empty()
    }

    fn matches(&self, text: &str, lower: &str) -> bool {
        self.contains
            .iter()
            .all(|needle| lower.contains(needle.as_str()))
            && self.regex.iter().all(|re| re.is_match(text))
            && self
                .line_regex
                .iter()
                .all(|re| text.lines().any(|line| re.is_match(line)))
            && self.all.iter().all(|g| g.matches(text, lower))
            && (self.any.is_empty() || self.any.iter().any(|g| g.matches(text, lower)))
            && !self.not_.iter().any(|g| g.matches(text, lower))
    }
}

#[derive(Debug)]
struct Rule {
    id: String,
    state: State,
    priority: i32,
    region: Region,
    keep_previous: bool,
    gate: Gate,
}

/// One agent's rules.
#[derive(Debug)]
pub struct RuleSet {
    id: String,
    names: Vec<String>,
    rules: Vec<Rule>,
}

impl RuleSet {
    /// Reads a rule file. Errors name the rule and say what's wrong.
    pub fn parse(source: &str) -> Result<Self, String> {
        let def: FileDef = toml::from_str(source).map_err(|e| e.to_string())?;
        let _ = (&def.version, def.min_engine_version, &def.updated_at);
        if def.id.trim().is_empty() {
            return Err("the file has no id".into());
        }
        if def.rules.len() > MAX_RULES {
            return Err(format!("{}: more than {MAX_RULES} rules", def.id));
        }
        let mut names: Vec<String> = std::iter::once(&def.id)
            .chain(&def.names)
            .chain(&def.aliases)
            .map(|n| n.to_lowercase())
            .collect();
        names.dedup();

        let mut rules = Vec::with_capacity(def.rules.len());
        for r in &def.rules {
            let _ = (r.visible_idle, r.visible_blocker, r.visible_working);
            let fail = |msg: String| format!("{} rule {:?}: {msg}", def.id, r.id);
            let region = Region::parse(&r.region).map_err(fail)?;
            let gate = Gate::compile(
                &r.contains,
                &r.regex,
                &r.line_regex,
                &r.all,
                &r.any,
                &r.not_,
                0,
            )
            .map_err(fail)?;
            if gate.is_empty() {
                return Err(fail(
                    "it has nothing to match, so it would match every screen".into(),
                ));
            }
            rules.push(Rule {
                id: r.id.clone(),
                state: match r.state {
                    Some(StateDef::Idle) => State::Idle,
                    Some(StateDef::Working) => State::Working,
                    Some(StateDef::Blocked) => State::Blocked,
                    Some(StateDef::Unknown) | None => State::Unknown,
                },
                priority: r.priority,
                region,
                keep_previous: r.skip_state_update,
                gate,
            });
        }
        Ok(RuleSet {
            id: def.id,
            names,
            rules,
        })
    }

    /// The agent's id, such as `claude`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// True when `name` is one of this agent's names.
    pub fn answers_to(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        self.names.contains(&name)
    }

    /// Reads one screen. The matching rule with the highest priority wins; on a
    /// tie the first one in the file does. No match means idle.
    pub fn detect(&self, screen: &Screen<'_>) -> Detection {
        let mut best: Option<&Rule> = None;
        for rule in &self.rules {
            if best.is_some_and(|b| b.priority >= rule.priority) {
                continue;
            }
            let text = rule.region.slice(screen);
            if rule.gate.matches(text, &text.to_lowercase()) {
                best = Some(rule);
            }
        }
        match best {
            Some(rule) => Detection {
                state: (!rule.keep_previous).then_some(rule.state),
                rule: Some(rule.id.clone()),
            },
            None => Detection {
                state: Some(State::Idle),
                rule: None,
            },
        }
    }

    /// Every rule and whether it matches this screen, for debugging a rule file.
    pub fn explain(&self, screen: &Screen<'_>) -> Vec<(String, i32, bool)> {
        self.rules
            .iter()
            .map(|rule| {
                let text = rule.region.slice(screen);
                (
                    rule.id.clone(),
                    rule.priority,
                    rule.gate.matches(text, &text.to_lowercase()),
                )
            })
            .collect()
    }
}

/// Every agent's rules, keyed by id.
#[derive(Debug, Default)]
pub struct RuleBook {
    sets: BTreeMap<String, RuleSet>,
}

impl RuleBook {
    /// The rules that ship with Sawtooth.
    pub fn bundled() -> Self {
        let mut book = RuleBook::default();
        for (name, source) in BUNDLED {
            let set = RuleSet::parse(source)
                .unwrap_or_else(|e| panic!("bundled rules {name}.toml are broken: {e}"));
            book.sets.insert(set.id.clone(), set);
        }
        book
    }

    /// Adds every `*.toml` in `dir`. A file replaces the bundled rules with the
    /// same id. Returns the files that couldn't be read, with why; the rest still load.
    pub fn load_dir(&mut self, dir: &Path) -> Vec<String> {
        let mut problems = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return problems;
        };
        let mut paths: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
            .collect();
        paths.sort();
        for path in paths {
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|s| RuleSet::parse(&s))
            {
                Ok(set) => {
                    self.sets.insert(set.id.clone(), set);
                }
                Err(e) => problems.push(format!("{}: {e}", path.display())),
            }
        }
        problems
    }

    pub fn insert(&mut self, set: RuleSet) {
        self.sets.insert(set.id.clone(), set);
    }

    /// The rules for an agent, by id or any of its names.
    pub fn get(&self, name: &str) -> Option<&RuleSet> {
        self.sets
            .get(&name.to_lowercase())
            .or_else(|| self.sets.values().find(|set| set.answers_to(name)))
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.sets.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen<'a>(text: &'a str, title: &'a str) -> Screen<'a> {
        Screen {
            text,
            title,
            progress: "",
        }
    }

    fn state(book: &RuleBook, agent: &str, text: &str, title: &str) -> Option<State> {
        book.get(agent).unwrap().detect(&screen(text, title)).state
    }

    #[test]
    fn bundled_rules_load() {
        let book = RuleBook::bundled();
        let ids: Vec<_> = book.ids().collect();
        assert_eq!(ids, ["claude", "codex", "gemini", "opencode", "pi"]);
        assert!(book.get("claude-code").is_some());
        assert!(book.get("gemini-cli").is_some());
    }

    #[test]
    fn highest_priority_wins_and_nothing_means_idle() {
        let set = RuleSet::parse(
            r#"
            id = "toy"
            [[rules]]
            id = "busy"
            state = "working"
            priority = 10
            contains = ["esc to interrupt"]
            [[rules]]
            id = "asks"
            state = "blocked"
            priority = 20
            contains = ["Proceed?"]
            "#,
        )
        .unwrap();
        let both = screen("thinking (esc to interrupt)\nProceed?", "");
        assert_eq!(set.detect(&both).state, Some(State::Blocked));
        assert_eq!(set.detect(&both).rule.as_deref(), Some("asks"));
        let busy = screen("thinking (ESC to interrupt)", "");
        assert_eq!(set.detect(&busy).state, Some(State::Working));
        let quiet = screen("> ", "");
        assert_eq!(
            set.detect(&quiet),
            Detection {
                state: Some(State::Idle),
                rule: None
            }
        );
    }

    #[test]
    fn gates_all_any_not() {
        let set = RuleSet::parse(
            r#"
            id = "toy"
            [[rules]]
            id = "ask"
            state = "blocked"
            any = [{ contains = ["allow?"] }, { line_regex = ['^\s*❯ Yes'] }]
            not = [{ contains = ["esc to cancel"] }]
            "#,
        )
        .unwrap();
        assert_eq!(
            set.detect(&screen("  ❯ Yes", "")).state,
            Some(State::Blocked)
        );
        assert_eq!(
            set.detect(&screen("allow?\nesc to cancel", "")).state,
            Some(State::Idle)
        );
    }

    #[test]
    fn keep_previous_rule_returns_none() {
        let set = RuleSet::parse(
            r#"
            id = "toy"
            [[rules]]
            id = "viewer"
            state = "unknown"
            skip_state_update = true
            contains = ["showing detailed transcript"]
            "#,
        )
        .unwrap();
        let d = set.detect(&screen("Showing detailed transcript", ""));
        assert_eq!(d.state, None);
        assert_eq!(d.rule.as_deref(), Some("viewer"));
    }

    #[test]
    fn rejects_broken_files() {
        assert!(
            RuleSet::parse("id = \"x\"\n[[rules]]\nid = \"r\"\nstate = \"working\"\n").is_err()
        );
        assert!(
            RuleSet::parse(
                "id = \"x\"\n[[rules]]\nid = \"r\"\nregion = \"middle\"\ncontains = [\"a\"]\n"
            )
            .is_err()
        );
        assert!(RuleSet::parse("id = \"x\"\n[[rules]]\nid = \"r\"\nregex = [\"(\"]\n").is_err());
        assert!(RuleSet::parse("id = \"x\"\ncolour = \"red\"\n").is_err());
    }

    // A few screens per bundled agent, built from the strings each rule looks for.
    #[test]
    fn claude_title_spinner_is_working() {
        let book = RuleBook::bundled();
        assert_eq!(
            state(&book, "claude", "> ", "⠋ Fixing the tax bug"),
            Some(State::Working)
        );
        assert_eq!(
            state(&book, "claude", "> ", "✳ Claude Code"),
            Some(State::Idle)
        );
    }

    #[test]
    fn codex_action_required_title_is_blocked() {
        let book = RuleBook::bundled();
        assert_eq!(
            state(&book, "codex", "", "Action Required"),
            Some(State::Blocked)
        );
        assert_eq!(state(&book, "codex", "", "⠋ codex"), Some(State::Working));
    }

    #[test]
    fn gemini_opencode_pi() {
        let book = RuleBook::bundled();
        assert_eq!(
            state(
                &book,
                "gemini",
                "│ Do you trust the files in this folder?\n│ ● 1. Trust folder (x)",
                ""
            ),
            Some(State::Blocked)
        );
        assert_eq!(
            state(&book, "gemini", "⠼ Thinking (esc to cancel, 3s)", ""),
            Some(State::Working)
        );
        assert_eq!(
            state(&book, "gemini", "│ Apply this change?\n│ ● 1. Yes", ""),
            Some(State::Blocked)
        );
        assert_eq!(
            state(
                &book,
                "opencode",
                "△ Permission required\nbash: rm -rf dist",
                ""
            ),
            Some(State::Blocked)
        );
        assert_eq!(
            state(&book, "opencode", "  esc to interrupt", ""),
            Some(State::Working)
        );
        assert_eq!(state(&book, "pi", "⠙ Working...", ""), Some(State::Working));
        assert_eq!(state(&book, "pi", "> ", ""), Some(State::Idle));
    }
}
