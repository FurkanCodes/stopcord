//! Work coming in from outside, pulled on a timer, and keeping the machine awake.
//!
//! Outside text never starts an agent by itself. Whatever arrives becomes a
//! proposal for a person, unless they turned that off for that one source.

use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::engine::{Due, Engine, due, parse_schedule};
use crate::factories::{Factory, Kind, Part, PullStatus, Stage, Ticket, Waiting};
use crate::runs::{self, Run};
use crate::settings::{self, SlackSettings};
use crate::tickets;
use crate::work;

/// Two pulls of one source can't start closer together than this.
const MIN_PULL_GAP: Duration = Duration::from_secs(5);

/// What the engine keeps for intake.
#[derive(Default)]
pub struct Intake {
    awake: Option<Child>,
    pulls: std::collections::HashMap<(String, String), Instant>,
    pulling: std::collections::HashSet<(String, String)>,
    slack: Option<(SlackSettings, Arc<std::sync::atomic::AtomicBool>)>,
    /// Whether the Slack connection is up, and what it last said. Nothing while it's off.
    pub slack_state: Option<(bool, String)>,
    /// Mentions from people not allowed yet, newest first.
    pub slack_knocks: Vec<crate::slack::Knock>,
}

impl Engine {
    /// Applies the machine's settings: connects to Slack, or stops, as they say.
    pub fn apply_settings(self: &Arc<Self>) {
        let settings = settings::load(self.store.home()).unwrap_or_default();
        let mut intake = self.intake.lock().unwrap();
        // No default factory is fine: then each message names its own.
        let wanted = Some(settings.slack.clone()).filter(|s| s.enabled);
        if intake.slack.as_ref().map(|(s, _)| s) != wanted.as_ref() {
            if let Some((_, stop)) = intake.slack.take() {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            intake.slack_state = wanted.as_ref().map(|_| (false, "connecting".to_string()));
            if let Some(s) = wanted {
                let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
                intake.slack = Some((s.clone(), Arc::clone(&stop)));
                let me = Arc::clone(self);
                std::thread::spawn(move || me.run_slack(s, stop));
            }
        }
    }

    /// Starts the Slack connection again, as after its tokens change, so its state is fresh at once.
    pub fn restart_slack(self: &Arc<Self>) {
        if let Some((_, stop)) = self.intake.lock().unwrap().slack.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.apply_settings();
    }

    pub fn slack_state(&self) -> Option<(bool, String)> {
        self.intake.lock().unwrap().slack_state.clone()
    }

    /// Asks for a pull, unless one is running or just ran.
    fn pull_soon(self: &Arc<Self>, factory: &str, part: &str) {
        let key = (factory.to_string(), part.to_string());
        {
            let mut intake = self.intake.lock().unwrap();
            let recent = intake.pulls.get(&key).is_some_and(|t| t.elapsed() < MIN_PULL_GAP);
            if recent || !intake.pulling.insert(key.clone()) {
                return;
            }
            intake.pulls.insert(key.clone(), Instant::now());
        }
        let me = Arc::clone(self);
        std::thread::spawn(move || {
            let _ = me.pull(&key.0, &key.1);
            me.intake.lock().unwrap().pulling.remove(&key);
        });
    }

    /// Runs a source's command now. Returns how many proposals were new.
    pub fn pull(self: &Arc<Self>, factory: &str, part_id: &str) -> Result<usize, String> {
        let f = self.store.get(factory).map_err(|e| format!("{e:#}"))?;
        let part = f
            .parts
            .iter()
            .find(|p| p.kind == Kind::Tickets && p.id == part_id)
            .cloned()
            .ok_or_else(|| format!("there's no source {part_id}"))?;
        // Not set up yet isn't a failure: nothing ran, so nothing is raised or recorded.
        if let Some(why) = tickets::not_ready(self.store.home(), &f, &part) {
            return Err(format!("{} can't pull yet: {why}", part.name));
        }
        let record = |status: PullStatus| {
            let _ = self.store.update(factory, |fa| {
                fa.pulls.insert(part.id.clone(), status.clone());
            });
        };
        match tickets::run_part(self.store.home(), &f, &part) {
            Ok(tickets) => {
                self.clear_alert(factory, &format!("pull:{}", part.id));
                let count = tickets.len();
                let added = self.propose(factory, &part, tickets);
                record(PullStatus { at: runs::now_ms(), ok: true, tickets: count, new: added, error: String::new() });
                Ok(added)
            }
            Err(e) => {
                let full = format!("{e:#}");
                // The alert is a line on a card; the whole error stays on the source, in setup.
                let first = full.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("it failed");
                let text = format!("{} couldn't pull: {first}", part.name);
                self.store
                    .update(factory, |fa| work::raise(fa, &format!("pull:{}", part.id), &text, None, runs::now_ms()))
                    .ok();
                self.log(factory, Run::new("source", format!("{} couldn't pull: {full}", part.name)).ok(false).part(&part.id));
                record(PullStatus { at: runs::now_ms(), ok: false, tickets: 0, new: 0, error: full.clone() });
                Err(full)
            }
        }
    }

    pub(crate) fn propose(self: &Arc<Self>, factory: &str, part: &Part, tickets: Vec<Ticket>) -> usize {
        let added = self
            .store
            .update(factory, |f| work::propose(f, &part.id, tickets, part.auto_start))
            .unwrap_or(0);
        if added > 0 {
            self.log(
                factory,
                Run::new("source", format!("{} brought in {added} new item{}", part.name, if added == 1 { "" } else { "s" }))
                    .ok(true)
                    .part(&part.id),
            );
        }
        added
    }

    /// Pulls every source that's due. A laptop that slept catches up on waking.
    pub(crate) fn pull_due(self: &Arc<Self>, f: &Factory) {
        let now_ms = runs::now_ms();
        for part in f.parts.iter().filter(|p| p.kind == Kind::Tickets && !p.every.trim().is_empty()) {
            let Some(schedule) = parse_schedule(&part.every) else { continue };
            let key = format!("source:{}", part.id);
            let Some(last) = f.fired.get(&key).copied() else {
                let _ = self.store.update(&f.id, |fa| {
                    fa.fired.entry(key.clone()).or_insert(now_ms);
                });
                continue;
            };
            if due(schedule, crate::engine::local(last), crate::engine::local(now_ms)) == Due::No {
                continue;
            }
            // A source waiting for its keys is skipped, and pulls as soon as it has them.
            if tickets::not_ready(self.store.home(), f, part).is_some() {
                continue;
            }
            if self.store.update(&f.id, |fa| fa.fired.insert(key.clone(), now_ms)).is_ok() {
                self.pull_soon(&f.id, &part.id);
            }
        }
    }

    /// Holds the machine awake while work is running, if the person asked for that.
    pub(crate) fn keep_awake(&self, factories: &[Factory]) {
        let wanted = settings::load(self.store.home()).map(|s| s.keep_awake).unwrap_or(false)
            && factories
                .iter()
                .any(|f| f.work.iter().any(|w| w.stage == Stage::Running && w.waiting == Waiting::No));
        let mut intake = self.intake.lock().unwrap();
        match (&mut intake.awake, wanted) {
            (None, true) => {
                let mut cmd = if cfg!(target_os = "macos") {
                    // -s only applies on mains power, so a laptop on battery still sleeps.
                    let mut c = Command::new("caffeinate");
                    c.args(["-i", "-s"]);
                    c
                } else {
                    let mut c = Command::new("systemd-inhibit");
                    c.args(["--what=idle:sleep", "--who=Sawtooth", "--why=A factory is working", "sleep", "infinity"]);
                    c
                };
                intake.awake = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok();
            }
            (Some(child), false) => {
                let _ = child.kill();
                let _ = child.wait();
                intake.awake = None;
            }
            _ => {}
        }
    }
}
