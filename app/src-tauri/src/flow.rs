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
use crate::factories::{Alert, Factory, Finding, LineStage, Stage, StageKind, Waiting, WorkItem};
use crate::line::{self, Action, Step};
use crate::runs::{self, Run};
use crate::{gate, harness, hooks, shell, worktree};

const GATE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// A `st done` usually lands before the terminal looks idle, but not always.
const REPORT_GRACE: Duration = Duration::from_secs(4);

const TRUST_SETTLE: Duration = Duration::from_millis(1200);
const TRUST_STEP: Duration = Duration::from_millis(250);

pub fn alert_key(work: &str) -> String {
    format!("work:{work}")
}

fn item_of<'a>(f: &'a Factory, key: &str) -> Option<&'a WorkItem> {
    f.work.iter().find(|w| w.ticket.key == key)
}

fn stage_of<'a>(f: &'a Factory, w: &WorkItem) -> Option<&'a LineStage> {
    f.line.iter().find(|s| s.id == w.line_stage)
}

/// Quotes a whole command line for `sh -c`, with its environment in front.
fn command_line(argv: &[String], env: &[(String, String)]) -> String {
    let mut out: Vec<String> = env.iter().map(|(k, v)| format!("{k}={}", shell::quote(v))).collect();
    if !out.is_empty() {
        out.insert(0, "env".to_string());
    }
    out.extend(argv.iter().map(|a| shell::quote(a)));
    out.join(" ")
}

/// A check's command, inside a sandbox with no network unless the check asks for it.
/// The gate is code an agent could have edited, so it never runs unconfined if it can be helped.
fn confined_check(store_dir: &Path, snapshot: &Path, command: &str, network: bool, program: &Path) -> Result<String, String> {
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
    fn harness_for(&self, f: &Factory, agent: Option<&crate::factories::Agent>) -> String {
        agent
            .map(|a| a.harness.clone())
            .filter(|h| !h.trim().is_empty())
            .or_else(|| Some(f.defaults.harness.clone()).filter(|h| !h.trim().is_empty()))
            .or_else(|| f.tickets().map(|s| s.harness.clone()).filter(|h| !h.trim().is_empty()))
            .unwrap_or_default()
    }

    /// Puts a claimed item on the line.
    pub(crate) fn begin_on_line(self: &Arc<Self>, factory: &str, key: &str) {
        let step = self.store.update(factory, |f| line::begin(f, key)).unwrap_or(Step::Nothing);
        self.state.lock().unwrap().starting.remove(&(factory.to_string(), key.to_string()));
        self.step(factory, key, step);
    }

    /// Does what the line said to do.
    pub(crate) fn step(self: &Arc<Self>, factory: &str, key: &str, step: Step) {
        let Ok(f) = self.store.get(factory) else { return };
        let Some(w) = item_of(&f, key).cloned() else { return };
        if step != Step::Nothing {
            self.clear_alert(factory, &alert_key(&w.id));
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
            Step::Person { stage, why, text } => {
                let (kind, event) = match why {
                    Waiting::Approve => ("approve", "approve"),
                    Waiting::Handoff => ("handoff", "handoff"),
                    Waiting::Question => ("question", "question"),
                    Waiting::Failed | Waiting::No => ("failed", "failed"),
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
                    Run::new("line", format!("{key} is waiting at {stage}: {text}"))
                        .ok(why != Waiting::Failed)
                        .work(&w.id)
                        .by("engine"),
                );
                self.report_back(&f, &w, event, &text);
            }
            Step::End { outcome } => {
                let _ = self.store.update(factory, |fa| {
                    if let Some(i) = fa.work.iter_mut().find(|i| i.ticket.key == key) {
                        i.stage = Stage::Ended;
                        i.ended_at = runs::now_ms();
                        i.terminal = None;
                    }
                });
                for t in w.agents.values() {
                    let _ = self.bridge.call("terminal.close", json!({ "id": t }));
                }
                self.log(factory, Run::new("line", format!("{key} {outcome}")).ok(true).work(&w.id).by("engine"));
                self.report_back(&f, &w, &outcome, &format!("{key} {outcome}"));
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
        let agent = f.agents.iter().find(|a| a.id == agent_id).cloned();
        let alive = |t: &str| {
            self.state.lock().unwrap().terminals.get(t).is_some_and(|v| v["state"] != "exited")
        };
        let existing = w.agents.get(agent_id).filter(|t| alive(t)).cloned();
        let st = self.store.home().join("bin/st").display().to_string();
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
        let brief = line::brief(agent.as_ref().map_or("", |a| a.prompt.as_str()), &w, &skills, &st);
        let (id, text) = if let Some(t) = existing {
            let text = if note.is_empty() {
                brief
            } else {
                format!("{note}\n\nWhen you've fixed it, commit and run: {st} done \"what you changed\".")
            };
            (t, text)
        } else {
            let command = self.harness_for(&f, agent.as_ref());
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
                ..StartRequest::default()
            };
            let t = match crate::engine::start_terminal_with(&self.store, &self.bridge, &self.program, req, self.deny_ports()) {
                Ok(t) => t,
                Err(e) => {
                    self.fail_step(factory, key, format!("{key} couldn't start {stage}: {}", e.message));
                    return;
                }
            };
            let id = t["id"].as_str().unwrap_or_default().to_string();
            let cwd = t["cwd"].as_str().unwrap_or_default().to_string();
            self.state.lock().unwrap().terminals.insert(id.clone(), t);
            let base = worktree::git(Path::new(&f.path), &["rev-parse", "HEAD"])
                .and_then(|head| worktree::git(Path::new(&cwd), &["merge-base", &head, "HEAD"]))
                .unwrap_or_default();
            let on_worktree = if cwd == f.path { String::new() } else { cwd };
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

    fn deny_ports(&self) -> Vec<u16> {
        crate::settings::load(self.store.home())
            .map(|s| vec![s.webhooks.port])
            .unwrap_or_default()
    }

    /// Runs one gate's checks on a clean copy of the agent's commit.
    fn gate_checks(&self, f: &Factory, w: &WorkItem, stage: &LineStage) -> (bool, String) {
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
            }
        }
        (passed, all)
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
                        let dup = fa
                            .work
                            .iter()
                            .find(|i| i.ticket.key == key)
                            .is_some_and(|i| i.findings.iter().any(|x| &x.file == file && x.text == text));
                        if !dup {
                            line::add_finding(
                                fa,
                                key,
                                Finding {
                                    file: file.clone(),
                                    line: 0,
                                    severity: "high".into(),
                                    text,
                                    stage: stage.id.clone(),
                                },
                            );
                        }
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
            "done" => self.store.update(&factory, |f| line::agent_done(f, &key, &text)),
            "ask" => self.store.update(&factory, |f| line::agent_stopped(f, &key, &format!("{key} asks: {text}"))),
            "finding" => {
                let finding = Finding {
                    file: report["file"].as_str().unwrap_or_default().to_string(),
                    line: report["line"].as_u64().unwrap_or(0),
                    severity: report["severity"].as_str().unwrap_or("medium").to_string(),
                    text: text.clone(),
                    stage: String::new(),
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
            let idle = me.state.lock().unwrap().terminals.get(&terminal).is_some_and(|t| t["state"] == "idle");
            if still && idle {
                let waiting = me.store.get(&factory).ok().and_then(|f| item_of(&f, &key).map(|w| w.waiting)).unwrap_or_default();
                if waiting == Waiting::No {
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
        }
    }

    /// A site was blocked.
    pub(crate) fn on_denied(self: &Arc<Self>, event: &Value) {
        let (Some(terminal), Some(host)) = (event["id"].as_str(), event["host"].as_str()) else {
            return;
        };
        let Some(factory) = self.terminal_factory(terminal) else { return };
        let name = self.name_of(terminal);
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
        self.log(factory, Run::new("line", format!("{action} on {key}")).work(&w.id).by("person"));
        let act = match action {
            "approve" => Action::Approve,
            "sendBack" => Action::SendBack { note: note.to_string() },
            "treatAsDone" => Action::TreatAsDone,
            "drop" => Action::Drop,
            "merge" => {
                let me = Arc::clone(self);
                let (fid, key) = (factory.to_string(), key.to_string());
                std::thread::spawn(move || me.merge(&fid, &key));
                return Ok(());
            }
            other => return Err(format!("unknown action {other}")),
        };
        let step = self.store.update(factory, |fa| line::person(fa, key, act)).map_err(|e| format!("{e:#}"))?;
        self.step(factory, key, step);
        Ok(())
    }

    /// Runs every gate once more, then merges. A merge that can't happen leaves the work at handoff.
    fn merge(self: &Arc<Self>, factory: &str, key: &str) {
        let Ok(f) = self.store.get(factory) else { return };
        let Some(w) = item_of(&f, key).cloned() else { return };
        let say = |text: String| {
            let _ = self.store.update(factory, |fa| {
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
        for stage in f.line.iter().filter(|s| s.kind == StageKind::Gate) {
            let (passed, output) = self.gate_checks(&f, &w, stage);
            if !passed {
                say(format!("{key} can't merge: {} failed again.\n{output}", stage.id));
                return;
            }
        }
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
        let step = self.store.update(factory, |fa| line::person(fa, key, Action::Merged)).unwrap_or(Step::Nothing);
        self.step(factory, key, step);
    }

    /// Runs the factory's report commands for this event, with the details in the environment.
    fn report_back(&self, f: &Factory, w: &WorkItem, event: &str, text: &str) {
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
                    &Run::new("report", format!("{} {}", r.id, if ok { "sent" } else { "failed" })).ok(ok).work(&work).by("engine").detail(&output),
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
        let Some(trust) = all.iter().find(|h| h.id == agent).and_then(|h| h.trust.clone()).filter(|t| t.auto) else {
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
