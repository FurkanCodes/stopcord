//! The line: which stage a work item is at and what happens next.
//!
//! This is only the rule book. It changes a `Factory` and says what the engine
//! has to do, and the engine does the starting, typing and running. Every step
//! is saved before anything happens because of it, so a restart can pick up
//! from where the line says the work is.
//!
//! Agents can finish a stage. Only the engine or a person picks what comes next.

use crate::factories::{Factory, Finding, LineStage, Note, Stage, StageKind, Waiting, WorkItem};

/// Notes are for the next roles to read, so each is capped and only the newest are kept.
const NOTE_CHARS: usize = 4000;
const NOTES_KEPT: usize = 12;
/// How many times a stage may hand work on with `st send` before a person is asked, when it sets no limit.
const SENDS: u32 = 2;

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Keeps what a stage wrote for the ones after it.
fn add_note(w: &mut WorkItem, stage: &LineStage, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let text: String = if text.chars().count() > NOTE_CHARS { format!("{}…", text.chars().take(NOTE_CHARS).collect::<String>()) } else { text.to_string() };
    w.notes.push(Note { stage: stage.id.clone(), agent: stage.agent.clone(), text, at: now_ms() });
    let extra = w.notes.len().saturating_sub(NOTES_KEPT);
    w.notes.drain(..extra);
}

/// What the engine has to do now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Put this agent to work. `note` is why work came back to it, if it did.
    Agent {
        stage: String,
        agent: String,
        note: String,
    },
    /// Run this gate.
    Gate { stage: String },
    /// A person has to look.
    Person { stage: String, why: Waiting, text: String },
    /// The line has finished.
    End { outcome: String },
    /// Nothing to do: the event didn't apply.
    Nothing,
}

/// What a person can do to a work item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Approve,
    SendBack { note: String },
    /// The agent has really finished, whatever it did or didn't report.
    TreatAsDone,
    Merged,
    Drop,
}

fn stage<'a>(f: &'a Factory, id: &str) -> Option<&'a LineStage> {
    f.line.iter().find(|s| s.id == id)
}

fn item<'a>(f: &'a mut Factory, key: &str) -> Option<&'a mut WorkItem> {
    f.work.iter_mut().find(|w| w.ticket.key == key)
}

/// Puts a claimed work item on the line. It starts where a person chose, or at the first stage.
pub fn begin(f: &mut Factory, key: &str) -> Step {
    let Some(w) = f.work.iter().find(|w| w.ticket.key == key) else {
        return Step::Nothing;
    };
    let at = if w.start_at.is_empty() || stage(f, &w.start_at).is_none() {
        f.line.first().map(|s| s.id.clone())
    } else {
        Some(w.start_at.clone())
    };
    match at {
        Some(at) => enter(f, key, &at, ""),
        None => Step::Nothing,
    }
}

/// Moves a work item to a stage and says what that stage needs.
pub fn enter(f: &mut Factory, key: &str, id: &str, note: &str) -> Step {
    let Some(s) = stage(f, id).cloned() else {
        return Step::Nothing;
    };
    let Some(w) = item(f, key) else {
        return Step::Nothing;
    };
    w.line_stage = s.id.clone();
    w.stage = Stage::Running;
    // The terminal answering for this stage, once one is. Its old one may still be open for later.
    w.terminal = None;
    match s.kind {
        StageKind::Agent => {
            w.waiting = Waiting::No;
            w.stage_note = note.to_string();
            Step::Agent {
                stage: s.id,
                agent: s.agent,
                note: note.to_string(),
            }
        }
        StageKind::Gate => {
            w.waiting = Waiting::No;
            Step::Gate { stage: s.id }
        }
        StageKind::Human => {
            let why = if s.then.is_empty() { Waiting::Handoff } else { Waiting::Approve };
            w.waiting = why;
            let text = if why == Waiting::Handoff {
                format!("{key} is ready for you to merge")
            } else {
                format!("{key} is waiting for your approval")
            };
            Step::Person { stage: s.id, why, text }
        }
    }
}

fn next(f: &mut Factory, key: &str, s: &LineStage) -> Step {
    if s.then.is_empty() {
        finish(f, key, "finished the line")
    } else {
        enter(f, key, &s.then, "")
    }
}

fn finish(f: &mut Factory, key: &str, outcome: &str) -> Step {
    if let Some(w) = item(f, key) {
        w.waiting = Waiting::No;
        w.outcome = outcome.to_string();
    }
    Step::End {
        outcome: outcome.to_string(),
    }
}

fn current(f: &Factory, key: &str) -> Option<LineStage> {
    let w = f.work.iter().find(|w| w.ticket.key == key)?;
    if w.stage != Stage::Running {
        return None;
    }
    stage(f, &w.line_stage).cloned()
}

/// The agent said it's done. Only moves work if an agent stage is really what's running.
pub fn agent_done(f: &mut Factory, key: &str, summary: &str) -> Step {
    let Some(s) = current(f, key) else {
        return Step::Nothing;
    };
    if s.kind != StageKind::Agent {
        return Step::Nothing;
    }
    if let Some(w) = item(f, key) {
        w.summary = summary.to_string();
        add_note(w, &s, summary);
    }
    if let Some(back) = send_back_clear_findings(f, key, &s) {
        return back;
    }
    next(f, key, &s)
}

/// Findings with an obvious fix go back to the builder, up to the stage's limit, so a person
/// only sees what needs a decision. Past the limit they stay on the item for handoff.
fn send_back_clear_findings(f: &mut Factory, key: &str, s: &LineStage) -> Option<Step> {
    if s.on_fail.is_empty() {
        return None;
    }
    let w = item(f, key)?;
    let is_clear = |x: &Finding| x.stage == s.id && x.kind == "clear";
    let clear: Vec<Finding> = w.findings.iter().filter(|x| is_clear(x)).cloned().collect();
    let count = w.loops.entry(s.id.clone()).or_insert(0);
    if clear.is_empty() || *count >= s.max_loops {
        return None;
    }
    *count += 1;
    w.findings.retain(|x| !is_clear(x));
    let list: Vec<String> = clear
        .iter()
        .map(|x| match (x.file.is_empty(), x.line) {
            (true, _) => format!("- {}", x.text),
            (false, 0) => format!("- {}: {}", x.file, x.text),
            (false, line) => format!("- {}:{line}: {}", x.file, x.text),
        })
        .collect();
    let note = format!("The {} stage found these. Fix them, commit, then finish.\n\n{}", s.id, list.join("\n"));
    Some(enter(f, key, &s.on_fail, &note))
}

/// A merge command ran, but only the branch landing finishes the work.
pub fn landing(f: &mut Factory, key: &str) -> bool {
    match item(f, key) {
        Some(w) if w.waiting == Waiting::Handoff => {
            w.waiting = Waiting::Landing;
            true
        }
        _ => false,
    }
}

/// The agent stopped: it asked something, or went quiet without saying it was done.
/// An agent hands the work to another stage, with a message for it. Each stage may do this only so
/// often before a person is asked, so two roles can't pass a ticket back and forth forever.
pub fn agent_sends(f: &mut Factory, key: &str, to: &str, message: &str) -> Step {
    let Some(s) = current(f, key) else {
        return Step::Nothing;
    };
    if s.kind != StageKind::Agent || to == s.id {
        return Step::Nothing;
    }
    let Some(target) = stage(f, to).cloned() else {
        let text = format!("{key}: {} tried to send the work to {to}, which isn't a stage on this line. It said: {message}", s.id);
        return agent_stopped(f, key, &text);
    };
    let limit = if s.max_loops > 0 { s.max_loops } else { SENDS };
    let Some(w) = item(f, key) else {
        return Step::Nothing;
    };
    let sent = w.loops.entry(format!("send:{}", s.id)).or_insert(0);
    if *sent >= limit {
        let text = format!("{key}: {} wants to send the work to {to} again, after {limit} times. It said: {message}", s.id);
        return agent_stopped(f, key, &text);
    }
    *sent += 1;
    w.summary = message.to_string();
    add_note(w, &s, &format!("sent the work to {to}: {message}"));
    let who = if s.agent.is_empty() { String::new() } else { format!(" ({})", s.agent) };
    let note = format!("The {} stage{who} sent the work to you, {to}, with this message:\n\n{message}", s.id);
    enter(f, key, &target.id, &note)
}

pub fn agent_stopped(f: &mut Factory, key: &str, text: &str) -> Step {
    let Some(s) = current(f, key) else {
        return Step::Nothing;
    };
    if s.kind != StageKind::Agent {
        return Step::Nothing;
    }
    if let Some(w) = item(f, key) {
        w.waiting = Waiting::Question;
    }
    Step::Person {
        stage: s.id,
        why: Waiting::Question,
        text: text.to_string(),
    }
}

/// What a person sees for a question. One still unanswered stays above the new one, so neither hides the other.
pub fn question_text(f: &Factory, key: &str, text: &str) -> String {
    let earlier = f.work.iter().find(|w| w.ticket.key == key && w.waiting == Waiting::Question).and_then(|w| f.alerts.iter().find(|a| a.work == w.id && a.kind == "question"));
    match earlier {
        Some(alert) => format!("{}\n\n{text}", alert.text),
        None => format!("{key} asks: {text}"),
    }
}

/// A person answered in the terminal, so the agent is working again.
pub fn agent_resumed(f: &mut Factory, key: &str) {
    if let Some(w) = item(f, key)
        && w.waiting == Waiting::Question
    {
        w.waiting = Waiting::No;
    }
}

/// The active terminal went away while its stage was running.
pub fn terminal_lost(f: &mut Factory, key: &str) -> Step {
    let Some(s) = current(f, key) else {
        return Step::Nothing;
    };
    if let Some(w) = item(f, key) {
        w.waiting = Waiting::Failed;
    }
    Step::Person {
        stage: s.id,
        why: Waiting::Failed,
        text: format!("{key} lost its terminal. Its work is still on its branch. Send it back, or drop it."),
    }
}

pub fn add_finding(f: &mut Factory, key: &str, finding: Finding) {
    if let Some(w) = item(f, key) {
        // Each re-review starts fresh, and shouldn't say the same thing to a person twice.
        let repeat = w.findings.iter().any(|x| x.file == finding.file && x.line == finding.line && x.text == finding.text && x.kind == finding.kind);
        if !repeat {
            w.findings.push(finding);
        }
    }
}

/// A gate finished. Failing sends work back until the line's limit, then asks a person.
pub fn gate_result(f: &mut Factory, key: &str, passed: bool, output: &str) -> Step {
    let Some(s) = current(f, key) else {
        return Step::Nothing;
    };
    if s.kind != StageKind::Gate {
        return Step::Nothing;
    }
    if passed {
        return next(f, key, &s);
    }
    let Some(w) = item(f, key) else {
        return Step::Nothing;
    };
    let count = w.loops.entry(s.id.clone()).or_insert(0);
    *count += 1;
    let count = *count;
    if s.on_fail.is_empty() || count > s.max_loops {
        w.waiting = Waiting::Failed;
        let tries = if s.on_fail.is_empty() {
            "It has nowhere to go back to.".to_string()
        } else {
            format!("It came back {} times.", s.max_loops)
        };
        return Step::Person {
            stage: s.id.clone(),
            why: Waiting::Failed,
            text: format!("{key} still fails {}. {tries}", s.id),
        };
    }
    let note = format!("The {} stage failed. Fix it, then finish.\n\n{output}", s.id);
    enter(f, key, &s.on_fail, &note)
}

/// Where sending work back goes: the stage's `onFail`, then the nearest earlier one (a reviewer
/// can't change anything, so work goes on to whoever it sends fixes to), then the last agent stage.
fn send_back_to(f: &Factory, s: &LineStage) -> Option<String> {
    if !s.on_fail.is_empty() {
        return Some(s.on_fail.clone());
    }
    let at = f.line.iter().position(|t| t.id == s.id)?;
    let earlier = &f.line[..at];
    earlier
        .iter()
        .rev()
        .find(|t| !t.on_fail.is_empty())
        .map(|t| t.on_fail.clone())
        .or_else(|| earlier.iter().rev().find(|t| t.kind == StageKind::Agent).map(|t| t.id.clone()))
}

/// A person decided something.
pub fn person(f: &mut Factory, key: &str, action: Action) -> Step {
    if action == Action::Drop {
        return finish(f, key, "dropped");
    }
    let Some(s) = current(f, key) else {
        return Step::Nothing;
    };
    let waiting = f.work.iter().find(|w| w.ticket.key == key).map(|w| w.waiting).unwrap_or_default();
    match action {
        Action::Approve if waiting == Waiting::Approve => next(f, key, &s),
        // A failed gate can be waved through. Only a person can do that.
        Action::Approve if waiting == Waiting::Failed && s.kind == StageKind::Gate => next(f, key, &s),
        Action::TreatAsDone if s.kind == StageKind::Agent && waiting != Waiting::No => next(f, key, &s),
        Action::Merged if matches!(waiting, Waiting::Handoff | Waiting::Landing) => finish(f, key, "merged"),
        Action::SendBack { note } if waiting != Waiting::No => match send_back_to(f, &s) {
            Some(to) => {
                let note = if note.trim().is_empty() {
                    "This came back to you. Look at it again.".to_string()
                } else {
                    note
                };
                enter(f, key, &to, &note)
            }
            None => Step::Nothing,
        },
        _ => Step::Nothing,
    }
}

/// What the engine types first for a stage. The role prompt comes first, then the ticket, then how to report.
fn brief_body(prompt: &str, w: &WorkItem, skills: &[(String, String, String)]) -> String {
    let mut out = String::new();
    if !prompt.trim().is_empty() {
        out.push_str(prompt.trim());
        out.push_str("\n\n");
    }
    let head = format!("{}: {}", w.ticket.key, w.ticket.title);
    out.push_str(head.trim());
    for extra in [&w.ticket.body, &w.ticket.url] {
        if !extra.trim().is_empty() {
            out.push_str("\n\n");
            out.push_str(extra.trim());
        }
    }
    if !w.notes.is_empty() {
        out.push_str("\n\nNotes from the stages before this one, oldest first:");
        for n in &w.notes {
            let who = if n.agent.is_empty() { String::new() } else { format!(", {}", n.agent) };
            out.push_str(&format!("\n\n[{}{who}] {}", n.stage, n.text.trim()));
        }
    } else if !w.summary.trim().is_empty() {
        out.push_str("\n\nThe stage before this one said: ");
        out.push_str(w.summary.trim());
    }
    if !skills.is_empty() {
        out.push_str("\n\nSkills you can read when you need them:");
        for (name, description, path) in skills {
            out.push_str(&format!("\n- {name}: {description} ({path})"));
        }
    }
    out
}

/// `others` are the stages this one can hand work to with `st send`.
pub fn brief(prompt: &str, w: &WorkItem, skills: &[(String, String, String)], st: &str, others: &[String]) -> String {
    let finding = if prompt.contains("st finding") {
        format!("\n\nTo report a finding, run: {st} finding --file a.rs --line 9 --severity high --kind clear \"what's wrong and the fix\"")
    } else {
        String::new()
    };
    let send = if others.is_empty() {
        String::new()
    } else {
        format!(" To hand the work to another stage with a message instead, run: {st} send <stage> \"what it needs to know\". The stages: {}.", others.join(", "))
    };
    format!(
        "{}{finding}\n\nIf your role changes files, commit them. If it says not to, leave every file alone. When you've finished, run: {st} done \"your note\". The stages after you read it, so say what you did, what you aren't sure of, and what they should check. For a long note, pipe it in: {st} done - < note.md.{send} If you need a person, run: {st} ask \"your question\".",
        brief_body(prompt, w, skills)
    )
}

/// A scorer changes nothing, and its score is what ends its stage.
pub fn scorer_brief(prompt: &str, w: &WorkItem, skills: &[(String, String, String)], st: &str, _others: &[String]) -> String {
    format!(
        "{}\n\nDon't change or commit anything. Look at this work, then give it a score from 0 to 10 as your last step: {st} score 7 \"one line on why\"",
        brief_body(prompt, w, skills)
    )
}

/// The longest diff a role that answers once is shown. Past it, it's told the rest was cut.
pub const ONCE_DIFF_CHARS: usize = 60_000;

/// A role that answers once can't run anything, so it gets the diff instead, and its answer is its report.
pub fn once_brief(prompt: &str, w: &WorkItem, scorer: bool, diff: &str) -> String {
    let mut out = brief_body(prompt, w, &[]);
    if diff.trim().is_empty() {
        out.push_str("\n\nNothing has changed on this work's branch yet.");
    } else {
        let cut: String = diff.chars().take(ONCE_DIFF_CHARS).collect();
        let more = if cut.len() < diff.len() { "\n[the rest of the diff was cut]" } else { "" };
        out.push_str(&format!("\n\nThe changes so far, against where this work started:\n<diff>\n{}{more}\n</diff>", cut.trim_end()));
    }
    out.push_str(
        "\n\nYou can't run anything or open files, so answer from what's here. Your answer is your note: the stages after you read it, so say what you found, what you aren't sure of, and what they should check.\n\nPut each problem on its own line, like this: FINDING clear high src/a.rs:9 what's wrong and the fix. Write clear when the fix is obvious and judgment when a person should decide, then high, medium or low.",
    );
    if scorer {
        out.push_str("\n\nEnd with one line that scores the work from 0 to 10, like this: score 7 one line on why");
    }
    // A role prompt written for a terminal says to run st. Here the lines above stand in for it.
    if ["done", "finding", "score", "ask", "send"].iter().any(|v| prompt.contains(&format!("st {v}"))) {
        out.push_str("\n\nWhere your role says to run st, don't: write the line above instead, and this answer is your report.");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::{Ticket, WorkItem};

    fn s(id: &str, kind: StageKind, then: &str) -> LineStage {
        LineStage {
            id: id.into(),
            kind,
            agent: if kind == StageKind::Agent { format!("{id}-agent") } else { String::new() },
            then: then.into(),
            ..LineStage::default()
        }
    }

    fn factory() -> Factory {
        Factory {
            line: vec![
                s("plan", StageKind::Agent, "approve"),
                s("approve", StageKind::Human, "build"),
                s("build", StageKind::Agent, "verify"),
                LineStage {
                    on_fail: "build".into(),
                    max_loops: 2,
                    ..s("verify", StageKind::Gate, "review")
                },
                s("review", StageKind::Agent, "handoff"),
                s("handoff", StageKind::Human, ""),
            ],
            work: vec![WorkItem {
                id: "w_1".into(),
                ticket: Ticket {
                    key: "A".into(),
                    title: "Fix it".into(),
                    ..Ticket::default()
                },
                stage: Stage::Starting,
                ..WorkItem::default()
            }],
            ..Factory::default()
        }
    }

    #[test]
    fn a_role_that_answers_once_gets_the_diff_and_answers_instead_of_running_st() {
        let w = &factory().work[0];
        let b = once_brief("Review it. Report each problem using st finding.", w, true, "diff --git a/x b/x\n+y\n");
        assert!(b.contains("<diff>\ndiff --git a/x b/x\n+y\n</diff>") && b.contains("FINDING clear high"), "{b}");
        assert!(b.contains("score 7") && b.contains("Where your role says to run st, don't"), "{b}");
        let plain = once_brief("Read it first and judge.", w, false, "");
        assert!(plain.contains("Nothing has changed") && !plain.contains("score 7") && !plain.contains("run st, don't"), "{plain}");
        let long = once_brief("", w, false, &"x".repeat(ONCE_DIFF_CHARS + 10));
        assert!(long.contains("[the rest of the diff was cut]"));
    }

    fn at(f: &Factory) -> (&str, Waiting) {
        (f.work[0].line_stage.as_str(), f.work[0].waiting)
    }

    fn to(f: &mut Factory, stage: &str) {
        enter(f, "A", stage, "");
    }

    #[test]
    fn work_starts_at_the_first_stage() {
        let mut f = factory();
        assert_eq!(
            begin(&mut f, "A"),
            Step::Agent {
                stage: "plan".into(),
                agent: "plan-agent".into(),
                note: "".into()
            }
        );
        assert_eq!(f.work[0].stage, Stage::Running);
    }

    #[test]
    fn a_person_can_choose_where_it_starts() {
        let mut f = factory();
        f.work[0].start_at = "build".into();
        assert!(matches!(begin(&mut f, "A"), Step::Agent { stage, .. } if stage == "build"));
    }

    #[test]
    fn an_agent_finishing_moves_to_the_next_stage() {
        let mut f = factory();
        begin(&mut f, "A");
        assert!(matches!(agent_done(&mut f, "A", "planned"), Step::Person { why: Waiting::Approve, .. }));
        assert_eq!(at(&f), ("approve", Waiting::Approve));
        assert_eq!(f.work[0].summary, "planned");
    }

    #[test]
    fn an_agent_saying_done_at_a_gate_or_person_stage_moves_nothing() {
        let mut f = factory();
        to(&mut f, "verify");
        assert_eq!(agent_done(&mut f, "A", "trust me"), Step::Nothing);
        assert_eq!(at(&f).0, "verify");
        to(&mut f, "approve");
        assert_eq!(agent_done(&mut f, "A", "trust me"), Step::Nothing);
        assert_eq!(at(&f), ("approve", Waiting::Approve));
    }

    #[test]
    fn a_passing_gate_moves_on() {
        let mut f = factory();
        to(&mut f, "verify");
        assert!(matches!(gate_result(&mut f, "A", true, ""), Step::Agent { stage, .. } if stage == "review"));
    }

    #[test]
    fn a_failing_gate_goes_back_with_the_failure_until_the_limit() {
        let mut f = factory();
        to(&mut f, "verify");
        for n in 1..=2 {
            match gate_result(&mut f, "A", false, "assert failed") {
                Step::Agent { stage, note, .. } => {
                    assert_eq!(stage, "build");
                    assert!(note.contains("assert failed"), "{note}");
                }
                other => panic!("{other:?}"),
            }
            assert_eq!(f.work[0].loops["verify"], n);
            to(&mut f, "verify");
        }
        assert!(matches!(gate_result(&mut f, "A", false, "x"), Step::Person { why: Waiting::Failed, .. }));
        assert_eq!(at(&f), ("verify", Waiting::Failed));
    }

    #[test]
    fn a_reviewer_finishing_always_goes_to_a_person() {
        let mut f = factory();
        to(&mut f, "review");
        assert!(matches!(agent_done(&mut f, "A", "looks clean"), Step::Person { why: Waiting::Handoff, .. }));
    }

    #[test]
    fn nothing_but_a_person_finishes_the_line() {
        let mut f = factory();
        to(&mut f, "handoff");
        assert_eq!(agent_done(&mut f, "A", "merged it"), Step::Nothing);
        assert_eq!(gate_result(&mut f, "A", true, ""), Step::Nothing);
        assert_eq!(person(&mut f, "A", Action::Merged), Step::End { outcome: "merged".into() });
    }

    #[test]
    fn approving_only_works_on_something_waiting_for_approval() {
        let mut f = factory();
        to(&mut f, "build");
        assert_eq!(person(&mut f, "A", Action::Approve), Step::Nothing);
        to(&mut f, "approve");
        assert!(matches!(person(&mut f, "A", Action::Approve), Step::Agent { stage, .. } if stage == "build"));
    }

    #[test]
    fn sending_back_goes_to_whoever_makes_the_fixes_with_the_note() {
        let mut f = factory();
        to(&mut f, "handoff");
        match person(&mut f, "A", Action::SendBack { note: "rename it".into() }) {
            Step::Agent { stage, note, .. } => {
                assert_eq!(stage, "build");
                assert_eq!(note, "rename it");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_person_can_wave_a_failed_gate_through_but_a_running_one_cant_be() {
        let mut f = factory();
        to(&mut f, "verify");
        assert_eq!(person(&mut f, "A", Action::Approve), Step::Nothing);
        f.work[0].waiting = Waiting::Failed;
        assert!(matches!(person(&mut f, "A", Action::Approve), Step::Agent { stage, .. } if stage == "review"));
    }

    #[test]
    fn a_second_question_doesnt_hide_the_first() {
        let mut f = factory();
        to(&mut f, "build");
        assert_eq!(question_text(&f, "A", "which db?"), "A asks: which db?");
        agent_stopped(&mut f, "A", "A asks: which db?");
        let work = f.work.last().unwrap().id.clone();
        f.alerts.push(crate::factories::Alert { key: "k".into(), at: 0, text: "A asks: which db?".into(), terminal: None, kind: "question".into(), work, host: String::new() });
        assert_eq!(question_text(&f, "A", "test connectivity"), "A asks: which db?\n\ntest connectivity");
        agent_resumed(&mut f, "A");
        assert_eq!(question_text(&f, "A", "next one"), "A asks: next one", "an answered question doesn't come back");
    }

    #[test]
    fn an_agent_that_stops_waits_for_a_person_then_carries_on() {
        let mut f = factory();
        to(&mut f, "build");
        assert!(matches!(agent_stopped(&mut f, "A", "which db?"), Step::Person { why: Waiting::Question, .. }));
        agent_resumed(&mut f, "A");
        assert_eq!(at(&f), ("build", Waiting::No));
    }

    #[test]
    fn treat_as_done_needs_a_stopped_agent() {
        let mut f = factory();
        to(&mut f, "build");
        assert_eq!(person(&mut f, "A", Action::TreatAsDone), Step::Nothing);
        agent_stopped(&mut f, "A", "quiet");
        assert!(matches!(person(&mut f, "A", Action::TreatAsDone), Step::Gate { stage } if stage == "verify"));
    }

    #[test]
    fn a_lost_terminal_waits_for_a_person_instead_of_ending_the_work() {
        let mut f = factory();
        to(&mut f, "build");
        assert!(matches!(terminal_lost(&mut f, "A"), Step::Person { why: Waiting::Failed, .. }));
        assert_eq!(f.work[0].stage, Stage::Running);
        assert!(matches!(person(&mut f, "A", Action::SendBack { note: "".into() }), Step::Agent { .. } | Step::Nothing));
    }

    #[test]
    fn dropping_works_from_anywhere() {
        let mut f = factory();
        to(&mut f, "build");
        assert_eq!(person(&mut f, "A", Action::Drop), Step::End { outcome: "dropped".into() });
    }

    #[test]
    fn an_ended_item_ignores_everything() {
        let mut f = factory();
        to(&mut f, "build");
        f.work[0].stage = Stage::Ended;
        assert_eq!(agent_done(&mut f, "A", "x"), Step::Nothing);
        assert_eq!(gate_result(&mut f, "A", true, ""), Step::Nothing);
        assert_eq!(person(&mut f, "A", Action::Approve), Step::Nothing);
    }

    #[test]
    fn every_stage_after_reads_the_notes_of_the_ones_before() {
        let mut f = factory();
        begin(&mut f, "A");
        agent_done(&mut f, "A", "planned three steps. unsure about the migration");
        person(&mut f, "A", Action::Approve);
        agent_done(&mut f, "A", "built steps 1 and 2");
        let notes: Vec<&str> = f.work[0].notes.iter().map(|n| n.stage.as_str()).collect();
        assert_eq!(notes, ["plan", "build"]);
        let text = brief("You review.", &f.work[0], &[], "st", &[]);
        assert!(text.contains("[plan, plan-agent] planned three steps") && text.contains("[build, build-agent] built steps 1 and 2"), "{text}");
        assert!(text.find("[plan,").unwrap() < text.find("[build,").unwrap());
    }

    #[test]
    fn a_stage_sends_work_to_another_with_a_message_until_its_limit() {
        let mut f = factory();
        begin(&mut f, "A");
        agent_done(&mut f, "A", "plan");
        person(&mut f, "A", Action::Approve);
        assert_eq!(at(&f).0, "build");
        let step = agent_sends(&mut f, "A", "plan", "step 3 needs a migration");
        let Step::Agent { stage, note, .. } = step else { panic!("{step:?}") };
        assert_eq!(stage, "plan");
        assert!(note.contains("The build stage") && note.contains("step 3 needs a migration"), "{note}");
        assert!(f.work[0].notes.last().unwrap().text.contains("sent the work to plan"));
        // Back at build, it may send once more, then a person is asked.
        agent_done(&mut f, "A", "replanned");
        person(&mut f, "A", Action::Approve);
        assert!(matches!(agent_sends(&mut f, "A", "plan", "again"), Step::Agent { .. }));
        agent_done(&mut f, "A", "replanned again");
        person(&mut f, "A", Action::Approve);
        let Step::Person { why, text, .. } = agent_sends(&mut f, "A", "plan", "once more") else { panic!() };
        assert_eq!(why, Waiting::Question);
        assert!(text.contains("again, after 2 times"), "{text}");
    }

    #[test]
    fn sending_to_a_stage_that_isnt_there_asks_a_person() {
        let mut f = factory();
        begin(&mut f, "A");
        assert_eq!(agent_sends(&mut f, "A", "plan", "self"), Step::Nothing);
        let Step::Person { text, .. } = agent_sends(&mut f, "A", "deploy", "ship it") else { panic!() };
        assert!(text.contains("isn't a stage on this line"), "{text}");
    }

    #[test]
    fn the_brief_has_the_role_the_ticket_the_skills_and_how_to_report() {
        let f = factory();
        let text = brief(
            "You build.",
            &f.work[0],
            &[("crash".into(), "Investigate a crash".into(), "/d/skills/crash/SKILL.md".into())],
            "/h/bin/st",
            &["review".to_string()],
        );
        assert!(text.starts_with("You build.\n\nA: Fix it"), "{text}");
        assert!(text.contains("/h/bin/st send <stage>") && text.contains("The stages: review."), "{text}");
        assert!(text.contains("/d/skills/crash/SKILL.md"));
        assert!(text.contains("/h/bin/st done"));
        assert!(text.contains("/h/bin/st ask"));
    }

    #[test]
    fn the_same_finding_is_only_kept_once() {
        let mut f = factory();
        f.work.push(WorkItem { ticket: Ticket { key: "R".into(), ..Ticket::default() }, ..WorkItem::default() });
        for _ in 0..2 {
            add_finding(&mut f, "R", Finding { file: "a.rs".into(), text: "same".into(), ..Finding::default() });
        }
        assert_eq!(f.work.last().unwrap().findings.len(), 1);
    }

    #[test]
    fn clear_findings_loop_up_to_the_limit_and_judgment_calls_wait_for_a_person() {
        let mut f = factory();
        let review = f.line.iter_mut().find(|s| s.id == "review").unwrap();
        review.on_fail = "build".into();
        review.max_loops = 1;
        f.work.push(WorkItem { id: "w".into(), ticket: crate::factories::Ticket { key: "R".into(), ..Default::default() }, stage: Stage::Running, line_stage: "review".into(), ..Default::default() });
        let found = |kind: &str| Finding { file: "a.rs".into(), line: 9, text: "off by one".into(), stage: "review".into(), kind: kind.into(), ..Default::default() };
        add_finding(&mut f, "R", found("clear"));
        add_finding(&mut f, "R", found("judgment"));
        let back = agent_done(&mut f, "R", "reviewed");
        assert!(matches!(&back, Step::Agent { note, .. } if note.contains("a.rs:9") ), "{back:?}");
        assert_eq!(f.work.last_mut().unwrap().findings.len(), 1, "the judgment call stays for the person");

        f.work.last_mut().unwrap().line_stage = "review".into();
        add_finding(&mut f, "R", found("clear"));
        assert!(matches!(agent_done(&mut f, "R", "again"), Step::Person { why: Waiting::Handoff, .. }), "past the limit it goes to the person");
        assert_eq!(f.work.last_mut().unwrap().findings.len(), 2, "and the person sees what's left");
    }
}
