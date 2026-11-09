//! Benchmarks: fixed tasks run down a factory's own line, to see whether a change to
//! the factory made it better. A benchmark never merges and never asks a person.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::engine::Engine;
use crate::factories::{BenchRef, Factory, Store, Ticket, slug};
use crate::flow::{GATE_TIMEOUT, confined_check};
use crate::line::Step;
use crate::runs::{self, Run};
use crate::{gate, hooks, work, worktree};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BenchResult {
    pub at: u64,
    pub suite: String,
    pub run: String,
    pub task: String,
    pub passed: bool,
    pub seconds: u64,
    /// How many times a stage failed before it got this far.
    pub loops: u32,
    pub findings: usize,
    pub why: String,
}

fn results_file(store: &Store, id: &str) -> PathBuf {
    store.state_dir(id).join("benchmarks.jsonl")
}

/// Every result so far, newest first.
pub fn results(store: &Store, id: &str) -> Vec<BenchResult> {
    let text = std::fs::read_to_string(results_file(store, id)).unwrap_or_default();
    text.lines().rev().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn record(store: &Store, id: &str, result: &BenchResult) {
    let Ok(line) = serde_json::to_string(result) else { return };
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(results_file(store, id)) {
        let _ = writeln!(file, "{line}");
    }
}

impl Engine {
    /// Queues every task in a suite. Returns the run's id.
    pub fn run_benchmark(self: &Arc<Self>, factory: &str, suite_id: &str) -> Result<String, String> {
        let f = self.store.get(factory).map_err(|e| format!("{e:#}"))?;
        if f.line.is_empty() {
            return Err("A benchmark runs down the line, and this factory has none.".to_string());
        }
        let suite = f.benchmarks.iter().find(|s| s.id == suite_id).cloned().ok_or_else(|| format!("there's no benchmark {suite_id}"))?;
        let run = (runs::now_ms() / 1000 % 1_000_000).to_string();
        let mut queued = Vec::new();
        for task in &suite.tasks {
            let key = format!("bench-{}-{}-{run}", slug(&suite.id), slug(&task.key));
            if !suite.base.is_empty() {
                // A fixed starting commit keeps runs comparable as main moves on.
                worktree::git(Path::new(&f.path), &["branch", &work::branch_for(&key), &suite.base]).map_err(|e| format!("couldn't start from {}: {e:#}", suite.base))?;
            }
            queued.push((
                Ticket { key, title: task.title.clone(), body: task.body.clone(), url: String::new() },
                BenchRef { suite: suite.id.clone(), run: run.clone(), task: task.key.clone(), check: task.check.clone(), begun_at: 0 },
            ));
        }
        let tickets: Vec<Ticket> = queued.iter().map(|(t, _)| t.clone()).collect();
        self.store
            .update(factory, |fa| {
                work::feed(fa, tickets);
                for (ticket, bench) in &queued {
                    if let Some(w) = fa.work.iter_mut().find(|w| w.ticket.key == ticket.key) {
                        w.bench = Some(bench.clone());
                        w.start_at = suite.start_at.clone();
                    }
                }
            })
            .map_err(|e| format!("{e:#}"))?;
        self.log(factory, Run::new("benchmark", format!("{} started with {} tasks", suite.id, suite.tasks.len())).by("person"));
        self.pump_tickets();
        Ok(run)
    }

    fn bench_passed(&self, f: &Factory, worktree: &Path, base: &str, check: &str, id: &str) -> (bool, String) {
        let into = self.store.state_dir(&f.id).join("gates").join(id);
        let snap = match gate::snapshot(Path::new(&f.path), worktree, base, &[], &into) {
            Ok(s) => s,
            Err(e) => return (false, format!("{e:#}")),
        };
        match confined_check(self.store.home(), &snap.dir, check, false, &self.program) {
            Ok(command) => {
                let (ok, output) = hooks::run_command(&command, &snap.dir, &mut std::io::sink(), Some(GATE_TIMEOUT));
                (ok, runs::last_lines(&output, 20))
            }
            Err(e) => (false, e),
        }
    }

    /// A benchmark item stopped at a person. Scores it, cleans up, and moves on without asking anyone.
    pub(crate) fn finish_bench(self: &Arc<Self>, factory: &str, key: &str, finished: bool, why: String) {
        let Ok(f) = self.store.get(factory) else { return };
        let Some(w) = f.work.iter().find(|w| w.ticket.key == key).cloned() else { return };
        let Some(bench) = w.bench.clone() else { return };
        let (passed, why) = if finished {
            let (ok, output) = self.bench_passed(&f, Path::new(&w.worktree), &w.base, &bench.check, &w.id);
            (ok, if ok { String::new() } else { output })
        } else {
            (false, why)
        };
        // Cleaned up first, so a recorded result means this task has fully finished.
        self.step(factory, key, Step::End { outcome: "benchmarked".to_string() });
        for t in w.agents.values() {
            let _ = self.bridge.call("terminal.close", json!({ "id": t }));
        }
        let project = Path::new(&f.path);
        if !w.worktree.is_empty() {
            let _ = worktree::git(project, &["worktree", "remove", "--force", &w.worktree]);
        }
        let _ = worktree::git(project, &["branch", "-D", &w.branch]);
        let now = runs::now_ms();
        record(
            &self.store,
            factory,
            &BenchResult {
                at: now,
                suite: bench.suite.clone(),
                run: bench.run.clone(),
                task: bench.task.clone(),
                passed,
                seconds: now.saturating_sub(bench.begun_at) / 1000,
                loops: w.loops.values().sum(),
                findings: w.findings.len(),
                why,
            },
        );
        self.log(factory, Run::new("benchmark", format!("{} / {} {}", bench.suite, bench.task, if passed { "passed" } else { "failed" })).ok(passed).work(&w.id).by("engine"));
    }
}
