//! The run log: what a factory's parts did, one JSON object per line in `state/events.jsonl`.

use std::io::Write;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::factories::Store;

/// The most output kept with a run.
const DETAIL_MAX: usize = 4096;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    /// Milliseconds since the Unix epoch.
    pub at: u64,
    /// "check", "trigger", "ticket", "terminal", "knowledge" or "factory".
    pub kind: String,
    /// One plain sentence.
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    /// The part that ran, by id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
    /// The daemon terminal it ran for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    /// The work item it was for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<String>,
    /// Who caused it: "engine", "person" or "agent".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// The end of the output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Run {
    pub fn new(kind: &str, text: impl Into<String>) -> Run {
        Run {
            at: now_ms(),
            kind: kind.into(),
            text: text.into(),
            ..Run::default()
        }
    }

    pub fn ok(mut self, ok: bool) -> Run {
        self.ok = Some(ok);
        self
    }

    pub fn work(mut self, id: &str) -> Run {
        self.work = Some(id.into());
        self
    }

    pub fn by(mut self, who: &str) -> Run {
        self.by = Some(who.into());
        self
    }

    pub fn part(mut self, id: &str) -> Run {
        self.part = Some(id.into());
        self
    }

    pub fn terminal(mut self, id: Option<&str>) -> Run {
        self.terminal = id.filter(|s| !s.is_empty()).map(String::from);
        self
    }

    pub fn detail(mut self, text: &str) -> Run {
        let text = tail(text, DETAIL_MAX);
        if !text.trim().is_empty() {
            self.detail = Some(text.to_string());
        }
        self
    }
}

/// The last `max` bytes of `text`, cut at a character boundary.
pub fn tail(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// The last `n` lines of `text`.
pub fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

pub fn append(store: &Store, id: &str, run: &Run) -> Result<()> {
    let file = store.runs_file(id);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut line = serde_json::to_string(run)?;
    line.push('\n');
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

/// The newest `limit` runs, newest first. Lines that don't parse are skipped.
pub fn read(store: &Store, id: &str, limit: usize) -> Result<Vec<Run>> {
    let text = match std::fs::read_to_string(store.runs_file(id)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    Ok(text
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str(l).ok())
        .take(limit)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_read_newest_first() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        assert!(read(&store, "f", 10).unwrap().is_empty());
        append(
            &store,
            "f",
            &Run::new("check", "Tests passed").ok(true).part("t"),
        )
        .unwrap();
        append(
            &store,
            "f",
            &Run::new("trigger", "Tidy ran").terminal(Some("t3")),
        )
        .unwrap();
        let runs = read(&store, "f", 10).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].kind, "trigger");
        assert_eq!(runs[1].ok, Some(true));
        assert_eq!(read(&store, "f", 1).unwrap().len(), 1);
        let line = std::fs::read_to_string(store.runs_file("f")).unwrap();
        assert!(!line.contains("detail"), "None fields are left out: {line}");
    }

    #[test]
    fn tails_cut_at_characters() {
        assert_eq!(tail("héllo", 3), "llo");
        assert_eq!(tail("ab€", 2), "");
        assert_eq!(last_lines("a\nb\nc\n", 2), "b\nc");
    }
}
