//! A panel: agents take turns at one stage with `st say`, each in its own terminal. Stopcord types
//! each turn into the next member's terminal, and a judge that answers once scores every round.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::item_of;
use crate::engine::Engine;
use crate::factories::Score;
use crate::line::{self, Step};
use crate::runs::Run;

/// How long a turn waits for the work to be free, as when the next member is still being briefed.
const WAIT_FOR_WORK: Duration = Duration::from_secs(30);

impl Engine {
    /// A member said its piece. Its words go to the next member, or the round is scored first. The
    /// judge can take a while, so this runs on its own thread and takes the step itself.
    pub(super) fn panel_say(self: &Arc<Self>, factory: &str, key: &str, text: &str) {
        let me = Arc::clone(self);
        let (factory, key, text) = (factory.to_string(), key.to_string(), text.to_string());
        std::thread::spawn(move || {
            let guard = (factory.clone(), key.clone());
            // Nothing else may move the work between this turn and the next: not the tick, not a restart.
            let since = Instant::now();
            while !me.state.lock().unwrap().stepping.insert(guard.clone()) {
                if since.elapsed() > WAIT_FOR_WORK {
                    me.log(&factory, Run::new("line", format!("{key}: a panel turn waited too long for the work to be free, so it was dropped")).ok(false).by("engine"));
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let over = me.store.update(&factory, |f| line::panel_said(f, &key, &text)).ok().flatten();
            let step = match over {
                None => {
                    me.log(&factory, Run::new("line", format!("{key}: something was said outside a panel, so it went nowhere")).by("engine"));
                    Step::Nothing
                }
                Some(over) => {
                    let score = if over { me.score_panel(&factory, &key) } else { None };
                    // An isolated stage's kept files come back when it ends, as they do on st done.
                    let ends = over && me.store.get(&factory).ok().is_some_and(|f| line::panel_settles(&f, &key, score));
                    if ends && let Err(why) = me.finish_copy(&factory, &key) {
                        me.state.lock().unwrap().stepping.remove(&guard);
                        me.fail_step(&factory, &key, format!("{key}'s panel settled it, but its copy couldn't be collected: {why}"));
                        return;
                    }
                    me.store.update(&factory, |f| line::panel_next(f, &key, over, score)).unwrap_or(Step::Nothing)
                }
            };
            me.state.lock().unwrap().stepping.remove(&guard);
            me.step(&factory, &key, step);
        });
    }

    /// Asks the panel's judge, once, how settled the talk is. Nothing when there's no judge or no answer.
    fn score_panel(self: &Arc<Self>, factory: &str, key: &str) -> Option<u32> {
        let f = self.store.get(factory).ok()?;
        let w = item_of(&f, key)?.clone();
        let s = f.line.iter().find(|s| s.id == w.line_stage)?;
        let judge = f.scorers.iter().chain(&f.agents).find(|a| a.id == s.judge)?;
        let panel = w.panel.as_ref()?;
        let command = self.harness_for(&f, Some(judge));
        if command.trim().is_empty() {
            return None;
        }
        let prompt = line::panel_brief(&judge.prompt, &w, &panel.said);
        let fail = |why: String| {
            self.log(factory, Run::new("line", format!("{key}: {} couldn't score round {} at {}: {why}", judge.id, panel.round, s.id)).ok(false).work(&w.id).by("engine"));
            None
        };
        let answer = match self.one_shot(&f, &command, &prompt) {
            Ok(answer) => answer,
            Err(why) => return fail(why),
        };
        let reports = match crate::oneshot::read_answer(&answer, true) {
            Ok(reports) => reports,
            Err(why) => return fail(why),
        };
        let score = reports.iter().find(|r| r["verb"] == "score")?;
        let (n, why) = (score["score"].as_u64().unwrap_or(0) as u32, score["text"].as_str().unwrap_or_default().to_string());
        let stage = s.id.clone();
        let _ = self.store.update(factory, |fa| {
            if let Some(i) = fa.work.iter_mut().find(|i| i.ticket.key == key) {
                i.scores.push(Score { stage: stage.clone(), score: n, why: why.clone() });
            }
        });
        self.log(factory, Run::new("line", format!("{key}: {} scored round {} at {stage} {n}/10: {why}", judge.id, panel.round)).ok(true).work(&w.id).by("engine"));
        Some(n)
    }
}
