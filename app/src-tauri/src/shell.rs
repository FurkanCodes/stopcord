//! Running things the way the person's own terminal would: in their login shell,
//! so the PATH, version managers and aliases from their shell setup all apply.
//! An app opened from the Finder gets almost none of that on its own.

use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// The person's login shell.
pub fn user_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/zsh".into())
}

/// What to start for a new terminal. With a command, it runs in an interactive
/// login shell, and the shell stays when the command ends, the way a terminal tab
/// does. Without one, it's just the shell.
pub fn argv(shell: &str, command: Option<&str>) -> Vec<String> {
    match command.map(str::trim).filter(|c| !c.is_empty()) {
        None => vec![shell.into(), "-l".into()],
        Some(command) => vec![
            shell.into(),
            "-l".into(),
            "-i".into(),
            "-c".into(),
            format!("{command}; exec {} -l", quote(shell)),
        ],
    }
}

pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Which of `names` the login shell can run. Names that aren't plain command
/// names are left out. Gives up after `timeout`, finding nothing.
pub fn installed(shell: &str, names: &[String], timeout: Duration) -> Vec<String> {
    let names: Vec<&String> = names
        .iter()
        .filter(|n| {
            !n.is_empty()
                && n.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        })
        .collect();
    if names.is_empty() {
        return Vec::new();
    }
    let list = names
        .iter()
        .map(|n| n.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    // Shell start-up files may print things, so each answer is marked.
    let script = format!(
        "for c in {list}; do command -v \"$c\" >/dev/null 2>&1 && printf '@@stopcord:%s\\n' \"$c\"; done"
    );
    let Ok(mut child) = Command::new(shell)
        .args(["-l", "-i", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let reading = thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Vec::new();
            }
        }
    }
    let text = reading.join().unwrap_or_default();
    text.lines()
        .filter_map(|line| line.strip_prefix("@@stopcord:"))
        .map(String::from)
        .collect()
}

/// What `command` prints, run in the login shell the way a terminal would, or nothing if it fails
/// or runs past `timeout`. Shell start-up files may print too, so callers pick out what they need.
pub fn output(shell: &str, command: &str, timeout: Duration) -> Option<String> {
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", command])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let reading = thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return None,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    reading.join().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_keeps_its_shell_after_it_ends() {
        assert_eq!(argv("/bin/zsh", None), ["/bin/zsh", "-l"]);
        assert_eq!(argv("/bin/zsh", Some("  ")), ["/bin/zsh", "-l"]);
        assert_eq!(
            argv("/bin/zsh", Some("claude")),
            ["/bin/zsh", "-l", "-i", "-c", "claude; exec '/bin/zsh' -l"]
        );
    }

    #[test]
    fn finds_what_the_shell_can_run() {
        let names = vec![
            "sh".to_string(),
            "no-such-command-stopcord".to_string(),
            "rm -rf".to_string(),
        ];
        assert_eq!(
            installed("/bin/sh", &names, Duration::from_secs(10)),
            ["sh"]
        );
    }
}
