//! `st`: how an agent tells the factory what it's doing.
//!
//! It's this same program, reached through a small script in `~/.sawtooth/bin`.
//! It talks over the socket its terminal was given, and that socket is what proves
//! who is speaking.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

const USAGE: &str = "usage: st done \"what you did\"\n       st ask \"your question\"\n       st note \"something worth keeping\"\n       st finding --file a.rs --line 9 --severity high \"what's wrong\"";

/// Writes the script that puts `st` on a terminal's PATH.
pub fn install(home: &Path, program: &Path) -> std::io::Result<()> {
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin)?;
    let file = bin.join("st");
    let quoted = program.display().to_string().replace('\'', r"'\''");
    let script = format!("#!/bin/sh\nexec '{quoted}' st \"$@\"\n");
    if std::fs::read_to_string(&file).ok().as_deref() != Some(script.as_str()) {
        std::fs::write(&file, script)?;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// Turns the command line into the report it sends.
pub fn parse(args: &[String]) -> Result<Value, String> {
    let (verb, rest) = args.split_first().ok_or(USAGE)?;
    match verb.as_str() {
        "done" | "ask" | "note" => Ok(json!({ "verb": verb, "text": rest.join(" ") })),
        "finding" => {
            let (mut file, mut line, mut severity, mut text) = (String::new(), 0u64, String::new(), Vec::new());
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--file" => file = it.next().cloned().ok_or(USAGE)?,
                    "--line" => line = it.next().and_then(|n| n.parse().ok()).ok_or(USAGE)?,
                    "--severity" => severity = it.next().cloned().ok_or(USAGE)?,
                    other => text.push(other.to_string()),
                }
            }
            Ok(json!({ "verb": "finding", "file": file, "line": line, "severity": severity, "text": text.join(" ") }))
        }
        _ => Err(USAGE.to_string()),
    }
}

fn send(socket: &str, report: &Value) -> Result<(), String> {
    let mut s = UnixStream::connect(socket).map_err(|e| format!("couldn't reach Sawtooth: {e}"))?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    writeln!(s, "{report}").map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(s).read_line(&mut reply).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&reply).map_err(|e| e.to_string())?;
    if v["ok"] == true {
        Ok(())
    } else {
        Err(v["error"].as_str().unwrap_or("Sawtooth refused that").to_string())
    }
}

/// What `sawtooth-app st ...` does. Returns the exit code.
pub fn main(args: &[String]) -> i32 {
    let Ok(socket) = std::env::var("SAWTOOTH_REPORT") else {
        eprintln!("st: this isn't a Sawtooth factory terminal");
        return 2;
    };
    let outcome = parse(args).and_then(|report| send(&socket, &report));
    match outcome {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("st: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_words_become_a_report() {
        assert_eq!(parse(&args(&["done", "fixed", "it"])).unwrap(), json!({"verb":"done","text":"fixed it"}));
        assert_eq!(parse(&args(&["ask", "which db?"])).unwrap()["verb"], "ask");
    }

    #[test]
    fn a_finding_keeps_its_place() {
        let r = parse(&args(&["finding", "--file", "a.rs", "--line", "9", "--severity", "high", "off", "by", "one"])).unwrap();
        assert_eq!(r["file"], "a.rs");
        assert_eq!(r["line"], 9);
        assert_eq!(r["severity"], "high");
        assert_eq!(r["text"], "off by one");
    }

    #[test]
    fn nonsense_shows_how_to_use_it() {
        assert!(parse(&args(&[])).unwrap_err().contains("usage"));
        assert!(parse(&args(&["merge"])).unwrap_err().contains("usage"));
        assert!(parse(&args(&["finding", "--line", "x"])).unwrap_err().contains("usage"));
    }

    #[test]
    fn the_script_calls_this_program_and_can_be_run() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), Path::new("/Apps/It's here/app")).unwrap();
        let file = dir.path().join("bin/st");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains(r"'/Apps/It'\''s here/app' st"), "{text}");
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o755);
    }
}
