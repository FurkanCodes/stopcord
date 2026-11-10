//! The line a new factory can start with, and ready-made source settings.

use serde::Serialize;

use crate::factories::{Agent, Defaults, Factory, Level, LineStage, Runner, StageKind};

fn agent(id: &str, description: &str, harness: &str, prompt: &str) -> Agent {
    Agent {
        id: id.into(),
        description: description.into(),
        harness: harness.into(),
        runner: String::new(),
        prompt: prompt.into(),
    }
}

const PLANNER: &str = "You plan one change. Read the request and the code it touches. Write the smallest plan that settles what's unclear, in a file named PLAN.md, and commit it. Do not change anything else. If nothing is unclear, say so and stop.";

const BUILDER: &str = "You make one change. Stay inside what was asked. Write a failing test first when the change can be tested. Keep the diff small and leave unrelated code alone. Commit your work.";

const REVIEWER: &str = "You review a diff as if a stranger wrote it. Compare the branch with where it started. For each real problem report the file and line, the impact, and the fix, using st finding. Use --kind clear when the fix is obvious and needs no decision, and --kind judgment when a person should decide. Do not change any files and do not approve anything. A person decides.";

/// A line from plan to handoff. `harness` starts every agent, and `reviewer` the reviewer when it differs.
pub fn standard_line(f: &mut Factory, harness: &str, reviewer: &str, tests: Option<&str>, level: Level) {
    f.defaults = Defaults {
        harness: harness.to_string(),
        runner: "default".into(),
        at_once: 2,
    };
    f.runners.push(Runner {
        id: "default".into(),
        description: "Where agents run unless they say otherwise".into(),
        level,
        ..Runner::default()
    });
    f.agents = vec![
        agent("planner", "Writes the smallest plan that settles what's unclear", "", PLANNER),
        agent("builder", "Makes the change", "", BUILDER),
        agent("reviewer", "Reports problems, changes nothing", if reviewer == harness { "" } else { reviewer }, REVIEWER),
    ];
    let stage = |id: &str, kind: StageKind, then: &str| LineStage {
        id: id.into(),
        kind,
        then: then.into(),
        ..LineStage::default()
    };
    let mut line = vec![
        LineStage {
            agent: "planner".into(),
            isolated: true,
            keep: vec!["PLAN.md".into()],
            ..stage("plan", StageKind::Agent, "approve")
        },
        stage("approve", StageKind::Human, "build"),
        LineStage {
            agent: "builder".into(),
            ..stage("build", StageKind::Agent, if tests.is_some() { "verify" } else { "review" })
        },
    ];
    if tests.is_some() {
        line.push(LineStage {
            checks: vec!["tests".into()],
            protect: vec!["tests/**".into(), "test/**".into(), "**/*.test.*".into(), "**/*.spec.*".into()],
            on_fail: "build".into(),
            max_loops: 3,
            ..stage("verify", StageKind::Gate, "review")
        });
    }
    line.push(LineStage {
        agent: "reviewer".into(),
        isolated: true,
        on_fail: "build".into(),
        max_loops: 2,
        ..stage("review", StageKind::Agent, "handoff")
    });
    line.push(stage("handoff", StageKind::Human, ""));
    f.line = line;
}

/// Ready-made settings for a source, shown in the window.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceTemplate {
    pub id: &'static str,
    pub name: &'static str,
    pub command: &'static str,
    pub every: &'static str,
    /// What the person has to do first.
    pub needs: &'static str,
    /// The keys its command reads, which the person sets on the source.
    pub keys: &'static [&'static str],
}

pub fn source_templates() -> Vec<SourceTemplate> {
    vec![
        SourceTemplate {
            id: "sentry",
            name: "Sentry",
            // The stack trace comes along, fetched here with your token, so an agent never needs Sentry access.
            // The event goes through stdin because a whole event can be too long for one argument on Linux.
            command: r##"set -o pipefail; api="${SENTRY_URL:-https://sentry.io}/api/0"; auth="Authorization: Bearer $SENTRY_AUTH_TOKEN"; curl -sSf -H "$auth" "$api/projects/$SENTRY_ORG/$SENTRY_PROJECT/issues/?query=is:unresolved%20level:fatal&limit=25" | jq -c '.[]' | while read -r issue; do id=$(printf %s "$issue" | jq -r .id); { curl -sSf -H "$auth" "$api/organizations/$SENTRY_ORG/issues/$id/events/latest/" || echo '{}'; } | jq --argjson issue "$issue" '. as $event | {key: $issue.shortId, title: $issue.title, url: $issue.permalink, body: ([$issue.culprit, (($event.entries // []) | map(select(.type == "exception")) | map(.data.values // []) | (add // []) | map("\(.type): \(.value)\n" + ([.stacktrace.frames[]? | select(.inApp) | "  at \(.function) (\(.filename):\(.lineNo))"] | reverse | .[:30] | join("\n"))) | join("\n\n"))] | map(select(. != null and . != "")) | join("\n\n"))}'; done | jq -s ."##,
            every: "every 5 minutes",
            needs: "Set the keys below, and have jq installed. SENTRY_URL is only for self-hosted Sentry.",
            keys: &["SENTRY_AUTH_TOKEN", "SENTRY_ORG", "SENTRY_PROJECT", "SENTRY_URL"],
        },
        SourceTemplate {
            id: "github",
            name: "GitHub issues",
            command: r##"gh issue list --state open --json number,title,body,url | jq '[.[] | {key: ("#" + (.number|tostring)), title, body, url}]'"##,
            every: "every 5 minutes",
            needs: "Sign in with gh auth login, and have jq installed.",
            keys: &[],
        },
        SourceTemplate {
            id: "linear",
            name: "Linear",
            command: r#"curl -sf -H "Authorization: $LINEAR_API_KEY" -H "Content-Type: application/json" -d '{"query":"{ issues(filter:{state:{type:{eq:\"unstarted\"}}}, first:50){ nodes { identifier title description url } } }"}' https://api.linear.app/graphql | jq '[.data.issues.nodes[] | {key: .identifier, title, body: .description, url}]'"#,
            every: "every 5 minutes",
            needs: "Set the key below, and have jq installed.",
            keys: &["LINEAR_API_KEY"],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::validate;
    use crate::factories::{Kind, Part};

    fn with_tests() -> Factory {
        Factory {
            parts: vec![Part {
                id: "tests".into(),
                kind: Kind::Check,
                ..Part::default()
            }],
            ..Factory::default()
        }
    }

    #[test]
    fn the_standard_line_is_valid_with_or_without_tests() {
        let mut f = with_tests();
        standard_line(&mut f, "claude", "codex", Some("npm test"), Level::Contained);
        assert_eq!(validate(&f), Vec::<String>::new());
        assert!(f.line.iter().any(|s| s.id == "verify"));

        let mut f = Factory::default();
        standard_line(&mut f, "claude", "claude", None, Level::Worktree);
        assert_eq!(validate(&f), Vec::<String>::new());
        assert!(!f.line.iter().any(|s| s.id == "verify"));
    }

    #[test]
    fn the_reviewer_runs_on_another_harness_when_one_is_given() {
        let mut f = Factory::default();
        standard_line(&mut f, "claude", "codex", None, Level::Worktree);
        assert_eq!(f.agents.iter().find(|a| a.id == "reviewer").unwrap().harness, "codex");
        assert_eq!(f.agents.iter().find(|a| a.id == "builder").unwrap().harness, "");
    }

    #[test]
    fn a_review_can_only_send_clear_fixes_back() {
        let mut f = Factory::default();
        standard_line(&mut f, "claude", "codex", None, Level::Worktree);
        let review = f.line.iter().find(|s| s.id == "review").unwrap();
        assert_eq!(review.then, "handoff");
        // It can only send clear fixes back, a limited number of times. It never reaches past the person at handoff.
        assert_eq!((review.on_fail.as_str(), review.max_loops, review.then.as_str()), ("build", 2, "handoff"));
    }

    #[test]
    fn every_source_template_has_what_it_needs() {
        for t in source_templates() {
            assert!(!t.command.is_empty() && !t.needs.is_empty(), "{}", t.id);
            assert!(crate::engine::parse_schedule(t.every).is_some(), "{}", t.id);
            assert!(t.command.contains("key"), "{}", t.id);
            for k in t.keys {
                assert!(crate::vm::secret_name(k).is_ok(), "{}: {k} isn't a name a key can have", t.id);
                assert!(t.command.contains(&format!("${k}")) || t.command.contains(&format!("${{{k}")), "{} never reads {k}", t.id);
            }
        }
    }
}
