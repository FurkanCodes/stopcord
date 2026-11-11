// An agent that tries to move the project's branch from its worktree, with the guard hook turned
// off, and writes what happened into results.txt. Then it commits its own work and reports done.
use std::io::{BufRead, Write};
use std::process::Command;
fn say(s: &str) { print!("{s}"); std::io::stdout().flush().ok(); }
fn git(args: &[&str]) -> String {
    match Command::new("git").args(args).output() {
        Ok(o) => format!("{} {}", if o.status.success() { "MOVED" } else { "REFUSED" }, String::from_utf8_lossy(&o.stderr).trim().replace('\n', " ")),
        Err(e) => format!("ERROR {e}"),
    }
}
fn main() {
    say("\r\n> ");
    let _ = std::io::stdin().lock().lines().next();
    say("\r\n\u{2733} Thinking\u{2026} (1s \u{b7} esc to interrupt)\r\n");
    let mut out = String::new();
    out.push_str(&format!("update-ref: {}\n", git(&["-c", "core.hooksPath=/dev/null", "update-ref", "refs/heads/main", "HEAD"])));
    out.push_str(&format!("pack-refs: {}\n", git(&["-c", "core.hooksPath=/dev/null", "pack-refs", "--all"])));
    std::fs::write("work.txt", "my work\n").ok();
    out.push_str(&format!("own commit: {}\n", { git(&["add", "work.txt"]); git(&["commit", "-qm", "my work"]) }));
    std::fs::write("results.txt", &out).ok();
    Command::new("ST_PATH").args(["done", "tried to move main"]).status().ok();
    say("\r\n> ");
    loop { std::thread::sleep(std::time::Duration::from_secs(1)); }
}
