// Draws a question form the way Claude Code does, then waits: a terminal that needs you, with no model.
use std::io::{Read, Write};
fn main() {
    let mut out = std::io::stdout();
    let rule = "─".repeat(60);
    write!(out, "{rule}\r\n Which database should I use?\r\n\r\n ❯ 1. sqlite\r\n   2. postgres\r\n\r\n Enter to confirm · Esc to cancel\r\n").ok();
    out.flush().ok();
    let mut buf = [0u8; 64];
    loop {
        if std::io::stdin().read(&mut buf).unwrap_or(0) == 0 {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
}
