// Takes its brief, asks a question, then stays busy past the typing timeout, goes idle, and takes a late answer.
// Compiled as `claude`, which is how the daemon recognizes it.
use std::io::{BufRead, Write};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
fn say(s: &str) { print!("{s}"); std::io::stdout().flush().ok(); }
fn main() {
    let briefed = Arc::new(AtomicBool::new(false));
    let b = briefed.clone();
    std::thread::spawn(move || while !b.load(Ordering::SeqCst) {
        say("\r\n> ");
        std::thread::sleep(Duration::from_millis(900));
    });
    let lines = Arc::new(AtomicU64::new(0));
    let l = lines.clone();
    std::thread::spawn(move || { for _ in std::io::stdin().lock().lines() { l.fetch_add(1, Ordering::SeqCst); } });
    while lines.load(Ordering::SeqCst) == 0 { std::thread::sleep(Duration::from_millis(50)); }
    briefed.store(true, Ordering::SeqCst);
    say("\r\n\u{2733} Thinking\u{2026} (1s \u{b7} esc to interrupt)\r\n");
    std::thread::sleep(Duration::from_secs(3));
    Command::new("ST_PATH").args(["ask", "Which database should I use?"]).status().ok();
    let spin = |secs: u64| {
        let end = std::time::Instant::now() + Duration::from_secs(secs);
        let mut n = 2u64;
        while std::time::Instant::now() < end {
            say(&format!("\u{2733} Thinking\u{2026} ({n}s \u{b7} esc to interrupt)\r\n"));
            std::thread::sleep(Duration::from_millis(700));
            n += 1;
        }
    };
    spin(220);
    say("\x1b[2J\x1b[H> ");
    let answered = Arc::new(AtomicBool::new(false));
    let a = answered.clone();
    std::thread::spawn(move || while !a.load(Ordering::SeqCst) {
        say("\r\n> ");
        std::thread::sleep(Duration::from_millis(900));
    });
    let seen = lines.load(Ordering::SeqCst);
    while lines.load(Ordering::SeqCst) == seen { std::thread::sleep(Duration::from_millis(50)); }
    answered.store(true, Ordering::SeqCst);
    spin(100000);
}
