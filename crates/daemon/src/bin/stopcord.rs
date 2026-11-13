//! `stopcord`: start and drive the terminal daemon from a shell.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use stopcord_daemon::viewer::QueryFilter;
use stopcord_daemon::{Client, Config, Daemon, paths};
use serde_json::{Value, json};

const HELP: &str = "\
stopcord: run coding-agent terminals in the background and see what each needs.

  stopcord start [--name N] [--cwd DIR] [--agent A] -- COMMAND [ARGS...]
  stopcord ls                    every terminal and its state
  stopcord attach ID             type into a terminal (Ctrl-] to leave)
  stopcord read ID [--lines N]   print what a terminal shows
  stopcord send ID TEXT [--enter]
  stopcord seen ID               mark a finished turn as looked at
  stopcord watch                 print state changes as they happen
  stopcord explain ID            which rule decided a terminal's state
  stopcord stop ID               end a terminal
  stopcord daemon                run the daemon in this shell
  stopcord shutdown              stop the daemon and every terminal

The daemon starts by itself when needed. It listens on $STOPCORD_SOCKET,
or ~/.stopcord/stopcord.sock. Rule files in ~/.stopcord/rules replace the
bundled ones with the same id.";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("stopcord: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<()> {
    let Some((command, rest)) = args.split_first() else {
        println!("{HELP}");
        return Ok(());
    };
    match command.as_str() {
        "help" | "-h" | "--help" => {
            println!("{HELP}");
            Ok(())
        }
        "daemon" => Daemon::new(Config::default()).serve(),
        "start" => start(rest),
        "ls" | "list" => list(),
        "attach" => attach(one_id(rest)?),
        "read" => read(rest),
        "send" => send(rest),
        "seen" => {
            connect()?.call("terminal.seen", json!({ "id": one_id(rest)? }))?;
            Ok(())
        }
        "watch" => watch(),
        "explain" => {
            let result = connect()?.call("terminal.explain", json!({ "id": one_id(rest)? }))?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        "stop" => {
            connect()?.call("terminal.close", json!({ "id": one_id(rest)? }))?;
            Ok(())
        }
        "shutdown" => match Client::connect(&paths::socket()) {
            Ok(mut client) => client.call("daemon.shutdown", json!({})).map(|_| ()),
            Err(_) => Ok(()),
        },
        other => bail!("unknown command {other:?}. Run `stopcord help`."),
    }
}

fn one_id(rest: &[String]) -> Result<&str> {
    rest.first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("which terminal? Give its id, such as t1"))
}

/// Connects to the daemon, starting it first if it isn't running.
fn connect() -> Result<Client> {
    Client::connect_or_start(&std::env::current_exe()?)
}

fn start(rest: &[String]) -> Result<()> {
    let split = rest.iter().position(|a| a == "--");
    let (flags, argv) = match split {
        Some(i) => (&rest[..i], rest[i + 1..].to_vec()),
        None => (&rest[..0], rest.to_vec()),
    };
    if argv.is_empty() {
        bail!("what should it run? `stopcord start -- claude`");
    }
    let mut params = json!({ "argv": argv });
    let cwd = std::env::current_dir()?.display().to_string();
    params["cwd"] = json!(cwd);
    if let Some((rows, cols)) = window_size() {
        params["rows"] = json!(rows);
        params["cols"] = json!(cols);
    }
    let mut it = flags.iter();
    while let Some(flag) = it.next() {
        let value = it.next().ok_or_else(|| anyhow!("{flag} needs a value"))?;
        match flag.as_str() {
            "--name" => params["name"] = json!(value),
            "--cwd" => params["cwd"] = json!(value),
            "--agent" => params["agent"] = json!(value),
            other => bail!("unknown option {other:?}"),
        }
    }
    let info = connect()?.call("terminal.start", params)?;
    println!("{}", info["id"].as_str().unwrap_or("?"));
    Ok(())
}

fn label(t: &Value) -> String {
    if t["state"] == "exited" {
        return "exited".into();
    }
    if t["needs_you"].as_bool().unwrap_or(false) {
        return if t["state"] == "blocked" {
            "needs you: asking".into()
        } else {
            "needs you: done".into()
        };
    }
    match t["state"].as_str() {
        Some("unknown") => "shell".into(),
        Some(s) => s.into(),
        None => "?".into(),
    }
}

fn list() -> Result<()> {
    let result = connect()?.call("terminal.list", json!({}))?;
    let terminals = result["terminals"].as_array().cloned().unwrap_or_default();
    if terminals.is_empty() {
        println!("No terminals. Start one with `stopcord start -- <command>`.");
        return Ok(());
    }
    println!(
        "{:<5} {:<18} {:<9} {:<17} TITLE",
        "ID", "NAME", "AGENT", "STATE"
    );
    for t in terminals {
        println!(
            "{:<5} {:<18} {:<9} {:<17} {}",
            t["id"].as_str().unwrap_or(""),
            truncate(t["name"].as_str().unwrap_or(""), 18),
            t["agent"].as_str().unwrap_or("-"),
            label(&t),
            truncate(t["title"].as_str().unwrap_or(""), 40),
        );
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max - 1).chain(['…']).collect()
    }
}

fn read(rest: &[String]) -> Result<()> {
    let id = one_id(rest)?;
    let mut params = json!({ "id": id });
    if let Some(i) = rest.iter().position(|a| a == "--lines") {
        let n: u64 = rest
            .get(i + 1)
            .ok_or_else(|| anyhow!("--lines needs a number"))?
            .parse()?;
        params["lines"] = json!(n);
    }
    let result = connect()?.call("terminal.read", params)?;
    println!("{}", result["text"].as_str().unwrap_or(""));
    Ok(())
}

fn send(rest: &[String]) -> Result<()> {
    let id = one_id(rest)?;
    let enter = rest.iter().any(|a| a == "--enter");
    let text: Vec<&str> = rest[1..]
        .iter()
        .filter(|a| *a != "--enter")
        .map(String::as_str)
        .collect();
    connect()?.call(
        "terminal.send",
        json!({ "id": id, "text": text.join(" "), "enter": enter }),
    )?;
    Ok(())
}

fn watch() -> Result<()> {
    let (ack, events) = connect()?.subscribe(json!({}))?;
    for t in ack["terminals"].as_array().into_iter().flatten() {
        print_state(t);
    }
    for event in events {
        let event = event?;
        match event["event"].as_str() {
            Some("terminal.closed") => println!("{} closed", event["id"].as_str().unwrap_or("?")),
            Some(_) => print_state(&event["terminal"]),
            None => {}
        }
    }
    Ok(())
}

fn print_state(t: &Value) {
    println!(
        "{:<5} {:<18} {:<9} {}",
        t["id"].as_str().unwrap_or(""),
        truncate(t["name"].as_str().unwrap_or(""), 18),
        t["agent"].as_str().unwrap_or("-"),
        label(t)
    );
}

// ---- attach -----------------------------------------------------------------

const DETACH: u8 = 0x1d; // Ctrl-]

fn window_size() -> Option<(u16, u16)> {
    // SAFETY: TIOCGWINSZ fills the winsize struct it is given.
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) };
    (ok == 0 && ws.ws_row > 0 && ws.ws_col > 0).then_some((ws.ws_row, ws.ws_col))
}

/// Puts the shell's terminal in raw mode, and puts it back when dropped.
struct RawMode {
    saved: Option<PathBuf>,
}

impl RawMode {
    fn enter() -> Result<RawMode> {
        let saved = Command::new("stty")
            .arg("-g")
            .stdin(Stdio::inherit())
            .output()?;
        if !saved.status.success() {
            bail!("attach needs to run in a terminal");
        }
        let settings = String::from_utf8_lossy(&saved.stdout).trim().to_string();
        Command::new("stty")
            .args(["raw", "-echo"])
            .stdin(Stdio::inherit())
            .status()?;
        let path = std::env::temp_dir().join(format!("stopcord-stty-{}", std::process::id()));
        std::fs::write(&path, settings)?;
        Ok(RawMode { saved: Some(path) })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if let Some(path) = self.saved.take() {
            if let Ok(settings) = std::fs::read_to_string(&path) {
                let _ = Command::new("stty")
                    .arg(settings.trim())
                    .stdin(Stdio::inherit())
                    .status();
            }
            let _ = std::fs::remove_file(path);
        }
        // Undo modes the program may have turned on in our terminal.
        let mut out = std::io::stdout();
        let _ = out.write_all(b"\x1b[?1049l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\x1b[<u\x1b[?25h\x1b[0m\r\n");
        let _ = out.flush();
    }
}

fn attach(id: &str) -> Result<()> {
    let mut control = connect()?;
    let info = control.call("terminal.get", json!({ "id": id }))?;
    if info["state"] == "exited" {
        bail!("{id} has ended");
    }
    let mut size = window_size();
    if let Some((rows, cols)) = size {
        control.call(
            "terminal.resize",
            json!({ "id": id, "rows": rows, "cols": cols }),
        )?;
    }
    let (_, events) = connect()?.subscribe(json!({ "output": [id] }))?;
    let redraw = control.call("terminal.redraw", json!({ "id": id }))?;
    let raw = RawMode::enter()?;

    let mut out = std::io::stdout();
    out.write_all(b"\x1b[H\x1b[2J")?;
    out.write_all(&B64.decode(redraw["data"].as_str().unwrap_or(""))?)?;
    out.flush()?;

    // Output up to this chunk is already on screen.
    let drawn = redraw["seq"].as_u64().unwrap_or(0);
    let watched = id.to_string();
    let output = thread::spawn(move || {
        let mut filter = QueryFilter::default();
        let mut out = std::io::stdout();
        for event in events.flatten() {
            match event["event"].as_str() {
                Some("terminal.output")
                    if event["id"] == watched.as_str()
                        && event["seq"].as_u64().unwrap_or(u64::MAX) > drawn =>
                {
                    let bytes = B64
                        .decode(event["data"].as_str().unwrap_or(""))
                        .unwrap_or_default();
                    let _ = out.write_all(&filter.filter(&bytes));
                    let _ = out.flush();
                }
                Some("terminal.exited") if event["terminal"]["id"] == watched.as_str() => {
                    return "it ended";
                }
                Some("terminal.closed") if event["id"] == watched.as_str() => {
                    return "it was closed";
                }
                _ => {}
            }
        }
        "the daemon stopped"
    });

    let mut stdin = std::io::stdin();
    let mut buf = [0u8; 4096];
    let mut last_size_check = Instant::now();
    let why = loop {
        if output.is_finished() {
            break output.join().unwrap_or("it ended");
        }
        let n = match stdin.read(&mut buf) {
            Ok(0) | Err(_) => break "input closed",
            Ok(n) => n,
        };
        if let Some(pos) = buf[..n].iter().position(|&b| b == DETACH) {
            if pos > 0 {
                control.call(
                    "terminal.send",
                    json!({ "id": id, "data": B64.encode(&buf[..pos]) }),
                )?;
            }
            break "detached";
        }
        control.call(
            "terminal.send",
            json!({ "id": id, "data": B64.encode(&buf[..n]) }),
        )?;
        if last_size_check.elapsed() > Duration::from_millis(300) {
            last_size_check = Instant::now();
            let now = window_size();
            if now != size {
                size = now;
                if let Some((rows, cols)) = now {
                    control.call(
                        "terminal.resize",
                        json!({ "id": id, "rows": rows, "cols": cols }),
                    )?;
                }
            }
        }
    };
    drop(raw);
    eprintln!("stopcord: left {id}: {why}. It keeps running; `stopcord attach {id}` to come back.");
    Ok(())
}
