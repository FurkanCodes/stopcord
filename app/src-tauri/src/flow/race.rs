//! A race: several agents do one stage at once, each in its own copy of the branch. The next gate's
//! checks run in every copy, and one copy that passed comes back to the real branch. The rest are dropped.

use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use super::{REPORT_GRACE, item_of};
use crate::engine::{Engine, StartRequest};
use crate::factories::{Factory, LineStage, Race, Racer, StageKind, Waiting, WorkItem};
use crate::line::{self, Step};
use crate::runs::{self, Run};
use crate::worktree;

const RUNNING: &str = "running";
const CHECKING: &str = "checking";
const PASSED: &str = "passed";
const FAILED: &str = "failed";
const OUT: &str = "out";

/// The agents that race at a stage, its own agent first.
pub(crate) fn racers_of(s: &LineStage) -> Vec<String> {
    std::iter::once(s.agent.clone()).chain(s.race.iter().cloned()).collect()
}

/// The gate right after a race. Its checks decide which copies pass.
fn next_gate<'a>(f: &'a Factory, s: &LineStage) -> Option<&'a LineStage> {
    f.line.iter().find(|g| g.id == s.then && g.kind == StageKind::Gate)
}

/// A racer's terminal, as the work item lists it beside its agents' own.
fn racer_key(stage: &str, agent: &str) -> String {
    format!("{stage}·{agent}")
}

fn race_note(field: usize, gate: Option<&LineStage>) -> String {
    let judged = match gate {
        Some(g) => format!("The {} checks run in each copy", g.id),
        None => "Each copy is looked at".to_string(),
    };
    format!("You're one of {field} agents doing this at once, each in its own copy of the branch. Commit your work, then finish as above. {judged}, and only one copy is kept.")
}

impl Engine {
    fn race_dir(&self, factory: &str, w: &WorkItem, agent: &str) -> std::path::PathBuf {
        self.copy_dir(factory, w, &format!("race-{agent}"))
    }

    /// Starts every racer at a race stage, or, when the race is already running, as after a restart,
    /// looks at where it got to.
    pub(super) fn start_race(self: &Arc<Self>, factory: &str, key: &str, stage: &str, note: &str) {
        let Ok(f) = self.store.get(factory) else { return };
        let Some(w) = item_of(&f, key).cloned() else { return };
        let Some(s) = f.line.iter().find(|x| x.id == stage).cloned() else { return };
        if w.race.as_ref().is_some_and(|r| r.stage == stage) {
            self.check_racers(factory, key);
            return;
        }
        // Racers left from an earlier round at this stage are done with.
        self.close_racers(factory, &w, stage);
        let project = Path::new(&f.path);
        let real = match worktree::ensure(project, &self.store.worktrees_dir(factory), &w.branch) {
            Ok(real) if real != project => real,
            Ok(_) => {
                self.fail_step(factory, key, format!("{key} can't race at {stage}: a race needs the work in its own worktree, and this factory works in the project folder."));
                return;
            }
            Err(e) => {
                self.fail_step(factory, key, format!("{key} couldn't get a worktree to race in at {stage}: {e:#}"));
                return;
            }
        };
        let base = worktree::git(project, &["rev-parse", "HEAD"])
            .and_then(|head| worktree::git(&real, &["merge-base", &head, "HEAD"]))
            .unwrap_or_default();
        let note = if note.is_empty() { w.stage_note.clone() } else { note.to_string() };
        let _ = self.store.update(factory, |fa| {
            if let Some(i) = fa.work.iter_mut().find(|i| i.ticket.key == key) {
                i.worktree = real.display().to_string();
                if i.base.is_empty() {
                    i.base = base.clone();
                }
                i.race = Some(Race { stage: stage.to_string(), racers: Vec::new() });
                i.stage_note.clear();
            }
        });
        let Some(w) = self.store.get(factory).ok().and_then(|f| item_of(&f, key).cloned()) else { return };
        let names = racers_of(&s);
        self.log(factory, Run::new("line", format!("{key}: {} race at {stage}", names.join(", "))).ok(true).work(&w.id).by("engine"));
        for agent in &names {
            self.start_racer(&f, &w, &s, agent, names.len(), &note, &real);
        }
    }

    fn start_racer(self: &Arc<Self>, f: &Factory, w: &WorkItem, s: &LineStage, agent_id: &str, field: usize, note: &str, real: &Path) {
        let key = w.ticket.key.as_str();
        let out = |terminal: String, why: String| {
            self.log(&f.id, Run::new("line", format!("{key}: {agent_id} is out of the race at {}: {why}", s.id)).ok(false).work(&w.id).by("engine"));
            self.save_racer(&f.id, key, Racer { agent: agent_id.to_string(), terminal, state: OUT.into(), note: why, at: runs::now_ms() });
        };
        let agent = f.agents.iter().chain(&f.scorers).find(|a| a.id == agent_id);
        let command = self.harness_for(f, agent);
        if command.trim().is_empty() {
            return out(String::new(), "it has no harness. Set one on the agent or in the factory's defaults".into());
        }
        let copy = self.race_dir(&f.id, w, agent_id);
        if let Err(e) = worktree::throwaway_copy(Path::new(&f.path), real, &w.branch, &copy) {
            return out(String::new(), format!("its copy couldn't be made: {e:#}"));
        }
        crate::runner::carry_setup(real, &copy);
        if let Some(part) = f.knowledge() {
            crate::kb::link(&self.store.knowledge_file(&f.id), &copy, &part.read_as);
        }
        let req = StartRequest {
            factory_id: Some(f.id.clone()),
            command: Some(command),
            name: Some(format!("{key} · {} · {agent_id}", s.id)),
            branch: Some(w.branch.clone()),
            runner: agent.map(|a| a.runner.clone()),
            workspace: Some(copy.clone()),
            ..StartRequest::default()
        };
        let t = match crate::engine::start_terminal(&self.store, &self.bridge, &self.program, req) {
            Ok(t) => t,
            Err(e) => return out(String::new(), format!("its terminal couldn't start: {}", e.message)),
        };
        let id = t["id"].as_str().unwrap_or_default().to_string();
        let cwd = t["cwd"].as_str().unwrap_or_default().to_string();
        self.state.lock().unwrap().terminals.insert(id.clone(), t);
        let chosen = crate::runner::pick(f, agent.map(|a| a.runner.as_str()));
        if let Err(why) = crate::runner::wait_for_setup(chosen, Path::new(&cwd)) {
            return out(id, format!("its terminal couldn't start: {why}"));
        }
        // Saved before the brief, so a racer that finishes at once is already in the race.
        self.save_racer(&f.id, key, Racer { agent: agent_id.to_string(), terminal: id.clone(), state: RUNNING.into(), ..Racer::default() });
        let st = if crate::runner::in_vm(chosen) { "st".to_string() } else { self.store.home().join("bin/st").display().to_string() };
        let prompt = agent.map_or("", |a| a.prompt.as_str());
        let mut text = format!("{}\n\n{}", line::brief(prompt, w, &self.skills_for(f, agent_id), &st, &[]), race_note(field, next_gate(f, s)));
        if !note.is_empty() {
            text.push_str(&format!("\n\n{note}"));
        }
        if let Err(e) = self.type_engine_turn(&id, &text) {
            out(id, format!("it never got its brief: {e}"));
        }
    }

    /// Adds a racer to the race at the work's stage, or replaces the one with its name.
    fn save_racer(&self, factory: &str, key: &str, racer: Racer) {
        let _ = self.store.update(factory, |f| {
            let Some(w) = f.work.iter_mut().find(|w| w.ticket.key == key) else { return };
            let stage = w.line_stage.clone();
            if !racer.terminal.is_empty() {
                w.agents.insert(racer_key(&stage, &racer.agent), racer.terminal.clone());
            }
            let race = w.race.get_or_insert_with(|| Race { stage, racers: Vec::new() });
            match race.racers.iter_mut().find(|r| r.agent == racer.agent) {
                Some(r) => *r = racer.clone(),
                None => race.racers.push(racer.clone()),
            }
        });
    }

    fn set_racer(&self, factory: &str, key: &str, agent: &str, change: impl Fn(&mut Racer)) {
        let _ = self.store.update(factory, |f| {
            if let Some(r) = f.work.iter_mut().find(|w| w.ticket.key == key).and_then(|w| w.race.as_mut()).and_then(|r| r.racers.iter_mut().find(|r| r.agent == agent)) {
                change(r);
            }
        });
    }

    /// The racer still running in this terminal: factory, work key and agent.
    fn racer_on(&self, terminal: &str) -> Option<(String, String, String)> {
        for f in self.store.list().unwrap_or_default() {
            for w in &f.work {
                if let Some(r) = w.race.as_ref().and_then(|r| r.racers.iter().find(|r| r.terminal == terminal && r.state == RUNNING)) {
                    return Some((f.id.clone(), w.ticket.key.clone(), r.agent.clone()));
                }
            }
        }
        None
    }

    /// A report from a racer's terminal. False when this terminal isn't racing.
    pub(super) fn race_report(self: &Arc<Self>, terminal: &str, report: &Value) -> bool {
        let Some((factory, key, agent)) = self.racer_on(terminal) else { return false };
        let verb = report["verb"].as_str().unwrap_or_default();
        let text = report["text"].as_str().unwrap_or_default().to_string();
        let work = self.store.get(&factory).ok().and_then(|f| item_of(&f, &key).map(|w| w.id.clone())).unwrap_or_default();
        self.log(&factory, Run::new("agent", format!("{agent} racing: {verb}: {text}")).terminal(Some(terminal)).work(&work).by("agent"));
        match verb {
            "done" => {
                if let Err(why) = self.mirror_vm(&factory, terminal) {
                    self.racer_out(&factory, &key, &agent, format!("its work couldn't come back from the VM: {why}"));
                    return true;
                }
                self.set_racer(&factory, &key, &agent, |r| {
                    r.state = CHECKING.into();
                    r.note = text.clone();
                    r.at = runs::now_ms();
                });
                let me = Arc::clone(self);
                std::thread::spawn(move || me.check_racer(&factory, &key, &agent));
            }
            // A race doesn't wait for one racer's question. The others carry on.
            "ask" | "send" => self.racer_out(&factory, &key, &agent, format!("it stopped to ask: {text}")),
            _ => {}
        }
        true
    }

    fn racer_out(self: &Arc<Self>, factory: &str, key: &str, agent: &str, why: String) {
        self.log(factory, Run::new("line", format!("{key}: {agent} is out of the race: {why}")).ok(false).by("engine"));
        self.set_racer(factory, key, agent, |r| {
            r.state = OUT.into();
            r.note = why.clone();
            r.at = runs::now_ms();
        });
        self.finish_race_if_over(factory, key);
    }

    /// Runs the next gate's checks in one racer's copy.
    fn check_racer(self: &Arc<Self>, factory: &str, key: &str, agent: &str) {
        let guard = (factory.to_string(), key.to_string(), agent.to_string());
        if !self.state.lock().unwrap().checking.insert(guard.clone()) {
            return;
        }
        let checked = (|| {
            let f = self.store.get(factory).ok()?;
            let w = item_of(&f, key).cloned()?;
            let s = f.line.iter().find(|s| s.id == w.line_stage)?;
            let Some(gate) = next_gate(&f, s) else { return Some((true, String::new(), String::new())) };
            let into = self.store.state_dir(factory).join("gates").join(format!("{}-{agent}", w.id));
            let copy = self.race_dir(factory, &w, agent);
            // The copy's commits live only in the copy. The snapshot is made from the project, so they go there first.
            if let Err(e) = worktree::git(Path::new(&w.worktree), &["fetch", "--quiet", &copy.display().to_string(), "HEAD"]) {
                return Some((false, format!("its commits couldn't be fetched: {e:#}"), gate.id.clone()));
            }
            let (ok, why) = self.checks_in(&f, &w, gate, &copy, &into);
            Some((ok, why, gate.id.clone()))
        })();
        if let Some((ok, why, gate)) = checked {
            if !gate.is_empty() {
                self.log(factory, Run::new("line", format!("{key}: {agent}'s copy {} {gate}", if ok { "passed" } else { "failed" })).ok(ok).by("engine"));
            }
            self.set_racer(factory, key, agent, |r| {
                r.state = if ok { PASSED } else { FAILED }.into();
                if !ok {
                    r.note = format!("failed {gate}: {}", runs::last_lines(&why, 6));
                }
            });
        }
        self.state.lock().unwrap().checking.remove(&guard);
        self.finish_race_if_over(factory, key);
    }

    /// The tick's look at a race: a racer whose terminal went away is out, and a copy whose checks
    /// stopped with the app is checked again.
    pub(super) fn check_racers(self: &Arc<Self>, factory: &str, key: &str) {
        let Some(race) = self.store.get(factory).ok().and_then(|f| item_of(&f, key).and_then(|w| w.race.clone())) else { return };
        for r in &race.racers {
            let alive = self.state.lock().unwrap().terminals.get(&r.terminal).is_some_and(|t| t["state"] != "exited");
            let checking = self.state.lock().unwrap().checking.contains(&(factory.to_string(), key.to_string(), r.agent.clone()));
            if r.state == RUNNING && !alive {
                self.racer_out(factory, key, &r.agent, "its terminal went away".into());
            } else if r.state == CHECKING && !checking {
                let me = Arc::clone(self);
                let (fid, key, agent) = (factory.to_string(), key.to_string(), r.agent.clone());
                std::thread::spawn(move || me.check_racer(&fid, &key, &agent));
            }
        }
        self.finish_race_if_over(factory, key);
    }

    /// A racer's turn ended. If it didn't report, it's reminded once, then it's out.
    pub(super) fn racer_turn_finished(self: &Arc<Self>, terminal: &str, engine_turn: bool) -> bool {
        if self.racer_on(terminal).is_none() {
            return false;
        }
        // A person talking to a racer is no reason to drop it.
        if !engine_turn {
            return true;
        }
        let me = Arc::clone(self);
        let terminal = terminal.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(REPORT_GRACE);
            me.collect_reports(&terminal);
            let Some((factory, key, agent)) = me.racer_on(&terminal) else {
                me.state.lock().unwrap().reminded.remove(&terminal);
                return;
            };
            let idle = me.state.lock().unwrap().terminals.get(&terminal).is_some_and(|t| t["state"] == "idle");
            if !idle {
                return;
            }
            if me.state.lock().unwrap().reminded.insert(terminal.clone()) {
                let in_vm = me.state.lock().unwrap().terminals.get(&terminal).and_then(crate::vm::spec_of).is_some();
                let st = if in_vm { "st".to_string() } else { me.store.home().join("bin/st").display().to_string() };
                if me.type_engine_turn(&terminal, &format!("You stopped without reporting. If you've finished, commit and run: {st} done \"one line on what you did\".")).is_ok() {
                    return;
                }
            }
            me.state.lock().unwrap().reminded.remove(&terminal);
            me.racer_out(&factory, &key, &agent, "it went quiet without saying it was done".into());
        });
        true
    }

    /// Once no racer is running or being checked, picks the copy to keep and moves the work on.
    fn finish_race_if_over(self: &Arc<Self>, factory: &str, key: &str) {
        let guard = (factory.to_string(), key.to_string());
        // Whoever holds the work now will look again, and so will the next tick.
        if !self.state.lock().unwrap().stepping.insert(guard.clone()) {
            return;
        }
        let step = self.decide_race(factory, key);
        self.state.lock().unwrap().stepping.remove(&guard);
        if let Some(step) = step {
            self.step(factory, key, step);
        }
    }

    fn decide_race(self: &Arc<Self>, factory: &str, key: &str) -> Option<Step> {
        let f = self.store.get(factory).ok()?;
        let w = item_of(&f, key).cloned()?;
        let race = w.race.clone()?;
        if race.stage != w.line_stage || w.waiting != Waiting::No || race.racers.is_empty() {
            return None;
        }
        let s = f.line.iter().find(|s| s.id == race.stage)?.clone();
        // Every racer is in the race before the first is briefed, unless one never got that far.
        if race.racers.len() < racers_of(&s).len() || race.racers.iter().any(|r| r.state == RUNNING || r.state == CHECKING) {
            return None;
        }
        let mut passing: Vec<&Racer> = race.racers.iter().filter(|r| r.state == PASSED).collect();
        passing.sort_by_key(|r| r.at);
        if passing.is_empty() {
            let each: Vec<String> = race.racers.iter().map(|r| format!("- {} {}: {}", r.agent, r.state, r.note.lines().next().unwrap_or_default())).collect();
            let text = format!("{key}: none of the {} agents racing at {} passed. Their terminals are still open.\n{}", race.racers.len(), s.id, each.join("\n"));
            let _ = self.store.update(factory, |fa| {
                if let Some(i) = fa.work.iter_mut().find(|i| i.ticket.key == key) {
                    i.race = None;
                    i.waiting = Waiting::Failed;
                }
            });
            return Some(Step::Person { stage: s.id.clone(), why: Waiting::Failed, text });
        }
        let picked = if passing.len() > 1 && !s.judge.is_empty() { self.judge_race(&f, &w, &s, &passing) } else { None };
        let winner = picked.as_deref().and_then(|name| passing.iter().find(|r| r.agent == name)).copied().unwrap_or(passing[0]).clone();
        let how = match (&picked, passing.len()) {
            (Some(_), _) => format!(", picked by {}", s.judge),
            (None, 1) => String::new(),
            (None, _) => ", the first to pass".to_string(),
        };
        // The winner's commits come back to the real branch. Its copy started there, so it's a fast-forward.
        let copy = self.race_dir(factory, &w, &winner.agent);
        let real = Path::new(&w.worktree);
        let back = worktree::git(real, &["fetch", "--quiet", &copy.display().to_string(), "HEAD"]).and_then(|_| worktree::git(real, &["merge", "--ff-only", "--quiet", "FETCH_HEAD"]));
        if let Err(e) = back {
            self.fail_step(factory, key, format!("{key}: {} won the race at {}, but its work couldn't come back to the branch: {e:#}", winner.agent, s.id));
            return None;
        }
        self.close_racers(factory, &w, &s.id);
        for r in &race.racers {
            let _ = std::fs::remove_dir_all(self.race_dir(factory, &w, &r.agent));
        }
        self.log(factory, Run::new("line", format!("{key}: {} won the race at {}{how}", winner.agent, s.id)).ok(true).work(&w.id).by("engine"));
        let note = format!("{} won the race at {}{how}. It said: {}", winner.agent, s.id, winner.note);
        self.store
            .update(factory, |fa| {
                if let Some(i) = fa.work.iter_mut().find(|i| i.ticket.key == key) {
                    i.race = None;
                }
                line::agent_done(fa, key, &note)
            })
            .ok()
    }

    /// Asks the stage's judge which passing copy to keep. Nothing when it can't say, and the first to pass is kept.
    fn judge_race(self: &Arc<Self>, f: &Factory, w: &WorkItem, s: &LineStage, passing: &[&Racer]) -> Option<String> {
        let judge = f.scorers.iter().chain(&f.agents).find(|a| a.id == s.judge)?;
        let command = self.harness_for(f, Some(judge));
        if command.trim().is_empty() {
            return None;
        }
        let each = line::ONCE_DIFF_CHARS / passing.len().max(1);
        let copies: Vec<(String, String, String)> = passing
            .iter()
            .map(|r| {
                let diff = worktree::git(&self.race_dir(&f.id, w, &r.agent), &["diff", &format!("{}..HEAD", w.base)]).unwrap_or_default();
                (r.agent.clone(), r.note.clone(), diff.chars().take(each).collect())
            })
            .collect();
        let names: Vec<String> = passing.iter().map(|r| r.agent.clone()).collect();
        let prompt = line::race_brief(&judge.prompt, w, &copies);
        match self.one_shot(f, &command, &prompt) {
            Ok(answer) => {
                let name = crate::oneshot::pick_stage(&answer, &names);
                if name.is_none() {
                    self.log(&f.id, Run::new("line", format!("{}: {} didn't name one of {}, so the first to pass is kept", w.ticket.key, judge.id, names.join(", "))).ok(false).work(&w.id).by("engine"));
                }
                name
            }
            Err(why) => {
                self.log(&f.id, Run::new("line", format!("{}: {} couldn't judge the race: {why}", w.ticket.key, judge.id)).ok(false).work(&w.id).by("engine"));
                None
            }
        }
    }

    /// Closes the racers' terminals at a stage, and forgets them.
    fn close_racers(&self, factory: &str, w: &WorkItem, stage: &str) {
        let prefix = racer_key(stage, "");
        for (_, t) in w.agents.iter().filter(|(k, _)| k.starts_with(&prefix)) {
            let _ = self.bridge.call("terminal.close", json!({ "id": t }));
        }
        let _ = self.store.update(factory, |f| {
            if let Some(i) = f.work.iter_mut().find(|i| i.ticket.key == w.ticket.key) {
                i.agents.retain(|k, _| !k.starts_with(&prefix));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stages_own_agent_races_first() {
        let s = LineStage { agent: "builder".into(), race: vec!["codex".into(), "gemini".into()], ..LineStage::default() };
        assert_eq!(racers_of(&s), ["builder", "codex", "gemini"]);
        assert!(race_note(3, None).contains("one of 3 agents"));
    }
}
