//! The daemon itself: terminals, the detection loop, and the socket.

use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use sawtooth_detect::{RuleBook, Screen};
use sawtooth_term::{Event, SpawnSpec, Terminal};
use serde_json::{Value, json};

use crate::confine::{Confine, Notify};
use crate::identify::identify;
use crate::paths;
use crate::tracker::Tracker;

/// How often terminals with new output are read again.
const TICK: Duration = Duration::from_millis(150);

pub struct Config {
    pub socket: PathBuf,
    /// Rule files here replace the bundled ones. `None` uses only the bundled rules.
    pub rules_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            socket: paths::socket(),
            rules_dir: Some(paths::rules_dir()),
        }
    }
}

pub struct Daemon {
    inner: Arc<Inner>,
}

struct Inner {
    socket: PathBuf,
    rules: RuleBook,
    rule_problems: Vec<String>,
    terminals: Mutex<BTreeMap<u64, Arc<Entry>>>,
    next_id: AtomicU64,
    subscribers: Mutex<Vec<Subscriber>>,
    stopping: AtomicBool,
    /// Sockets and filters made for terminals, by token.
    confines: Mutex<BTreeMap<String, Arc<Confine>>>,
    next_confine: AtomicU64,
}

struct Entry {
    num: u64,
    id: String,
    name: Mutex<String>,
    argv: Vec<String>,
    cwd: String,
    declared_agent: Option<String>,
    started_at: u64,
    term: Arc<Terminal>,
    /// New output since the last reading.
    dirty: Arc<AtomicBool>,
    /// Set once the program ends: its exit code, when it had one.
    exit: Arc<Mutex<Option<Option<u32>>>>,
    status: Mutex<Status>,
    confine: Option<String>,
}

#[derive(Default)]
struct Status {
    tracker: Tracker,
    title: String,
    /// A program the terminal's shell is running that isn't an agent Sawtooth knows,
    /// such as a home-made agent or `npm test`, by name.
    running: Option<String>,
}

enum OutputFilter {
    None,
    All,
    Only(HashSet<String>),
}

struct Subscriber {
    tx: Sender<String>,
    output: OutputFilter,
}

struct ApiError {
    code: &'static str,
    message: String,
}

fn err(code: &'static str, message: impl Into<String>) -> ApiError {
    ApiError {
        code,
        message: message.into(),
    }
}

type Reply = std::result::Result<Value, ApiError>;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Daemon {
    pub fn new(config: Config) -> Daemon {
        let mut rules = RuleBook::bundled();
        let rule_problems = config
            .rules_dir
            .as_deref()
            .map(|dir| rules.load_dir(dir))
            .unwrap_or_default();
        for problem in &rule_problems {
            eprintln!("sawtooth: skipped a rule file: {problem}");
        }
        Daemon {
            inner: Arc::new(Inner {
                socket: config.socket,
                rules,
                rule_problems,
                terminals: Mutex::new(BTreeMap::new()),
                next_id: AtomicU64::new(1),
                subscribers: Mutex::new(Vec::new()),
                stopping: AtomicBool::new(false),
                confines: Mutex::new(BTreeMap::new()),
                next_confine: AtomicU64::new(1),
            }),
        }
    }

    /// Listens on the socket until `daemon.shutdown`. Fails if another daemon owns it.
    pub fn serve(&self) -> Result<()> {
        let listener = bind(&self.inner.socket)?;
        let _ = std::fs::remove_dir_all(self.inner.run_dir());
        eprintln!("sawtooth: listening on {}", self.inner.socket.display());

        let detecting = Arc::clone(&self.inner);
        thread::Builder::new()
            .name("sawtooth-detect".into())
            .spawn(move || detect_loop(&detecting))?;

        for stream in listener.incoming() {
            if self.inner.stopping.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = stream else { continue };
            let inner = Arc::clone(&self.inner);
            thread::Builder::new()
                .name("sawtooth-conn".into())
                .spawn(move || {
                    let _ = serve_connection(&inner, stream);
                })?;
        }
        self.inner.shutdown_terminals();
        let _ = std::fs::remove_file(&self.inner.socket);
        Ok(())
    }
}

fn bind(socket: &std::path::Path) -> Result<UnixListener> {
    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("couldn't create {}", dir.display()))?;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    // Two daemons can start at once (the app connects from more than one thread).
    // The first to lock this file owns the socket; the lock lasts as long as the process.
    let lock_path = socket.with_extension("lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("couldn't open {}", lock_path.display()))?;
    // SAFETY: flock on a file descriptor we own.
    if unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        bail!(
            "a Sawtooth daemon is already running on {}",
            socket.display()
        );
    }
    std::mem::forget(lock);
    if socket.exists() {
        if UnixStream::connect(socket).is_ok() {
            bail!(
                "a Sawtooth daemon is already running on {}",
                socket.display()
            );
        }
        std::fs::remove_file(socket).ok();
    }
    let listener = UnixListener::bind(socket)
        .with_context(|| format!("couldn't listen on {}", socket.display()))?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

// ---- Terminals and events ---------------------------------------------------

impl Inner {
    fn entry(&self, id: &str) -> std::result::Result<Arc<Entry>, ApiError> {
        let num: Option<u64> = id.strip_prefix('t').and_then(|n| n.parse().ok());
        num.and_then(|n| self.terminals.lock().unwrap().get(&n).cloned())
            .ok_or_else(|| err("no_terminal", format!("there is no terminal {id:?}")))
    }

    fn entries(&self) -> Vec<Arc<Entry>> {
        self.terminals.lock().unwrap().values().cloned().collect()
    }

    fn broadcast(&self, event: &Value, output_for: Option<&str>) {
        let line = event.to_string();
        let mut subs = self.subscribers.lock().unwrap();
        subs.retain(|sub| {
            let wanted = match (output_for, &sub.output) {
                (None, _) => true,
                (Some(_), OutputFilter::All) => true,
                (Some(id), OutputFilter::Only(ids)) => ids.contains(id),
                (Some(_), OutputFilter::None) => false,
            };
            !wanted || sub.tx.send(line.clone()).is_ok()
        });
    }

    fn announce(&self, kind: &str, entry: &Entry) {
        self.broadcast(&json!({ "event": kind, "terminal": entry.info() }), None);
    }

    fn run_dir(&self) -> PathBuf {
        self.socket.parent().map(|p| p.join("run")).unwrap_or_else(|| PathBuf::from("run"))
    }

    fn confine(&self, token: &str) -> std::result::Result<Arc<Confine>, ApiError> {
        self.confines
            .lock()
            .unwrap()
            .get(token)
            .cloned()
            .ok_or_else(|| err("no_confine", format!("there is no confine {token:?}")))
    }

    /// The confine of a terminal, by terminal id.
    fn confine_of(&self, id: &str) -> std::result::Result<Arc<Confine>, ApiError> {
        match self.entry(id)?.confine.clone() {
            Some(token) => self.confine(&token),
            None => Err(err("no_confine", format!("{id} has no report socket"))),
        }
    }

    fn release(&self, token: &str) {
        if let Some(c) = self.confines.lock().unwrap().remove(token)
            && let Ok(c) = Arc::try_unwrap(c)
        {
            c.close();
        }
    }

    /// Makes the report socket and, with `allow`, the network filter a terminal will use.
    fn prepare(self: &Arc<Self>, params: &Value) -> Reply {
        let allow = params.get("allow").and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        });
        let token = format!("c{}", self.next_confine.fetch_add(1, Ordering::SeqCst));
        let weak: Weak<Inner> = Arc::downgrade(self);
        let notify: Notify = Arc::new(move |kind, mut event| {
            if let Some(inner) = weak.upgrade() {
                event["event"] = json!(kind);
                inner.broadcast(&event, None);
            }
        });
        let remote = params.get("vm").and_then(Value::as_bool).unwrap_or(false);
        let confine = Confine::start_with(&self.run_dir(), &token, allow, notify, remote)
            .map_err(|e| err("confine_failed", format!("{e:#}")))?;
        let info = confine.info();
        self.confines.lock().unwrap().insert(token, Arc::new(confine));
        Ok(info)
    }

    fn shutdown_terminals(&self) {
        for entry in self.entries() {
            let _ = entry.term.kill();
        }
    }

    fn start(self: &Arc<Self>, params: &Value) -> Reply {
        let argv: Vec<String> = params
            .get("argv")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if argv.is_empty() {
            return Err(err("invalid_params", "argv must list the command to run"));
        }
        let cwd = match params.get("cwd").and_then(Value::as_str) {
            Some(dir) => paths::expand(dir),
            None => std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/")),
        };
        if !cwd.is_dir() {
            return Err(err(
                "invalid_params",
                format!("{} is not a folder", cwd.display()),
            ));
        }
        let declared_agent = params
            .get("agent")
            .and_then(Value::as_str)
            .map(String::from);
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| {
                declared_agent
                    .clone()
                    .unwrap_or_else(|| argv[0].rsplit('/').next().unwrap_or(&argv[0]).to_string())
            });
        let num = self.next_id.fetch_add(1, Ordering::SeqCst);
        let id = format!("t{num}");

        let mut spec = SpawnSpec::new(argv.clone());
        spec.cwd = Some(cwd.clone());
        spec.rows = params
            .get("rows")
            .and_then(Value::as_u64)
            .map(|n| n.clamp(2, 1000) as u16)
            .unwrap_or(40);
        spec.cols = params
            .get("cols")
            .and_then(Value::as_u64)
            .map(|n| n.clamp(2, 1000) as u16)
            .unwrap_or(120);
        spec.env.push(("SAWTOOTH_TERMINAL".into(), id.clone()));
        spec.env
            .push(("SAWTOOTH_SOCKET".into(), self.socket.display().to_string()));
        if let Some(env) = params.get("env").and_then(Value::as_object) {
            for (key, value) in env {
                if let Some(value) = value.as_str() {
                    spec.env.push((key.clone(), value.to_string()));
                }
            }
        }

        let confine = params.get("confine").and_then(Value::as_str).map(String::from);
        if let Some(token) = &confine {
            let c = self.confine(token)?;
            c.bind(&id);
            spec.env
                .push(("SAWTOOTH_REPORT".into(), c.report.display().to_string()));
        }

        let dirty = Arc::new(AtomicBool::new(true));
        let exit = Arc::new(Mutex::new(None));
        let weak: Weak<Inner> = Arc::downgrade(self);
        let (dirty_cb, exit_cb, id_cb) = (Arc::clone(&dirty), Arc::clone(&exit), id.clone());
        let term = Terminal::spawn(spec, move |event| {
            let Some(inner) = weak.upgrade() else { return };
            match event {
                Event::Output { data, seq } => {
                    dirty_cb.store(true, Ordering::SeqCst);
                    let event = json!({ "event": "terminal.output", "id": id_cb, "seq": seq, "data": B64.encode(&data) });
                    inner.broadcast(&event, Some(&id_cb));
                }
                Event::Exited(code) => {
                    *exit_cb.lock().unwrap() = Some(code);
                    if let Ok(entry) = inner.entry(&id_cb) {
                        inner.announce("terminal.exited", &entry);
                    }
                }
            }
        })
        .map_err(|e| err("start_failed", format!("{e:#}")))?;

        let entry = Arc::new(Entry {
            num,
            id,
            name: Mutex::new(name),
            argv,
            cwd: cwd.display().to_string(),
            declared_agent,
            started_at: now_ms(),
            term,
            dirty,
            exit,
            status: Mutex::new(Status::default()),
            confine,
        });
        self.terminals
            .lock()
            .unwrap()
            .insert(num, Arc::clone(&entry));
        self.announce("terminal.started", &entry);
        if entry.exit.lock().unwrap().is_some() {
            self.announce("terminal.exited", &entry);
        }
        Ok(entry.info())
    }
}

impl Entry {
    fn info(&self) -> Value {
        let status = self.status.lock().unwrap();
        let exit = *self.exit.lock().unwrap();
        let t = &status.tracker;
        json!({
            "id": self.id,
            "name": *self.name.lock().unwrap(),
            "argv": self.argv,
            "cwd": self.cwd,
            "pid": self.term.pid(),
            "agent": t.agent,
            "state": if exit.is_some() { "exited" } else { t.state.as_str() },
            "needs_you": exit.is_none() && t.needs_you(),
            "unseen": t.unseen,
            "rule": t.rule,
            "title": status.title,
            "running": status.running,
            "exit_code": exit.flatten(),
            "started_at": self.started_at,
        })
    }

    /// Reads the screen and updates the state. Returns true when it changed.
    fn read(&self, rules: &RuleBook) -> bool {
        let snapshot = self.term.snapshot();
        let foreground = self.term.foreground();
        let agent = identify(
            rules,
            foreground.as_ref(),
            self.term.pid(),
            self.declared_agent.as_deref(),
        );
        let detection = agent.as_deref().and_then(|a| rules.get(a)).map(|set| {
            set.detect(&Screen {
                text: &snapshot.text,
                title: &snapshot.title,
                progress: &snapshot.progress,
            })
        });
        let running = match (&agent, &foreground, self.term.pid()) {
            (None, Some(p), Some(own)) if p.pid != own => Some(p.name.clone()),
            _ => None,
        };
        let mut status = self.status.lock().unwrap();
        let mut changed = status.tracker.observe(agent, detection, Instant::now());
        if status.running != running {
            status.running = running;
            changed = true;
        }
        if status.title != snapshot.title {
            status.title = snapshot.title;
            changed = true;
        }
        changed
    }
}

fn detect_loop(inner: &Arc<Inner>) {
    while !inner.stopping.load(Ordering::SeqCst) {
        for entry in inner.entries() {
            if entry.exit.lock().unwrap().is_some() {
                continue;
            }
            let fresh = entry.dirty.swap(false, Ordering::SeqCst);
            let settling = entry.status.lock().unwrap().tracker.settling();
            if (fresh || settling) && entry.read(&inner.rules) {
                inner.announce("terminal.changed", &entry);
            }
        }
        thread::sleep(TICK);
    }
}

// ---- The socket protocol ----------------------------------------------------

fn serve_connection(inner: &Arc<Inner>, stream: UnixStream) -> Result<()> {
    let mut writer = stream.try_clone()?;
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_line(
                    &mut writer,
                    &json!({ "id": "", "error": { "code": "bad_json", "message": e.to_string() } }),
                )?;
                continue;
            }
        };
        let id = request.get("id").cloned().unwrap_or(json!(""));
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(json!({}));

        if method == "events.subscribe" {
            return stream_events(inner, &mut writer, id, &params);
        }
        let reply = handle(inner, method, &params);
        let response = match reply {
            Ok(result) => json!({ "id": id, "result": result }),
            Err(e) => json!({ "id": id, "error": { "code": e.code, "message": e.message } }),
        };
        write_line(&mut writer, &response)?;
        if method == "daemon.shutdown" {
            inner.stopping.store(true, Ordering::SeqCst);
            // Wake the accept loop so it sees the flag.
            let _ = UnixStream::connect(&inner.socket);
            return Ok(());
        }
    }
    Ok(())
}

fn write_line(writer: &mut UnixStream, value: &Value) -> Result<()> {
    let mut line = value.to_string();
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    Ok(())
}

fn stream_events(
    inner: &Arc<Inner>,
    writer: &mut UnixStream,
    id: Value,
    params: &Value,
) -> Result<()> {
    let output = match params.get("output") {
        Some(Value::String(s)) if s == "all" => OutputFilter::All,
        Some(Value::Array(ids)) => OutputFilter::Only(
            ids.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
        ),
        _ => OutputFilter::None,
    };
    let (tx, rx): (Sender<String>, Receiver<String>) = mpsc::channel();
    // Register before taking the list, so nothing falls between the two.
    inner
        .subscribers
        .lock()
        .unwrap()
        .push(Subscriber { tx, output });
    let terminals: Vec<Value> = inner.entries().iter().map(|e| e.info()).collect();
    write_line(
        writer,
        &json!({ "id": id, "result": { "subscribed": true, "terminals": terminals } }),
    )?;
    for line in rx {
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
    }
    Ok(())
}

fn str_param<'a>(params: &'a Value, key: &str) -> std::result::Result<&'a str, ApiError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| err("invalid_params", format!("{key} is required")))
}

fn handle(inner: &Arc<Inner>, method: &str, params: &Value) -> Reply {
    match method {
        "ping" => Ok(json!({ "pong": true, "version": env!("CARGO_PKG_VERSION") })),

        "daemon.info" => Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "socket": inner.socket.display().to_string(),
            "agents": inner.rules.ids().collect::<Vec<_>>(),
            "rule_problems": inner.rule_problems,
            "terminals": inner.entries().len(),
        })),

        "daemon.shutdown" => Ok(json!({ "stopping": true })),

        "terminal.start" => inner.start(params),

        "confine.prepare" => inner.prepare(params),

        "confine.release" => {
            inner.release(str_param(params, "token")?);
            Ok(json!({ "released": true }))
        }

        "confine.allow" => {
            let c = inner.confine_of(str_param(params, "id")?)?;
            let always = params.get("always").and_then(Value::as_bool).unwrap_or(false);
            let done = c.allow(str_param(params, "host")?, always);
            Ok(json!({ "allowed": done, "list": c.allowed() }))
        }

        "terminal.reports" => {
            let c = inner.confine_of(str_param(params, "id")?)?;
            Ok(json!({ "reports": c.take_reports() }))
        }

        "terminal.list" => {
            Ok(json!({ "terminals": inner.entries().iter().map(|e| e.info()).collect::<Vec<_>>() }))
        }

        "terminal.get" => Ok(inner.entry(str_param(params, "id")?)?.info()),

        "terminal.read" => {
            let entry = inner.entry(str_param(params, "id")?)?;
            let snap = entry.term.snapshot();
            let text = match params.get("lines").and_then(Value::as_u64) {
                Some(n) => {
                    let lines: Vec<&str> = snap.text.lines().collect();
                    lines[lines.len().saturating_sub(n as usize)..].join("\n")
                }
                None => snap.text.clone(),
            };
            Ok(
                json!({ "text": text, "title": snap.title, "rows": snap.rows, "cols": snap.cols, "alternate": snap.alternate }),
            )
        }

        "terminal.redraw" => {
            let entry = inner.entry(str_param(params, "id")?)?;
            let snap = entry.term.snapshot();
            let redraw = entry.term.redraw();
            Ok(
                json!({ "data": B64.encode(&redraw.data), "seq": redraw.seq, "rows": snap.rows, "cols": snap.cols }),
            )
        }

        "terminal.send" => {
            let entry = inner.entry(str_param(params, "id")?)?;
            let mut bytes = Vec::new();
            if let Some(text) = params.get("text").and_then(Value::as_str) {
                bytes.extend_from_slice(text.as_bytes());
            }
            if let Some(data) = params.get("data").and_then(Value::as_str) {
                bytes.extend(
                    B64.decode(data)
                        .map_err(|_| err("invalid_params", "data must be base64"))?,
                );
            }
            if params
                .get("enter")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                bytes.push(b'\r');
            }
            if entry.exit.lock().unwrap().is_some() {
                return Err(err("exited", format!("{} has ended", entry.id)));
            }
            entry
                .term
                .write(&bytes)
                .map_err(|e| err("write_failed", e.to_string()))?;
            if entry.status.lock().unwrap().tracker.input() {
                inner.announce("terminal.changed", &entry);
            }
            Ok(json!({ "sent": bytes.len() }))
        }

        "terminal.rename" => {
            let entry = inner.entry(str_param(params, "id")?)?;
            let name = str_param(params, "name")?.trim();
            if name.is_empty() {
                return Err(err("invalid_params", "name can't be empty"));
            }
            *entry.name.lock().unwrap() = name.to_string();
            inner.announce("terminal.changed", &entry);
            Ok(entry.info())
        }

        "terminal.seen" => {
            let entry = inner.entry(str_param(params, "id")?)?;
            if entry.status.lock().unwrap().tracker.seen() {
                inner.announce("terminal.changed", &entry);
            }
            Ok(entry.info())
        }

        "terminal.resize" => {
            let entry = inner.entry(str_param(params, "id")?)?;
            let size = |key| {
                params
                    .get(key)
                    .and_then(Value::as_u64)
                    .map(|n| n.clamp(2, 1000) as u16)
            };
            let (Some(rows), Some(cols)) = (size("rows"), size("cols")) else {
                return Err(err("invalid_params", "rows and cols are required"));
            };
            entry
                .term
                .resize(rows, cols)
                .map_err(|e| err("resize_failed", e.to_string()))?;
            entry.dirty.store(true, Ordering::SeqCst);
            Ok(json!({ "rows": rows, "cols": cols }))
        }

        "terminal.explain" => {
            let entry = inner.entry(str_param(params, "id")?)?;
            let snap = entry.term.snapshot();
            let foreground = entry.term.foreground();
            let agent = identify(
                &inner.rules,
                foreground.as_ref(),
                entry.term.pid(),
                entry.declared_agent.as_deref(),
            );
            let screen = Screen {
                text: &snap.text,
                title: &snap.title,
                progress: &snap.progress,
            };
            let rules: Vec<Value> = agent
                .as_deref()
                .and_then(|a| inner.rules.get(a))
                .map(|set| {
                    set.explain(&screen)
                        .into_iter()
                        .map(|(id, priority, matched)| json!({ "id": id, "priority": priority, "matched": matched }))
                        .collect()
                })
                .unwrap_or_default();
            let decided = agent
                .as_deref()
                .and_then(|a| inner.rules.get(a))
                .map(|set| set.detect(&screen));
            Ok(json!({
                "agent": agent,
                "process": foreground.map(|p| json!({ "pid": p.pid, "name": p.name, "argv": p.argv })),
                "title": snap.title,
                "progress": snap.progress,
                "state": decided.as_ref().and_then(|d| d.state).map(|s| s.as_str()),
                "rule": decided.and_then(|d| d.rule),
                "rules": rules,
            }))
        }

        "terminal.close" => {
            let entry = inner.entry(str_param(params, "id")?)?;
            let _ = entry.term.kill();
            inner.terminals.lock().unwrap().remove(&entry.num);
            if let Some(token) = &entry.confine {
                inner.release(token);
            }
            inner.broadcast(&json!({ "event": "terminal.closed", "id": entry.id }), None);
            Ok(json!({ "closed": entry.id }))
        }

        "" => Err(err("invalid_request", "method is required")),
        other => Err(err(
            "unknown_method",
            format!("there is no method {other:?}"),
        )),
    }
}
