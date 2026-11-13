//! Tells what an agent in a terminal is doing from what the terminal shows.
//!
//! Each agent has a rule file. A rule names a region of the screen (the title,
//! the last few lines, the prompt box…) and what to look for there. Of the rules
//! that match, the one with the highest priority decides the state. A known agent
//! that matches nothing is idle.
//!
//! The approach and the rule format follow herdr's detection manifests
//! (<https://github.com/herdrdev/herdr>, Apache-2.0). The bundled rule files in
//! `rules/` are adapted from herdr's; this crate's code is Stopcord's own.

mod regions;
mod rules;

pub use regions::Region;
pub use rules::{RuleBook, RuleSet};

use serde::Serialize;

/// What an agent is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Waiting for the next instruction.
    Idle,
    /// Busy with a turn.
    Working,
    /// Stopped on a question only a person can answer.
    Blocked,
    /// Not an agent Stopcord knows, or a screen no rule can read.
    Unknown,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Working => "working",
            State::Blocked => "blocked",
            State::Unknown => "unknown",
        }
    }
}

/// What the terminal shows right now.
#[derive(Debug, Clone, Copy)]
pub struct Screen<'a> {
    /// The visible rows, top to bottom, trailing spaces and empty trailing rows removed.
    pub text: &'a str,
    /// The last title the program set (OSC 0 or 2).
    pub title: &'a str,
    /// The last progress report (OSC 9;4), without the leading `9;`, such as `4;0`.
    pub progress: &'a str,
}

/// The outcome of reading one screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    /// `None` means keep the previous state: the matched rule saw a screen,
    /// such as a transcript viewer, that says nothing about the agent.
    pub state: Option<State>,
    /// The id of the rule that decided it. `None` when nothing matched.
    pub rule: Option<String>,
}
