// Plays any role on the line. Each turn runs ROOT_PATH/turn.sh with its role, so a test decides what the role does.
// Compiled as `claude`, which is how the daemon recognizes it.
use std::io::{BufRead, Write};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
fn say(s: &str) { print!("{s}"); std::io::stdout().flush().ok(); }
fn spinner(n: u64) { say(&format!("\u{2733} Thinking\u{2026} ({n}s \u{b7} esc to interrupt)\r\n")); }
fn main() {
    let count = Arc::new(AtomicU64::new(0));
    let text = Arc::new(Mutex::new(String::new()));
    let (c, t) = (count.clone(), text.clone());
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().flatten() {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("ROOT_PATH/agent.log") {
                let _ = writeln!(f, "{line}");
            }
            t.lock().unwrap().push_str(&line);
            t.lock().unwrap().push('\n');
            c.fetch_add(1, Ordering::SeqCst);
        }
    });
    let mut role = String::new();
    let mut bad = false;
    loop {
        let seen = count.load(Ordering::SeqCst);
        say("\x1b[2J\x1b[H> ");
        let mut tick = Instant::now();
        while count.load(Ordering::SeqCst) == seen {
            if tick.elapsed() > Duration::from_millis(900) { say("\r\n> "); tick = Instant::now(); }
            std::thread::sleep(Duration::from_millis(30));
        }
        let mut last = count.load(Ordering::SeqCst);
        loop {
            std::thread::sleep(Duration::from_millis(700));
            let now = count.load(Ordering::SeqCst);
            if now == last { break; }
            last = now;
        }
        let got = std::mem::take(&mut *text.lock().unwrap());
        if role.is_empty() {
            for r in ["planner", "builder", "reviewer"] {
                if got.contains(&format!("ROLE={r}")) { role = r.to_string(); }
            }
            bad = got.contains("GATEFAIL");
        }
        spinner(1);
        let mut child = Command::new("/bin/sh").arg("ROOT_PATH/turn.sh").arg(&role).arg(if bad { "bad" } else { "good" }).spawn().unwrap();
        let mut n = 2;
        while child.try_wait().unwrap().is_none() {
            spinner(n);
            n += 1;
            std::thread::sleep(Duration::from_millis(400));
        }
        for _ in 0..3 { spinner(n); n += 1; std::thread::sleep(Duration::from_millis(400)); }
    }
}
