//! Typing a prompt into an agent's terminal: one at a time, and never over
//! something a person is in the middle of writing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

/// The terminal, as far as typing needs it.
pub trait Port: Send + Sync {
    fn get(&self, id: &str) -> Result<Value, String>;
    fn send(&self, id: &str, data: &[u8]) -> Result<(), String>;
}

/// Between pasting a prompt and pressing Enter, so the harness has taken the paste in.
const PASTE_GAP: Duration = Duration::from_millis(300);

pub struct Typist {
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// Characters a person has typed and not yet sent, by terminal.
    drafts: Mutex<HashMap<String, usize>>,
    ready: Duration,
    settle: Duration,
    poll: Duration,
    /// How long a terminal must stay idle before it counts as ready.
    steady: Duration,
}

/// How many characters are waiting unsent after a person sends `data` to a terminal.
pub fn draft_after(mut len: usize, data: &[u8]) -> usize {
    // A lone Esc clears the input box in the harnesses that have one.
    if data == b"\x1b" {
        return 0;
    }
    let mut pasting = false;
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if b == 0x1b {
            match data.get(i + 1) {
                Some(b'[') => {
                    let mut end = i + 2;
                    while end < data.len() && !(0x40..=0x7e).contains(&data[end]) {
                        end += 1;
                    }
                    match &data[i + 2..(end + 1).min(data.len())] {
                        b"200~" => pasting = true,
                        b"201~" => pasting = false,
                        _ => {}
                    }
                    i = end + 1;
                }
                Some(b'O') => i += 3,
                Some(_) => i += 2,
                None => i += 1,
            }
            continue;
        }
        match b {
            b'\r' | b'\n' if pasting => len += 1,
            b'\r' | b'\n' | 0x03 | 0x15 => len = 0,
            0x7f | 0x08 => len = len.saturating_sub(1),
            b if b < 0x20 => {}
            // The rest of a UTF-8 letter: Backspace removes the whole letter.
            b if b & 0xc0 == 0x80 => {}
            _ => len += 1,
        }
        i += 1;
    }
    len
}

impl Typist {
    pub fn new(ready: Duration, settle: Duration, poll: Duration, steady: Duration) -> Typist {
        Typist {
            locks: Mutex::default(),
            drafts: Mutex::default(),
            ready,
            settle,
            poll,
            steady,
        }
    }

    /// A person sent `data` to the terminal.
    pub fn person_typed(&self, id: &str, data: &[u8]) {
        let mut drafts = self.drafts.lock().unwrap();
        let len = draft_after(drafts.get(id).copied().unwrap_or(0), data);
        drafts.insert(id.to_string(), len);
    }

    /// The agent started working, so whatever was typed has gone in.
    pub fn turn_began(&self, id: &str) {
        self.drafts.lock().unwrap().remove(id);
    }

    pub fn forget(&self, id: &str) {
        self.drafts.lock().unwrap().remove(id);
        self.locks.lock().unwrap().remove(id);
    }

    fn writing(&self, id: &str) -> bool {
        self.drafts.lock().unwrap().get(id).is_some_and(|n| *n > 0)
    }

    /// Waits until an agent has sat steadily at its prompt and nobody is writing,
    /// then types `prompt` and presses Enter. Holds the terminal until the turn
    /// has begun, so the next prompt can't take the idle screen from before this
    /// one for a free terminal. Only says it worked once the agent has started.
    pub fn type_prompt(
        &self,
        port: &dyn Port,
        id: &str,
        prompt: &str,
        name: &str,
        about_to_type: &dyn Fn(),
    ) -> Result<(), String> {
        let lock = Arc::clone(self.locks.lock().unwrap().entry(id.to_string()).or_default());
        let _held = lock.lock().unwrap();
        let mut deadline = Instant::now() + self.ready;
        let mut idle_since: Option<Instant> = None;
        loop {
            let t = port.get(id).map_err(|e| format!("{name} is gone: {e}"))?;
            if t["state"] == "exited" {
                return Err(format!("{name} ended before the prompt could be typed"));
            }
            // It's asking a person something and already shows as needing them. Waiting on
            // that isn't the agent being slow, so it doesn't use up the time.
            if t["state"] == "blocked" {
                deadline = Instant::now() + self.ready;
            }
            let writing = self.writing(id);
            if !t["agent"].is_null() && t["state"] == "idle" && !writing {
                if idle_since.get_or_insert_with(Instant::now).elapsed() >= self.steady {
                    break;
                }
            } else {
                idle_since = None;
            }
            if Instant::now() > deadline {
                return Err(if writing {
                    format!("Someone was writing in {name}, so the prompt wasn't typed over it")
                } else {
                    format!(
                        "{name} wasn't ready for a prompt within {} seconds, so nothing was typed",
                        self.ready.as_secs()
                    )
                });
            }
            thread::sleep(self.poll);
        }
        about_to_type();
        let paste = format!("\x1b[200~{}\x1b[201~", prompt.trim());
        port.send(id, paste.as_bytes())?;
        thread::sleep(self.poll.min(PASTE_GAP));
        port.send(id, b"\r")?;
        let settled = Instant::now() + self.settle;
        while Instant::now() < settled {
            match port.get(id) {
                Ok(t) if t["state"] == "idle" => thread::sleep(self.poll),
                _ => return Ok(()),
            }
        }
        Err(format!(
            "The prompt was typed into {name}, but the agent didn't start on it"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A terminal whose agent starts working on Enter and goes idle again shortly after.
    #[derive(Default)]
    struct Fake {
        state: Mutex<String>,
        sent: Mutex<Vec<String>>,
        turn: Duration,
        /// The harness isn't really ready: Enter doesn't start a turn.
        deaf: bool,
        first_send: Mutex<Option<Instant>>,
    }

    impl Fake {
        fn idle(turn: Duration) -> Arc<Fake> {
            Arc::new(Fake {
                state: Mutex::new("idle".into()),
                sent: Mutex::default(),
                turn,
                deaf: false,
                first_send: Mutex::default(),
            })
        }

        fn sent(&self) -> Vec<String> {
            self.sent.lock().unwrap().clone()
        }
    }

    struct Shared(Arc<Fake>);

    impl Port for Shared {
        fn get(&self, _id: &str) -> Result<Value, String> {
            Ok(json!({ "state": *self.0.state.lock().unwrap(), "agent": "claude" }))
        }

        fn send(&self, _id: &str, data: &[u8]) -> Result<(), String> {
            let text = String::from_utf8_lossy(data).into_owned();
            self.0.sent.lock().unwrap().push(text.clone());
            self.0.first_send.lock().unwrap().get_or_insert_with(Instant::now);
            if text == "\r" && !self.0.deaf {
                *self.0.state.lock().unwrap() = "working".into();
                let me = Arc::clone(&self.0);
                thread::spawn(move || {
                    thread::sleep(me.turn);
                    *me.state.lock().unwrap() = "idle".into();
                });
            }
            Ok(())
        }
    }

    fn typist(ready_ms: u64) -> Arc<Typist> {
        Arc::new(Typist::new(
            Duration::from_millis(ready_ms),
            Duration::from_millis(500),
            Duration::from_millis(5),
            Duration::from_millis(40),
        ))
    }

    #[test]
    fn drafts_count_what_is_still_unsent() {
        assert_eq!(draft_after(0, b"hey"), 3);
        assert_eq!(draft_after(3, b"\x7f"), 2);
        assert_eq!(draft_after(0, b"\x7f"), 0);
        assert_eq!(draft_after(3, b"\r"), 0);
        assert_eq!(draft_after(0, b"hey\r"), 0);
        assert_eq!(draft_after(3, b"\x03"), 0);
        assert_eq!(draft_after(3, b"\x15"), 0);
        // A lone Esc clears the box in the harnesses that have one.
        assert_eq!(draft_after(3, b"\x1b"), 0);
    }

    #[test]
    fn keys_that_type_nothing_dont_count() {
        assert_eq!(draft_after(0, b"\x1b[I"), 0, "window focus");
        assert_eq!(draft_after(2, b"\x1b[A"), 2, "arrow");
        assert_eq!(draft_after(2, b"\x1bOP"), 2, "F1");
        assert_eq!(draft_after(2, b"\t"), 2, "tab");
    }

    #[test]
    fn a_turkish_letter_is_one_character() {
        assert_eq!(draft_after(0, "ş".as_bytes()), 1);
        assert_eq!(draft_after(1, b"\x7f"), 0);
    }

    #[test]
    fn a_pasted_newline_isnt_sending() {
        assert_eq!(draft_after(0, b"\x1b[200~a\nb\x1b[201~"), 3);
    }

    #[test]
    fn waits_until_the_person_has_sent_what_they_were_writing() {
        let fake = Fake::idle(Duration::from_millis(50));
        let t = typist(2000);
        t.person_typed("t1", b"half a thou");
        let (t2, f2) = (Arc::clone(&t), Arc::clone(&fake));
        let job = thread::spawn(move || t2.type_prompt(&Shared(f2), "t1", "fix it", "term", &|| ()));
        thread::sleep(Duration::from_millis(100));
        assert!(fake.sent().is_empty(), "typed over a draft: {:?}", fake.sent());
        t.person_typed("t1", b"\r");
        job.join().unwrap().unwrap();
        assert!(fake.sent()[0].contains("fix it"));
    }

    #[test]
    fn gives_up_rather_than_type_over_a_person() {
        let fake = Fake::idle(Duration::from_millis(50));
        let t = typist(60);
        t.person_typed("t1", b"still writing");
        let err = t.type_prompt(&Shared(Arc::clone(&fake)), "t1", "fix it", "term", &|| ()).unwrap_err();
        assert!(fake.sent().is_empty());
        assert!(err.contains("writing"), "{err}");
    }

    #[test]
    fn a_turn_starting_clears_the_draft() {
        // Pressing "1" on a permission menu has no Enter, but it starts the turn.
        let t = typist(60);
        t.person_typed("t1", b"1");
        t.turn_began("t1");
        let fake = Fake::idle(Duration::from_millis(50));
        t.type_prompt(&Shared(Arc::clone(&fake)), "t1", "go", "term", &|| ()).unwrap();
        assert_eq!(fake.sent().len(), 2);
    }

    #[test]
    fn a_question_on_screen_stops_the_clock() {
        // A harness that asks something at startup waits for a person, however long that takes.
        let fake = Fake::idle(Duration::from_millis(50));
        *fake.state.lock().unwrap() = "blocked".into();
        let t = typist(60);
        let f2 = Arc::clone(&fake);
        let t2 = Arc::clone(&t);
        let job = thread::spawn(move || t2.type_prompt(&Shared(f2), "t1", "go", "term", &|| ()));
        thread::sleep(Duration::from_millis(250));
        assert!(!job.is_finished(), "gave up while a person was being asked");
        *fake.state.lock().unwrap() = "idle".into();
        job.join().unwrap().unwrap();
        assert!(fake.sent()[0].contains("go"));
    }

    #[test]
    fn it_waits_for_a_steady_idle_after_a_question() {
        // The screen goes idle the moment a dialog closes, while the harness is still loading.
        let fake = Fake::idle(Duration::from_millis(50));
        *fake.state.lock().unwrap() = "blocked".into();
        let t = typist(2000);
        let (t2, f2) = (Arc::clone(&t), Arc::clone(&fake));
        let job = thread::spawn(move || t2.type_prompt(&Shared(f2), "t1", "go", "term", &|| ()));
        thread::sleep(Duration::from_millis(30));
        *fake.state.lock().unwrap() = "idle".into();
        let idle_at = Instant::now();
        job.join().unwrap().unwrap();
        let waited = fake.first_send.lock().unwrap().unwrap() - idle_at;
        assert!(waited >= Duration::from_millis(40), "typed after only {waited:?}");
    }

    #[test]
    fn a_flicker_of_idle_starts_the_wait_over() {
        let fake = Fake::idle(Duration::from_millis(50));
        *fake.state.lock().unwrap() = "working".into();
        // A steady wait far longer than a 15ms flicker, so a slow machine oversleeping one doesn't count as steady.
        let t = Arc::new(Typist::new(Duration::from_millis(2000), Duration::from_millis(500), Duration::from_millis(5), Duration::from_millis(300)));
        let (t2, f2) = (Arc::clone(&t), Arc::clone(&fake));
        let job = thread::spawn(move || t2.type_prompt(&Shared(f2), "t1", "go", "term", &|| ()));
        for _ in 0..3 {
            *fake.state.lock().unwrap() = "idle".into();
            thread::sleep(Duration::from_millis(15));
            *fake.state.lock().unwrap() = "working".into();
            thread::sleep(Duration::from_millis(15));
        }
        assert!(fake.sent().is_empty(), "typed during a flicker");
        *fake.state.lock().unwrap() = "idle".into();
        job.join().unwrap().unwrap();
    }

    #[test]
    fn it_says_so_when_the_prompt_started_nothing() {
        let fake = Arc::new(Fake {
            state: Mutex::new("idle".into()),
            deaf: true,
            ..Fake::default()
        });
        let t = Typist::new(
            Duration::from_millis(500),
            Duration::from_millis(60),
            Duration::from_millis(5),
            Duration::from_millis(10),
        );
        let err = t.type_prompt(&Shared(fake), "t1", "go", "term", &|| ()).unwrap_err();
        assert!(err.contains("didn't start"), "{err}");
    }

    #[test]
    fn a_turn_is_only_marked_when_the_prompt_is_really_going_in() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let fake = Fake::idle(Duration::from_millis(50));
        *fake.state.lock().unwrap() = "working".into();
        let marked = Arc::new(AtomicBool::new(false));
        let t = typist(2000);
        let (t2, f2, m2) = (Arc::clone(&t), Arc::clone(&fake), Arc::clone(&marked));
        let job = thread::spawn(move || {
            t2.type_prompt(&Shared(f2), "t1", "go", "term", &|| m2.store(true, Ordering::SeqCst))
        });
        thread::sleep(Duration::from_millis(60));
        assert!(!marked.load(Ordering::SeqCst), "marked while the agent's own turn was running");
        *fake.state.lock().unwrap() = "idle".into();
        job.join().unwrap().unwrap();
        assert!(marked.load(Ordering::SeqCst));

        // A prompt that never gets typed never marks anything.
        let quiet = Arc::new(AtomicBool::new(false));
        let t = typist(30);
        t.person_typed("t1", b"writing");
        let q2 = Arc::clone(&quiet);
        let fake = Fake::idle(Duration::from_millis(50));
        assert!(t.type_prompt(&Shared(fake), "t1", "go", "term", &|| q2.store(true, Ordering::SeqCst)).is_err());
        assert!(!quiet.load(Ordering::SeqCst));
    }

    #[test]
    fn two_prompts_at_once_go_in_one_after_the_other() {
        let fake = Fake::idle(Duration::from_millis(60));
        let t = typist(5000);
        let jobs: Vec<_> = ["first", "second"]
            .into_iter()
            .map(|p| {
                let (t, f) = (Arc::clone(&t), Arc::clone(&fake));
                thread::spawn(move || t.type_prompt(&Shared(f), "t1", p, "term", &|| ()))
            })
            .collect();
        for j in jobs {
            j.join().unwrap().unwrap();
        }
        let sent = fake.sent();
        assert_eq!(sent.len(), 4, "{sent:?}");
        assert_eq!(sent[1], "\r", "a second paste landed inside the first: {sent:?}");
        assert_eq!(sent[3], "\r");
    }
}
