//! Turning a failure that keeps happening into a knowledge edit a person can review.
//!
//! The run log is the evidence, so nothing new is stored but a note that a failure
//! was already proposed. The proposal lands in the knowledge base like any other
//! agent write, and waits in the same review.

use std::collections::BTreeSet;

use crate::engine::Engine;
use crate::factories::{Factory, Part};
use crate::runs::{self, Run};

/// A failure has to come back this often before it's worth a person's time.
const MIN_FAILURES: usize = 3;
/// One bad fix loop isn't a lesson about the project.
const MIN_ITEMS: usize = 2;
/// How far back in the run log to look.
const LOOK_BACK: usize = 2000;

#[derive(Debug, PartialEq, Eq)]
pub struct Repeat {
    pub shape: String,
    pub line: String,
    pub failures: usize,
    pub items: Vec<String>,
}

/// The line that says what went wrong: the first that looks like an error, else the last.
fn error_line(output: &str) -> String {
    let lines: Vec<&str> = output.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let looks_wrong = |l: &&&str| {
        let lower = l.to_lowercase();
        ["error", "fail", "panic", "expected", "should", "cannot", "not found"].iter().any(|w| lower.contains(w))
            && !l.starts_with("npm ")
    };
    lines.iter().find(looks_wrong).or(lines.last()).map(|l| l.chars().take(200).collect()).unwrap_or_default()
}

/// The same error with different numbers, like line numbers or timings, is the same error.
fn shape(line: &str) -> String {
    let mut out = String::new();
    for c in line.chars() {
        let c = if c.is_ascii_digit() { '#' } else { c };
        if !(c == '#' && out.ends_with('#')) {
            out.push(c);
        }
    }
    out
}

/// Whether the newest failure of a check has come back often enough, across enough work.
pub fn repeated(log: &[Run], part: &str) -> Option<Repeat> {
    let failed: Vec<&Run> = log
        .iter()
        .filter(|r| r.kind == "check" && r.ok == Some(false) && r.part.as_deref() == Some(part) && r.work.is_some())
        .collect();
    let newest = failed.iter().max_by_key(|r| r.at)?;
    let line = error_line(newest.detail.as_deref().unwrap_or_default());
    let wanted = shape(&line);
    let same: Vec<&&Run> = failed
        .iter()
        .filter(|r| shape(&error_line(r.detail.as_deref().unwrap_or_default())) == wanted)
        .collect();
    let items: BTreeSet<String> = same.iter().filter_map(|r| r.work.clone()).collect();
    (same.len() >= MIN_FAILURES && items.len() >= MIN_ITEMS).then(|| Repeat {
        shape: wanted,
        line,
        failures: same.len(),
        items: items.into_iter().collect(),
    })
}

/// The knowledge edit, naming the work items so a reviewer can find the runs behind it.
pub fn proposal(check: &str, r: &Repeat) -> String {
    format!(
        "\n## Repeated failure: {check}\n\nThe {check} check keeps failing this way. Look for it before reporting work as done.\n\n> {}\n\nEvidence: {} failures across {} in the run log.\n",
        r.line,
        r.failures,
        r.items.join(", "),
    )
}

impl Engine {
    /// Called after a check fails. Proposes one knowledge edit per distinct repeated failure.
    pub(crate) fn learn_from_failure(&self, f: &Factory, part: &Part) {
        if f.knowledge().is_none() {
            return;
        }
        let Ok(log) = runs::read(&self.store, &f.id, LOOK_BACK) else { return };
        let Some(repeat) = repeated(&log, &part.id) else { return };
        let mark = format!("learned:{}:{}", part.id, repeat.shape);
        let first_time = self.store.update(&f.id, |fa| {
            let first = !fa.seen.contains(&mark);
            if first {
                fa.seen.push(mark.clone());
            }
            first
        });
        if !matches!(first_time, Ok(true)) {
            return;
        }
        let file = self.store.knowledge_file(&f.id);
        let mut text = std::fs::read_to_string(&file).unwrap_or_default();
        let edit = proposal(&part.name, &repeat);
        text.push_str(&edit);
        if std::fs::write(&file, text).is_ok() {
            self.log(&f.id, Run::new("knowledge", format!("{} kept failing the same way, so a knowledge edit is waiting for review", part.name)).part(&part.id).by("engine").detail(&edit));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fail(work: &str, at: u64, detail: &str) -> Run {
        Run { at, ..Run::new("check", "failed").ok(false).part("tests").work(work).detail(detail) }
    }

    #[test]
    fn a_failure_becomes_a_lesson_only_when_it_returns_across_different_work() {
        let same = |n: u64| format!("> x@1.0.0 test\nvalue.txt:{n} should say 2\n");
        let one_item = vec![fail("a", 1, &same(1)), fail("a", 2, &same(2)), fail("a", 3, &same(3))];
        assert_eq!(repeated(&one_item, "tests"), None);

        let log = vec![fail("a", 1, &same(1)), fail("b", 2, &same(22)), fail("b", 3, &same(3)), fail("c", 4, "something else broke")];
        assert_eq!(repeated(&log, "tests"), None, "the newest failure is a different one");

        let log = vec![fail("c", 1, "something else broke"), fail("a", 2, &same(1)), fail("b", 3, &same(22)), fail("b", 4, &same(3))];
        let r = repeated(&log, "tests").unwrap();
        assert_eq!((r.failures, r.items.clone()), (3, vec!["a".to_string(), "b".to_string()]));
        let text = proposal("Tests", &r);
        assert!(text.contains("should say 2") && text.contains("3 failures across a, b"), "{text}");
    }
}
