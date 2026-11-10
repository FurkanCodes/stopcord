//! The engine's side of the line: doing what `line.rs` says to do.
//!
//! Each step is saved by `line.rs` before anything here happens, so if the app
//! stops halfway, `resume` finds the work where the line says it is.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sawtooth_sandbox::{Network, Policy, wrap};
use serde_json::{Value, json};

use crate::engine::{Engine, StartRequest};
use crate::factories::{Alert, Factory, Finding, LineStage, Score, Stage, StageKind, Waiting, WorkItem};
use crate::line::{self, Action, Step};
use crate::runs::{self, Run};
use crate::{gate, git, harness, hooks, shell, worktree};

const LANDED_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const GATE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// A `st done` usually lands before the terminal looks idle, but not always.
const REPORT_GRACE: Duration = Duration::from_secs(4);

const TRUST_SETTLE: Duration = Duration::from_millis(1200);
const TRUST_STEP: Duration = Duration::from_millis(250);

/// A person's action, the way the timeline says it.
fn person_said(action: &str, key: &str, note: &str) -> String {
    let word = match action {
        "approve" => "approved",
        "sendBack" => "sent back",
        "treatAsDone" => "marked done",
        "drop" => "dropped",
        "merge" => "asked to merge",
        "landed" => "marked as merged",
        "answer" => "answered",
        other => other,
    };
    let note = note.trim();
    if note.is_empty() || !matches!(action, "sendBack" | "answer") {
        format!("{word} {key}")
    } else {
        format!("{word} {key}: {note}")
    }
}

pub fn alert_key(work: &str) -> String {
    format!("work:{work}")
}

/// A person's answer that couldn't be typed. It's about one try: the next try, or the agent moving on, ends it.
pub fn answer_alert(key: &str) -> String {
    format!("answer:{key}")
}

/// Tells an agent in a throwaway copy what will survive it.
fn copy_note(stage: &LineStage) -> String {
    let back = if stage.keep.is_empty() { "Nothing you change here comes back.".to_string() } else { format!("Only {} comes back from it.", stage.keep.join(", ")) };
    format!("This folder is a throwaway copy of the branch, deleted when the work ends. {back}")
}

fn item_of<'a>(f: &'a Factory, key: &str) -> Option<&'a WorkItem> {
    f.work.iter().find(|w| w.ticket.key == key)
}

fn stage_of<'a>(f: &'a Factory, w: &WorkItem) -> Option<&'a LineStage> {
    f.line.iter().find(|s| s.id == w.line_stage)
}

/// Quotes a whole command line for `sh -c`, with its environment in front.
pub(crate) fn command_line(argv: &[String], env: &[(String, String)]) -> String {
    let mut out: Vec<String> = env.iter().map(|(k, v)| format!("{k}={}", shell::quote(v))).collect();
    if !out.is_empty() {
        out.insert(0, "env".to_string());
    }
    out.extend(argv.iter().map(|a| shell::quote(a)));
    out.join(" ")
}

/// A check's command, inside a sandbox with no network unless the check asks for it.
/// The gate is code an agent could have edited, so it never runs unconfined if it can be helped.
pub(crate) fn confined_check(store_dir: &Path, snapshot: &Path, command: &str, network: bool, program: &Path) -> Result<String, String> {
    let login = vec![shell::user_shell(), "-l".to_string(), "-c".to_string(), command.to_string()];
    if sawtooth_sandbox::available().is_err() {
        return Ok(command_line(&login, &[]));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let mut p = Policy::new(snapshot.to_path_buf(), home, program.to_path_buf());
    p.network = if network { Network::Open } else { Network::Off };
    p.deny_read.push(store_dir.join("secrets"));
    let wrapped = wrap(&p, &login, &store_dir.join("run")).map_err(|e| format!("{e:#}"))?;
    Ok(command_line(&wrapped.argv, &wrapped.env))
}

impl Engine {
    pub(crate) fn harness_for(&self, f: &Factory, agent: Option<&crate::factories::Agent>) -> String {
        agent
            .map(|a| a.harness.clone())
            .filter(|h| !h.trim().is_empty())
            .or_else(|| Some(f.defaults.harness.clone()).filter(|h| !h.trim().is_empty()))
            .or_else(|| f.parts.iter().filter(|p| p.kind == crate::factories::Kind::Tickets).map(|s| s.harness.clone()).find(|h| !h.trim().is_empty()))
            .unwrap_or_default()
    }

    /// Puts a claimed item on the line.
    pub(crate) fn begin_on_line(self: &Arc<Self>, factory: &str, key: &str) {
        let step = self
            .store
            .update(factory, |f| {
                if let Some(b) = f.work.iter_mut().find(|w| w.ticket.key == key).and_then(|w| w.bench.as_mut()) {
                    b.begun_at = runs::now_ms();
                }
                line::begin(f, key)
            })
            .unwrap_or(Step::Nothing);
        self.state.lock().unwrap().starting.remove(&(factory.to_string(), key.to_string()));
        self.step(factory, key, step);
    }

    /// Does what the line said to do.
    pub(crate) fn step(self: &Arc<Self>, factory: &str, key: &str, step: Step) {
        let Ok(f) = self.store.get(factory) else { return };
        let Some(w) = item_of(&f, key).cloned() else { return };
        if step != Step::Nothing {
            self.clear_alert(factory, &alert_key(&w.id));
            self.clear_alert(factory, &answer_alert(key));
        }
        match step {
            Step::Nothing => {}
            Step::Agent { stage, agent, note } => {
                self.log(factory, Run::new("line", format!("{key} moved to {stage}")).work(&w.id).by("engine"));
                let me = Arc::clone(self);
                let (fid, key) = (factory.to_string(), key.to_string());
                std::thread::spawn(move || me.run_agent(&fid, &key, &stage, &agent, &note));
            }
            Step::Gate { stage } => {
                self.log(factory, Run::new("line", format!("{key} moved to {stage}")).work(&w.id).by("engine"));
                let me = Arc::clone(self);
                let (fid, key) = (factory.to_string(), key.to_string());
                std::thread::spawn(move || me.run_gate(&fid, &key, &stage));
            }
            Step::Person { why, text, .. } if w.bench.is_some() => {
                let me = Arc::clone(self);
                let (fid, key) = (factory.to_string(), key.to_string());
                std::thread::spawn(move || me.finish_bench(&fid, &key, why == Waiting::Handoff, text));
            }
            Step::Person { stage, why, text } => {
                self.flag_before_asking(factory, key, &stage, why, &w);
                if why == Waiting::Handoff {
                    self.drop_site_requests(factory, &w);
                    let _ = self.store.update(factory, |fa| {
                        if let Some(i) = fa.work.iter_mut().find(|i| i.ticket.key == key) {
                            i.merge_error.clear();
                        }
                    });
                }
                let (kind, event) = match why {
                    Waiting::Approve => ("approve", "approve"),
                    Waiting::Handoff => ("handoff", "handoff"),
                    Waiting::Question => ("question", "question"),
                    Waiting::Failed | Waiting::No | Waiting::Landing => ("failed", "failed"),
                };
                let terminal = w.terminal.clone().or_else(|| w.agents.values().next_back().cloned());
                let _ = self.store.update(factory, |fa| {
                    crate::work::raise_kind(
                        fa,
                        Alert {
                            key: alert_key(&w.id),
                            at: runs::now_ms(),
                            text: text.clone(),
                            terminal: terminal.clone(),
                            kind: kind.to_string(),
                            work: w.id.clone(),
                            host: String::new(),
                        },
                    );
                });
                self.log(
                    factory,
                    Run::new("line", text.clone())
                        .ok(why != Waiting::Failed)
                        .work(&w.id)
                        .by("engine"),
                );
                self.report_back(&f, &w, event, &text);
            }
            Step::End { outcome } => {
                self.drop_site_requests(factory, &w);
                let _ = self.store.update(factory, |fa| {
                    if let Some(i) = fa.work.iter_mut().find(|i| i.ticket.key == key) {
                        i.stage = Stage::Ended;
                        i.ended_at = runs::now_ms();
                        i.terminal = None;
                    }
                });
                let in_vm: Vec<crate::vm::Spec> = {
                    let state = self.state.lock().unwrap();
                    w.agents.values().filter_map(|t| state.terminals.get(t).and_then(crate::vm::spec_of)).collect()
                };
                for t in w.agents.values() {
                    let _ = self.bridge.call("terminal.close", json!({ "id": t }));
                }
                // The containers go first: a volume still in use can't be removed.
                if let Some(docker) = crate::vm::docker() {
                    for spec in &in_vm {
                        crate::vm::cleanup(&docker, spec);
                    }
                }
                self.forget_vm_copies(factory, &w);
                let _ = std::fs::remove_dir_all(self.copies_root(factory, &w));
                self.log(factory, Run::new("line", format!("{key} {outcome}")).ok(true).work(&w.id).by("engine"));
                if w.bench.is_none() {
                    self.report_back(&f, &w, &outcome, &format!("{key} {outcome}"));
                }
            }
        }
    }

    fn fail_step(self: &Arc<Self>, factory: &str, key: &str, text: String) {
        let step = self.store.update(factory, |f| {
            if let Some(w) = f.work.iter_mut().find(|w| w.ticket.key == key) {
                w.waiting = Waiting::Failed;
            }
            let stage = f.work.iter().find(|w| w.ticket.key == key).map(|w| w.line_stage.clone()).unwrap_or_default();
            Step::Person { stage, why: Waiting::Failed, text: text.clone() }
        });
        if let Ok(step) = step {
            self.step(factory, key, step);
        }
    }

    fn run_agent(self: &Arc<Self>, factory: &str, key: &str, stage: &str, agent_id: &str, note: &str) {
        let guard = (factory.to_string(), key.to_string());
        if !self.state.lock().unwrap().stepping.insert(guard.clone()) {
            return;
        }
        self.run_agent_inner(factory, key, stage, agent_id, note);
        self.state.lock().unwrap().stepping.remove(&guard);
    }

    fn run_agent_inner(self: &Arc<Self>, factory: &str, key: &str, stage: &str, agent_id: &str, note: &str) {
        let Ok(f) = self.store.get(factory) else { return };
        let Some(w) = item_of(&f, key).cloned() else { return };
        let agent = f.agents.iter().chain(&f.scorers).find(|a| a.id == agent_id).cloned();
        let is_scorer = f.scorers.iter().any(|s| s.id == agent_id);
        let alive = |t: &str| {
            self.state.lock().unwrap().terminals.get(t).is_some_and(|v| v["state"] != "exited")
        };
        let existing = w.agents.get(agent_id).filter(|t| alive(t)).cloned();
        let isolated = f.line.iter().find(|s| s.id == stage && s.isolated).cloned();
        let copy = match &isolated {
            Some(_) => match self.prepare_copy(&f, &w, stage, existing.is_some()) {
                Ok(copy) => copy,
                Err(why) => {
                    self.fail_step(factory, key, format!("{key} couldn't make a copy for {stage}: {why}"));
                    return;
                }
            },
            None => None,
        };
        // In a VM, st is on the PATH; the Mac's path to it isn't there.
        let in_vm = crate::runner::in_vm(crate::runner::pick(&f, agent.as_ref().map(|a| a.runner.as_str())));
        let st = if in_vm { "st".to_string() } else { self.store.home().join("bin/st").display().to_string() };
        let skills: Vec<(String, String, String)> = f
            .skills
            .iter()
            .filter(|s| s.agent.is_empty() || s.agent == agent_id)
            .map(|s| {
                let dir = self.store.definition_dir(factory);
                let base = if s.agent.is_empty() {
                    dir.join("skills")
                } else {
                    dir.join("agents").join(&s.agent).join("skills")
                };
                (s.id.clone(), s.description.clone(), base.join(&s.id).join("SKILL.md").display().to_string())
            })
            .collect();
        let write = if is_scorer { line::scorer_brief } else { line::brief };
        let brief = write(agent.as_ref().map_or("", |a| a.prompt.as_str()), &w, &skills, &st);
        let (id, text) = if let Some(t) = existing {
            // The folder may have moved on since the last round, as when kept files came back.
            if let Err(why) = self.mirror_vm(factory, &t) {
                self.fail_step(factory, key, format!("{key} couldn't bring the VM up to date for {stage}: {why}"));
                return;
            }
            let text = if note.is_empty() {
                brief
            } else {
                format!("{note}\n\nWhen you've fixed it, commit and run: {st} done \"what you changed\".")
            };
            (t, text)
        } else {
            // A benchmark swaps only the agent under test. Everything else stays as a control.
            let swapped = w.bench.as_ref().filter(|b| b.agent == agent_id && !b.candidate.is_empty()).map(|b| b.candidate.clone());
            let command = swapped.unwrap_or_else(|| self.harness_for(&f, agent.as_ref()));
            if command.trim().is_empty() {
                let text = format!("{key} needs a harness for {stage}. Set one on the agent or in the factory's defaults.");
                self.fail_step(factory, key, text);
                return;
            }
            let req = StartRequest {
                factory_id: Some(factory.to_string()),
                command: Some(command),
                name: Some(format!("{key} · {stage}")),
                branch: Some(w.branch.clone()),
                runner: agent.as_ref().map(|a| a.runner.clone()),
                workspace: copy.as_ref().map(|(_, c)| c.clone()),
                ..StartRequest::default()
            };
            let t = match crate::engine::start_terminal(&self.store, &self.bridge, &self.program, req) {
                Ok(t) => t,
                Err(e) => {
                    self.fail_step(factory, key, format!("{key} couldn't start {stage}: {}", e.message));
                    return;
                }
            };
            let id = t["id"].as_str().unwrap_or_default().to_string();
            let cwd = t["cwd"].as_str().unwrap_or_default().to_string();
            self.state.lock().unwrap().terminals.insert(id.clone(), t);
            let chosen = crate::runner::pick(&f, agent.as_ref().map(|a| a.runner.as_str()));
            if let Err(why) = crate::runner::wait_for_setup(chosen, Path::new(&cwd)) {
                let _ = self.store.update(factory, |fa| {
                    if let Some(i) = fa.work.iter_mut().find(|i| i.ticket.key == key) {
                        i.terminal = Some(id.clone());
                        i.agents.insert(agent_id.to_string(), id.clone());
                    }
                });
                self.fail_step(factory, key, format!("{key} couldn't start {stage}: {why}"));
                return;
            }
            // The work item records the real worktree, even when this stage runs in a copy of it.
            let real = copy.as_ref().map_or_else(|| cwd.clone(), |(r, _)| r.display().to_string());
            let base = worktree::git(Path::new(&f.path), &["rev-parse", "HEAD"])
                .and_then(|head| worktree::git(Path::new(&real), &["merge-base", &head, "HEAD"]))
                .unwrap_or_default();
            let on_worktree = if real == f.path { String::new() } else { real };
            let _ = self.store.update(factory, |fa| {
                if let Some(i) = fa.work.iter_mut().find(|i| i.ticket.key == key) {
                    i.worktree = on_worktree.clone();
                    if i.base.is_empty() {
                        i.base = base.clone();
                    }
                }
            });
            let text = if note.is_empty() { brief } else { format!("{brief}\n\n{note}") };
            (id, text)
        };
        let text = match (&isolated, &copy) {
            (Some(s), Some(_)) => format!("{text}\n\n{}", copy_note(s)),
            _ => text,
        };
        let _ = self.store.update(factory, |fa| {
            if let Some(i) = fa.work.iter_mut().find(|i| i.ticket.key == key) {
                i.terminal = Some(id.clone());
                i.agents.insert(agent_id.to_string(), id.clone());
            }
        });
        match self.type_engine_turn(&id, &text) {
            Ok(()) => self.log(
                factory,
                Run::new("line", format!("{key}: {agent_id} started on {stage}"))
                    .ok(true)
                    .terminal(Some(&id))
                    .work(&w.id)
                    .by("engine"),
            ),
            Err(e) => self.fail_step(factory, key, format!("{key} has a terminal for {stage} but never got its brief: {e}")),
        }
    }

    /// Brings a VM terminal's folder and its copy in the VM into step. Nothing for other terminals.
    fn mirror_vm(&self, factory: &str, terminal: &str) -> Result<(), String> {
        let t = self.state.lock().unwrap().terminals.get(terminal).cloned();
        let Some(spec) = t.as_ref().and_then(crate::vm::spec_of) else { return Ok(()) };
        let docker = crate::vm::available()?;
        let what = match crate::vm::sync(&spec, &docker)? {
            crate::vm::Synced::Took => Some("brought back the work from the VM"),
            crate::vm::Synced::Sent => Some("copied the work into the VM"),
            crate::vm::Synced::Same => None,
        };
        if let Some(what) = what {
            self.log(factory, Run::new("vm", format!("{what} for {}", spec.dir.display())).terminal(Some(terminal)).by("engine"));
        }
        // The knowledge base isn't in git, so it comes back on its own, into the review.
        match crate::vm::knowledge(&spec, &docker) {
            Ok(true) => self.log(factory, Run::new("knowledge", "An agent in the VM changed the knowledge base, so the edit is waiting for review").terminal(Some(terminal)).by("engine")),
            Ok(false) => {}
            Err(why) => self.log(factory, Run::new("knowledge", format!("Knowledge-base edits stayed in the VM: {why}")).ok(false).terminal(Some(terminal)).by("engine")),
        }
        Ok(())
    }

    /// A VM terminal ended. Its containers go, and a terminal that wasn't on the line keeps what it did.
    pub(crate) fn vm_exited(self: &Arc<Self>, terminal: &Value) {
        let Some(spec) = crate::vm::spec_of(terminal) else { return };
        let Some(docker) = crate::vm::docker() else { return };
        crate::vm::cleanup(&docker, &spec);
        crate::vm::remove_orphaned_volumes(&docker);
        let id = terminal["id"].as_str().unwrap_or_default();
        // The line collects its own work when an agent says it's done, and a new terminal takes back anything left.
        if self.item_on(id).is_some() {
            return;
        }
        let Some(factory) = self.factory_of(terminal).map(|f| f.id) else { return };
        if let Err(why) = self.mirror_vm(&factory, id) {
            self.log(&factory, Run::new("vm", format!("the work in {} stayed in the VM: {why}", spec.dir.display())).ok(false).terminal(Some(id)).by("engine"));
        }
    }

    /// The work is over, so its copies in the VM go too.
    fn forget_vm_copies(&self, factory: &str, w: &WorkItem) {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(self.copies_root(factory, w)).into_iter().flatten().flatten().map(|e| e.path()).collect();
        if !w.worktree.is_empty() {
            dirs.push(PathBuf::from(&w.worktree));
        }
        for dir in dirs {
            if crate::vm::record(self.store.home(), &dir).exists() {
                crate::vm::forget(self.store.home(), &dir);
            }
        }
    }

    fn copies_root(&self, factory: &str, w: &WorkItem) -> PathBuf {
        self.store.worktrees_dir(factory).join(".copies").join(&w.id)
    }

    fn copy_dir(&self, factory: &str, w: &WorkItem, stage: &str) -> PathBuf {
        let name: String = stage.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
        self.copies_root(factory, w).join(name)
    }

    /// Makes an isolated stage's copy, or refreshes the one its open terminal is in. Returns the
    /// real worktree and the copy, or nothing when the project has no worktree to protect.
    fn prepare_copy(&self, f: &Factory, w: &WorkItem, stage: &str, reuse: bool) -> Result<Option<(PathBuf, PathBuf)>, String> {
        let project = Path::new(&f.path);
        let real = worktree::ensure(project, &self.store.worktrees_dir(&f.id), &w.branch).map_err(|e| format!("{e:#}"))?;
        if real == project {
            return Ok(None);
        }
        let copy = self.copy_dir(&f.id, w, stage);
        let made = if reuse && copy.join(".git").is_dir() {
            worktree::refresh_copy(&copy, &real, &w.branch)
        } else {
            worktree::throwaway_copy(project, &real, &w.branch, &copy)
        };
        made.map_err(|e| format!("{e:#}"))?;
        crate::runner::carry_setup(&real, &copy);
        if let Some(part) = f.knowledge() {
            crate::kb::link(&self.store.knowledge_file(&f.id), &copy, &part.read_as);
        }
        Ok(Some((real, copy)))
    }

    /// An isolated stage is over: its kept files come back and everything else stays behind.
    fn finish_copy(&self, factory: &str, key: &str) -> Result<(), String> {
        let Ok(f) = self.store.get(factory) else { return Ok(()) };
        let Some(w) = item_of(&f, key) else { return Ok(()) };
        let Some(stage) = f.line.iter().find(|s| s.id == w.line_stage && s.isolated) else { return Ok(()) };
        let (copy, real) = (self.copy_dir(factory, w, &stage.id), PathBuf::from(&w.worktree));
        if w.worktree.is_empty() || !copy.join(".git").is_dir() {
            return Ok(());
        }
        let head = worktree::git(&real, &["rev-parse", "HEAD"]).map_err(|e| format!("{e:#}"))?;
        let links = f.knowledge().map(|p| p.read_as.clone()).unwrap_or_default();
        let thrown: Vec<String> = gate::changed_since(&copy, &head)
            .unwrap_or_default()
            .into_iter()
            .filter(|file| !stage.keep.contains(file) && !links.contains(file))
            .collect();
        if !thrown.is_empty() {
            self.log(factory, Run::new("line", format!("{key}: {} changed {} in its copy, and that was thrown away", stage.id, thrown.join(", "))).work(&w.id).by("engine"));
        }
        let taken = worktree::take_kept(&copy, &real, &stage.keep).map_err(|e| format!("{e:#}"))?;
        if !taken.is_empty() {
            worktree::commit_files(&real, &taken, &format!("{} for {key}", stage.id)).map_err(|e| format!("{e:#}"))?;
        }
        Ok(())
    }

    /// Runs one gate's checks on a clean copy of the agent's commit.
    fn gate_checks(self: &Arc<Self>, f: &Factory, w: &WorkItem, stage: &LineStage) -> (bool, String) {
        let project = PathBuf::from(&f.path);
        let dir = if w.worktree.is_empty() { project.clone() } else { PathBuf::from(&w.worktree) };
        let into = self.store.state_dir(&f.id).join("gates").join(&w.id);
        let snap = match gate::snapshot(&project, &dir, &w.base, &stage.protect, &into) {
            Ok(s) => s,
            Err(e) => return (false, format!("{e:#}")),
        };
        let mut all = String::new();
        let mut passed = true;
        for id in &stage.checks {
            let Some(part) = f.parts.iter().find(|p| &p.id == id) else {
                continue;
            };
            let command = match confined_check(self.store.home(), &snap.dir, &part.command, part.network, &self.program) {
                Ok(c) => c,
                Err(e) => return (false, format!("couldn't start the check {}: {e}", part.name)),
            };
            let (ok, output) = hooks::run_command(&command, &snap.dir, &mut std::io::sink(), Some(GATE_TIMEOUT));
            self.log(
                &f.id,
                Run::new("check", format!("{} {} at {}", part.name, if ok { "passed" } else { "failed" }, stage.id))
                    .ok(ok)
                    .part(&part.id)
                    .work(&w.id)
                    .by("engine")
                    .detail(&output),
            );
            if !ok {
                passed = false;
                all.push_str(&format!("{} failed:\n{}\n", part.name, runs::last_lines(&output, 40)));
                self.learn_from_failure(f, part);
            }
        }
        (passed, all)
    }

    /// A request for a site from a stage that's over has nobody left to answer it for.
    fn drop_site_requests(&self, factory: &str, w: &WorkItem) {
        let _ = self.store.update(factory, |fa| {
            fa.alerts.retain(|a| a.kind != "denied" || !a.terminal.as_ref().is_some_and(|t| w.agents.values().any(|x| x == t)));
        });
    }

    /// What a person needs on screen to decide about this work.
    pub fn evidence(&self, factory: &str, key: &str) -> Result<git::Evidence, String> {
        let f = self.store.get(factory).map_err(|e| format!("{e:#}"))?;
        let w = item_of(&f, key).ok_or_else(|| format!("there's no work item {key}"))?;
        if w.worktree.is_empty() {
            return Ok(git::Evidence::default());
        }
        Ok(git::evidence(Path::new(&w.worktree), &w.base, Path::new(&f.path)))
    }

    /// Says what a merge is doing, for the window. Empty means it's over.
    fn merge_status(&self, factory: &str, key: &str, text: String) {
        let _ = self.store.update(factory, |fa| {
            if let Some(i) = fa.work.iter_mut().find(|i| i.ticket.key == key) {
                i.merging = text.clone();
            }
        });
    }

    /// What a person should know before answering, put on the work item itself.
    fn flag_before_asking(&self, factory: &str, key: &str, stage: &str, why: Waiting, w: &WorkItem) {
        let dir = Path::new(&w.worktree);
        let files = match why {
            Waiting::Handoff => gate::uncommitted(dir).unwrap_or_default(),
            Waiting::Approve => gate::changed_since(dir, &w.base).unwrap_or_default().into_iter().filter(|f| f != "PLAN.md").collect(),
            _ => Vec::new(),
        };
        if w.worktree.is_empty() || files.is_empty() {
            return;
        }
        let lead = if why == Waiting::Approve {
            "Already changed before anyone approved it"
        } else {
            "Left uncommitted, so merge will refuse it. Send it back, or clean up the worktree"
        };
        let text = format!("{lead}: {}", files.join(", "));
        let _ = self.store.update(factory, |fa| {
            line::add_finding(
                fa,
                key,
                Finding {
                    file: "worktree".into(),
                    severity: "high".into(),
                    text: text.clone(),
                    stage: stage.to_string(),
                    ..Finding::default()
                },
            );
        });
    }

    fn run_gate(self: &Arc<Self>, factory: &str, key: &str, stage_id: &str) {
        let guard = (factory.to_string(), key.to_string());
        if !self.state.lock().unwrap().stepping.insert(guard.clone()) {
            return;
        }
        let result = (|| {
            let f = self.store.get(factory).ok()?;
            let w = item_of(&f, key)?.clone();
            let stage = f.line.iter().find(|s| s.id == stage_id)?.clone();
            let (passed, output) = self.gate_checks(&f, &w, &stage);
            // A changed test is a person's business, whatever the result.
            if let Ok(changed) = gate::changed_protected(Path::new(&w.worktree), &w.base, &stage.protect) {
                let _ = self.store.update(factory, |fa| {
                    for file in &changed {
                        let text = "This protected test was changed. The gate ran the base branch's copy.".to_string();
                        line::add_finding(
                            fa,
                            key,
                            Finding {
                                file: file.clone(),
                                line: 0,
                                severity: "high".into(),
                                text,
                                stage: stage.id.clone(),
                                ..Finding::default()
                            },
                        );
                    }
                });
            }
            Some((passed, output))
        })();
        self.state.lock().unwrap().stepping.remove(&guard);
        let Some((passed, output)) = result else { return };
        let step = self.store.update(factory, |f| line::gate_result(f, key, passed, &output)).unwrap_or(Step::Nothing);
        self.step(factory, key, step);
    }

    /// Picks up work the app was in the middle of when it stopped.
    pub(crate) fn resume(self: &Arc<Self>, f: &Factory) {
        // A merge that was running when the app stopped isn't running any more.
        for w in f.work.iter().filter(|w| !w.merging.is_empty()) {
            if !self.state.lock().unwrap().stepping.contains(&(f.id.clone(), w.ticket.key.clone())) {
                self.merge_status(&f.id, &w.ticket.key, String::new());
            }
        }
        for w in f.work.iter().filter(|w| w.stage == Stage::Running && w.waiting == Waiting::No && !w.line_stage.is_empty()) {
            let guard = (f.id.clone(), w.ticket.key.clone());
            if self.state.lock().unwrap().stepping.contains(&guard) {
                continue;
            }
            let Some(stage) = stage_of(f, w) else { continue };
            match stage.kind {
                StageKind::Gate => {
                    let me = Arc::clone(self);
                    let (fid, key, sid) = (f.id.clone(), w.ticket.key.clone(), stage.id.clone());
                    std::thread::spawn(move || me.run_gate(&fid, &key, &sid));
                }
                StageKind::Agent if w.terminal.is_none() => {
                    let me = Arc::clone(self);
                    let (fid, key, sid, agent) = (f.id.clone(), w.ticket.key.clone(), stage.id.clone(), stage.agent.clone());
                    std::thread::spawn(move || me.run_agent(&fid, &key, &sid, &agent, ""));
                }
                _ => {}
            }
        }
    }

    /// The work item whose current stage this terminal is doing.
    fn item_on(&self, terminal: &str) -> Option<(String, String)> {
        for f in self.store.list().unwrap_or_default() {
            if let Some(w) = f.work.iter().find(|w| w.stage == Stage::Running && w.terminal.as_deref() == Some(terminal)) {
                return Some((f.id.clone(), w.ticket.key.clone()));
            }
        }
        None
    }

    /// Collects what a terminal has reported and acts on it.
    pub(crate) fn collect_reports(self: &Arc<Self>, terminal: &str) {
        let Ok(reply) = self.bridge.call("terminal.reports", json!({ "id": terminal })) else {
            return;
        };
        let reports: Vec<Value> = reply["reports"].as_array().cloned().unwrap_or_default();
        for report in reports {
            self.on_report(terminal, &report);
        }
    }

    fn on_report(self: &Arc<Self>, terminal: &str, report: &Value) {
        let verb = report["verb"].as_str().unwrap_or_default();
        let text = report["text"].as_str().unwrap_or_default().to_string();
        let Some((factory, key)) = self.item_on(terminal) else {
            if let Some(f) = self.terminal_factory(terminal) {
                self.log(&f, Run::new("agent", format!("{verb}: {text}")).terminal(Some(terminal)).by("agent"));
            }
            return;
        };
        let work_id = self.store.get(&factory).ok().and_then(|f| item_of(&f, &key).map(|w| w.id.clone())).unwrap_or_default();
        self.log(&factory, Run::new("agent", format!("{verb}: {text}")).terminal(Some(terminal)).work(&work_id).by("agent"));
        let step = match verb {
            "done" => {
                if let Err(why) = self.mirror_vm(&factory, terminal) {
                    self.fail_step(&factory, &key, format!("{key} finished, but its work couldn't come back from the VM: {why}"));
                    return;
                }
                if let Err(why) = self.finish_copy(&factory, &key) {
                    self.fail_step(&factory, &key, format!("{key} finished, but its copy couldn't be collected: {why}"));
                    return;
                }
                self.store.update(&factory, |f| line::agent_done(f, &key, &text))
            }
            "ask" => self.store.update(&factory, |f| {
                let question = line::question_text(f, &key, &text);
                line::agent_stopped(f, &key, &question)
            }),
            "score" => {
                let score = report["score"].as_u64().unwrap_or(0) as u32;
                self.store.update(&factory, |f| {
                    let Some(i) = f.work.iter_mut().find(|i| i.ticket.key == key) else {
                        return Step::Nothing;
                    };
                    i.scores.push(Score { stage: i.line_stage.clone(), score, why: text.clone() });
                    let summary = i.summary.clone();
                    let on_scorer = stage_of(f, item_of(f, &key).expect("just found")).is_some_and(|s| f.scorers.iter().any(|a| a.id == s.agent));
                    if on_scorer {
                        // The last stage's summary stays: a score is an opinion about it, not a report.
                        line::agent_done(f, &key, &summary)
                    } else {
                        Step::Nothing
                    }
                })
            }
            "finding" => {
                let finding = Finding {
                    file: report["file"].as_str().unwrap_or_default().to_string(),
                    line: report["line"].as_u64().unwrap_or(0),
                    severity: report["severity"].as_str().unwrap_or("medium").to_string(),
                    text: text.clone(),
                    stage: String::new(),
                    kind: report["kind"].as_str().unwrap_or("judgment").to_string(),
                };
                self.store.update(&factory, |f| {
                    let stage = item_of(f, &key).map(|w| w.line_stage.clone()).unwrap_or_default();
                    line::add_finding(f, &key, Finding { stage, ..finding.clone() });
                    Step::Nothing
                })
            }
            _ => Ok(Step::Nothing),
        };
        if let Ok(step) = step {
            self.step(&factory, &key, step);
        }
        if verb == "score" {
            self.learn_from_scores(&factory);
        }
    }

    fn terminal_factory(&self, terminal: &str) -> Option<String> {
        let t = self.state.lock().unwrap().terminals.get(terminal).cloned()?;
        self.factory_of(&t).map(|f| f.id)
    }

    /// An engine turn ended on a line. If the agent said nothing, a person has to look.
    pub(crate) fn line_turn_finished(self: &Arc<Self>, terminal: &str, engine_turn: bool) -> bool {
        let Some((factory, key)) = self.item_on(terminal) else {
            return false;
        };
        if !engine_turn {
            return true;
        }
        let me = Arc::clone(self);
        let terminal = terminal.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(REPORT_GRACE);
            me.collect_reports(&terminal);
            // Still on this terminal means it never said it was done.
            let still = me.item_on(&terminal).is_some_and(|(f, k)| f == factory && k == key);
            if !still {
                me.state.lock().unwrap().reminded.remove(&terminal);
            }
            let idle = me.state.lock().unwrap().terminals.get(&terminal).is_some_and(|t| t["state"] == "idle");
            if still && idle {
                let waiting = me.store.get(&factory).ok().and_then(|f| item_of(&f, &key).map(|w| w.waiting)).unwrap_or_default();
                if waiting == Waiting::No {
                    // Agents do forget the last step, and one reminder is cheaper than stopping the line.
                    if me.state.lock().unwrap().reminded.insert(terminal.clone()) {
                        let in_vm = me.state.lock().unwrap().terminals.get(&terminal).and_then(crate::vm::spec_of).is_some();
                        let st = if in_vm { "st".to_string() } else { me.store.home().join("bin/st").display().to_string() };
                        let nudge = format!("You stopped without reporting. If you've finished, run: {st} done \"one line on what you did\". If you need a person, run: {st} ask \"your question\".");
                        if me.type_engine_turn(&terminal, &nudge).is_ok() {
                            me.log(&factory, Run::new("line", format!("{key} stopped without reporting, so it was reminded once")).by("engine"));
                            return;
                        }
                    }
                    me.state.lock().unwrap().reminded.remove(&terminal);
                    let text = format!("{key} went quiet without saying it was done. Look at it, then answer it or mark it done.");
                    let step = me.store.update(&factory, |f| line::agent_stopped(f, &key, &text)).unwrap_or(Step::Nothing);
                    me.step(&factory, &key, step);
                }
            }
        });
        true
    }

    /// A person is talking to a stopped agent, so it isn't stopped any more.
    pub(crate) fn line_turn_began(self: &Arc<Self>, terminal: &str) {
        let Some((factory, key)) = self.item_on(terminal) else { return };
        let Ok(f) = self.store.get(&factory) else { return };
        let Some(w) = item_of(&f, &key) else { return };
        if w.waiting == Waiting::Question {
            let id = w.id.clone();
            let _ = self.store.update(&factory, |fa| line::agent_resumed(fa, &key));
            self.clear_alert(&factory, &alert_key(&id));
            self.clear_alert(&factory, &answer_alert(&key));
        }
    }

    /// A site was blocked.
    pub(crate) fn on_denied(self: &Arc<Self>, event: &Value) {
        let (Some(terminal), Some(host)) = (event["id"].as_str(), event["host"].as_str()) else {
            return;
        };
        let Some(factory) = self.terminal_factory(terminal) else { return };
        let name = self.name_of(terminal);
        let agent = self.state.lock().unwrap().terminals.get(terminal).and_then(|t| t["agent"].as_str().map(String::from));
        let (all, _) = harness::load(&self.store.home().join("harnesses"));
        // Harnesses phone home when they start and while they sit idle. Only a site wanted mid-task is worth stopping for.
        let working = self.state.lock().unwrap().terminals.get(terminal).is_some_and(|t| t["state"] == "working");
        // The runner this terminal's agent uses decides which sites nobody needs to be asked about.
        let quiet_here = self.store.get(&factory).ok().is_some_and(|f| {
            let role = f.work.iter().find_map(|w| w.agents.iter().find(|(_, t)| t.as_str() == terminal).map(|(a, _)| a.clone()));
            let asked = role.and_then(|a| f.agents.iter().chain(&f.scorers).find(|x| x.id == a).map(|x| x.runner.clone()));
            crate::runner::pick(&f, asked.as_deref()).is_some_and(|r| r.other_sites == crate::factories::OtherSites::Block || harness::matches(&r.network_quiet, host))
        });
        if !working || quiet_here || harness::is_quiet(all.iter().find(|h| Some(&h.id) == agent.as_ref()), host) {
            self.log(&factory, Run::new("network", format!("{name} was blocked from {host}")).terminal(Some(terminal)).by("engine"));
            return;
        }
        let _ = self.store.update(&factory, |f| {
            crate::work::raise_kind(
                f,
                Alert {
                    key: format!("denied:{terminal}:{host}"),
                    at: runs::now_ms(),
                    text: format!("{name} tried to reach {host}"),
                    terminal: Some(terminal.to_string()),
                    kind: "denied".to_string(),
                    work: String::new(),
                    host: host.to_string(),
                },
            );
        });
        self.log(&factory, Run::new("network", format!("{name} was blocked from {host}")).ok(false).terminal(Some(terminal)).by("engine"));
    }

    /// Answers a blocked-site alert. `always` also adds the site to the runner.
    pub fn answer_denied(self: &Arc<Self>, factory: &str, key: &str, allow: bool, always: bool) -> Result<(), String> {
        let f = self.store.get(factory).map_err(|e| format!("{e:#}"))?;
        let Some(a) = f.alerts.iter().find(|a| a.key == key).cloned() else {
            return Ok(());
        };
        if allow && let Some(t) = &a.terminal {
            self.bridge
                .call("confine.allow", json!({ "id": t, "host": a.host, "always": always }))
                .map_err(|e| e.message)?;
        }
        let _ = self.store.update(factory, |fa| {
            if allow && always {
                let runner = fa.defaults.runner.clone();
                if let Some(r) = fa.runners.iter_mut().find(|r| r.id == runner)
                    && !r.network_allow.contains(&a.host)
                {
                    r.network_allow.push(a.host.clone());
                }
            }
            crate::work::clear(fa, key);
        });
        let verdict = match (allow, always) {
            (true, true) => "always allowed",
            (true, false) => "allowed once",
            _ => "kept blocked",
        };
        self.log(factory, Run::new("network", format!("{} was {verdict}", a.host)).by("person"));
        Ok(())
    }

    /// A person acted on a work item from the window.
    pub fn work_action(self: &Arc<Self>, factory: &str, key: &str, action: &str, note: &str) -> Result<(), String> {
        let f = self.store.get(factory).map_err(|e| format!("{e:#}"))?;
        let w = item_of(&f, key).cloned().ok_or_else(|| format!("there's no work item {key}"))?;
        if w.stage == Stage::Ended {
            return Err(format!("{key} has already ended"));
        }
        let guard = (factory.to_string(), key.to_string());
        if action == "answer" {
            if w.waiting != Waiting::Question {
                return Err(format!("{key} isn't asking anything right now"));
            }
            if note.trim().is_empty() {
                return Err("Write an answer first.".to_string());
            }
            if w.terminal.is_none() {
                return Err(format!("{key} has no terminal to answer in. Send it back instead."));
            }
        }
        if action == "merge" && w.waiting != Waiting::Handoff {
            return Err(format!("{key} isn't ready to merge"));
        }
        // A second press while this is still happening is the same press.
        if matches!(action, "merge" | "answer") && !self.state.lock().unwrap().stepping.insert(guard.clone()) {
            return Ok(());
        }
        self.log(factory, Run::new("line", person_said(action, key, note)).work(&w.id).by("person"));
        let act = match action {
            "approve" => Action::Approve,
            "sendBack" => Action::SendBack { note: note.to_string() },
            "treatAsDone" => Action::TreatAsDone,
            "drop" => Action::Drop,
            "landed" => Action::Merged,
            "answer" => {
                let me = Arc::clone(self);
                let (fid, key, term, text) = (factory.to_string(), key.to_string(), w.terminal.clone().unwrap_or_default(), note.to_string());
                std::thread::spawn(move || {
                    me.clear_alert(&fid, &answer_alert(&key));
                    match me.type_engine_turn(&term, &text) {
                        // An agent can ask mid-turn and carry on, and then no new turn starts to clear the question.
                        Ok(()) => me.line_turn_began(&term),
                        Err(e) => {
                            let said = format!("{key}: the answer didn't reach the agent. {e}");
                            me.log(&fid, Run::new("line", said.clone()).ok(false).by("engine"));
                            me.alert(&fid, &answer_alert(&key), &said, Some(&term));
                        }
                    }
                    me.state.lock().unwrap().stepping.remove(&(fid.clone(), key.clone()));
                });
                return Ok(());
            }
            "merge" => {
                let me = Arc::clone(self);
                let (fid, key) = (factory.to_string(), key.to_string());
                std::thread::spawn(move || {
                    me.merge(&fid, &key);
                    me.merge_status(&fid, &key, String::new());
                    me.state.lock().unwrap().stepping.remove(&guard);
                });
                return Ok(());
            }
            other => return Err(format!("unknown action {other}")),
        };
        if matches!(act, Action::TreatAsDone) {
            if let Some(t) = &w.terminal {
                self.mirror_vm(factory, t).map_err(|why| format!("{key}'s work couldn't come back from the VM: {why}"))?;
            }
            self.finish_copy(factory, key)?;
        }
        let step = self.store.update(factory, |fa| line::person(fa, key, act)).map_err(|e| format!("{e:#}"))?;
        self.step(factory, key, step);
        Ok(())
    }

    /// Runs every gate once more, then merges. A merge that can't happen leaves the work at handoff.
    fn merge(self: &Arc<Self>, factory: &str, key: &str) {
        let Ok(f) = self.store.get(factory) else { return };
        let Some(w) = item_of(&f, key).cloned() else { return };
        // The gates take a while, and the work can be dropped while they run.
        let still_open = || self.store.get(factory).ok().is_some_and(|f| item_of(&f, key).is_some_and(|w| w.stage != Stage::Ended));
        let say = |text: String| {
            if !still_open() {
                return;
            }
            let _ = self.store.update(factory, |fa| {
                if let Some(i) = fa.work.iter_mut().find(|i| i.ticket.key == key) {
                    i.merge_error = text.clone();
                }
                crate::work::raise_kind(
                    fa,
                    Alert {
                        key: alert_key(&w.id),
                        at: runs::now_ms(),
                        text: text.clone(),
                        terminal: None,
                        kind: "handoff".to_string(),
                        work: w.id.clone(),
                        host: String::new(),
                    },
                );
            });
            self.log(factory, Run::new("line", text).ok(false).work(&w.id).by("engine"));
        };
        let _ = self.store.update(factory, |fa| {
            if let Some(i) = fa.work.iter_mut().find(|i| i.ticket.key == key) {
                i.merge_error.clear();
            }
        });
        let gates: Vec<&LineStage> = f.line.iter().filter(|s| s.kind == StageKind::Gate).collect();
        for (n, stage) in gates.iter().enumerate() {
            self.merge_status(factory, key, format!("running {}, {} of {}", stage.id, n + 1, gates.len()));
            let (passed, output) = self.gate_checks(&f, &w, stage);
            if !passed {
                say(format!("{key} can't merge: {} failed again.\n{output}", stage.id));
                return;
            }
        }
        if !still_open() {
            return;
        }
        self.merge_status(factory, key, "merging".to_string());
        let how = stage_of(&f, &w).map(|s| s.merge.clone()).unwrap_or_default();
        let project = Path::new(&f.path);
        if how.trim().is_empty() {
            if let Err(e) = worktree::git(project, &["merge", "--no-ff", "--no-edit", &w.branch]) {
                let _ = worktree::git(project, &["merge", "--abort"]);
                say(format!("{key} couldn't merge, so nothing changed: {e:#}"));
                return;
            }
        } else {
            let dir = if w.worktree.is_empty() { project.to_path_buf() } else { PathBuf::from(&w.worktree) };
            let (ok, output) = hooks::run_command(&hooks::login_command(&shell::user_shell(), &how), &dir, &mut std::io::sink(), Some(GATE_TIMEOUT));
            if !ok {
                say(format!("{key}'s merge command failed:\n{}", runs::last_lines(&output, 20)));
                return;
            }
        }
        if !how.trim().is_empty() {
            // A command might only open a pull request, so this isn't merged until it lands.
            if self.store.update(factory, |fa| line::landing(fa, key)).unwrap_or(false) {
                self.clear_alert(factory, &alert_key(&w.id));
                let text = format!("{key} was handed over by the merge command. It finishes when the branch lands.");
                self.log(factory, Run::new("line", text.clone()).work(&w.id).by("engine"));
                self.report_back(&f, &w, "landing", &text);
            }
            return;
        }
        let step = self.store.update(factory, |fa| line::person(fa, key, Action::Merged)).unwrap_or(Step::Nothing);
        self.step(factory, key, step);
    }

    /// Whether a branch at handoff, or handed over by a merge command, has really landed.
    fn has_landed(&self, f: &Factory, w: &WorkItem) -> bool {
        let project = Path::new(&f.path);
        let contains = |target: &str| -> bool {
            let Ok(tip) = worktree::git(project, &["rev-parse", "--verify", &w.branch]) else {
                return false;
            };
            // A branch with nothing on it is "merged" everywhere, and that means nothing.
            tip != w.base && worktree::git(project, &["merge-base", "--is-ancestor", &tip, target]).is_ok()
        };
        if w.waiting != Waiting::Landing {
            return contains("HEAD");
        }
        let landed = stage_of(f, w).map(|s| s.landed.clone()).unwrap_or_default();
        if landed.trim().is_empty() {
            let _ = worktree::git(project, &["fetch", "-q", "origin"]);
            return contains("origin/HEAD") || contains("HEAD");
        }
        let dir = if w.worktree.is_empty() { project.to_path_buf() } else { PathBuf::from(&w.worktree) };
        hooks::run_command(&hooks::login_command(&shell::user_shell(), &landed), &dir, &mut std::io::sink(), Some(LANDED_TIMEOUT)).0
    }

    /// Finishes work that landed, including anything merged outside Sawtooth.
    pub(crate) fn finish_landed(self: &Arc<Self>, f: &Factory) {
        for w in f.work.iter().filter(|w| w.stage == Stage::Running && matches!(w.waiting, Waiting::Handoff | Waiting::Landing)) {
            if w.bench.is_none() && self.has_landed(f, w) {
                self.log(&f.id, Run::new("line", format!("{} landed", w.ticket.key)).ok(true).work(&w.id).by("engine"));
                let step = self.store.update(&f.id, |fa| line::person(fa, &w.ticket.key, Action::Merged)).unwrap_or(Step::Nothing);
                self.step(&f.id, &w.ticket.key, step);
            }
        }
    }

    /// Runs the factory's report commands for this event, with the details in the environment.
    fn report_back(&self, f: &Factory, w: &WorkItem, event: &str, text: &str) {
        if w.source == crate::slack::SOURCE {
            self.slack_reply(&w.ticket.url, text);
        }
        let mine: Vec<_> = f.reports.iter().filter(|r| r.on.iter().any(|e| e == event)).cloned().collect();
        if mine.is_empty() {
            return;
        }
        let (fid, project, env) = (
            f.id.clone(),
            f.path.clone(),
            vec![
                ("ST_EVENT", event.to_string()),
                ("ST_KEY", w.ticket.key.clone()),
                ("ST_TITLE", w.ticket.title.clone()),
                ("ST_URL", w.ticket.url.clone()),
                ("ST_STAGE", w.line_stage.clone()),
                ("ST_BRANCH", w.branch.clone()),
                ("ST_SOURCE", w.source.clone()),
                ("ST_TEXT", text.to_string()),
            ],
        );
        let store = Arc::clone(&self.store);
        let work = w.id.clone();
        std::thread::spawn(move || {
            let prefix: String = env.iter().map(|(k, v)| format!("{k}={} ", shell::quote(v))).collect();
            for r in mine {
                // Details travel as environment values, never as part of the command text.
                let line = format!("{prefix}{}", hooks::login_command(&shell::user_shell(), &r.command));
                let (ok, output) = hooks::run_command(&line, Path::new(&project), &mut std::io::sink(), Some(Duration::from_secs(60)));
                let _ = runs::append(
                    &store,
                    &fid,
                    &Run::new("report", format!("report {} {}", r.id, if ok { "sent" } else { "failed" })).ok(ok).work(&work).by("engine").detail(&output),
                );
            }
        });
    }

    /// Answers a harness's folder-trust question, if the person chose that for this harness.
    pub(crate) fn maybe_trust(self: &Arc<Self>, terminal: &Value) {
        let (Some(id), Some(agent)) = (terminal["id"].as_str(), terminal["agent"].as_str()) else {
            return;
        };
        if self.factory_of(terminal).is_none() || self.state.lock().unwrap().trusted.contains(id) {
            return;
        }
        let (all, _) = harness::load(&self.store.home().join("harnesses"));
        let sandboxed = harness::in_sandbox(terminal);
        let Some(trust) = all.iter().find(|h| h.id == agent).and_then(|h| h.trust.clone()).filter(|t| t.auto || sandboxed) else {
            return;
        };
        let Ok(screen) = self.bridge.call("terminal.read", json!({ "id": id, "lines": 40 })) else {
            return;
        };
        if screen["text"].as_str().is_some_and(|s| s.contains(&trust.contains)) {
            self.state.lock().unwrap().trusted.insert(id.to_string());
            // A menu that has just appeared isn't listening yet, and Enter has to land after it moved.
            std::thread::sleep(TRUST_SETTLE);
            let answer = trust.answer.replace("\\r", "\r").replace("\\e", "\u{1b}");
            let keys = answer.strip_suffix('\r').unwrap_or(&answer);
            if !keys.is_empty() {
                let _ = crate::typing::Port::send(&self.bridge, id, keys.as_bytes());
                std::thread::sleep(TRUST_STEP);
            }
            if answer.ends_with('\r') {
                let _ = crate::typing::Port::send(&self.bridge, id, b"\r");
            }
            if let Some(f) = self.terminal_factory(id) {
                self.log(&f, Run::new("terminal", format!("{} was told to trust its folder", self.name_of(id))).terminal(Some(id)).by("engine"));
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_line_quotes_every_word_and_keeps_its_environment() {
        let line = command_line(&["a b".to_string(), "c'd".to_string()], &[("X".to_string(), "1 2".to_string())]);
        assert_eq!(line, r#"env X='1 2' 'a b' 'c'\''d'"#);
        assert_eq!(command_line(&["x".to_string()], &[]), "'x'");
    }
}
