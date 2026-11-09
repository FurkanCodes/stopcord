//! The engine: what makes a factory's parts act while the app runs, with the
//! window open or closed.
//!
//! It watches the daemon's terminals and the factories' files, and:
//! - runs "when a terminal goes quiet" checks after each finished turn, and
//!   types a failure back into the terminal;
//! - fires triggers (a finished turn, a schedule, a failed check, a fed ticket)
//!   by waiting until the terminal is ready and typing the trigger's prompt;
//! - starts fed tickets, a few at a time, each in its own worktree;
//! - notices when an agent changes the knowledge base.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::{Datelike, Local, Timelike, Weekday};
use sawtooth_daemon::Client;
use serde_json::{Value, json};

use crate::bridge::{Bridge, Failure};
use crate::factories::{Factory, Kind, Part, RunsIn, RunsOn, Store, Ticket, When, WorkItem};
use crate::runs::{self, Run};
use crate::work::{self, Change, Live, Retry};
use crate::{hooks, kb, shell, worktree};

/// Sends an event to the window.
pub type Emit = Arc<dyn Fn(&str, Value) + Send + Sync>;

const TICK: Duration = Duration::from_secs(2);
const READY_TIMEOUT: Duration = Duration::from_secs(90);
const QUIET_CHECK_TIMEOUT: Duration = Duration::from_secs(600);
/// Fix turns the engine starts in a row before it stops asking.
const MAX_FIX_TURNS: u32 = 3;

// ---- Starting terminals -----------------------------------------------------

#[derive(Debug, Default, Clone)]
pub struct StartRequest {
    pub factory_id: Option<String>,
    pub cwd: Option<String>,
    pub command: Option<String>,
    pub name: Option<String>,
    pub branch: Option<String>,
    pub rows: Option<u16>,
    pub cols: Option<u16>,
    /// What the person will ask it first, for the run log. The window types it in.
    pub task: Option<String>,
}

fn failure(e: anyhow::Error) -> Failure {
    Failure::new("factory", format!("{e:#}"))
}

/// Starts a terminal. In a factory with a branch, it gets its own worktree with
/// the knowledge base linked in. With a command, it runs in the login shell and
/// the shell stays after it ends.
pub fn start_terminal(store: &Store, bridge: &Bridge, req: StartRequest) -> Result<Value, Failure> {
    let factory = match &req.factory_id {
        Some(id) => Some(store.get(id).map_err(failure)?),
        None => None,
    };
    let branch = req.branch.clone().unwrap_or_default();
    let cwd = match &factory {
        Some(f) => {
            let project = Path::new(&f.path);
            let dir =
                worktree::ensure(project, &store.worktrees_dir(&f.id), &branch).map_err(failure)?;
            if dir != project
                && let Some(part) = f.knowledge()
            {
                kb::link(&store.knowledge_file(&f.id), &dir, &part.read_as);
            }
            dir.display().to_string()
        }
        None => req.cwd.clone().ok_or_else(|| {
            Failure::new("invalid_params", "a terminal needs a factory or a folder")
        })?,
    };
    let shell = shell::user_shell();
    let command = req.command.as_deref().filter(|c| !c.trim().is_empty());
    let fallback = command
        .and_then(|c| c.split_whitespace().next())
        .unwrap_or("shell")
        .to_string();
    let name = req
        .name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or(fallback);
    let mut params = json!({ "argv": shell::argv(&shell, command), "cwd": cwd, "name": name });
    if let (Some(rows), Some(cols)) = (req.rows, req.cols) {
        params["rows"] = json!(rows);
        params["cols"] = json!(cols);
    }
    let terminal = bridge.call("terminal.start", params)?;
    if let Some(f) = &factory {
        let on = if branch.trim().is_empty() || cwd == f.path {
            String::new()
        } else {
            format!(" on {}", branch.trim())
        };
        let asked = req
            .task
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| format!(", asked: {}", shorten(t, 120)))
            .unwrap_or_default();
        let _ = runs::append(
            store,
            &f.id,
            &Run::new("terminal", format!("{name} started{on}{asked}")).terminal(terminal["id"].as_str()),
        );
    }
    Ok(terminal)
}

// ---- Schedules --------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Schedule {
    Daily {
        hour: u32,
        minute: u32,
    },
    Weekly {
        day: Weekday,
        hour: u32,
        minute: u32,
    },
    Every {
        minutes: u32,
    },
}

/// Reads "every day 09:00", "every monday 9:00", "every 30 minutes", "every 2 hours", "every hour".
pub fn parse_schedule(text: &str) -> Option<Schedule> {
    let lower = text.trim().to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let rest = match words.as_slice() {
        ["every", rest @ ..] => rest,
        rest => rest,
    };
    let time = |s: &str| -> Option<(u32, u32)> {
        let (h, m) = s.split_once(':')?;
        let (h, m) = (h.parse().ok()?, m.parse().ok()?);
        (h < 24 && m < 60).then_some((h, m))
    };
    let day = |s: &str| -> Option<Weekday> {
        Some(match s.trim_end_matches('s') {
            "monday" | "mon" => Weekday::Mon,
            "tuesday" | "tue" => Weekday::Tue,
            "wednesday" | "wed" => Weekday::Wed,
            "thursday" | "thu" => Weekday::Thu,
            "friday" | "fri" => Weekday::Fri,
            "saturday" | "sat" => Weekday::Sat,
            "sunday" | "sun" => Weekday::Sun,
            _ => return None,
        })
    };
    match rest {
        ["day", t] | ["day", "at", t] => {
            time(t).map(|(hour, minute)| Schedule::Daily { hour, minute })
        }
        ["hour"] => Some(Schedule::Every { minutes: 60 }),
        ["minute"] => Some(Schedule::Every { minutes: 1 }),
        [n, unit] if n.parse::<u32>().is_ok() => {
            let n: u32 = n.parse().ok().filter(|n| *n > 0)?;
            match unit.trim_end_matches('s') {
                "minute" | "min" => Some(Schedule::Every { minutes: n }),
                "hour" => Some(Schedule::Every { minutes: n * 60 }),
                _ => None,
            }
        }
        [d, t] | [d, "at", t] => {
            let (hour, minute) = time(t)?;
            Some(Schedule::Weekly {
                day: day(d)?,
                hour,
                minute,
            })
        }
        _ => None,
    }
}

// ---- The engine -------------------------------------------------------------

#[derive(Default)]
struct State {
    /// The daemon's terminals as last heard, by id.
    terminals: HashMap<String, Value>,
    /// True once the terminal list came from the daemon.
    synced: bool,
    /// Terminals whose current turn the engine started by typing a prompt.
    engine_turns: HashSet<String>,
    /// Engine-started fix turns in a row, by terminal.
    fix_streak: HashMap<String, u32>,
    /// When each schedule last fired: (factory, part) -> a key for that minute or interval.
    fired: HashMap<(String, String), String>,
    /// Interval schedules: when they last fired.
    every_last: HashMap<(String, String), Instant>,
    /// Bytes of each run log already read.
    runs_seen: HashMap<String, u64>,
    factory_mtime: HashMap<String, SystemTime>,
    knowledge_differs: HashMap<String, bool>,
    /// When each terminal's current turn began.
    turn_started: HashMap<String, Instant>,
    /// When the knowledge base was last written, so a second change under review still reaches the window.
    knowledge_mtime: HashMap<String, SystemTime>,
    /// Ticket sources with no harness, already logged.
    warned: HashSet<String>,
    /// Tickets this run of the app is starting right now: (factory, key).
    starting: HashSet<(String, String)>,
    /// The terminal list was just read afresh, so a missing terminal went unwatched.
    resync: bool,
}

pub struct Engine {
    store: Arc<Store>,
    bridge: Bridge,
    program: PathBuf,
    emit: Emit,
    state: Mutex<State>,
    started: Instant,
}

impl Engine {
    pub fn new(store: Arc<Store>, program: PathBuf, emit: Emit) -> Arc<Engine> {
        Arc::new(Engine {
            store,
            bridge: Bridge::new(program.clone()),
            program,
            emit,
            state: Mutex::new(State::default()),
            started: Instant::now(),
        })
    }

    /// Starts the engine's threads. They run for as long as the app does.
    pub fn start(self: &Arc<Self>) {
        // Nothing logged before the app started counts as new.
        {
            let mut st = self.state.lock().unwrap();
            for f in self.store.list().unwrap_or_default() {
                st.runs_seen
                    .insert(f.id.clone(), file_len(&self.store.runs_file(&f.id)));
                if let Some(t) = mtime(&self.store.dir(&f.id).join("factory.json")) {
                    st.factory_mtime.insert(f.id.clone(), t);
                }
                st.knowledge_differs
                    .insert(f.id.clone(), knowledge_differs(&self.store, &f.id));
            }
        }
        let events = Arc::clone(self);
        thread::Builder::new()
            .name("sawtooth-engine-events".into())
            .spawn(move || events.listen())
            .expect("couldn't start the engine");
        let ticks = Arc::clone(self);
        thread::Builder::new()
            .name("sawtooth-engine-tick".into())
            .spawn(move || {
                loop {
                    ticks.tick();
                    thread::sleep(TICK);
                }
            })
            .expect("couldn't start the engine");
    }

    fn listen(self: Arc<Self>) {
        loop {
            if let Ok((ack, events)) =
                Client::connect_or_start(&self.program).and_then(|c| c.subscribe(json!({})))
            {
                {
                    {
                        let mut st = self.state.lock().unwrap();
                        st.terminals = ack["terminals"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|t| Some((t["id"].as_str()?.to_string(), t.clone())))
                            .collect();
                        st.synced = true;
                        st.resync = true;
                    }
                    self.pump_tickets();
                    for event in events {
                        let Ok(event) = event else { break };
                        self.on_event(&event);
                    }
                }
            }
            self.state.lock().unwrap().synced = false;
            thread::sleep(Duration::from_secs(1));
        }
    }

    fn on_event(self: &Arc<Self>, event: &Value) {
        match event["event"].as_str() {
            Some("terminal.closed") => {
                if let Some(id) = event["id"].as_str() {
                    self.state.lock().unwrap().terminals.remove(id);
                }
                self.pump_tickets();
            }
            Some(kind) => {
                let terminal = &event["terminal"];
                let Some(id) = terminal["id"].as_str() else {
                    return;
                };
                let before = self
                    .state
                    .lock()
                    .unwrap()
                    .terminals
                    .insert(id.to_string(), terminal.clone());
                let was_working = before.as_ref().is_some_and(|t| t["state"] == "working");
                if terminal["state"] == "working" && !was_working {
                    self.state.lock().unwrap().turn_started.insert(id.to_string(), Instant::now());
                }
                if kind == "terminal.changed" && was_working && terminal["state"] == "idle" {
                    let me = Arc::clone(self);
                    let terminal = terminal.clone();
                    thread::spawn(move || me.turn_finished(&terminal));
                }
                if kind == "terminal.exited" {
                    self.pump_tickets();
                }
            }
            None => {}
        }
    }

    /// The factory a terminal works for: its folder is the project or one of the factory's worktrees.
    fn factory_of(&self, terminal: &Value) -> Option<Factory> {
        let cwd = terminal["cwd"].as_str()?;
        let real = Path::new(cwd).canonicalize().ok();
        self.store.list().ok()?.into_iter().find(|f| {
            let roots = [PathBuf::from(&f.path), self.store.worktrees_dir(&f.id)];
            roots.iter().any(|root| {
                Path::new(cwd).starts_with(root)
                    || real.as_ref().is_some_and(|r| r.starts_with(root))
                    || root
                        .canonicalize()
                        .is_ok_and(|root| real.as_ref().is_some_and(|r| r.starts_with(&root)))
            })
        })
    }

    fn log(&self, factory: &str, run: Run) {
        let _ = runs::append(&self.store, factory, &run);
    }

    fn name_of(&self, id: &str) -> String {
        self.state
            .lock()
            .unwrap()
            .terminals
            .get(id)
            .and_then(|t| t["name"].as_str().map(String::from))
            .unwrap_or_else(|| id.to_string())
    }

    // ---- A finished turn ----------------------------------------------------

    fn turn_finished(self: &Arc<Self>, terminal: &Value) {
        let Some(id) = terminal["id"].as_str() else {
            return;
        };
        let Some(factory) = self.factory_of(terminal) else {
            return;
        };
        let name = terminal["name"].as_str().unwrap_or(id).to_string();
        let took = self
            .state
            .lock()
            .unwrap()
            .turn_started
            .remove(id)
            .map(|t| t.elapsed().as_secs())
            .map(|s| if s < 90 { format!(" after {s}s") } else { format!(" after {}m", (s + 30) / 60) })
            .unwrap_or_default();
        self.log(&factory.id, Run::new("terminal", format!("{name} finished a turn{took}")).terminal(Some(id)));
        let cwd = terminal["cwd"]
            .as_str()
            .unwrap_or(&factory.path)
            .to_string();
        let (engine_turn, streak) = {
            let mut st = self.state.lock().unwrap();
            let engine_turn = st.engine_turns.remove(id);
            let streak = st.fix_streak.entry(id.to_string()).or_insert(0);
            if engine_turn {
                *streak += 1;
            } else {
                *streak = 0;
            }
            (engine_turn, *streak)
        };
        let checks: Vec<&Part> = factory
            .parts
            .iter()
            .filter(|p| {
                p.kind == Kind::Check && p.runs_on == RunsOn::Quiet && !p.command.trim().is_empty()
            })
            .collect();
        let mut failed = false;
        if !checks.is_empty() {
            if streak > MAX_FIX_TURNS {
                if streak == MAX_FIX_TURNS + 1 {
                    self.log(
                        &factory.id,
                        Run::new(
                            "check",
                            format!("{name} was asked to fix a check {MAX_FIX_TURNS} times in a row, so Sawtooth stopped asking"),
                        )
                        .ok(false)
                        .terminal(Some(id)),
                    );
                }
                return;
            }
            for part in checks {
                let (ok, output) = hooks::run_command(
                    &part.command,
                    Path::new(&cwd),
                    &mut std::io::sink(),
                    Some(QUIET_CHECK_TIMEOUT),
                );
                self.log(
                    &factory.id,
                    Run::new(
                        "check",
                        format!(
                            "{} {} when {name} went quiet",
                            part.name,
                            if ok { "passed" } else { "failed" }
                        ),
                    )
                    .ok(ok)
                    .part(&part.id)
                    .terminal(Some(id))
                    .detail(&output),
                );
                if !ok && !failed {
                    failed = true;
                    let prompt = format!(
                        "The check {} failed. Fix it, then carry on.\n\n{}",
                        part.name,
                        runs::last_lines(&output, 20)
                    );
                    self.state
                        .lock()
                        .unwrap()
                        .engine_turns
                        .insert(id.to_string());
                    if let Err(e) = self.type_prompt(id, &prompt) {
                        self.state.lock().unwrap().engine_turns.remove(id);
                        self.log(
                            &factory.id,
                            Run::new("check", e).ok(false).terminal(Some(id)),
                        );
                    }
                }
            }
        }
        if !engine_turn && !failed {
            self.fire(&factory, When::Quiet, Some(id.to_string()));
        }
    }

    // ---- Triggers -----------------------------------------------------------

    /// Fires every trigger of the factory that waits for `when`. `source` is the terminal it came from.
    pub fn fire(self: &Arc<Self>, factory: &Factory, when: When, source: Option<String>) {
        for part in factory
            .parts
            .iter()
            .filter(|p| p.kind == Kind::Trigger && p.when == when)
        {
            let me = Arc::clone(self);
            let (factory, part, source) = (factory.clone(), part.clone(), source.clone());
            thread::spawn(move || me.run_trigger(&factory, &part, source.as_deref()));
        }
    }

    fn run_trigger(&self, factory: &Factory, part: &Part, source: Option<&str>) {
        if part.prompt.trim().is_empty() {
            self.log(
                &factory.id,
                Run::new(
                    "trigger",
                    format!("{} has no prompt yet, so it did nothing", part.name),
                )
                .ok(false)
                .part(&part.id),
            );
            return;
        }
        let target = match (part.runs_in, source) {
            (RunsIn::Same, Some(id)) => Ok(id.to_string()),
            (RunsIn::Own, _) => match self.own_terminal(factory, part) {
                Some(id) => Ok(id),
                None => self.start_for(factory, part, &part.name),
            },
            _ => self.start_for(factory, part, &part.name),
        };
        let id = match target {
            Ok(id) => id,
            Err(e) => {
                self.log(&factory.id, Run::new("trigger", e).ok(false).part(&part.id));
                return;
            }
        };
        self.state.lock().unwrap().engine_turns.insert(id.clone());
        match self.type_prompt(&id, &part.prompt) {
            Ok(()) => self.log(
                &factory.id,
                Run::new(
                    "trigger",
                    format!("{} typed its prompt into {}", part.name, self.name_of(&id)),
                )
                .ok(true)
                .part(&part.id)
                .terminal(Some(&id)),
            ),
            Err(e) => {
                self.state.lock().unwrap().engine_turns.remove(&id);
                self.log(
                    &factory.id,
                    Run::new("trigger", e)
                        .ok(false)
                        .part(&part.id)
                        .terminal(Some(&id)),
                );
            }
        }
    }

    /// A live terminal in the factory named after the trigger.
    fn own_terminal(&self, factory: &Factory, part: &Part) -> Option<String> {
        let candidates: Vec<Value> = {
            let st = self.state.lock().unwrap();
            st.terminals
                .values()
                .filter(|t| t["name"] == part.name.as_str() && t["state"] != "exited")
                .cloned()
                .collect()
        };
        candidates
            .into_iter()
            .find(|t| self.factory_of(t).is_some_and(|f| f.id == factory.id))
            .and_then(|t| t["id"].as_str().map(String::from))
    }

    /// Starts a terminal for a trigger in the project folder.
    fn start_for(&self, factory: &Factory, part: &Part, name: &str) -> Result<String, String> {
        if part.harness.trim().is_empty() {
            return Err(format!(
                "{} needs a start command to open a terminal, so it did nothing",
                part.name
            ));
        }
        let req = StartRequest {
            factory_id: Some(factory.id.clone()),
            command: Some(part.harness.clone()),
            name: Some(name.to_string()),
            ..StartRequest::default()
        };
        let t = start_terminal(&self.store, &self.bridge, req).map_err(|f| f.message)?;
        let id = t["id"].as_str().unwrap_or_default().to_string();
        self.state.lock().unwrap().terminals.insert(id.clone(), t);
        Ok(id)
    }

    /// Waits until an agent is at its prompt in the terminal, then types `prompt` and presses Enter.
    fn type_prompt(&self, id: &str, prompt: &str) -> Result<(), String> {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            let t = self
                .bridge
                .call("terminal.get", json!({ "id": id }))
                .map_err(|f| format!("{} is gone: {}", id, f.message))?;
            if t["state"] == "exited" {
                return Err(format!(
                    "{} ended before the prompt could be typed",
                    self.name_of(id)
                ));
            }
            if !t["agent"].is_null() && t["state"] == "idle" {
                break;
            }
            if Instant::now() > deadline {
                return Err(format!(
                    "{} wasn't ready for a prompt within {} seconds, so nothing was typed",
                    self.name_of(id),
                    READY_TIMEOUT.as_secs()
                ));
            }
            thread::sleep(Duration::from_millis(500));
        }
        let paste = format!("\x1b[200~{}\x1b[201~", prompt.trim());
        self.bridge
            .call(
                "terminal.send",
                json!({ "id": id, "data": B64.encode(paste) }),
            )
            .map_err(|f| f.message)?;
        thread::sleep(Duration::from_millis(300));
        self.bridge
            .call(
                "terminal.send",
                json!({ "id": id, "data": B64.encode("\r") }),
            )
            .map_err(|f| f.message)?;
        Ok(())
    }

    // ---- Tickets ------------------------------------------------------------

    /// Queues tickets for a factory. Returns how many were new.
    pub fn feed(self: &Arc<Self>, id: &str, tickets: Vec<Ticket>) -> anyhow::Result<usize> {
        let added = self.store.update(id, |f| work::feed(f, tickets))?;
        if added > 0 {
            self.log(
                id,
                Run::new(
                    "ticket",
                    format!("{added} ticket{} fed", if added == 1 { "" } else { "s" }),
                ),
            );
            let factory = self.store.get(id)?;
            self.fire(&factory, When::TicketFed, None);
        }
        self.pump_tickets();
        Ok(added)
    }

    /// Squares each factory's tickets with the live terminals, then starts what there's room for.
    /// The terminal list is read inside the save, so it can't be older than what it's compared with.
    fn pump_tickets(self: &Arc<Self>) {
        let (synced, lost) = {
            let st = self.state.lock().unwrap();
            (st.synced, st.resync)
        };
        if !synced {
            return;
        }
        for factory in self.store.list().unwrap_or_default() {
            let source = factory.tickets().cloned();
            let can_start = source
                .as_ref()
                .is_some_and(|s| !s.harness.trim().is_empty());
            if let Some(s) = &source
                && !can_start
                && !factory.queue.is_empty()
                && self.state.lock().unwrap().warned.insert(factory.id.clone())
            {
                self.log(
                    &factory.id,
                    Run::new("ticket", "Tickets are waiting, but the ticket source has no start command. Pick a harness for it.")
                        .ok(false)
                        .part(&s.id),
                );
            }
            let room = source.as_ref().map_or(1, |s| s.at_once.max(1) as usize);
            let now = runs::now_ms();
            let id = factory.id.clone();
            let result = self.store.update(&id, |f| {
                let mut st = self.state.lock().unwrap();
                if !st.synced {
                    return (Vec::new(), None);
                }
                let live: Vec<Live> = st
                    .terminals
                    .iter()
                    .filter(|(_, t)| t["state"] != "exited")
                    .map(|(tid, t)| Live {
                        id: tid.clone(),
                        name: t["name"].as_str().unwrap_or_default().to_string(),
                    })
                    .collect();
                let mine: HashSet<String> = st
                    .starting
                    .iter()
                    .filter(|(fid, _)| fid == &id)
                    .map(|(_, key)| key.clone())
                    .collect();
                let changes = work::settle(f, &live, &mine, lost, now);
                let next = if can_start {
                    work::claim(f, room, now)
                } else {
                    None
                };
                if let Some(item) = &next {
                    st.starting.insert((id.clone(), item.ticket.key.clone()));
                }
                (changes, next)
            });
            let Ok((changes, next)) = result else {
                continue;
            };
            for change in changes {
                self.log(&id, change_run(change));
            }
            if let (Some(item), Some(source)) = (next, source) {
                let me = Arc::clone(self);
                thread::spawn(move || me.start_ticket(&factory, &source, item));
            }
        }
        if lost {
            self.state.lock().unwrap().resync = false;
        }
    }

    fn start_ticket(self: &Arc<Self>, factory: &Factory, source: &Part, item: WorkItem) {
        let ticket = item.ticket;
        let branch = item.branch;
        let starting = (factory.id.clone(), ticket.key.clone());
        let mut title: String = ticket.title.chars().take(40).collect();
        if ticket.title.chars().count() > 40 {
            title.push('…');
        }
        let name = if title.is_empty() {
            ticket.key.clone()
        } else {
            format!("{} · {title}", ticket.key)
        };
        let req = StartRequest {
            factory_id: Some(factory.id.clone()),
            command: Some(source.harness.clone()),
            name: Some(name.clone()),
            branch: Some(branch.clone()),
            ..StartRequest::default()
        };
        let id = match start_terminal(&self.store, &self.bridge, req) {
            Ok(t) => {
                let id = t["id"].as_str().unwrap_or_default().to_string();
                let cwd = t["cwd"].as_str().unwrap_or_default().to_string();
                let worktree = if cwd == factory.path { String::new() } else { cwd };
                self.state.lock().unwrap().terminals.insert(id.clone(), t);
                let _ = self.store.update(&factory.id, |f| {
                    work::started(f, &ticket.key, &id, &worktree);
                });
                self.state.lock().unwrap().starting.remove(&starting);
                id
            }
            Err(f) => {
                let retry = self.store.update(&factory.id, |fa| {
                    work::start_failed(fa, &ticket.key, &f.message, runs::now_ms())
                });
                self.state.lock().unwrap().starting.remove(&starting);
                let text = match retry {
                    Ok(Retry::AfterSecs(s)) => {
                        format!("{} couldn't start: {}. Trying again in {s}s.", ticket.key, f.message)
                    }
                    _ => format!(
                        "{} couldn't start after {} tries, so it was dropped: {}. Feed it again once that's fixed.",
                        ticket.key,
                        work::MAX_START_ATTEMPTS,
                        f.message
                    ),
                };
                self.log(&factory.id, Run::new("ticket", text).ok(false).part(&source.id));
                return;
            }
        };
        let prompt = [
            format!("{}: {}", ticket.key, ticket.title)
                .trim()
                .to_string(),
            ticket.body.trim().to_string(),
            ticket.url.trim().to_string(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
        match self.type_prompt(&id, &prompt) {
            Ok(()) => self.log(
                &factory.id,
                Run::new("ticket", format!("{} started on {branch}: {}", ticket.key, ticket.title))
                    .ok(true)
                    .part(&source.id)
                    .terminal(Some(&id)),
            ),
            Err(e) => self.log(
                &factory.id,
                Run::new("ticket", format!("{}: {e}. It has a terminal but never got the ticket.", ticket.key))
                    .ok(false)
                    .terminal(Some(&id)),
            ),
        }
    }

    // ---- The tick -----------------------------------------------------------

    fn tick(self: &Arc<Self>) {
        let factories = self.store.list().unwrap_or_default();
        for f in &factories {
            self.watch_files(f);
            self.check_schedules(f);
        }
        self.pump_tickets();
    }

    fn watch_files(self: &Arc<Self>, f: &Factory) {
        // Factory settings changed on disk.
        let file = self.store.dir(&f.id).join("factory.json");
        if let Some(t) = mtime(&file) {
            let changed = self
                .state
                .lock()
                .unwrap()
                .factory_mtime
                .insert(f.id.clone(), t)
                != Some(t);
            if changed {
                (self.emit)("factory:changed", json!({ "id": f.id }));
            }
        }

        // New runs: tell the window, and fire triggers for failed checks.
        let runs_file = self.store.runs_file(&f.id);
        let len = file_len(&runs_file);
        let seen = self
            .state
            .lock()
            .unwrap()
            .runs_seen
            .insert(f.id.clone(), len);
        let from = seen.unwrap_or(0);
        if len > from {
            (self.emit)("factory:runs", json!({ "id": f.id }));
            if let Ok(text) = std::fs::read(&runs_file) {
                let new =
                    String::from_utf8_lossy(&text[(from as usize).min(text.len())..]).into_owned();
                for run in new
                    .lines()
                    .filter_map(|l| serde_json::from_str::<Run>(l).ok())
                {
                    if run.kind == "check"
                        && run.ok == Some(false)
                        && run.part.is_some()
                        && let Some(t) = run.terminal
                    {
                        self.fire(f, When::CheckFails, Some(t));
                    }
                }
            }
        }

        // The knowledge base, changed by someone other than the window.
        let differs = knowledge_differs(&self.store, &f.id);
        let mtime = std::fs::metadata(self.store.knowledge_file(&f.id))
            .and_then(|m| m.modified())
            .ok();
        let (before, rewritten) = {
            let mut st = self.state.lock().unwrap();
            let before = st.knowledge_differs.insert(f.id.clone(), differs);
            let last = match mtime {
                Some(t) => st.knowledge_mtime.insert(f.id.clone(), t),
                None => st.knowledge_mtime.remove(&f.id),
            };
            (before, last.is_some() && last != mtime)
        };
        if differs && before == Some(true) && rewritten {
            (self.emit)("knowledge:changed", json!({ "id": f.id }));
        } else if differs && before == Some(false) {
            self.log(
                &f.id,
                Run::new("knowledge", "The knowledge base changed. Review it."),
            );
            (self.emit)("knowledge:changed", json!({ "id": f.id }));
        } else if !differs && before == Some(true) {
            (self.emit)("knowledge:changed", json!({ "id": f.id }));
        }
    }

    fn check_schedules(self: &Arc<Self>, f: &Factory) {
        let now = Local::now();
        for part in f
            .parts
            .iter()
            .filter(|p| p.kind == Kind::Trigger && p.when == When::Schedule)
        {
            let Some(schedule) = parse_schedule(&part.schedule) else {
                continue;
            };
            let key = (f.id.clone(), part.id.clone());
            let due = match schedule {
                Schedule::Daily { hour, minute } => (now.hour(), now.minute()) == (hour, minute),
                Schedule::Weekly { day, hour, minute } => {
                    now.weekday() == day && (now.hour(), now.minute()) == (hour, minute)
                }
                Schedule::Every { minutes } => {
                    let mut st = self.state.lock().unwrap();
                    let last = *st
                        .every_last
                        .entry(key.clone())
                        .or_insert(self.started.max(Instant::now()));
                    if last.elapsed() >= Duration::from_secs(minutes as u64 * 60) {
                        st.every_last.insert(key.clone(), Instant::now());
                        true
                    } else {
                        false
                    }
                }
            };
            if !due {
                continue;
            }
            let stamp = now.format("%Y-%m-%d %H:%M").to_string();
            if self.state.lock().unwrap().fired.insert(key, stamp.clone()) == Some(stamp) {
                continue;
            }
            let me = Arc::clone(self);
            let (factory, part) = (f.clone(), part.clone());
            thread::spawn(move || me.run_trigger(&factory, &part, None));
        }
    }
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// True when the knowledge base differs from its last approved copy.
pub fn knowledge_differs(store: &Store, id: &str) -> bool {
    match (
        std::fs::read(store.knowledge_file(id)),
        std::fs::read(store.approved_knowledge_file(id)),
    ) {
        (Ok(a), Ok(b)) => a != b,
        _ => false,
    }
}

/// A change to a ticket, as a line for the run log.
fn change_run(change: Change) -> Run {
    match change {
        Change::Closed { key, branch } => Run::new(
            "ticket",
            format!("{key}'s terminal closed. Its work is on branch {branch}."),
        ),
        Change::Lost { key, branch } => Run::new(
            "ticket",
            format!("{key}'s terminal was gone when Sawtooth looked, so the ticket ended. What it committed is on branch {branch}, and anything unsaved is still in its worktree."),
        )
        .ok(false),
        Change::Requeued { key } => Run::new(
            "ticket",
            format!("{key} was starting when Sawtooth stopped, so it's queued again."),
        ),
        Change::Adopted { key, terminal } => Run::new(
            "ticket",
            format!("{key} was starting when Sawtooth stopped and its terminal is still open. Check that it got the ticket."),
        )
        .ok(false)
        .terminal(Some(&terminal)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules() {
        assert_eq!(
            parse_schedule("Every day 09:00"),
            Some(Schedule::Daily { hour: 9, minute: 0 })
        );
        assert_eq!(
            parse_schedule("every day at 8:30"),
            Some(Schedule::Daily {
                hour: 8,
                minute: 30
            })
        );
        assert_eq!(
            parse_schedule("Every Monday 09:00"),
            Some(Schedule::Weekly {
                day: Weekday::Mon,
                hour: 9,
                minute: 0
            })
        );
        assert_eq!(
            parse_schedule("every 30 minutes"),
            Some(Schedule::Every { minutes: 30 })
        );
        assert_eq!(
            parse_schedule("every 2 hours"),
            Some(Schedule::Every { minutes: 120 })
        );
        assert_eq!(
            parse_schedule("every hour"),
            Some(Schedule::Every { minutes: 60 })
        );
        assert_eq!(parse_schedule("every day 25:00"), None);
        assert_eq!(parse_schedule("sometimes"), None);
        assert_eq!(parse_schedule("every 0 minutes"), None);
    }

    #[test]
    fn knowledge_changes_are_noticed() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        std::fs::create_dir_all(store.dir("f")).unwrap();
        assert!(!knowledge_differs(&store, "f"));
        std::fs::write(store.knowledge_file("f"), "a").unwrap();
        std::fs::write(store.approved_knowledge_file("f"), "a").unwrap();
        assert!(!knowledge_differs(&store, "f"));
        std::fs::write(store.knowledge_file("f"), "b").unwrap();
        assert!(knowledge_differs(&store, "f"));
    }
}

/// At most `max` characters, cut at a word, with "…" when cut.
fn shorten(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let cut: String = flat.chars().take(max).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{cut}…")
}
