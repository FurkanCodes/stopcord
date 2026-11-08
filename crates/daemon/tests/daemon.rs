//! End to end: a real daemon on a temporary socket, real terminals, a toy agent.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use sawtooth_daemon::client::Events;
use sawtooth_daemon::{Client, Config, Daemon};
use serde_json::{Value, json};

/// The toy agent: busy while "esc to interrupt" shows, asking while "Proceed?" shows.
const TOY_RULES: &str = r#"
id = "toy"
names = ["toy"]

[[rules]]
id = "busy"
state = "working"
priority = 10
contains = ["esc to interrupt"]

[[rules]]
id = "asks"
state = "blocked"
priority = 20
contains = ["Proceed?"]
"#;

struct Fixture {
    _dir: tempfile::TempDir,
    socket: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let rules = dir.path().join("rules");
        std::fs::create_dir(&rules).unwrap();
        std::fs::write(rules.join("toy.toml"), TOY_RULES).unwrap();
        let socket = dir.path().join("s.sock");
        let config = Config {
            socket: socket.clone(),
            rules_dir: Some(rules),
        };
        thread::spawn(move || Daemon::new(config).serve().unwrap());
        let deadline = Instant::now() + Duration::from_secs(5);
        while Client::connect(&socket).is_err() {
            assert!(Instant::now() < deadline, "the daemon didn't start");
            thread::sleep(Duration::from_millis(20));
        }
        Fixture { _dir: dir, socket }
    }

    fn client(&self) -> Client {
        Client::connect(&self.socket).unwrap()
    }

    fn events(&self) -> Events {
        self.client().subscribe(json!({})).unwrap().1
    }
}

fn sh(script: &str) -> Value {
    json!(["/bin/sh", "-c", script])
}

/// Reads events until one about terminal `id` passes `want`.
fn wait_for(events: &mut Events, id: &str, what: &str, want: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    for event in events.by_ref() {
        let event = event.unwrap();
        let terminal = &event["terminal"];
        if terminal["id"] == id && want(terminal) {
            return terminal.clone();
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
    }
    panic!("events ended before {what}");
}

fn start(fx: &Fixture, script: &str) -> String {
    let cwd = Path::new("/tmp").display().to_string();
    let info = fx
        .client()
        .call(
            "terminal.start",
            json!({ "argv": sh(script), "agent": "toy", "cwd": cwd }),
        )
        .unwrap();
    info["id"].as_str().unwrap().to_string()
}

#[test]
fn a_finished_turn_needs_you_until_seen() {
    let fx = Fixture::new();
    let mut events = fx.events();
    let id = start(
        &fx,
        "read go; printf 'thinking (esc to interrupt)\\n'; sleep 1; printf '\\033[2J\\033[H> '; sleep 30",
    );

    fx.client()
        .call(
            "terminal.send",
            json!({ "id": id, "text": "go", "enter": true }),
        )
        .unwrap();
    let busy = wait_for(&mut events, &id, "working", |t| t["state"] == "working");
    assert_eq!(busy["agent"], "toy");
    assert_eq!(busy["needs_you"], false);

    let done = wait_for(&mut events, &id, "the finished turn", |t| {
        t["state"] == "idle"
    });
    assert_eq!(done["needs_you"], true, "{done}");

    fx.client()
        .call("terminal.seen", json!({ "id": id }))
        .unwrap();
    let seen = wait_for(&mut events, &id, "seen", |t| t["needs_you"] == false);
    assert_eq!(seen["state"], "idle");
}

#[test]
fn a_question_needs_you_and_typing_reaches_the_program() {
    let fx = Fixture::new();
    let mut events = fx.events();
    let id = start(
        &fx,
        "printf 'Proceed? '; read answer; echo \"got $answer\"; sleep 30",
    );

    let asking = wait_for(&mut events, &id, "blocked", |t| t["state"] == "blocked");
    assert_eq!(asking["needs_you"], true);
    assert_eq!(asking["rule"], "asks");

    let mut client = fx.client();
    client
        .call(
            "terminal.send",
            json!({ "id": id, "text": "yes", "enter": true }),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let screen = client.call("terminal.read", json!({ "id": id })).unwrap();
        if screen["text"].as_str().unwrap().contains("got yes") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the program never got the answer: {screen}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn list_exit_and_close() {
    let fx = Fixture::new();
    let mut events = fx.events();
    let mut client = fx.client();
    let id = start(&fx, "exit 4");

    let ended = wait_for(&mut events, &id, "the exit", |t| t["state"] == "exited");
    assert_eq!(ended["exit_code"], 4);
    assert_eq!(ended["needs_you"], false);

    let list = client.call("terminal.list", json!({})).unwrap();
    assert_eq!(list["terminals"].as_array().unwrap().len(), 1);
    let send = client.call("terminal.send", json!({ "id": id, "text": "x" }));
    assert!(send.unwrap_err().to_string().contains("exited"));

    let renamed = client
        .call(
            "terminal.rename",
            json!({ "id": id, "name": "  LIN-92 · totals " }),
        )
        .unwrap();
    assert_eq!(renamed["name"], "LIN-92 · totals");
    let blank = client.call("terminal.rename", json!({ "id": id, "name": " " }));
    assert!(blank.unwrap_err().to_string().contains("invalid_params"));

    client.call("terminal.close", json!({ "id": id })).unwrap();
    let list = client.call("terminal.list", json!({})).unwrap();
    assert!(list["terminals"].as_array().unwrap().is_empty());
    let missing = client.call("terminal.get", json!({ "id": id }));
    assert!(missing.unwrap_err().to_string().contains("no_terminal"));
}

#[test]
fn output_reaches_subscribers_who_ask_for_it() {
    let fx = Fixture::new();
    let id = start(&fx, "sleep 1; echo first-line; sleep 30");
    let (_, mut events) = fx.client().subscribe(json!({ "output": [id] })).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    use base64::Engine as _;
    let mut seen = String::new();
    for event in events.by_ref() {
        let event = event.unwrap();
        if event["event"] == "terminal.output" && event["id"] == id.as_str() {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(event["data"].as_str().unwrap())
                .unwrap();
            seen.push_str(&String::from_utf8_lossy(&bytes));
            if seen.contains("first-line") {
                break;
            }
        }
        assert!(Instant::now() < deadline, "no output arrived: {seen:?}");
    }
    let redraw = fx
        .client()
        .call("terminal.redraw", json!({ "id": id }))
        .unwrap();
    assert!(!redraw["data"].as_str().unwrap().is_empty());
}

#[test]
fn a_late_viewer_draws_only_chunks_after_the_redraw() {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD;
    let fx = Fixture::new();
    let id = start(&fx, "echo before; read go; echo after; sleep 30");
    let mut client = fx.client();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !client.call("terminal.read", json!({ "id": id })).unwrap()["text"]
        .as_str()
        .unwrap()
        .contains("before")
    {
        assert!(Instant::now() < deadline, "the program never started");
        thread::sleep(Duration::from_millis(20));
    }

    let (_, mut events) = fx.client().subscribe(json!({ "output": [id] })).unwrap();
    let redraw = client.call("terminal.redraw", json!({ "id": id })).unwrap();
    let drawn = redraw["seq"].as_u64().unwrap();
    assert!(drawn >= 1, "{redraw}");
    let screen = String::from_utf8_lossy(&b64.decode(redraw["data"].as_str().unwrap()).unwrap())
        .into_owned();
    assert!(
        screen.contains("before") && !screen.contains("after"),
        "{screen:?}"
    );

    client
        .call(
            "terminal.send",
            json!({ "id": id, "text": "go", "enter": true }),
        )
        .unwrap();
    let mut later = String::new();
    for event in events.by_ref() {
        let event = event.unwrap();
        if event["event"] != "terminal.output" {
            continue;
        }
        let seq = event["seq"].as_u64().unwrap();
        let text = String::from_utf8_lossy(&b64.decode(event["data"].as_str().unwrap()).unwrap())
            .into_owned();
        if seq <= drawn {
            assert!(
                !text.contains("after"),
                "a chunk the redraw holds had new output"
            );
            continue;
        }
        later.push_str(&text);
        if later.contains("after") {
            break;
        }
        assert!(
            Instant::now() < deadline + Duration::from_secs(5),
            "no new output: {later:?}"
        );
    }
}

#[test]
fn bad_requests_get_clear_errors() {
    let fx = Fixture::new();
    let mut client = fx.client();
    let unknown = client
        .call("terminal.dance", json!({}))
        .unwrap_err()
        .to_string();
    assert!(unknown.contains("unknown_method"), "{unknown}");
    let no_argv = client
        .call("terminal.start", json!({ "argv": [] }))
        .unwrap_err()
        .to_string();
    assert!(no_argv.contains("invalid_params"), "{no_argv}");
    let bad_cwd = client
        .call(
            "terminal.start",
            json!({ "argv": ["sh"], "cwd": "/no/such/folder" }),
        )
        .unwrap_err()
        .to_string();
    assert!(bad_cwd.contains("is not a folder"), "{bad_cwd}");
    let info = client.call("daemon.info", json!({})).unwrap();
    assert!(
        info["agents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a == "toy")
    );
    assert!(info["rule_problems"].as_array().unwrap().is_empty());
}
