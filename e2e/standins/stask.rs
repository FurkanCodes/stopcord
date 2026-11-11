// Takes its brief, asks through st ask, then waits for a person to type the answer into its terminal
// and reports done with it. Compiled as `claude`, which is how the daemon recognizes it.
use std::io::{BufRead, Write};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
fn say(s: &str) { print!("{s}"); std::io::stdout().flush().ok(); }
fn main() {
    let lines = Arc::new(AtomicU64::new(0));
    let last = Arc::new(Mutex::new(String::new()));
    let (l, t) = (lines.clone(), last.clone());
    std::thread::spawn(move || for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        *t.lock().unwrap() = line;
        l.fetch_add(1, Ordering::SeqCst);
    });
    say("> ");
    while lines.load(Ordering::SeqCst) == 0 { std::thread::sleep(Duration::from_millis(50)); }
    say("\r\n\u{2733} Thinking\u{2026} (1s \u{b7} esc to interrupt)\r\n");
    std::thread::sleep(Duration::from_secs(2));
    Command::new("ST_PATH").args(["ask", "Which database should I use?"]).status().ok();
    let asked = lines.load(Ordering::SeqCst);
    say("\x1b[2J\x1b[H> ");
    while lines.load(Ordering::SeqCst) == asked { std::thread::sleep(Duration::from_millis(100)); }
    let answer = last.lock().unwrap().clone();
    for n in 1..5 {
        say(&format!("\u{2733} Thinking\u{2026} ({n}s \u{b7} esc to interrupt)\r\n"));
        std::thread::sleep(Duration::from_millis(700));
    }
    Command::new("ST_PATH").args(["done", &format!("used {answer}")]).status().ok();
    say("\x1b[2J\x1b[H> ");
    loop { std::thread::sleep(Duration::from_secs(1)); }
}
