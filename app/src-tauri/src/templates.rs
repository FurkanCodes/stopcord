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

const REVIEWER: &str = "You review a diff as if a stranger wrote it. Compare the branch with where it started. For each real problem report the file and line, the impact, and the fix, using st finding. Do not change any files and do not approve anything. A person decides.";

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
    pub webhook: &'static str,
    pub secret: &'static str,
    pub every: &'static str,
    /// What the person has to do first.
    pub needs: &'static str,
}

pub fn source_templates() -> Vec<SourceTemplate> {
    vec![
        SourceTemplate {
            id: "sentry",
            name: "Sentry",
            command: r#"curl -sf -H "Authorization: Bearer $SENTRY_AUTH_TOKEN" "https://sentry.io/api/0/projects/$SENTRY_ORG/$SENTRY_PROJECT/issues/?query=is:unresolved%20level:fatal" | jq '[.[] | {key: .shortId, title: .title, body: .culprit, url: .permalink}]'"#,
            webhook: "sentry",
            secret: "SENTRY_WEBHOOK_SECRET",
            every: "every 5 minutes",
            needs: "Set SENTRY_AUTH_TOKEN, SENTRY_ORG and SENTRY_PROJECT in your shell, and have jq installed.",
        },
        SourceTemplate {
            id: "github",
            name: "GitHub issues",
            command: r##"gh issue list --state open --json number,title,body,url | jq '[.[] | {key: ("#" + (.number|tostring)), title, body, url}]'"##,
            webhook: "github",
            secret: "GITHUB_WEBHOOK_SECRET",
            every: "every 5 minutes",
            needs: "Sign in with gh auth login, and have jq installed.",
        },
        SourceTemplate {
            id: "linear",
            name: "Linear",
            command: r#"curl -sf -H "Authorization: $LINEAR_API_KEY" -H "Content-Type: application/json" -d '{"query":"{ issues(filter:{state:{type:{eq:\"unstarted\"}}}, first:50){ nodes { identifier title description url } } }"}' https://api.linear.app/graphql | jq '[.data.issues.nodes[] | {key: .identifier, title, body: .description, url}]'"#,
            webhook: "linear",
            secret: "LINEAR_WEBHOOK_SECRET",
            every: "every 5 minutes",
            needs: "Set LINEAR_API_KEY in your shell, and have jq installed.",
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
    fn a_review_never_moves_work_by_itself() {
        let mut f = Factory::default();
        standard_line(&mut f, "claude", "codex", None, Level::Worktree);
        let review = f.line.iter().find(|s| s.id == "review").unwrap();
        assert_eq!(review.then, "handoff");
        assert!(review.on_fail.is_empty());
    }

    #[test]
    fn every_source_template_has_what_it_needs() {
        for t in source_templates() {
            assert!(!t.command.is_empty() && !t.webhook.is_empty() && !t.secret.is_empty() && !t.needs.is_empty(), "{}", t.id);
            assert!(crate::engine::parse_schedule(t.every).is_some(), "{}", t.id);
            assert!(t.command.contains("key"), "{}", t.id);
        }
    }
}
