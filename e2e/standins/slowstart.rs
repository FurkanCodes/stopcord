// Like a harness still starting up: shows its prompt at once, but drops anything typed in the first
// few seconds, then answers the first line it gets. Compiled as `claude`, so it's read like one.
use std::io::{Read, Write};
use std::time::{Duration, Instant};
fn say(s: &str) { print!("{s}"); std::io::stdout().flush().ok(); }
fn main() {
    let started = Instant::now();
    say("\r\n> ");
    let mut line = String::new();
    let mut buf = [0u8; 256];
    let mut stdin = std::io::stdin();
    loop {
        let n = stdin.read(&mut buf).unwrap_or(0);
        if n == 0 { std::thread::sleep(Duration::from_millis(50)); continue; }
        if started.elapsed() < Duration::from_millis(2500) { continue; }
        for &b in &buf[..n] {
            if b == b'\r' || b == b'\n' {
                let got = line.replace("\x1b[200~", "").replace("\x1b[201~", "");
                say(&format!("\r\n\u{2733} Thinking\u{2026} (1s \u{b7} esc to interrupt)\r\n"));
                std::thread::sleep(Duration::from_millis(1500));
                say(&format!("got: {}\r\n> ", got.trim()));
                line.clear();
            } else {
                line.push(b as char);
            }
        }
    }
}
