//! What a terminal gets besides its screen: a socket to report on and, when it's
//! confined, a network filter.
//!
//! Both are made before the terminal starts, because the sandbox around it has
//! to name them. They live in the daemon since terminals outlive the app. Reports
//! wait here until the engine collects them, so an app restart loses nothing.
//!
//! The socket proves who is speaking: one socket, one terminal. Nothing an agent
//! says in a report can claim to be another terminal.
//!
//! A terminal in a VM can't reach a Unix socket on the Mac, so it may also get a port
//! on loopback. Anything on the Mac can reach loopback, so there a report has to start
//! with the terminal's own key.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::net::{SocketAddr, TcpListener, TcpStream};
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
    /// For a terminal in a VM: the loopback port it reports on, and the key that proves it's speaking.
    remote: Option<(u16, String)>,
}

/// Who a report goes to once it's read.
#[derive(Clone)]
struct Inbox {
    reports: Arc<Mutex<Vec<Value>>>,
    terminal: Arc<Mutex<Option<String>>>,
    notify: Notify,
}

impl Inbox {
    /// Reads one report from a connection and answers it. A report may start with the key, and
    /// has to when `required`.
    fn serve(&self, reader: impl Read, mut writer: impl Write, key: Option<&str>, required: bool) {
        let mut line = String::new();
        if BufReader::new(reader.take(MAX_REPORT_BYTES)).read_line(&mut line).is_err() {
            return;
        }
        let line = line.trim();
        let keyed = key.and_then(|key| line.strip_prefix(key)).and_then(|rest| rest.strip_prefix(' '));
        let body = match (keyed, required) {
            (Some(rest), _) => Some(rest),
            (None, false) => Some(line),
            (None, true) => None,
        };
        let reply = match body.map(clean_report) {
            None => json!({ "ok": false, "error": "that report didn't carry this terminal's key" }),
            Some(Ok(report)) => {
                {
                    let mut all = self.reports.lock().unwrap();
                    all.push(report.clone());
                    let extra = all.len().saturating_sub(KEEP_REPORTS);
                    all.drain(..extra);
                }
                let id = self.terminal.lock().unwrap().clone();
                (self.notify)("terminal.report", json!({ "id": id, "report": report }));
                json!({ "ok": true })
            }
            Some(Err(message)) => json!({ "ok": false, "error": message }),
        };
        let _ = writeln!(writer, "{reply}");
    }
}

/// A key nobody can guess, from the system's random source.
fn new_key() -> Result<String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .context("couldn't read /dev/urandom")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
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
        "done" | "ask" | "note" | "say" => Ok(json!({ "verb": verb, "text": text("text") })),
        // A stage name is a short slug: anything else isn't one, and the engine checks it's on the line.
        "send" => {
            let to = text("to");
            if to.is_empty() || to.len() > 64 || !to.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                return Err("send needs the name of a stage, like build".to_string());
            }
            Ok(json!({ "verb": verb, "to": to, "text": text("text") }))
        }
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
        Confine::start_with(run_dir, token, allow, notify, false)
    }

    /// `start`, and with `remote` a loopback port and key for a terminal in a VM.
    pub fn start_with(run_dir: &Path, token: &str, allow: Option<Vec<String>>, notify: Notify, remote: bool) -> Result<Confine> {
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

        let inbox = Inbox { reports: Arc::clone(&reports), terminal: Arc::clone(&terminal), notify };
        // On Linux a VM's relay reaches the socket itself, and its st sends the key there too.
        let key = if remote { Some(new_key()?) } else { None };
        let (accept_inbox, accept_stop, socket_key) = (inbox.clone(), Arc::clone(&stop), key.clone());
        thread::Builder::new().name("sawtooth-report".into()).spawn(move || {
            for stream in listener.incoming() {
                if accept_stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let (inbox, key) = (accept_inbox.clone(), socket_key.clone());
                thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    let Ok(writer) = stream.try_clone() else { return };
                    inbox.serve(stream, writer, key.as_deref(), false);
                });
            }
        })?;

        let remote = if let Some(key) = key {
            let tcp = TcpListener::bind("127.0.0.1:0").context("couldn't open a report port")?;
            let port = tcp.local_addr()?.port();
            let (accept_key, accept_stop) = (key.clone(), Arc::clone(&stop));
            thread::Builder::new().name("sawtooth-report-tcp".into()).spawn(move || {
                for stream in tcp.incoming() {
                    if accept_stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let (inbox, key) = (inbox.clone(), accept_key.clone());
                    thread::spawn(move || {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                        let Ok(writer) = stream.try_clone() else { return };
                        inbox.serve(stream, writer, Some(&key), true);
                    });
                }
            })?;
            Some((port, key))
        } else {
            None
        };

        Ok(Confine {
            token: token.to_string(),
            report,
            filter,
            filter_socket,
            reports,
            terminal,
            stop,
            remote,
        })
    }

    pub fn info(&self) -> Value {
        json!({
            "token": self.token,
            "report": self.report.display().to_string(),
            "port": self.filter.as_ref().map(Filter::port),
            "filter": self.filter.as_ref().map(|_| self.filter_socket.display().to_string()),
            "reportPort": self.remote.as_ref().map(|(port, _)| port),
            "key": self.remote.as_ref().map(|(_, key)| key),
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
        if let Some((port, _)) = &self.remote {
            let _ = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], *port)), Duration::from_secs(1));
        }
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
        assert_eq!(clean_report(r#"{"verb":"send","to":"plan","text":"needs a migration"}"#).unwrap()["to"], "plan");
        assert!(clean_report(r#"{"verb":"send","to":"../x","text":"hi"}"#).is_err());
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

    fn say_tcp(port: u16, line: &str) -> Value {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        writeln!(s, "{line}").unwrap();
        let mut out = String::new();
        BufReader::new(s).read_line(&mut out).unwrap();
        serde_json::from_str(&out).unwrap()
    }

    #[test]
    fn a_vm_reports_on_a_port_and_only_with_its_key() {
        let dir = tempfile::tempdir().unwrap();
        let c = Confine::start_with(dir.path(), "c1", None, quiet(), true).unwrap();
        let info = c.info();
        let port = info["reportPort"].as_u64().unwrap() as u16;
        let key = info["key"].as_str().unwrap().to_string();
        assert_eq!(key.len(), 32);
        assert_eq!(say_tcp(port, r#"{"verb":"note","text":"no key"}"#)["ok"], false);
        assert_eq!(say_tcp(port, &format!(r#"{}x {{"verb":"note","text":"wrong key"}}"#, &key[..31]))["ok"], false);
        assert_eq!(say_tcp(port, &format!(r#"{key} {{"verb":"note","text":"hi"}}"#))["ok"], true);
        let got = c.take_reports();
        assert_eq!(got.len(), 1, "only the keyed report counts: {got:?}");
        assert_eq!(got[0]["text"], "hi");
        assert_eq!(say(&c.report, r#"{"verb":"note","text":"the socket needs no key"}"#)["ok"], true);
        assert_eq!(say(&c.report, &format!(r#"{key} {{"verb":"note","text":"but takes one"}}"#))["ok"], true);
        let texts: Vec<Value> = c.take_reports().into_iter().map(|r| r["text"].clone()).collect();
        assert_eq!(texts, vec![json!("the socket needs no key"), json!("but takes one")]);
    }

    #[test]
    fn a_terminal_on_the_mac_gets_no_port() {
        let dir = tempfile::tempdir().unwrap();
        let c = Confine::start(dir.path(), "c1", None, quiet()).unwrap();
        assert!(c.info()["reportPort"].is_null() && c.info()["key"].is_null());
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
