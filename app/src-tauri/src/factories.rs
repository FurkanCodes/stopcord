//! Factories: a project folder and the parts that run it.
//!
//! Each factory has its own folder, `~/.sawtooth/factories/<id>/`. `definition/`
//! is what people edit, under git (see `definition.rs`). `state/` belongs to the
//! engine: `work.json` and the run log, `events.jsonl`. Keeping them apart means a
//! save from the window can't overwrite what the engine just did. Nothing is
//! written into the project itself except the links that let harnesses read the
//! knowledge base, and the git hooks that run checks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{definition, work};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// The one file every harness reads at start.
    #[default]
    Knowledge,
    /// A folder of Markdown in the project that agents read when they need it.
    Folder,
    /// A command that has to pass: on every commit, before a push, or when a terminal goes quiet.
    Check,
    /// Types a prompt into a terminal when something happens.
    Trigger,
    /// A command that lists tickets. A factory can have several.
    Tickets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunsOn {
    #[default]
    Commit,
    /// Before a push, the last step before a merge.
    Merge,
    /// When a terminal finishes a turn.
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum When {
    #[default]
    Quiet,
    Schedule,
    CheckFails,
    TicketFed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunsIn {
    /// The terminal that went quiet, or whose check failed.
    #[default]
    Same,
    /// A fresh terminal every time.
    New,
    /// One terminal kept for this trigger, started the first time.
    Own,
}

/// One part of a factory. Which fields matter depends on `kind`; the rest stay empty.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Part {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    /// Knowledge base: the file names harnesses read it as, such as AGENTS.md.
    pub read_as: Vec<String>,
    /// Folder: a folder in the project, relative to it, such as `specs/`.
    pub folder: String,
    /// Check and ticket source: the shell command to run.
    pub command: String,
    pub runs_on: RunsOn,
    pub when: When,
    /// Trigger on a schedule: "every day 09:00", "every monday 09:00", "every 30 minutes".
    pub schedule: String,
    pub runs_in: RunsIn,
    /// Trigger: what it types into the terminal.
    pub prompt: String,
    /// The command that starts an agent, for parts that start terminals:
    /// ticket sources, and triggers that run in a new terminal or their own.
    pub harness: String,
    /// Ticket source: how many ticket terminals run at once.
    pub at_once: u32,
    /// Check at a gate: whether it may use the network. Off by default.
    pub network: bool,
    /// Ticket source: how often to pull, such as "every 5 minutes". Empty pulls only by hand.
    pub every: String,
    /// Ticket source: start new items with no person. Off by default, since outside text reaches an agent.
    pub auto_start: bool,
}

/// How much an agent is confined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Its own folder and branch, and nothing else.
    #[default]
    Worktree,
    /// An OS sandbox around the terminal, with web traffic through a filter.
    Contained,
    /// A Linux virtual machine.
    Vm,
    /// The harness's own sandbox, turned on with its own flags. Sawtooth's web filter isn't used.
    Harness,
}

/// What happens to a site that's on neither list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OtherSites {
    /// Blocked, and a person is asked in NEEDS when an agent wanted it mid-task.
    #[default]
    Ask,
    /// Blocked without asking anyone.
    Block,
}

/// What agents may read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadMode {
    /// Everything except what's blocked.
    #[default]
    Everything,
    /// Only their folder, the tools any program needs, and what's listed.
    Listed,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn no_text(s: &str) -> bool {
    s.is_empty()
}

fn none<T>(v: &[T]) -> bool {
    v.is_empty()
}

fn zero(v: &u32) -> bool {
    *v == 0
}

/// `runners/<name>.yaml`: how an agent is confined.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Runner {
    #[serde(skip_serializing_if = "no_text")]
    pub id: String,
    #[serde(skip_serializing_if = "no_text")]
    pub description: String,
    pub level: Level,
    /// Domains it may reach, on top of what its harness needs.
    #[serde(skip_serializing_if = "none")]
    pub network_allow: Vec<String>,
    /// Extra folders it may write.
    #[serde(skip_serializing_if = "none")]
    pub write: Vec<String>,
    /// Extra paths it may not read.
    #[serde(skip_serializing_if = "none")]
    pub deny_read: Vec<String>,
    /// Built-in read blocks turned off, by name, from `runner::built_in_blocks`.
    #[serde(skip_serializing_if = "none")]
    pub unblock: Vec<String>,
    #[serde(skip_serializing_if = "is_default")]
    pub reads: ReadMode,
    /// When reads are listed: more paths agents may read.
    #[serde(skip_serializing_if = "none")]
    pub read: Vec<String>,
    /// Sites blocked without asking anyone, such as a harness checking for updates.
    #[serde(skip_serializing_if = "none")]
    pub network_quiet: Vec<String>,
    #[serde(skip_serializing_if = "is_default")]
    pub other_sites: OtherSites,
    /// VM level: a person's own command that runs a command inside their VM, replacing Sawtooth's
    /// container and everything it does. `{dir}` is the worktree and `{cmd}` the command.
    #[serde(skip_serializing_if = "no_text")]
    pub vm_command: String,
    /// VM level: the image agents run in, built FROM sawtooth-vm:latest. Empty means Sawtooth's own.
    #[serde(skip_serializing_if = "no_text")]
    pub vm_image: String,
    /// VM level: secrets each agent gets as environment variables, by name. The values stay in
    /// `~/.sawtooth/secrets`, under the same names.
    #[serde(skip_serializing_if = "none")]
    pub secrets: Vec<String>,
    /// Commands run in the worktree before the agent starts.
    #[serde(skip_serializing_if = "none")]
    pub setup_commands: Vec<String>,
}

/// `agents/<name>/agent.md`: a role and its prompt.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Agent {
    pub id: String,
    pub description: String,
    /// The command that starts it. Empty means the factory's default.
    pub harness: String,
    /// The runner it uses. Empty means the factory's default.
    pub runner: String,
    pub prompt: String,
}

/// `SKILL.md`: instructions an agent reads when it needs them.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Skill {
    pub id: String,
    /// The agent it belongs to. Empty means every agent.
    pub agent: String,
    pub description: String,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StageKind {
    #[default]
    Agent,
    Gate,
    Human,
}

/// One stage of a factory's line.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct LineStage {
    pub id: String,
    pub kind: StageKind,
    /// Agent stages: which agent does it.
    #[serde(skip_serializing_if = "no_text")]
    pub agent: String,
    /// Gate stages: the checks that have to pass.
    #[serde(skip_serializing_if = "none")]
    pub checks: Vec<String>,
    /// Gate stages: test files put back from the base branch before the checks run.
    #[serde(skip_serializing_if = "none")]
    pub protect: Vec<String>,
    /// The next stage. Empty means this is the end of the line.
    #[serde(skip_serializing_if = "no_text")]
    pub then: String,
    /// Where work goes when this stage fails or is sent back.
    #[serde(skip_serializing_if = "no_text")]
    pub on_fail: String,
    #[serde(skip_serializing_if = "zero")]
    pub max_loops: u32,
    /// Agent stages: works in a throwaway clone of the branch, so nothing it does reaches the real work.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub isolated: bool,
    /// Isolated stages: files in the top folder that come back from the copy and get committed.
    #[serde(skip_serializing_if = "none")]
    pub keep: Vec<String>,
    /// The last stage: how to merge. Empty merges the branch locally; anything else is a shell command.
    #[serde(skip_serializing_if = "no_text")]
    pub merge: String,
    /// The last stage: a command that passes once the merge has really landed, such as checking a pull request.
    #[serde(skip_serializing_if = "no_text")]
    pub landed: String,
}

/// A command run when work reaches an event, to tell the outside world.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Report {
    pub id: String,
    /// handoff, approve, question, failed, merged or dropped.
    pub on: Vec<String>,
    pub command: String,
}

/// `agentDefaults` in `factory.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Defaults {
    #[serde(skip_serializing_if = "no_text")]
    pub harness: String,
    #[serde(skip_serializing_if = "no_text")]
    pub runner: String,
    #[serde(skip_serializing_if = "zero")]
    pub at_once: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pos {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Ticket {
    /// The ticket's id in its tracker: "LIN-92", "#41".
    pub key: String,
    pub title: String,
    pub body: String,
    pub url: String,
}

/// Where a ticket is in its journey. See `work.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    /// Came from outside and waits for a person to start it.
    Proposed,
    #[default]
    Queued,
    Starting,
    Running,
    Ended,
}

/// Why a work item on a line is waiting for a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Waiting {
    #[default]
    No,
    /// A person stage: approve, or send back.
    Approve,
    /// The end of the line: merge, send back or drop.
    Handoff,
    /// The agent asked something, or stopped without saying it was done.
    Question,
    /// It failed more times than the line allows, or lost its terminal.
    Failed,
    /// A merge command ran, such as opening a pull request. Not finished until the branch lands.
    Landing,
}

/// Something a reviewer reported.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Finding {
    pub file: String,
    pub line: u64,
    pub severity: String,
    pub text: String,
    pub stage: String,
    /// "clear" when the fix needs no decision and can go straight back to the builder.
    /// Anything else waits for a person at handoff.
    pub kind: String,
}

/// A change to an agent's prompt, proposed from failures that keep happening. Nothing changes until a person applies it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Proposal {
    pub id: String,
    pub at: u64,
    pub agent: String,
    pub before: String,
    pub after: String,
    /// What keeps going wrong, in one line.
    pub why: String,
    /// The work items behind it, by id.
    pub evidence: Vec<String>,
}

/// One ticket and what has happened to it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkItem {
    /// Stays with it from intake to the end, across every terminal and run.
    pub id: String,
    pub ticket: Ticket,
    pub stage: Stage,
    pub branch: String,
    /// The daemon terminal working on it, once it has one.
    pub terminal: Option<String>,
    /// Its own folder, when it got a worktree.
    pub worktree: String,
    /// Failed tries at starting it.
    pub attempts: u32,
    /// Not before this time, in milliseconds, after a failed start.
    pub retry_at: u64,
    pub ended_at: u64,
    /// How it ended, in words.
    pub outcome: String,
    /// Which part it came from, for reporting back.
    pub source: String,
    /// The line stage it's at. Empty when the factory has no line, or before it starts.
    pub line_stage: String,
    /// Where a person chose to start it. Empty means the first stage.
    pub start_at: String,
    /// Times each stage has failed.
    pub loops: BTreeMap<String, u32>,
    pub waiting: Waiting,
    /// The terminal each agent has, so work coming back goes to the same conversation.
    pub agents: BTreeMap<String, String>,
    pub findings: Vec<Finding>,
    /// What the last stage to report said.
    pub summary: String,
    /// The commit it started from.
    pub base: String,
    /// What scorers said. Shown at handoff and never decides anything.
    pub scores: Vec<Score>,
    /// Set when this item is a benchmark task.
    pub bench: Option<BenchRef>,
    /// What a merge in progress is doing right now, in words.
    pub merging: String,
    /// Why the last merge didn't happen.
    pub merge_error: String,
}

/// A scorer's opinion of a piece of work.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Score {
    pub stage: String,
    /// From 0 to 10.
    pub score: u32,
    pub why: String,
}

/// Ties a work item to the benchmark run it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BenchRef {
    pub suite: String,
    pub run: String,
    pub task: String,
    /// The command that says whether the task was solved.
    pub check: String,
    pub begun_at: u64,
    /// The agent being compared, and the harness command it's trying. Empty means nothing is swapped.
    pub agent: String,
    pub candidate: String,
    pub repeat: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct BenchTask {
    pub key: String,
    pub title: String,
    pub body: String,
    /// Passing means solved. Agents never see it.
    pub check: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Suite {
    pub id: String,
    pub name: String,
    pub description: String,
    /// The one agent under test. Every other agent keeps its own harness, as a control.
    pub agent: String,
    /// Harness commands to compare for that agent. Empty runs the factory as it's configured.
    pub candidates: Vec<String>,
    /// How many times each task runs per candidate. Zero means once.
    pub repeats: u32,
    /// The commit every task starts from. Empty means the project's current commit.
    pub base: String,
    /// The agent stage to start at. Empty means the first stage.
    pub start_at: String,
    pub tasks: Vec<BenchTask>,
}

/// Something the engine had to stop or give up on, which needs a person to look.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Alert {
    /// What it's about, such as `ticket:LIN-92`. There's one alert per key.
    pub key: String,
    pub at: u64,
    pub text: String,
    /// The terminal to go to, when there is one.
    pub terminal: Option<String>,
    /// question, approve, handoff, blocked, denied or failed. Empty for older alerts.
    pub kind: String,
    /// The work item it's about, by id.
    pub work: String,
    /// For a denied site: the host.
    pub host: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Factory {
    pub id: String,
    pub name: String,
    /// The project folder, absolute and without symlinks.
    pub path: String,
    pub parts: Vec<Part>,
    pub agents: Vec<Agent>,
    pub skills: Vec<Skill>,
    pub runners: Vec<Runner>,
    /// `scorers/<name>/scorer.md`: agents that give work a score. Advisory, never a gate.
    pub scorers: Vec<Agent>,
    /// `benchmarks/<suite>/suite.yaml`.
    pub benchmarks: Vec<Suite>,
    /// The stages work moves through. Empty means no line: a factory works as a set of parts.
    pub line: Vec<LineStage>,
    pub defaults: Defaults,
    pub reports: Vec<Report>,
    /// Where parts were dragged to on the canvas, by node id. Others sit where the layout puts them.
    pub layout: BTreeMap<String, Pos>,
    /// Every ticket fed to this factory and where it stands. The engine owns this.
    pub work: Vec<WorkItem>,
    /// What's waiting for a person to look at. The engine owns this.
    pub alerts: Vec<Alert>,
    /// Outside items already proposed, so they don't come back. The engine owns this.
    pub seen: Vec<String>,
    /// Prompt changes waiting for a person. The engine owns this.
    pub proposals: Vec<Proposal>,
    /// When each schedule last fired, by part id, in milliseconds. The engine owns this.
    pub fired: BTreeMap<String, u64>,
    /// For the window: tickets waiting for a terminal. Always rebuilt from `work`.
    pub queue: Vec<Ticket>,
    /// For the window: tickets with a terminal, by key. Always rebuilt from `work`.
    pub running: BTreeMap<String, String>,
}

impl Factory {
    pub fn knowledge(&self) -> Option<&Part> {
        self.parts.iter().find(|p| p.kind == Kind::Knowledge)
    }

    pub fn tickets(&self) -> Option<&Part> {
        self.parts.iter().find(|p| p.kind == Kind::Tickets)
    }
}

/// Lowercase letters, digits and dashes, for ids, folder names and branches.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "factory".into()
    } else {
        out
    }
}

pub struct Store {
    home: PathBuf,
    /// Held across every read-change-write, so the window and the engine never lose each other's changes.
    lock: Mutex<()>,
    /// Held while an old `factory.json` becomes a definition folder.
    upgrading: Mutex<()>,
}

/// What the engine owns, in `state/work.json`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct State {
    work: Vec<WorkItem>,
    alerts: Vec<Alert>,
    fired: BTreeMap<String, u64>,
    layout: BTreeMap<String, Pos>,
    seen: Vec<String>,
    proposals: Vec<Proposal>,
}

fn write_atomic(file: &Path, text: &str) -> Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = file.with_extension("tmp-write");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, file)?;
    Ok(())
}

impl Store {
    /// `home` is Sawtooth's own folder, `~/.sawtooth`.
    pub fn new(home: PathBuf) -> Store {
        Store {
            home,
            lock: Mutex::new(()),
            upgrading: Mutex::new(()),
        }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn dir(&self, id: &str) -> PathBuf {
        self.home.join("factories").join(id)
    }

    /// What people edit.
    pub fn definition_dir(&self, id: &str) -> PathBuf {
        self.dir(id).join("definition")
    }

    /// What only the engine writes.
    pub fn state_dir(&self, id: &str) -> PathBuf {
        self.dir(id).join("state")
    }

    fn state_file(&self, id: &str) -> PathBuf {
        self.state_dir(id).join("work.json")
    }

    fn legacy_file(&self, id: &str) -> PathBuf {
        self.dir(id).join("factory.json")
    }

    fn is_factory(&self, id: &str) -> bool {
        self.definition_dir(id).join("factory.yaml").is_file() || self.legacy_file(id).is_file()
    }

    /// Newest change to a factory's definition or state, so the window hears about hand edits too.
    pub fn stamp(&self, id: &str) -> Option<std::time::SystemTime> {
        let state = std::fs::metadata(self.state_file(id)).and_then(|m| m.modified()).ok();
        definition::newest(&self.definition_dir(id)).max(state)
    }

    pub fn knowledge_file(&self, id: &str) -> PathBuf {
        self.definition_dir(id).join("knowledge.md")
    }

    /// The knowledge base as last approved. It differs from the file when an agent changed it.
    pub fn approved_knowledge_file(&self, id: &str) -> PathBuf {
        self.state_dir(id).join("knowledge.approved.md")
    }

    pub fn runs_file(&self, id: &str) -> PathBuf {
        self.state_dir(id).join("events.jsonl")
    }

    /// Where the factory's terminals get their worktrees.
    pub fn worktrees_dir(&self, id: &str) -> PathBuf {
        self.home.join("worktrees").join(id)
    }

    /// Factories whose settings can't be read: (id, what's wrong).
    pub fn broken(&self) -> Vec<(String, String)> {
        let Ok(entries) = std::fs::read_dir(self.home.join("factories")) else {
            return Vec::new();
        };
        let mut all: Vec<(String, String)> = entries
            .flatten()
            .filter(|e| self.is_factory(&e.file_name().to_string_lossy()))
            .filter_map(|e| {
                let id = e.file_name().to_string_lossy().into_owned();
                let problem = self.get(&id).err()?;
                Some((id, format!("{problem:#}")))
            })
            .collect();
        all.sort();
        all
    }

    /// Every factory that can be read, by name. One broken file mustn't stop the others; see `broken`.
    pub fn list(&self) -> Result<Vec<Factory>> {
        let root = self.home.join("factories");
        let entries = match std::fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("couldn't read {}", root.display())),
        };
        let mut all = Vec::new();
        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().into_owned();
            if self.is_factory(&id)
                && let Ok(factory) = self.get(&id)
            {
                all.push(factory);
            }
        }
        all.sort_by_key(|f| f.name.to_lowercase());
        Ok(all)
    }

    pub fn get(&self, id: &str) -> Result<Factory> {
        if !self.is_factory(id) {
            bail!("there is no factory {id:?}");
        }
        self.upgrade(id)?;
        let mut factory = definition::load(&self.definition_dir(id))?;
        factory.id = id.to_string();
        let file = self.state_file(id);
        match std::fs::read_to_string(&file) {
            Ok(text) => {
                let state: State = serde_json::from_str(&text)
                    .with_context(|| format!("{} isn't valid", file.display()))?;
                factory.work = state.work;
                factory.alerts = state.alerts;
                factory.fired = state.fired;
                factory.layout = state.layout;
                factory.seen = state.seen;
                factory.proposals = state.proposals;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("couldn't read {}", file.display())),
        }
        work::refresh_view(&mut factory);
        Ok(factory)
    }

    /// Turns an old `factory.json` into a definition folder and a state file, once.
    /// The old file is renamed last, so an interrupted upgrade simply runs again.
    fn upgrade(&self, id: &str) -> Result<()> {
        let old = self.legacy_file(id);
        if !old.is_file() {
            return Ok(());
        }
        let _guard = self.upgrading.lock().unwrap();
        if !old.is_file() {
            return Ok(());
        }
        let text = std::fs::read_to_string(&old).with_context(|| format!("couldn't read {}", old.display()))?;
        let mut factory: Factory =
            serde_json::from_str(&text).with_context(|| format!("{} isn't valid", old.display()))?;
        factory.id = id.to_string();
        work::migrate(&mut factory);
        let (dir, state) = (self.dir(id), self.state_dir(id));
        std::fs::create_dir_all(&state)?;
        std::fs::create_dir_all(self.definition_dir(id))?;
        for (from, to) in [
            (dir.join("knowledge.approved.md"), self.approved_knowledge_file(id)),
            (dir.join("runs.jsonl"), self.runs_file(id)),
        ] {
            if from.is_file() && !to.exists() {
                std::fs::rename(&from, &to)?;
            }
        }
        // Worktrees link to the old path, so a link stays there.
        let (kb_old, kb_new) = (dir.join("knowledge.md"), self.knowledge_file(id));
        if kb_old.is_file() && !kb_old.is_symlink() && !kb_new.exists() {
            std::fs::rename(&kb_old, &kb_new)?;
            std::os::unix::fs::symlink(&kb_new, &kb_old)?;
        }
        self.write_files(&factory, "Upgrade from factory.json")?;
        std::fs::rename(&old, dir.join("factory.json.upgraded"))?;
        Ok(())
    }

    /// Makes a new factory for a project folder. A folder can be one factory only.
    pub fn create(&self, folder: &Path, name: &str, parts: Vec<Part>) -> Result<Factory> {
        let _guard = self.lock.lock().unwrap();
        let path = folder
            .canonicalize()
            .with_context(|| format!("{} doesn't exist", folder.display()))?;
        if !path.is_dir() {
            bail!("{} is not a folder", path.display());
        }
        let path = path.display().to_string();
        let existing = self.list()?;
        if let Some(f) = existing.iter().find(|f| f.path == path) {
            bail!("{} is already the factory {}", path, f.name);
        }
        let name = if name.trim().is_empty() {
            Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "factory".into())
        } else {
            name.trim().to_string()
        };
        let base = slug(&name);
        let mut id = base.clone();
        let mut n = 2;
        while self.dir(&id).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        let factory = Factory {
            id,
            name,
            path,
            parts,
            ..Factory::default()
        };
        std::fs::create_dir_all(self.dir(&factory.id))?;
        self.write_files(&factory, "Create the factory")?;
        Ok(factory)
    }

    /// Replaces a factory's settings, keeping what the engine owns: the ticket queue and running tickets.
    pub fn save(&self, factory: &Factory) -> Result<Factory> {
        let _guard = self.lock.lock().unwrap();
        let current = self.get(&factory.id)?;
        let mut next = factory.clone();
        next.path = current.path;
        next.work = current.work;
        next.fired = current.fired;
        next.alerts = current.alerts;
        next.seen = current.seen;
        next.proposals = current.proposals;
        next.queue = current.queue;
        next.running = current.running;
        self.write_files(&next, "Change the factory")?;
        Ok(next)
    }

    /// Changes a factory in place. For the engine. A change that changes nothing writes nothing.
    pub fn update<T>(&self, id: &str, change: impl FnOnce(&mut Factory) -> T) -> Result<T> {
        let _guard = self.lock.lock().unwrap();
        let before = self.get(id)?;
        let mut factory = before.clone();
        let out = change(&mut factory);
        work::refresh_view(&mut factory);
        if factory != before {
            self.write_files(&factory, "Change the factory")?;
        }
        Ok(out)
    }

    /// Forgets a factory. Its folder moves to `~/.sawtooth/removed/`, so the
    /// knowledge base and run log are never lost by a click.
    pub fn remove(&self, id: &str) -> Result<PathBuf> {
        let _guard = self.lock.lock().unwrap();
        let from = self.dir(id);
        if !from.exists() {
            bail!("there is no factory {id:?}");
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let to = self.home.join("removed").join(format!("{id}-{stamp}"));
        std::fs::create_dir_all(to.parent().unwrap())?;
        std::fs::rename(&from, &to)?;
        Ok(to)
    }

    /// Writes what changed. The state goes first, so a definition is never newer than the work it describes.
    fn write_files(&self, factory: &Factory, message: &str) -> Result<()> {
        let state = State {
            work: factory.work.clone(),
            alerts: factory.alerts.clone(),
            fired: factory.fired.clone(),
            layout: factory.layout.clone(),
            seen: factory.seen.clone(),
            proposals: factory.proposals.clone(),
        };
        let file = self.state_file(&factory.id);
        let text = serde_json::to_string_pretty(&state)?;
        if std::fs::read_to_string(&file).ok().as_deref() != Some(text.as_str()) {
            write_atomic(&file, &text)?;
        }
        let dir = self.definition_dir(&factory.id);
        if definition::save(&dir, factory)? {
            definition::commit(&dir, message);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(kind: Kind, name: &str) -> Part {
        Part {
            id: slug(name),
            kind,
            name: name.into(),
            ..Part::default()
        }
    }

    #[test]
    fn create_list_save_and_remove() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        let work = tempfile::tempdir().unwrap();
        let zebra = work.path().join("zebra");
        let apple = work.path().join("Apple");
        std::fs::create_dir(&zebra).unwrap();
        std::fs::create_dir(&apple).unwrap();

        let z = store
            .create(&zebra, "", vec![part(Kind::Knowledge, "Knowledge base")])
            .unwrap();
        assert_eq!((z.id.as_str(), z.name.as_str()), ("zebra", "zebra"));
        let a = store.create(&apple, "Apple pie", vec![]).unwrap();
        assert_eq!(a.id, "apple-pie");
        let again = store.create(&zebra.join("../zebra"), "", vec![]);
        assert!(again.unwrap_err().to_string().contains("already"));

        let names: Vec<_> = store.list().unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["Apple pie", "zebra"]);

        // The engine queues a ticket; a save from the window keeps it.
        store
            .update(&z.id, |f| {
                work::feed(
                    f,
                    vec![Ticket {
                        key: "LIN-1".into(),
                        ..Ticket::default()
                    }],
                )
            })
            .unwrap();
        let mut edited = z.clone();
        edited.parts.push(part(Kind::Check, "Tests"));
        let saved = store.save(&edited).unwrap();
        assert_eq!(saved.queue.len(), 1);
        assert_eq!(saved.work.len(), 1);
        assert_eq!(store.get("zebra").unwrap().parts.len(), 2);

        let moved = store.remove("zebra").unwrap();
        assert!(moved.join("definition/factory.yaml").is_file());
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn a_broken_file_doesnt_take_the_other_factories_down() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        store.create(a.path(), "good", vec![]).unwrap();
        let bad = store.create(b.path(), "bad", vec![]).unwrap();
        std::fs::write(store.definition_dir(&bad.id).join("factory.yaml"), "{ oops").unwrap();

        let names: Vec<_> = store.list().unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["good"]);
        let broken = store.broken();
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].0, "bad");
        assert!(broken[0].1.contains("isn't valid"), "{broken:?}");
    }

    #[test]
    fn a_change_that_changes_nothing_doesnt_touch_the_files() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        let work = tempfile::tempdir().unwrap();
        let f = store.create(work.path(), "x", vec![]).unwrap();
        let file = store.state_dir(&f.id).join("work.json");
        let mut marked = std::fs::read_to_string(&file).unwrap();
        marked.push_str("\n\n");
        std::fs::write(&file, &marked).unwrap();
        store.update(&f.id, |_| ()).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), marked);
    }

    fn legacy(store: &Store, project: &Path) -> (String, Factory) {
        let id = "old".to_string();
        let f = Factory {
            id: id.clone(),
            name: "Old one".into(),
            path: project.display().to_string(),
            parts: vec![
                part(Kind::Knowledge, "Knowledge base"),
                Part {
                    command: "npm test".into(),
                    ..part(Kind::Check, "Tests")
                },
                Part {
                    prompt: "Tidy up.".into(),
                    when: When::Schedule,
                    schedule: "every day 09:00".into(),
                    ..part(Kind::Trigger, "Tidy")
                },
            ],
            fired: BTreeMap::from([("tidy".to_string(), 77)]),
            work: vec![WorkItem {
                ticket: Ticket {
                    key: "LIN-1".into(),
                    ..Ticket::default()
                },
                stage: Stage::Running,
                terminal: Some("t4".into()),
                branch: "lin-1".into(),
                ..WorkItem::default()
            }],
            alerts: vec![Alert {
                key: "source".into(),
                at: 5,
                text: "look".into(),
                terminal: None,
                ..Alert::default()
            }],
            layout: BTreeMap::from([("n".to_string(), Pos { x: 1.0, y: 2.0 })]),
            ..Factory::default()
        };
        let dir = store.dir(&id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("factory.json"), serde_json::to_string(&f).unwrap()).unwrap();
        std::fs::write(dir.join("knowledge.md"), "what we know").unwrap();
        std::fs::write(dir.join("knowledge.approved.md"), "what we approved").unwrap();
        std::fs::write(dir.join("runs.jsonl"), "{}\n").unwrap();
        (id, f)
    }

    #[test]
    fn an_old_factory_upgrades_with_nothing_lost() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        let (id, old) = legacy(&store, project.path());

        let new = store.get(&id).unwrap();
        assert_eq!(new.name, old.name);
        assert_eq!(new.path, old.path);
        assert_eq!(new.parts.len(), 3);
        assert_eq!(new.work.len(), 1);
        assert!(new.work[0].id.starts_with("w_"), "{:?}", new.work[0].id);
        assert_eq!(new.work[0].terminal.as_deref(), Some("t4"));
        assert_eq!(new.fired, old.fired);
        assert_eq!(new.alerts, old.alerts);
        assert_eq!(new.layout, old.layout);
        assert_eq!(new.running.get("LIN-1").map(String::as_str), Some("t4"));

        let dir = store.dir(&id);
        assert_eq!(std::fs::read_to_string(store.knowledge_file(&id)).unwrap(), "what we know");
        assert_eq!(std::fs::read_to_string(store.approved_knowledge_file(&id)).unwrap(), "what we approved");
        assert_eq!(std::fs::read_to_string(store.runs_file(&id)).unwrap(), "{}\n");
        // Worktrees made before the upgrade link to the old path.
        assert_eq!(std::fs::read_to_string(dir.join("knowledge.md")).unwrap(), "what we know");
        assert!(dir.join("factory.json.upgraded").is_file());
        assert!(!dir.join("factory.json").exists());
        // And it reads the same the second time.
        assert_eq!(store.get(&id).unwrap().work, new.work);
    }

    #[test]
    fn an_old_factory_that_cant_be_read_is_reported_and_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        std::fs::create_dir_all(store.dir("bad")).unwrap();
        std::fs::write(store.dir("bad").join("factory.json"), "{ oops").unwrap();
        assert_eq!(store.broken().len(), 1);
        assert!(store.dir("bad").join("factory.json").is_file());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn a_change_to_the_definition_is_committed_and_engine_changes_are_not() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        let project = tempfile::tempdir().unwrap();
        let f = store.create(project.path(), "x", vec![]).unwrap();
        let log = |store: &Store| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(store.definition_dir("x"))
                .args(["log", "--oneline"])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).lines().count()
        };
        assert_eq!(log(&store), 1);
        store
            .update(&f.id, |fa| {
                work::feed(
                    fa,
                    vec![Ticket {
                        key: "A".into(),
                        ..Ticket::default()
                    }],
                )
            })
            .unwrap();
        assert_eq!(log(&store), 1);
        let mut edited = store.get(&f.id).unwrap();
        edited.parts.push(part(Kind::Check, "Tests"));
        store.save(&edited).unwrap();
        assert_eq!(log(&store), 2);
    }

    #[test]
    fn a_save_with_a_bad_line_is_refused_and_changes_nothing() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        let project = tempfile::tempdir().unwrap();
        let f = store.create(project.path(), "x", vec![]).unwrap();
        let mut edited = f.clone();
        edited.line.push(LineStage {
            id: "build".into(),
            agent: "ghost".into(),
            ..LineStage::default()
        });
        assert!(store.save(&edited).is_err());
        assert!(store.get(&f.id).unwrap().line.is_empty());
    }

    #[test]
    fn slugs() {
        assert_eq!(
            slug("LIN-92 · Rounding in totals"),
            "lin-92-rounding-in-totals"
        );
        assert_eq!(slug("  ***  "), "factory");
        assert_eq!(slug("invoice_app"), "invoice-app");
    }
}
