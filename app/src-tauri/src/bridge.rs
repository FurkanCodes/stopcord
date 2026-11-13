//! The window's line to the daemon: one connection for calls, one for events.
//!
//! Events go to the window as they come. Output goes only for the terminals
//! the window is showing, with terminal queries taken out: the daemon already
//! answers those, and xterm.js would answer them a second time as typing.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use stopcord_daemon::Client;
use stopcord_daemon::client::DaemonError;
use stopcord_daemon::viewer::QueryFilter;
use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};

/// What the window gets back when a call fails.
#[derive(Debug, Serialize)]
pub struct Failure {
    /// The daemon's error code, or `no_daemon` when it couldn't be reached.
    pub code: String,
    pub message: String,
}

impl Failure {
    pub fn new(code: &str, message: impl Into<String>) -> Failure {
        Failure {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for Failure {
    fn from(e: anyhow::Error) -> Failure {
        match e.downcast_ref::<DaemonError>() {
            Some(d) => Failure::new(&d.code, d.message.clone()),
            None => Failure::new("no_daemon", format!("{e:#}")),
        }
    }
}

#[derive(Default)]
struct Viewer {
    filter: QueryFilter,
    /// How many views of this terminal are open. Opens and closes can arrive out
    /// of order, so the viewer goes only when this falls to zero.
    count: i32,
}

pub struct Bridge {
    /// This program. Run with `daemon`, it is the daemon.
    program: PathBuf,
    control: Mutex<Option<Client>>,
    viewers: Arc<Mutex<HashMap<String, Viewer>>>,
}

impl Bridge {
    pub fn new(program: PathBuf) -> Bridge {
        Bridge {
            program,
            control: Mutex::new(None),
            viewers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Calls the daemon, starting it if it isn't running. A dropped connection is
    /// made again once. An error the daemon itself gives is never retried.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, Failure> {
        let mut control = self.control.lock().unwrap();
        let mut last = None;
        for _ in 0..2 {
            let client = match control.as_mut() {
                Some(client) => client,
                None => control.insert(Client::connect_or_start(&self.program)?),
            };
            match client.call(method, params.clone()) {
                Ok(result) => return Ok(result),
                Err(e) if e.downcast_ref::<DaemonError>().is_some() => return Err(e.into()),
                Err(e) => {
                    *control = None;
                    last = Some(e);
                }
            }
        }
        Err(last.expect("the loop ran").into())
    }

    /// Starts sending a terminal's output to the window, and returns the screen
    /// as it is now. The window draws that, then only output with a higher `seq`.
    pub fn view(&self, id: &str) -> Result<Value, Failure> {
        self.viewers
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .count += 1;
        let redraw = self.call("terminal.redraw", json!({ "id": id }));
        if redraw.is_err() {
            self.unview(id);
        }
        redraw
    }

    pub fn unview(&self, id: &str) {
        let mut viewers = self.viewers.lock().unwrap();
        if let Some(viewer) = viewers.get_mut(id) {
            viewer.count -= 1;
            if viewer.count <= 0 {
                viewers.remove(id);
            }
        }
    }

    /// Listens to the daemon for as long as the app runs, and passes events to
    /// the window. If the daemon goes away it is started again.
    pub fn watch(&self, app: AppHandle) {
        let program = self.program.clone();
        let viewers = Arc::clone(&self.viewers);
        thread::Builder::new()
            .name("stopcord-events".into())
            .spawn(move || {
                loop {
                    let subscribed = Client::connect_or_start(&program)
                        .and_then(|c| c.subscribe(json!({ "output": "all" })));
                    match subscribed {
                        Ok((ack, events)) => {
                            let _ = app.emit("daemon:ready", &ack["terminals"]);
                            for event in events {
                                let Ok(event) = event else { break };
                                if event["event"] == "terminal.output" {
                                    forward_output(&app, &viewers, &event);
                                } else {
                                    let _ = app.emit("daemon:event", &event);
                                }
                            }
                            let _ = app.emit("daemon:lost", "The daemon stopped.");
                        }
                        Err(e) => {
                            let _ = app.emit("daemon:lost", format!("{e:#}"));
                        }
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            })
            .expect("couldn't start the event thread");
    }
}

fn forward_output(app: &AppHandle, viewers: &Mutex<HashMap<String, Viewer>>, event: &Value) {
    let Some(id) = event["id"].as_str() else {
        return;
    };
    let data = {
        let mut viewers = viewers.lock().unwrap();
        let Some(viewer) = viewers.get_mut(id) else {
            return;
        };
        let bytes = B64
            .decode(event["data"].as_str().unwrap_or(""))
            .unwrap_or_default();
        viewer.filter.filter(&bytes)
    };
    if data.is_empty() {
        return;
    }
    let _ = app.emit(
        "terminal:output",
        json!({ "id": id, "seq": event["seq"], "data": B64.encode(&data) }),
    );
}
