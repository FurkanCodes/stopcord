//! Work items: one ticket's journey through a factory, saved at every step.
//!
//! A ticket goes queued → starting → running → ended, and each step is written
//! to `state/work.json` before anything happens because of it. So a crash, a quit
//! or a reboot can't lose one, and one can't be started twice. This module is
//! only the bookkeeping; the engine does the starting.

use std::collections::{BTreeMap, HashSet};

use crate::factories::{Alert, Factory, Stage, Ticket, Waiting, WorkItem, slug};

/// Tries to start a ticket before giving up on it.
pub const MAX_START_ATTEMPTS: u32 = 3;
/// How long to wait before each retry, in milliseconds.
const RETRY_AFTER_MS: [u64; 2] = [30_000, 120_000];
/// Ended items kept, so what happened to a ticket can still be answered.
const KEEP_ENDED: usize = 50;
/// Alerts kept. Past this the oldest go, so a stuck factory can't grow without end.
const KEEP_ALERTS: usize = 30;
/// Proposals remembered so a rejected one doesn't come back.
const KEEP_SEEN: usize = 2000;

/// A terminal the daemon says is alive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Live {
    pub id: String,
    pub name: String,
}

/// What `settle` found, for the run log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The terminal closed while Sawtooth was watching.
    Closed { key: String, branch: String },
    /// The terminal was already gone when Sawtooth looked: a crash, a reboot, a restarted daemon.
    Lost { key: String, branch: String },
    /// It was starting when the app stopped, and no terminal came of it. It's queued again.
    Requeued { key: String },
    /// It was starting when the app stopped, and its terminal is open. Nobody knows if it got the ticket.
    Adopted { key: String, terminal: String },
    /// A stage's terminal went away. The line decides what that means.
    StageLost { key: String },
}

/// What happens after a ticket fails to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    AfterSecs(u64),
    GaveUp,
}

/// The branch a ticket's terminal works on: "LIN-92" gives `lin-92`, and a bare
/// number such as "#41" gives `ticket-41`.
pub fn branch_for(key: &str) -> String {
    let slug = slug(key);
    if slug.chars().all(|c| c.is_ascii_digit()) {
        format!("ticket-{slug}")
    } else {
        slug
    }
}

/// Adds or replaces the alert for `key`.
pub fn raise(f: &mut Factory, key: &str, text: &str, terminal: Option<&str>, now: u64) {
    clear(f, key);
    f.alerts.push(Alert {
        key: key.to_string(),
        at: now,
        text: text.to_string(),
        terminal: terminal.map(String::from),
        ..Alert::default()
    });
    let extra = f.alerts.len().saturating_sub(KEEP_ALERTS);
    f.alerts.drain(..extra);
}

/// Adds an alert a person can act on from NEEDS.
pub fn raise_kind(f: &mut Factory, alert: Alert) {
    clear(f, &alert.key);
    f.alerts.push(alert);
    let extra = f.alerts.len().saturating_sub(KEEP_ALERTS);
    f.alerts.drain(..extra);
}

pub fn clear(f: &mut Factory, key: &str) {
    f.alerts.retain(|a| a.key != key);
}

/// A person took over a terminal, so "stopped asking" about it no longer needs saying.
pub fn clear_fixes(f: &mut Factory, terminal: &str) {
    f.alerts
        .retain(|a| !(a.key.starts_with("fix:") && a.terminal.as_deref() == Some(terminal)));
}

/// The alert key for a ticket.
pub fn ticket_alert(key: &str) -> String {
    format!("ticket:{key}")
}

/// Files from before work items keep their tickets.
pub fn migrate(f: &mut Factory) {
    let mut used = next_id(f);
    for w in f.work.iter_mut().filter(|w| w.id.is_empty()) {
        w.id = format!("w_{used:04}");
        used += 1;
    }
    if !f.work.is_empty() {
        return;
    }
    let running: Vec<(String, String)> = f.running.iter().map(|(k, t)| (k.clone(), t.clone())).collect();
    for (key, terminal) in &running {
        f.work.push(WorkItem {
            ticket: Ticket {
                key: key.clone(),
                ..Ticket::default()
            },
            stage: Stage::Running,
            branch: branch_for(key),
            terminal: Some(terminal.clone()),
            id: new_id(f),
            ..WorkItem::default()
        });
    }
    for ticket in f.queue.clone() {
        let item = queued(ticket, new_id(f));
        f.work.push(item);
    }
}

/// The lowest number not used by a work item, counting up from the highest one in use.
fn next_id(f: &Factory) -> u32 {
    f.work
        .iter()
        .filter_map(|w| w.id.strip_prefix("w_")?.parse::<u32>().ok())
        .max()
        .map_or(1, |n| n + 1)
}

fn new_id(f: &Factory) -> String {
    format!("w_{:04}", next_id(f))
}

fn queued(ticket: Ticket, id: String) -> WorkItem {
    WorkItem {
        id,
        branch: branch_for(&ticket.key),
        ticket,
        ..WorkItem::default()
    }
}

/// Rebuilds what the window reads. A ticket still starting has no terminal to show, so it counts as waiting.
pub fn refresh_view(f: &mut Factory) {
    f.queue = f
        .work
        .iter()
        .filter(|w| matches!(w.stage, Stage::Queued | Stage::Starting))
        .map(|w| w.ticket.clone())
        .collect();
    f.running = f
        .work
        .iter()
        .filter(|w| w.stage == Stage::Running)
        .filter_map(|w| Some((w.ticket.key.clone(), w.terminal.clone()?)))
        .collect::<BTreeMap<_, _>>();
}

/// Queues tickets. A ticket already in the factory is skipped; one that ended starts over. Returns how many were new.
pub fn feed(f: &mut Factory, tickets: Vec<Ticket>) -> usize {
    let mut added = 0;
    for ticket in tickets {
        if ticket.key.is_empty() {
            continue;
        }
        match f.work.iter().position(|w| w.ticket.key == ticket.key) {
            Some(i) if f.work[i].stage != Stage::Ended => continue,
            Some(i) => {
                f.work.remove(i);
            }
            None => {}
        }
        clear(f, &ticket_alert(&ticket.key));
        let item = queued(ticket, new_id(f));
        f.work.push(item);
        added += 1;
    }
    added
}

/// Adds tickets from outside as proposals, which wait for a person. A ticket already
/// here, or seen from this source before, is left out. Returns how many were new.
pub fn propose(f: &mut Factory, source: &str, tickets: Vec<Ticket>, auto_start: bool) -> usize {
    let mut added = 0;
    for ticket in tickets {
        if ticket.key.is_empty() {
            continue;
        }
        let seen_key = format!("{source}:{}", ticket.key);
        if f.work.iter().any(|w| w.ticket.key == ticket.key) || f.seen.contains(&seen_key) {
            continue;
        }
        f.seen.push(seen_key);
        let mut item = queued(ticket, new_id(f));
        item.source = source.to_string();
        item.stage = if auto_start { Stage::Queued } else { Stage::Proposed };
        f.work.push(item);
        added += 1;
    }
    let extra = f.seen.len().saturating_sub(KEEP_SEEN);
    f.seen.drain(..extra);
    added
}

/// A person accepted a proposal. It joins the queue, to start at `start_at`, or the first stage when empty.
pub fn accept(f: &mut Factory, key: &str, start_at: &str) -> bool {
    match item(f, key) {
        Some(w) if w.stage == Stage::Proposed => {
            w.stage = Stage::Queued;
            w.start_at = start_at.to_string();
            true
        }
        _ => false,
    }
}

/// A person doesn't want a proposal. It's forgotten, and `seen` stops it coming back.
pub fn reject(f: &mut Factory, key: &str) -> bool {
    let before = f.work.len();
    f.work.retain(|w| !(w.ticket.key == key && w.stage == Stage::Proposed));
    f.work.len() != before
}

/// Marks the next ticket as starting, if there's room. Tickets still starting
/// count against `room`, and two tickets on one branch never run together.
pub fn claim(f: &mut Factory, room: usize, now: u64) -> Option<WorkItem> {
    let live = |w: &&WorkItem| matches!(w.stage, Stage::Starting | Stage::Running);
    let busy: Vec<String> = f.work.iter().filter(live).map(|w| w.branch.clone()).collect();
    // Work waiting for a person isn't using a seat.
    let seated = f.work.iter().filter(live).filter(|w| w.waiting == Waiting::No).count();
    if seated >= room {
        return None;
    }
    let item = f
        .work
        .iter_mut()
        .find(|w| w.stage == Stage::Queued && w.retry_at <= now && !busy.contains(&w.branch))?;
    item.stage = Stage::Starting;
    Some(item.clone())
}

fn item<'a>(f: &'a mut Factory, key: &str) -> Option<&'a mut WorkItem> {
    f.work.iter_mut().find(|w| w.ticket.key == key)
}

/// A ticket got its terminal. `worktree` is empty when it works in the project itself.
pub fn started(f: &mut Factory, key: &str, terminal: &str, worktree: &str) {
    if let Some(w) = item(f, key) {
        w.stage = Stage::Running;
        w.terminal = Some(terminal.to_string());
        w.worktree = worktree.to_string();
    }
}

/// A ticket's terminal couldn't start. It waits and tries again, then gives up.
pub fn start_failed(f: &mut Factory, key: &str, why: &str, now: u64) -> Retry {
    let Some(w) = item(f, key) else {
        return Retry::GaveUp;
    };
    w.attempts += 1;
    let retry = match RETRY_AFTER_MS.get(w.attempts as usize - 1) {
        Some(ms) if w.attempts < MAX_START_ATTEMPTS => {
            w.stage = Stage::Queued;
            w.retry_at = now + ms;
            Retry::AfterSecs(ms / 1000)
        }
        _ => {
            w.stage = Stage::Ended;
            w.ended_at = now;
            w.outcome = format!("couldn't start: {why}");
            Retry::GaveUp
        }
    };
    trim(f);
    retry
}

/// Squares the books with the terminals that are really alive. `starting` holds
/// the tickets this run of the app is starting right now; any other ticket marked
/// starting was interrupted. `lost` says this is the first look since connecting,
/// so a terminal missing now went while nobody was watching.
pub fn settle(
    f: &mut Factory,
    live: &[Live],
    starting: &HashSet<String>,
    lost: bool,
    now: u64,
) -> Vec<Change> {
    let mut changes = Vec::new();
    for w in &mut f.work {
        let key = w.ticket.key.clone();
        match w.stage {
            Stage::Running if !w.line_stage.is_empty() => {
                // Work on a line outlives any one terminal. Only a stage in progress can lose one.
                let gone = w.terminal.as_ref().is_some_and(|t| !live.iter().any(|l| &l.id == t));
                if gone && w.waiting == Waiting::No {
                    w.terminal = None;
                    changes.push(Change::StageLost { key });
                }
            }
            Stage::Running => {
                let alive = w
                    .terminal
                    .as_ref()
                    .is_some_and(|t| live.iter().any(|l| &l.id == t));
                if alive {
                    continue;
                }
                let branch = w.branch.clone();
                w.stage = Stage::Ended;
                w.ended_at = now;
                if lost {
                    w.outcome = "its terminal was lost".into();
                    changes.push(Change::Lost { key, branch });
                } else {
                    w.outcome = "its terminal closed".into();
                    changes.push(Change::Closed { key, branch });
                }
            }
            Stage::Starting if !starting.contains(&key) => {
                let prefix = format!("{key} · ");
                let open = live
                    .iter()
                    .find(|l| l.name == key || l.name.starts_with(&prefix));
                if let Some(l) = open {
                    w.stage = Stage::Running;
                    w.terminal = Some(l.id.clone());
                    changes.push(Change::Adopted {
                        key,
                        terminal: l.id.clone(),
                    });
                } else {
                    w.stage = Stage::Queued;
                    changes.push(Change::Requeued { key });
                }
            }
            _ => {}
        }
    }
    f.alerts.retain(|a| {
        a.terminal
            .as_ref()
            .is_none_or(|t| live.iter().any(|l| &l.id == t))
    });
    trim(f);
    changes
}

/// Drops the oldest ended items past the limit.
fn trim(f: &mut Factory) {
    let ended = f.work.iter().filter(|w| w.stage == Stage::Ended).count();
    let mut extra = ended.saturating_sub(KEEP_ENDED);
    if extra == 0 {
        return;
    }
    let mut oldest: Vec<u64> = f
        .work
        .iter()
        .filter(|w| w.stage == Stage::Ended)
        .map(|w| w.ended_at)
        .collect();
    oldest.sort_unstable();
    let cutoff = oldest[extra - 1];
    f.work.retain(|w| {
        if extra > 0 && w.stage == Stage::Ended && w.ended_at <= cutoff {
            extra -= 1;
            false
        } else {
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket(key: &str) -> Ticket {
        Ticket {
            key: key.into(),
            title: format!("title of {key}"),
            ..Ticket::default()
        }
    }

    fn fed(keys: &[&str]) -> Factory {
        let mut f = Factory::default();
        feed(&mut f, keys.iter().map(|k| ticket(k)).collect());
        f
    }

    fn live(id: &str, name: &str) -> Live {
        Live {
            id: id.into(),
            name: name.into(),
        }
    }

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    fn stage(f: &Factory, key: &str) -> Stage {
        f.work.iter().find(|w| w.ticket.key == key).unwrap().stage
    }

    fn alerts(f: &Factory) -> Vec<&str> {
        f.alerts.iter().map(|a| a.key.as_str()).collect()
    }

    #[test]
    fn one_alert_per_thing() {
        let mut f = Factory::default();
        raise(&mut f, "ticket:A", "first", None, 1);
        raise(&mut f, "ticket:A", "second", None, 2);
        assert_eq!(f.alerts.len(), 1);
        assert_eq!((f.alerts[0].text.as_str(), f.alerts[0].at), ("second", 2));
        clear(&mut f, "ticket:A");
        assert!(f.alerts.is_empty());
    }

    #[test]
    fn alerts_dont_pile_up_forever() {
        let mut f = Factory::default();
        for i in 0..(KEEP_ALERTS as u64 + 5) {
            raise(&mut f, &format!("k{i}"), "x", None, i);
        }
        assert_eq!(f.alerts.len(), KEEP_ALERTS);
        assert_eq!(f.alerts[0].key, "k5", "the oldest go first");
    }

    #[test]
    fn feeding_a_ticket_again_clears_its_alert() {
        let mut f = fed(&["A"]);
        claim(&mut f, 1, 0).unwrap();
        started(&mut f, "A", "t1", "");
        settle(&mut f, &[], &none(), true, 5);
        raise(&mut f, "ticket:A", "lost", None, 5);
        feed(&mut f, vec![ticket("A")]);
        assert!(alerts(&f).is_empty());
    }

    #[test]
    fn an_alert_about_a_terminal_goes_when_the_terminal_does() {
        let mut f = Factory::default();
        raise(&mut f, "fix:t1:tests", "stopped", Some("t1"), 1);
        raise(&mut f, "fix:t2:tests", "stopped", Some("t2"), 1);
        raise(&mut f, "schedule:s", "missed", None, 1);
        settle(&mut f, &[live("t2", "x")], &none(), false, 5);
        assert_eq!(alerts(&f), ["fix:t2:tests", "schedule:s"]);
    }

    #[test]
    fn a_person_taking_over_clears_only_that_terminals_fix_alerts() {
        let mut f = Factory::default();
        raise(&mut f, "fix:t1:tests", "stopped", Some("t1"), 1);
        raise(&mut f, "fix:t2:tests", "stopped", Some("t2"), 1);
        raise(&mut f, "ticket:A", "never got it", Some("t1"), 1);
        clear_fixes(&mut f, "t1");
        assert_eq!(alerts(&f), ["fix:t2:tests", "ticket:A"]);
    }

    #[test]
    fn branches() {
        assert_eq!(branch_for("LIN-92"), "lin-92");
        assert_eq!(branch_for("#41"), "ticket-41");
    }

    #[test]
    fn a_claimed_ticket_is_never_off_the_books() {
        let mut f = fed(&["A"]);
        let item = claim(&mut f, 1, 0).unwrap();
        assert_eq!(item.ticket.key, "A");
        // The moment it stops being queued it is recorded as starting.
        assert_eq!(stage(&f, "A"), Stage::Starting);
    }

    #[test]
    fn a_starting_ticket_counts_against_the_limit() {
        let mut f = fed(&["A", "B"]);
        assert!(claim(&mut f, 1, 0).is_some());
        assert!(claim(&mut f, 1, 0).is_none(), "at_once 1 must not start B yet");
        assert_eq!(stage(&f, "B"), Stage::Queued);
    }

    #[test]
    fn feeding_a_starting_or_running_ticket_again_does_nothing() {
        let mut f = fed(&["A"]);
        claim(&mut f, 1, 0).unwrap();
        assert_eq!(feed(&mut f, vec![ticket("A")]), 0);
        started(&mut f, "A", "t1", "");
        assert_eq!(feed(&mut f, vec![ticket("A")]), 0);
        assert_eq!(f.work.len(), 1);
    }

    #[test]
    fn an_ended_ticket_can_be_fed_again() {
        let mut f = fed(&["A"]);
        claim(&mut f, 1, 0).unwrap();
        started(&mut f, "A", "t1", "");
        settle(&mut f, &[], &none(), false, 5);
        assert_eq!(stage(&f, "A"), Stage::Ended);
        assert_eq!(feed(&mut f, vec![ticket("A")]), 1);
        assert_eq!(f.work.len(), 1);
        assert_eq!(stage(&f, "A"), Stage::Queued);
    }

    #[test]
    fn two_tickets_with_the_same_branch_never_run_together() {
        let mut f = fed(&["LIN-1", "lin 1"]);
        assert_eq!(claim(&mut f, 5, 0).unwrap().ticket.key, "LIN-1");
        assert!(claim(&mut f, 5, 0).is_none());
    }

    #[test]
    fn a_ticket_that_cant_start_waits_and_doesnt_block_the_next() {
        let mut f = fed(&["BAD", "GOOD"]);
        claim(&mut f, 1, 1000).unwrap();
        assert_eq!(start_failed(&mut f, "BAD", "nope", 1000), Retry::AfterSecs(30));
        assert_eq!(stage(&f, "BAD"), Stage::Queued);
        // Inside the wait, the ticket behind it gets the room.
        assert_eq!(claim(&mut f, 1, 2000).unwrap().ticket.key, "GOOD");
    }

    #[test]
    fn a_ticket_that_never_starts_ends_with_the_reason() {
        let mut f = fed(&["BAD"]);
        let mut now = 0;
        let mut last = Retry::GaveUp;
        for _ in 0..MAX_START_ATTEMPTS {
            assert!(claim(&mut f, 1, now).is_some());
            last = start_failed(&mut f, "BAD", "branch is checked out", now);
            now += 10 * 60 * 1000;
        }
        assert_eq!(last, Retry::GaveUp);
        assert_eq!(stage(&f, "BAD"), Stage::Ended);
        assert!(f.work[0].outcome.contains("branch is checked out"));
        assert!(claim(&mut f, 1, now).is_none());
    }

    #[test]
    fn a_terminal_that_closes_ends_its_ticket_and_says_where_the_work_is() {
        let mut f = fed(&["LIN-1"]);
        claim(&mut f, 1, 0).unwrap();
        started(&mut f, "LIN-1", "t1", "");
        let changes = settle(&mut f, &[], &none(), false, 5);
        assert_eq!(
            changes,
            [Change::Closed {
                key: "LIN-1".into(),
                branch: "lin-1".into()
            }]
        );
    }

    #[test]
    fn a_terminal_gone_after_a_restart_is_lost_not_closed() {
        let mut f = fed(&["LIN-1"]);
        claim(&mut f, 1, 0).unwrap();
        started(&mut f, "LIN-1", "t1", "");
        let changes = settle(&mut f, &[], &none(), true, 5);
        assert_eq!(
            changes,
            [Change::Lost {
                key: "LIN-1".into(),
                branch: "lin-1".into()
            }]
        );
        assert_eq!(f.work[0].ended_at, 5);
    }

    #[test]
    fn a_live_terminal_keeps_its_ticket_running() {
        let mut f = fed(&["A"]);
        claim(&mut f, 1, 0).unwrap();
        started(&mut f, "A", "t1", "");
        assert!(settle(&mut f, &[live("t1", "A")], &none(), false, 5).is_empty());
        assert_eq!(stage(&f, "A"), Stage::Running);
    }

    #[test]
    fn a_ticket_this_app_is_still_starting_is_left_alone() {
        let mut f = fed(&["A"]);
        claim(&mut f, 1, 0).unwrap();
        let mine: HashSet<String> = ["A".to_string()].into();
        assert!(settle(&mut f, &[], &mine, false, 5).is_empty());
        assert_eq!(stage(&f, "A"), Stage::Starting);
    }

    #[test]
    fn a_ticket_interrupted_while_starting_goes_back_in_the_queue() {
        let mut f = fed(&["A"]);
        claim(&mut f, 1, 0).unwrap();
        let changes = settle(&mut f, &[], &none(), true, 5);
        assert_eq!(changes, [Change::Requeued { key: "A".into() }]);
        assert_eq!(stage(&f, "A"), Stage::Queued);
    }

    #[test]
    fn a_ticket_interrupted_after_its_terminal_opened_is_adopted_not_doubled() {
        let mut f = fed(&["LIN-1"]);
        claim(&mut f, 1, 0).unwrap();
        let changes = settle(
            &mut f,
            &[live("t9", "LIN-1 · title of LIN-1")],
            &none(),
            true,
            5,
        );
        assert_eq!(
            changes,
            [Change::Adopted {
                key: "LIN-1".into(),
                terminal: "t9".into()
            }]
        );
        assert_eq!(stage(&f, "LIN-1"), Stage::Running);
        assert_eq!(f.work[0].terminal.as_deref(), Some("t9"));
    }

    #[test]
    fn old_files_become_work_items() {
        let mut f = Factory {
            queue: vec![ticket("Q")],
            running: BTreeMap::from([("R".to_string(), "t1".to_string())]),
            ..Factory::default()
        };
        migrate(&mut f);
        assert_eq!(stage(&f, "Q"), Stage::Queued);
        assert_eq!(stage(&f, "R"), Stage::Running);
        assert_eq!(f.work.iter().find(|w| w.ticket.key == "R").unwrap().terminal.as_deref(), Some("t1"));
    }

    #[test]
    fn the_window_sees_starting_tickets_as_queued() {
        let mut f = fed(&["A", "B", "C"]);
        claim(&mut f, 3, 0).unwrap();
        claim(&mut f, 3, 0).unwrap();
        started(&mut f, "B", "t2", "");
        refresh_view(&mut f);
        let queued: Vec<_> = f.queue.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(queued, ["A", "C"]);
        assert_eq!(f.running.get("B").map(String::as_str), Some("t2"));
        assert_eq!(f.running.len(), 1);
    }

    #[test]
    fn only_the_newest_ended_items_are_kept() {
        let mut f = Factory::default();
        for i in 0..(KEEP_ENDED + 5) {
            let key = format!("T{i}");
            feed(&mut f, vec![ticket(&key)]);
            claim(&mut f, 1, 0).unwrap();
            started(&mut f, &key, "t", "");
            settle(&mut f, &[], &none(), false, i as u64);
        }
        assert_eq!(f.work.len(), KEEP_ENDED);
        assert!(f.work.iter().all(|w| w.ended_at >= 5));
    }
}

#[cfg(test)]
mod proposal_tests {
    use super::*;

    fn t(key: &str) -> Ticket {
        Ticket {
            key: key.into(),
            ..Ticket::default()
        }
    }

    #[test]
    fn proposals_wait_for_a_person_and_never_start_by_themselves() {
        let mut f = Factory::default();
        assert_eq!(propose(&mut f, "sentry", vec![t("S-1")], false), 1);
        assert_eq!(f.work[0].stage, Stage::Proposed);
        assert!(claim(&mut f, 5, 0).is_none());
        assert!(accept(&mut f, "S-1", "build"));
        assert_eq!(f.work[0].start_at, "build");
        assert!(claim(&mut f, 5, 0).is_some());
    }

    #[test]
    fn the_same_ticket_is_only_proposed_once() {
        let mut f = Factory::default();
        propose(&mut f, "sentry", vec![t("S-1")], false);
        assert_eq!(propose(&mut f, "sentry", vec![t("S-1"), t("S-2")], false), 1);
        assert_eq!(f.work.len(), 2);
    }

    #[test]
    fn a_rejected_proposal_doesnt_come_back() {
        let mut f = Factory::default();
        propose(&mut f, "sentry", vec![t("S-1")], false);
        assert!(reject(&mut f, "S-1"));
        assert!(f.work.is_empty());
        assert_eq!(propose(&mut f, "sentry", vec![t("S-1")], false), 0);
    }

    #[test]
    fn starting_by_itself_is_a_choice_made_per_source() {
        let mut f = Factory::default();
        propose(&mut f, "gh", vec![t("A")], true);
        assert_eq!(f.work[0].stage, Stage::Queued);
    }

    #[test]
    fn work_waiting_for_a_person_doesnt_take_a_seat() {
        let mut f = Factory::default();
        feed(&mut f, vec![t("A"), t("B")]);
        let a = claim(&mut f, 1, 0).unwrap();
        started(&mut f, &a.ticket.key, "t1", "");
        assert!(claim(&mut f, 1, 0).is_none());
        f.work[0].waiting = Waiting::Handoff;
        assert_eq!(claim(&mut f, 1, 0).unwrap().ticket.key, "B");
    }

    #[test]
    fn line_work_doesnt_end_when_a_terminal_closes() {
        let mut f = Factory::default();
        feed(&mut f, vec![t("A")]);
        f.work[0].stage = Stage::Running;
        f.work[0].line_stage = "build".into();
        f.work[0].terminal = Some("t1".into());
        let changes = settle(&mut f, &[], &HashSet::new(), false, 0);
        assert_eq!(changes, vec![Change::StageLost { key: "A".into() }]);
        assert_eq!(f.work[0].stage, Stage::Running);
        f.work[0].waiting = Waiting::Handoff;
        assert!(settle(&mut f, &[], &HashSet::new(), false, 0).is_empty());
    }
}
