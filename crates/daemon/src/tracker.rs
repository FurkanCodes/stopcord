//! Turns screen readings into a steady state, and decides when a terminal needs you.
//!
//! A terminal needs you when its agent is blocked on a question, or when it has
//! finished a turn you haven't looked at yet. Typing into it, or marking it seen,
//! clears the second kind.
//!
//! A turn counts only after something was typed into the terminal. Agents show
//! spinners while they start up, and that isn't work anyone is waiting on.

use std::time::{Duration, Instant};

use stopcord_detect::{Detection, State};

/// How long a working agent must look idle before its turn counts as finished.
/// Spinners and redraws flicker through idle-looking frames mid-turn.
pub const SETTLE: Duration = Duration::from_millis(1200);

#[derive(Debug, Clone)]
pub struct Tracker {
    pub agent: Option<String>,
    pub state: State,
    /// A finished turn or a new question nobody has looked at.
    pub unseen: bool,
    /// The rule that decided the current state.
    pub rule: Option<String>,
    /// Something has been typed in since the terminal started.
    prompted: bool,
    idle_since: Option<Instant>,
}

impl Default for Tracker {
    fn default() -> Self {
        Tracker {
            agent: None,
            state: State::Unknown,
            unseen: false,
            rule: None,
            prompted: false,
            idle_since: None,
        }
    }
}

impl Tracker {
    /// Blocked, or a finished turn not yet seen.
    pub fn needs_you(&self) -> bool {
        self.state == State::Blocked || self.unseen
    }

    /// True while waiting to see whether an idle-looking screen holds.
    pub fn settling(&self) -> bool {
        self.idle_since.is_some()
    }

    /// Applies one reading. `detection` is `None` when no agent is running.
    /// Returns true when anything a viewer shows changed.
    pub fn observe(
        &mut self,
        agent: Option<String>,
        detection: Option<Detection>,
        now: Instant,
    ) -> bool {
        let before = (self.agent.clone(), self.state, self.unseen);
        if agent != self.agent {
            self.agent = agent;
            self.idle_since = None;
        }
        match detection {
            None => {
                self.state = State::Unknown;
                self.rule = None;
                self.idle_since = None;
            }
            Some(Detection { state: None, .. }) => {}
            Some(Detection {
                state: Some(next),
                rule,
            }) => {
                if next == State::Idle && self.state == State::Working {
                    let since = *self.idle_since.get_or_insert(now);
                    if now.duration_since(since) >= SETTLE {
                        self.state = State::Idle;
                        self.rule = rule;
                        self.unseen = self.prompted;
                        self.idle_since = None;
                    }
                } else {
                    self.idle_since = None;
                    if next == State::Blocked && self.state != State::Blocked {
                        self.unseen = true;
                    }
                    if next == State::Working {
                        self.unseen = false;
                    }
                    self.state = next;
                    self.rule = rule;
                }
            }
        }
        before != (self.agent.clone(), self.state, self.unseen)
    }

    /// The person looked. Returns true when that changed anything.
    pub fn seen(&mut self) -> bool {
        std::mem::replace(&mut self.unseen, false)
    }

    /// Something was typed in: from now on a finished turn needs you.
    /// Typing also counts as looking. Returns true when that changed anything.
    pub fn input(&mut self) -> bool {
        self.prompted = true;
        self.seen()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(state: State) -> Option<Detection> {
        Some(Detection {
            state: Some(state),
            rule: None,
        })
    }

    #[test]
    fn a_finished_turn_waits_to_settle_then_needs_you() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let agent = || Some("claude".to_string());
        t.input();
        assert!(t.observe(agent(), d(State::Working), t0));
        assert!(!t.observe(agent(), d(State::Idle), t0 + Duration::from_millis(100)));
        assert_eq!(t.state, State::Working);
        assert!(t.settling());
        // A spinner frame comes back mid-turn: still working, settling resets.
        t.observe(agent(), d(State::Working), t0 + Duration::from_millis(500));
        assert!(!t.settling());
        t.observe(agent(), d(State::Idle), t0 + Duration::from_millis(600));
        assert!(t.observe(agent(), d(State::Idle), t0 + Duration::from_millis(1900)));
        assert_eq!(t.state, State::Idle);
        assert!(t.needs_you());
        assert!(t.seen());
        assert!(!t.needs_you());
    }

    #[test]
    fn a_question_needs_you_at_once_and_seeing_it_does_not_clear_it() {
        let mut t = Tracker::default();
        let now = Instant::now();
        t.observe(Some("codex".into()), d(State::Blocked), now);
        assert!(t.needs_you());
        t.seen();
        assert!(t.needs_you(), "still blocked");
        t.observe(Some("codex".into()), d(State::Working), now);
        assert!(!t.needs_you());
    }

    #[test]
    fn a_startup_spinner_is_not_a_finished_turn() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        t.observe(Some("codex".into()), d(State::Working), t0);
        t.observe(Some("codex".into()), d(State::Idle), t0);
        t.observe(Some("codex".into()), d(State::Idle), t0 + SETTLE);
        assert_eq!(t.state, State::Idle);
        assert!(!t.needs_you(), "nobody had asked it anything yet");
    }

    #[test]
    fn starting_idle_is_not_news() {
        let mut t = Tracker::default();
        t.observe(Some("pi".into()), d(State::Idle), Instant::now());
        assert!(!t.needs_you());
    }

    #[test]
    fn keep_previous_and_no_agent() {
        let mut t = Tracker::default();
        let now = Instant::now();
        t.observe(Some("claude".into()), d(State::Working), now);
        t.observe(
            Some("claude".into()),
            Some(Detection {
                state: None,
                rule: Some("viewer".into()),
            }),
            now,
        );
        assert_eq!(t.state, State::Working);
        t.observe(None, None, now);
        assert_eq!(t.state, State::Unknown);
        assert_eq!(t.agent, None);
    }
}
