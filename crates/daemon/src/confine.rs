//! What a terminal gets besides its screen: a socket to report on and, when it's
//! confined, a network filter.
//!
//! Both are made before the terminal starts, because the sandbox around it has
//! to name them. They live in the daemon since terminals outlive the app. Reports
//! wait here until the engine collects them, so an app restart loses nothing.
//!
//! The socket proves who is speaking: one socket, one terminal. Nothing an agent
//! says in a report can claim to be another terminal.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use sawtooth_sandbox::{Denied, Filter};
use serde_json::{Value, json};

/// Reports kept for one terminal. Past this an agent is misbehaving, so the oldest go.
const KEEP_REPORTS: usize = 200;
const MAX_REPORT_BYTES: u64 = 64 * 1024;
const MAX_TEXT_CHARS: usize = 8000;
/// A site blocked again this soon isn't reported again.
const DENIED_QUIET: Duration = Duration::from_secs(30);

/// What the daemon should do when a terminal has something to say.
pub type Notify = Arc<dyn Fn(&str, Value) + Send + Sync>;

pub struct Confine {
    pub token: String,
    pub report: PathBuf,
    filter: Option<Filter>,
    filter_socket: PathBuf,
    reports: Arc<Mutex<Vec<Value>>>,
    /// The terminal this belongs to, once it has started.
    terminal: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
}

/// Accepts what `st` sends: one JSON object per connection.
pub fn clean_report(raw: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(raw).map_err(|_| "that isn't valid JSON".to_string())?;
    let text = |key: &str| -> String {
        v.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .take(MAX_TEXT_CHARS)
            .collect()
    };
    let verb = text("verb");
    match verb.as_str() {
        "done" | "ask" | "note" => Ok(json!({ "verb": verb, "text": text("text") })),
        "score" => match v["score"].as_u64().filter(|n| *n <= 10) {
            Some(score) => Ok(json!({ "verb": verb, "score": score, "text": text("text") })),
            None => Err("a score is a whole number from 0 to 10".to_string()),
        },
        "finding" => {
            let severity = match text("severity").as_str() {
                s @ ("high" | "medium" | "low") => s.to_string(),
                _ => "medium".to_string(),
            };
            Ok(json!({
                "verb": verb,
                "text": text("text"),
                "file": text("file"),
                "line": v.get("line").and_then(Value::as_u64).unwrap_or(0),
                "severity": severity,
                "kind": if text("kind") == "clear" { "clear" } else { "judgment" },
            }))
        }
        other => Err(format!("st doesn't know {other:?}")),
    }
}

impl Confine {
    /// `run_dir` must be private to this user. `allow` of `None` means no filter.
    pub fn start(run_dir: &Path, token: &str, allow: Option<Vec<String>>, notify: Notify) -> Result<Confine> {
        std::fs::create_dir_all(run_dir).with_context(|| format!("couldn't create {}", run_dir.display()))?;
        let _ = std::fs::set_permissions(run_dir, std::fs::Permissions::from_mode(0o700));
        let report = run_dir.join(format!("{token}.sock"));
        let filter_socket = run_dir.join(format!("{token}.net"));
        let _ = std::fs::remove_file(&report);
        let listener = UnixListener::bind(&report).with_context(|| format!("couldn't listen on {}", report.display()))?;
        std::fs::set_permissions(&report, std::fs::Permissions::from_mode(0o600))?;

        let reports: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let terminal: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));

        let filter = match allow {
            None => None,
            Some(allow) => {
                let seen: Mutex<HashMap<String, Instant>> = Mutex::new(HashMap::new());
                let (notify, terminal, token) = (Arc::clone(&notify), Arc::clone(&terminal), token.to_string());
                let on_denied = Arc::new(move |d: Denied| {
                    {
                        let mut seen = seen.lock().unwrap();
                        let now = Instant::now();
                        if seen.get(&d.host).is_some_and(|t| now.duration_since(*t) < DENIED_QUIET) {
                            return;
                        }
                        seen.insert(d.host.clone(), now);
                    }
                    let id = terminal.lock().unwrap().clone();
                    notify(
                        "terminal.denied",
                        json!({ "id": id, "token": token, "host": d.host, "port": d.port }),
                    );
                });
                Some(Filter::start(allow, filter_socket.clone(), on_denied)?)
            }
        };

        let (accept_reports, accept_terminal, accept_stop, accept_notify) =
            (Arc::clone(&reports), Arc::clone(&terminal), Arc::clone(&stop), notify);
        thread::Builder::new().name("sawtooth-report".into()).spawn(move || {
            for stream in listener.incoming() {
                if accept_stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let (reports, terminal, notify) =
                    (Arc::clone(&accept_reports), Arc::clone(&accept_terminal), Arc::clone(&accept_notify));
                thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    let Ok(mut writer) = stream.try_clone() else { return };
                    let mut line = String::new();
                    if BufReader::new(stream.take(MAX_REPORT_BYTES)).read_line(&mut line).is_err() {
                        return;
                    }
                    let reply = match clean_report(line.trim()) {
                        Ok(report) => {
                            {
                                let mut all = reports.lock().unwrap();
                                all.push(report.clone());
                                let extra = all.len().saturating_sub(KEEP_REPORTS);
                                all.drain(..extra);
                            }
                            let id = terminal.lock().unwrap().clone();
                            notify("terminal.report", json!({ "id": id, "report": report }));
                            json!({ "ok": true })
                        }
                        Err(message) => json!({ "ok": false, "error": message }),
                    };
                    let _ = writeln!(writer, "{reply}");
                });
            }
        })?;

        Ok(Confine {
            token: token.to_string(),
            report,
            filter,
            filter_socket,
            reports,
            terminal,
            stop,
        })
    }

    pub fn info(&self) -> Value {
        json!({
            "token": self.token,
            "report": self.report.display().to_string(),
            "port": self.filter.as_ref().map(Filter::port),
            "filter": self.filter.as_ref().map(|_| self.filter_socket.display().to_string()),
        })
    }

    pub fn bind(&self, terminal: &str) {
        *self.terminal.lock().unwrap() = Some(terminal.to_string());
    }

    /// What the agent has said since the last time anyone asked.
    pub fn take_reports(&self) -> Vec<Value> {
        std::mem::take(&mut *self.reports.lock().unwrap())
    }

    pub fn allow(&self, host: &str, always: bool) -> bool {
        match &self.filter {
            Some(f) if always => {
                f.allow_always(host);
                true
            }
            Some(f) => {
                f.allow_once(host);
                true
            }
            None => false,
        }
    }

    pub fn allowed(&self) -> Vec<String> {
        self.filter.as_ref().map(Filter::allowed).unwrap_or_default()
    }

    pub fn close(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop so it sees the flag.
        let _ = std::os::unix::net::UnixStream::connect(&self.report);
        let _ = std::fs::remove_file(&self.report);
        if let Some(f) = self.filter.take() {
            f.stop();
        }
        let _ = std::fs::remove_file(&self.filter_socket);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    fn quiet() -> Notify {
        Arc::new(|_, _| ())
    }

    fn say(socket: &Path, line: &str) -> Value {
        let mut s = UnixStream::connect(socket).unwrap();
        writeln!(s, "{line}").unwrap();
        let mut out = String::new();
        BufReader::new(s).read_line(&mut out).unwrap();
        serde_json::from_str(&out).unwrap()
    }

    #[test]
    fn a_report_waits_until_it_is_collected_and_only_once() {
        let dir = tempfile::tempdir().unwrap();
        let c = Confine::start(dir.path(), "c1", None, quiet()).unwrap();
        assert_eq!(say(&c.report, r#"{"verb":"done","text":"all set"}"#)["ok"], true);
        let got = c.take_reports();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["text"], "all set");
        assert!(c.take_reports().is_empty());
    }

    #[test]
    fn a_report_names_the_terminal_the_socket_belongs_to() {
        let dir = tempfile::tempdir().unwrap();
        let events: Arc<Mutex<Vec<Value>>> = Arc::default();
        let log = Arc::clone(&events);
        let c = Confine::start(dir.path(), "c1", None, Arc::new(move |_, v| log.lock().unwrap().push(v))).unwrap();
        c.bind("t9");
        // Whatever the report claims, the socket decides.
        say(&c.report, r#"{"verb":"note","text":"hi","id":"t1"}"#);
        assert_eq!(events.lock().unwrap()[0]["id"], "t9");
    }

    #[test]
    fn nonsense_is_turned_away() {
        let dir = tempfile::tempdir().unwrap();
        let c = Confine::start(dir.path(), "c1", None, quiet()).unwrap();
        assert_eq!(say(&c.report, "not json")["ok"], false);
        assert_eq!(say(&c.report, r#"{"verb":"merge"}"#)["ok"], false);
        assert!(c.take_reports().is_empty());
    }

    #[test]
    fn findings_keep_their_place_and_get_a_known_severity() {
        let r = clean_report(r#"{"verb":"finding","text":"off by one","file":"a.rs","line":9,"severity":"urgent"}"#).unwrap();
        assert_eq!(r["file"], "a.rs");
        assert_eq!(r["line"], 9);
        assert_eq!(r["severity"], "medium");
    }

    #[test]
    fn a_flood_of_reports_cant_grow_without_end() {
        let dir = tempfile::tempdir().unwrap();
        let c = Confine::start(dir.path(), "c1", None, quiet()).unwrap();
        for i in 0..(KEEP_REPORTS + 5) {
            say(&c.report, &format!(r#"{{"verb":"note","text":"{i}"}}"#));
        }
        let got = c.take_reports();
        assert_eq!(got.len(), KEEP_REPORTS);
        assert_eq!(got.last().unwrap()["text"], format!("{}", KEEP_REPORTS + 4));
    }

    #[test]
    fn the_socket_is_private_and_goes_when_closed() {
        let dir = tempfile::tempdir().unwrap();
        let c = Confine::start(dir.path(), "c1", None, quiet()).unwrap();
        let path = c.report.clone();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        c.close();
        assert!(!path.exists());
    }
}
