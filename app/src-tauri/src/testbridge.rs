//! Debug builds only: lets an end-to-end test ask the window questions.
//!
//! Listens on `$STOPCORD_TEST_SOCKET`, or `<stopcord home>/app-test.sock` (mode 0600). Each line sent is the body of an
//! async JavaScript function; the window runs it and the JSON of what it returns comes back
//! as one line. Tests use it to find where things are, then click them with real input.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use tauri::{AppHandle, Listener, Manager};

pub fn start(app: AppHandle, home: &Path) {
    let socket = std::env::var_os("STOPCORD_TEST_SOCKET")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join("app-test.sock"));
    let _ = std::fs::remove_file(&socket);
    let Ok(listener) = UnixListener::bind(&socket) else {
        eprintln!("stopcord: couldn't open the test socket {}", socket.display());
        return;
    };
    let _ = std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600));

    let waiting: Arc<Mutex<HashMap<u64, Sender<Value>>>> = Arc::default();
    let answers = Arc::clone(&waiting);
    app.listen("test:result", move |event| {
        let Ok(result) = serde_json::from_str::<Value>(event.payload()) else { return };
        let Some(id) = result["id"].as_u64() else { return };
        if let Some(tx) = answers.lock().unwrap().remove(&id) {
            let _ = tx.send(result);
        }
    });

    thread::spawn(move || {
        let mut next = 0u64;
        for stream in listener.incoming().flatten() {
            let Ok(reader) = stream.try_clone() else { continue };
            let mut writer = stream;
            for line in BufReader::new(reader).lines() {
                let Ok(script) = line else { break };
                next += 1;
                let (tx, rx) = mpsc::channel();
                waiting.lock().unwrap().insert(next, tx);
                let wrapped = format!(
                    "(async () => {{ const emit = (v) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {{ event: 'test:result', payload: v }}); \
                     try {{ const value = await (async () => {{ {script} }})(); await emit({{ id: {next}, ok: true, value: value === undefined ? null : value }}); }} \
                     catch (e) {{ await emit({{ id: {next}, ok: false, error: String(e && e.stack || e) }}); }} }})()"
                );
                let answer = match app.get_webview_window("main") {
                    Some(window) if window.eval(&wrapped).is_ok() => rx
                        .recv_timeout(Duration::from_secs(20))
                        .unwrap_or_else(|_| json!({ "ok": false, "error": "no answer in 20s" })),
                    _ => json!({ "ok": false, "error": "no window" }),
                };
                waiting.lock().unwrap().remove(&next);
                if writeln!(writer, "{answer}").is_err() {
                    break;
                }
            }
        }
    });
}
