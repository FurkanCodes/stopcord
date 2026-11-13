//! A blocking client for the daemon's socket.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::paths;

pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next: u64,
}

/// The daemon answered, and the answer was an error. Anything else that goes wrong
/// in a call is the connection's fault.
#[derive(Debug, Clone)]
pub struct DaemonError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for DaemonError {}

impl Client {
    pub fn connect(socket: &Path) -> Result<Client> {
        let stream = UnixStream::connect(socket)
            .with_context(|| format!("no Stopcord daemon on {}", socket.display()))?;
        Ok(Client {
            reader: BufReader::new(stream.try_clone()?),
            writer: stream,
            next: 1,
        })
    }

    /// Connects to the daemon on [`paths::socket`], first starting `program daemon`
    /// in its own session if nothing answers. Its output goes to [`paths::log_file`].
    pub fn connect_or_start(program: &Path) -> Result<Client> {
        let socket = paths::socket();
        if let Ok(client) = Client::connect(&socket) {
            return Ok(client);
        }
        // Unix sockets take paths of at most 103 bytes on macOS (107 on Linux).
        let len = socket.as_os_str().len();
        if len > 103 {
            bail!(
                "the daemon's socket path is {len} characters, and sockets allow 103: {}. Set STOPCORD_SOCKET to a shorter path.",
                socket.display()
            );
        }
        std::fs::create_dir_all(paths::home())?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(paths::log_file())?;
        let mut daemon = Command::new(program);
        daemon
            .arg("daemon")
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        // SAFETY: setsid is async-signal-safe; it detaches the daemon from the caller's session.
        unsafe {
            daemon.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let mut child = daemon.spawn().context("couldn't start the daemon")?;
        // Collect the child when it exits, so an early failure doesn't leave a zombie.
        thread::spawn(move || child.wait());
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(client) = Client::connect(&socket) {
                return Ok(client);
            }
            thread::sleep(Duration::from_millis(50));
        }
        bail!(
            "the daemon didn't start; see {}",
            paths::log_file().display()
        )
    }

    fn send(&mut self, method: &str, params: Value) -> Result<String> {
        let id = format!("c{}", self.next);
        self.next += 1;
        let mut line = json!({ "id": id, "method": method, "params": params }).to_string();
        line.push('\n');
        self.writer.write_all(line.as_bytes())?;
        Ok(id)
    }

    fn receive(&mut self) -> Result<Value> {
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            bail!("the daemon closed the connection");
        }
        Ok(serde_json::from_str(&line)?)
    }

    /// Calls a method and returns its result, or the daemon's error as an error.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.send(method, params)?;
        let reply = self.receive()?;
        if reply.get("id").and_then(Value::as_str) != Some(id.as_str()) {
            bail!("the daemon answered a different request");
        }
        unwrap_reply(reply)
    }

    /// Subscribes to events. Returns the list of terminals at that moment and
    /// the stream of events that follow.
    pub fn subscribe(mut self, params: Value) -> Result<(Value, Events)> {
        self.send("events.subscribe", params)?;
        let ack = unwrap_reply(self.receive()?)?;
        Ok((ack, Events { client: self }))
    }
}

fn unwrap_reply(reply: Value) -> Result<Value> {
    if let Some(error) = reply.get("error") {
        let text = |key: &str, default: &str| {
            error
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or(default)
                .to_string()
        };
        return Err(DaemonError {
            code: text("code", "error"),
            message: text("message", ""),
        }
        .into());
    }
    reply
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow!("the daemon sent an empty reply"))
}

pub struct Events {
    client: Client,
}

impl Iterator for Events {
    type Item = Result<Value>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.client.receive() {
            Err(e) if e.to_string().contains("closed the connection") => None,
            other => Some(other),
        }
    }
}
