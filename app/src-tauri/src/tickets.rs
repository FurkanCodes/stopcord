//! The ticket source: a command the person writes that prints tickets as JSON,
//! such as `gh issue list --json number,title,body,url`.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use serde_json::Value;

use crate::factories::{Factory, Kind, Part, Ticket};
use crate::shell;

const TIMEOUT: Duration = Duration::from_secs(60);

/// Runs a ticket source's command, the first one if none is named, and reads the tickets it prints.
pub fn run(home: &Path, factory: &Factory, source: Option<&str>) -> Result<Vec<Ticket>> {
    let part = source
        .and_then(|id| factory.parts.iter().find(|p| p.kind == Kind::Tickets && p.id == id))
        .or_else(|| factory.tickets())
        .ok_or_else(|| anyhow!("This factory has no ticket source. Add one from Add parts."))?;
    run_part(home, factory, part)
}

/// Why a source can't pull yet, said so a person knows what to fill in, or nothing when it's ready.
/// Checked before every pull, so a half-set-up source waits quietly instead of failing every few minutes.
pub fn not_ready(home: &Path, factory: &Factory, part: &Part) -> Option<String> {
    if part.command.trim().is_empty() {
        return Some("it has no pull command yet".into());
    }
    if !part.connection.is_empty() {
        if let Some(why) = crate::connections::not_ready(home, &part.connection) {
            return Some(why);
        }
    }
    let unset: Vec<&str> = part.vars.iter().filter(|(_, v)| v.trim().is_empty()).map(|(k, _)| k.as_str()).collect();
    if !unset.is_empty() {
        return Some(format!("set {} on this source", unset.join(" and ")));
    }
    let set: Vec<String> = crate::settings::source_env(home, &factory.id, &part.id, &part.keys).into_iter().map(|(k, _)| k).collect();
    let empty: Vec<&str> = part.keys.iter().filter(|k| !set.contains(k)).map(String::as_str).collect();
    if !empty.is_empty() {
        return Some(format!("{} {} no value", empty.join(" and "), if empty.len() == 1 { "has" } else { "have" }));
    }
    None
}

/// What a source's command gets in its environment: its connection's credentials, its own plain
/// settings, then its own keys, each later one winning.
pub fn env_for(home: &Path, factory: &Factory, part: &Part) -> Vec<(String, String)> {
    let mut env = crate::connections::env(home, &part.connection);
    env.extend(part.vars.iter().filter(|(k, v)| !v.trim().is_empty() && crate::vm::secret_name(k).is_ok()).map(|(k, v)| (k.clone(), v.trim().to_string())));
    env.extend(crate::settings::source_env(home, &factory.id, &part.id, &part.keys));
    env
}

/// Runs one source's command, with its connection, settings and keys, and reads the tickets it prints.
pub fn run_part(home: &Path, factory: &Factory, part: &Part) -> Result<Vec<Ticket>> {
    if let Some(why) = not_ready(home, factory, part) {
        bail!("{} can't pull yet: {why}", part.name);
    }
    let output = run_in_login_shell(&part.command, Path::new(&factory.path), &env_for(home, factory, part), TIMEOUT)?;
    parse(&output)
}

/// Runs `command` in the person's login shell, in `cwd`. Returns stdout, or stderr as the error.
/// `env` goes in the environment, never the command line, where `ps` would show a key to anyone.
pub fn run_in_login_shell(command: &str, cwd: &Path, env: &[(String, String)], timeout: Duration) -> Result<String> {
    let mut child = Command::new(shell::user_shell())
        .args(["-l", "-c", command])
        .envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let out_t = thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let err_t = thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "The ticket command took longer than {} seconds, so it was stopped.",
                timeout.as_secs()
            );
        }
        thread::sleep(Duration::from_millis(50));
    };
    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();
    if !status.success() {
        let why = stderr.trim();
        bail!(
            "The ticket command failed{}",
            if why.is_empty() {
                ".".to_string()
            } else {
                format!(": {}", crate::runs::tail(why, 600))
            }
        );
    }
    Ok(stdout)
}

fn text(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    for key in keys {
        match obj.get(*key) {
            Some(Value::String(s)) if !s.trim().is_empty() => return s.trim().to_string(),
            Some(Value::Number(n)) => return n.to_string(),
            _ => {}
        }
    }
    String::new()
}

/// Reads a JSON array of ticket objects.
pub fn parse(output: &str) -> Result<Vec<Ticket>> {
    let value: Value = serde_json::from_str(output.trim()).map_err(|_| {
        anyhow!(
            "The ticket command has to print a JSON list of tickets, and it printed: {}",
            crate::runs::tail(output.trim(), 200)
        )
    })?;
    let Value::Array(items) = value else {
        bail!(
            "The ticket command has to print a JSON list, like [{{\"number\": 41, \"title\": \"…\"}}]."
        );
    };
    let mut tickets = Vec::new();
    for item in items {
        let Value::Object(obj) = item else { continue };
        let key = match ["number", "key", "identifier", "id"]
            .iter()
            .find_map(|k| obj.get(*k))
        {
            Some(Value::Number(n)) => format!("#{n}"),
            Some(Value::String(s)) => s.trim().to_string(),
            _ => String::new(),
        };
        let title = text(&obj, &["title", "name", "summary"]);
        if key.is_empty() && title.is_empty() {
            continue;
        }
        tickets.push(Ticket {
            key: if key.is_empty() {
                format!("#{}", tickets.len() + 1)
            } else {
                key
            },
            title,
            body: text(&obj, &["body", "description"]),
            url: text(&obj, &["url", "html_url", "link"]),
        });
    }
    Ok(tickets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::{Kind, Part};

    #[test]
    fn reads_common_shapes() {
        let t = parse(
            r#"[{"number": 41, "title": "Fix totals", "body": "b", "html_url": "u"},
                {"identifier": "LIN-92", "name": "Rounding", "description": "d"},
                {"id": "x"}, 3]"#,
        )
        .unwrap();
        assert_eq!(t.len(), 3);
        assert_eq!((t[0].key.as_str(), t[0].url.as_str()), ("#41", "u"));
        assert_eq!((t[1].key.as_str(), t[1].body.as_str()), ("LIN-92", "d"));
        assert_eq!(t[2].title, "");
    }

    #[test]
    fn bad_output_says_what_it_needs() {
        assert!(
            parse("hello")
                .unwrap_err()
                .to_string()
                .contains("JSON list")
        );
        assert!(parse("{}").unwrap_err().to_string().contains("JSON list"));
    }

    #[test]
    fn runs_the_command_in_the_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("t.json"),
            r#"[{"key":"A-1","title":"One"}]"#,
        )
        .unwrap();
        let mut f = Factory {
            path: dir.path().display().to_string(),
            ..Factory::default()
        };
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        assert!(
            run(home, &f, None)
                .unwrap_err()
                .to_string()
                .contains("no ticket source")
        );
        f.parts.push(Part {
            kind: Kind::Tickets,
            command: "cat t.json".into(),
            ..Part::default()
        });
        assert_eq!(run(home, &f, None).unwrap()[0].key, "A-1");
        f.parts[0].command = "echo nope >&2; exit 3".into();
        assert!(run(home, &f, None).unwrap_err().to_string().contains("nope"));
    }

    #[test]
    fn a_source_says_what_it_still_needs_and_waits_for_it() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let dir = tempfile::tempdir().unwrap();
        let f = Factory { id: "web".into(), path: dir.path().display().to_string(), ..Factory::default() };
        let mut part = Part {
            id: "sentry".into(),
            name: "Sentry".into(),
            kind: Kind::Tickets,
            connection: "sentry".into(),
            vars: [("SENTRY_ORG".to_string(), String::new()), ("SENTRY_PROJECT".to_string(), "web".to_string())].into(),
            command: r#"printf '[{"key":"%s-%s-%s","title":"t"}]' "$SENTRY_AUTH_TOKEN" "$SENTRY_ORG" "$SENTRY_PROJECT""#.into(),
            ..Part::default()
        };
        assert!(not_ready(h, &f, &part).unwrap().contains("Sentry isn't connected"), "the connection comes first");
        let why = run_part(h, &f, &part).unwrap_err().to_string();
        assert!(why.contains("can't pull yet") && why.contains("auth token"), "nothing runs: {why}");
        crate::connections::set(h, "sentry", "SENTRY_AUTH_TOKEN", "tok").unwrap();
        assert_eq!(not_ready(h, &f, &part).as_deref(), Some("set SENTRY_ORG on this source"));
        part.vars.insert("SENTRY_ORG".into(), " shop ".into());
        assert_eq!(not_ready(h, &f, &part), None);
        assert_eq!(run_part(h, &f, &part).unwrap()[0].key, "tok-shop-web", "the connection's token and the source's own settings");
        part.keys = vec!["EXTRA".into()];
        assert_eq!(not_ready(h, &f, &part).as_deref(), Some("EXTRA has no value"));
    }

    #[test]
    fn a_source_gets_its_keys_in_its_environment() {
        let home = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let f = Factory { id: "web".into(), path: dir.path().display().to_string(), ..Factory::default() };
        let part = Part {
            id: "linear".into(),
            kind: Kind::Tickets,
            keys: vec!["LINEAR_API_KEY".into()],
            command: r#"printf '[{"key":"%s","title":"t"}]' "${LINEAR_API_KEY:-none}""#.into(),
            ..Part::default()
        };
        assert!(run_part(home.path(), &f, &part).unwrap_err().to_string().contains("LINEAR_API_KEY has no value"), "nothing set yet, so nothing runs");
        crate::settings::set_secret(home.path(), &crate::settings::source_key("web", "linear", "LINEAR_API_KEY"), "lin_123").unwrap();
        assert_eq!(run_part(home.path(), &f, &part).unwrap()[0].key, "lin_123");
        let other = Factory { id: "api".into(), ..f.clone() };
        assert!(run_part(home.path(), &other, &part).is_err(), "another factory's source has its own keys");
    }
}
