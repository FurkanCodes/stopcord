//! Work items: one ticket's journey through a factory, saved at every step.
//!
//! A ticket goes queued → starting → running → ended, and each step is written
//! to `factory.json` before anything happens because of it. So a crash, a quit
//! or a reboot can't lose one, and one can't be started twice. This module is
//! only the bookkeeping; the engine does the starting.

use std::collections::{BTreeMap, HashSet};

use crate::factories::{Factory, Stage, Ticket, WorkItem, slug};

/// Tries to start a ticket before giving up on it.
pub const MAX_START_ATTEMPTS: u32 = 3;
/// How long to wait before each retry, in milliseconds.
const RETRY_AFTER_MS: [u64; 2] = [30_000, 120_000];
/// Ended items kept, so what happened to a ticket can still be answered.
const KEEP_ENDED: usize = 50;

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

/// Files from before work items keep their tickets.
pub fn migrate(f: &mut Factory) {
    if !f.work.is_empty() {
        return;
    }
    for (key, terminal) in &f.running {
        f.work.push(WorkItem {
            ticket: Ticket {
                key: key.clone(),
                ..Ticket::default()
            },
            stage: Stage::Running,
            branch: branch_for(key),
            terminal: Some(terminal.clone()),
            ..WorkItem::default()
        });
    }
    for ticket in &f.queue {
        f.work.push(queued(ticket.clone()));
    }
}

fn queued(ticket: Ticket) -> WorkItem {
    WorkItem {
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
        f.work.push(queued(ticket));
        added += 1;
    }
    added
}

/// Marks the next ticket as starting, if there's room. Tickets still starting
/// count against `room`, and two tickets on one branch never run together.
pub fn claim(f: &mut Factory, room: usize, now: u64) -> Option<WorkItem> {
    let busy: Vec<String> = f
        .work
        .iter()
        .filter(|w| matches!(w.stage, Stage::Starting | Stage::Running))
        .map(|w| w.branch.clone())
        .collect();
    if busy.len() >= room {
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
