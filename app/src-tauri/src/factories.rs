//! Factories: a project folder and the parts that run it.
//!
//! Each factory has its own folder, `~/.sawtooth/factories/<id>/`, holding
//! `factory.json`, the knowledge base (`knowledge.md`) and the run log
//! (`runs.jsonl`). Nothing is written into the project itself except the links
//! that let harnesses read the knowledge base, and the git hooks that run checks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// The one file every harness reads at start.
    #[default]
    Knowledge,
    /// A folder of Markdown in the project that agents read when they need it.
    Folder,
    /// A command that has to pass: on every commit, before a push, or when a terminal goes quiet.
    Check,
    /// Types a prompt into a terminal when something happens.
    Trigger,
    /// A command that lists tickets. A factory has at most one.
    Tickets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunsOn {
    #[default]
    Commit,
    /// Before a push, the last step before a merge.
    Merge,
    /// When a terminal finishes a turn.
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum When {
    #[default]
    Quiet,
    Schedule,
    CheckFails,
    TicketFed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunsIn {
    /// The terminal that went quiet, or whose check failed.
    #[default]
    Same,
    /// A fresh terminal every time.
    New,
    /// One terminal kept for this trigger, started the first time.
    Own,
}

/// One part of a factory. Which fields matter depends on `kind`; the rest stay empty.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Part {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    /// Knowledge base: the file names harnesses read it as, such as AGENTS.md.
    pub read_as: Vec<String>,
    /// Folder: a folder in the project, relative to it, such as `specs/`.
    pub folder: String,
    /// Check and ticket source: the shell command to run.
    pub command: String,
    pub runs_on: RunsOn,
    pub when: When,
    /// Trigger on a schedule: "every day 09:00", "every monday 09:00", "every 30 minutes".
    pub schedule: String,
    pub runs_in: RunsIn,
    /// Trigger: what it types into the terminal.
    pub prompt: String,
    /// The command that starts an agent, for parts that start terminals:
    /// ticket sources, and triggers that run in a new terminal or their own.
    pub harness: String,
    /// Ticket source: how many ticket terminals run at once.
    pub at_once: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pos {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Ticket {
    /// The ticket's id in its tracker: "LIN-92", "#41".
    pub key: String,
    pub title: String,
    pub body: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Factory {
    pub id: String,
    pub name: String,
    /// The project folder, absolute and without symlinks.
    pub path: String,
    pub parts: Vec<Part>,
    /// Where parts were dragged to on the canvas, by node id. Others sit where the layout puts them.
    pub layout: BTreeMap<String, Pos>,
    /// Tickets fed and waiting for a terminal. The engine owns this.
    pub queue: Vec<Ticket>,
    /// Tickets with a terminal, by ticket key: the terminal id. The engine owns this.
    pub running: BTreeMap<String, String>,
}

impl Factory {
    pub fn knowledge(&self) -> Option<&Part> {
        self.parts.iter().find(|p| p.kind == Kind::Knowledge)
    }

    pub fn tickets(&self) -> Option<&Part> {
        self.parts.iter().find(|p| p.kind == Kind::Tickets)
    }
}

/// Lowercase letters, digits and dashes, for ids, folder names and branches.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "factory".into()
    } else {
        out
    }
}

pub struct Store {
    home: PathBuf,
    /// Held across every read-change-write, so the window and the engine never lose each other's changes.
    lock: Mutex<()>,
}

impl Store {
    /// `home` is Sawtooth's own folder, `~/.sawtooth`.
    pub fn new(home: PathBuf) -> Store {
        Store {
            home,
            lock: Mutex::new(()),
        }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn dir(&self, id: &str) -> PathBuf {
        self.home.join("factories").join(id)
    }

    pub fn knowledge_file(&self, id: &str) -> PathBuf {
        self.dir(id).join("knowledge.md")
    }

    /// The knowledge base as last approved. It differs from the file when an agent changed it.
    pub fn approved_knowledge_file(&self, id: &str) -> PathBuf {
        self.dir(id).join("knowledge.approved.md")
    }

    pub fn runs_file(&self, id: &str) -> PathBuf {
        self.dir(id).join("runs.jsonl")
    }

    /// Where the factory's terminals get their worktrees.
    pub fn worktrees_dir(&self, id: &str) -> PathBuf {
        self.home.join("worktrees").join(id)
    }

    /// Every factory, by name.
    pub fn list(&self) -> Result<Vec<Factory>> {
        let root = self.home.join("factories");
        let entries = match std::fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("couldn't read {}", root.display())),
        };
        let mut all = Vec::new();
        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().into_owned();
            if entry.path().join("factory.json").is_file() {
                all.push(self.get(&id)?);
            }
        }
        all.sort_by_key(|f| f.name.to_lowercase());
        Ok(all)
    }

    pub fn get(&self, id: &str) -> Result<Factory> {
        let file = self.dir(id).join("factory.json");
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("there is no factory {id:?}"))?;
        let mut factory: Factory = serde_json::from_str(&text)
            .with_context(|| format!("{} isn't valid", file.display()))?;
        factory.id = id.to_string();
        Ok(factory)
    }

    /// Makes a new factory for a project folder. A folder can be one factory only.
    pub fn create(&self, folder: &Path, name: &str, parts: Vec<Part>) -> Result<Factory> {
        let _guard = self.lock.lock().unwrap();
        let path = folder
            .canonicalize()
            .with_context(|| format!("{} doesn't exist", folder.display()))?;
        if !path.is_dir() {
            bail!("{} is not a folder", path.display());
        }
        let path = path.display().to_string();
        let existing = self.list()?;
        if let Some(f) = existing.iter().find(|f| f.path == path) {
            bail!("{} is already the factory {}", path, f.name);
        }
        let name = if name.trim().is_empty() {
            Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "factory".into())
        } else {
            name.trim().to_string()
        };
        let base = slug(&name);
        let mut id = base.clone();
        let mut n = 2;
        while self.dir(&id).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        let factory = Factory {
            id,
            name,
            path,
            parts,
            ..Factory::default()
        };
        std::fs::create_dir_all(self.dir(&factory.id))?;
        self.write(&factory)?;
        Ok(factory)
    }

    /// Replaces a factory's settings, keeping what the engine owns: the ticket queue and running tickets.
    pub fn save(&self, factory: &Factory) -> Result<Factory> {
        let _guard = self.lock.lock().unwrap();
        let current = self.get(&factory.id)?;
        let mut next = factory.clone();
        next.path = current.path;
        next.queue = current.queue;
        next.running = current.running;
        self.write(&next)?;
        Ok(next)
    }

    /// Changes a factory in place. For the engine.
    pub fn update<T>(&self, id: &str, change: impl FnOnce(&mut Factory) -> T) -> Result<T> {
        let _guard = self.lock.lock().unwrap();
        let mut factory = self.get(id)?;
        let out = change(&mut factory);
        self.write(&factory)?;
        Ok(out)
    }

    /// Forgets a factory. Its folder moves to `~/.sawtooth/removed/`, so the
    /// knowledge base and run log are never lost by a click.
    pub fn remove(&self, id: &str) -> Result<PathBuf> {
        let _guard = self.lock.lock().unwrap();
        let from = self.dir(id);
        if !from.exists() {
            bail!("there is no factory {id:?}");
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let to = self.home.join("removed").join(format!("{id}-{stamp}"));
        std::fs::create_dir_all(to.parent().unwrap())?;
        std::fs::rename(&from, &to)?;
        Ok(to)
    }

    fn write(&self, factory: &Factory) -> Result<()> {
        let dir = self.dir(&factory.id);
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("factory.json");
        let tmp = dir.join("factory.json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(factory)?)?;
        std::fs::rename(&tmp, &file)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(kind: Kind, name: &str) -> Part {
        Part {
            id: slug(name),
            kind,
            name: name.into(),
            ..Part::default()
        }
    }

    #[test]
    fn create_list_save_and_remove() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().to_path_buf());
        let work = tempfile::tempdir().unwrap();
        let zebra = work.path().join("zebra");
        let apple = work.path().join("Apple");
        std::fs::create_dir(&zebra).unwrap();
        std::fs::create_dir(&apple).unwrap();

        let z = store
            .create(&zebra, "", vec![part(Kind::Knowledge, "Knowledge base")])
            .unwrap();
        assert_eq!((z.id.as_str(), z.name.as_str()), ("zebra", "zebra"));
        let a = store.create(&apple, "Apple pie", vec![]).unwrap();
        assert_eq!(a.id, "apple-pie");
        let again = store.create(&zebra.join("../zebra"), "", vec![]);
        assert!(again.unwrap_err().to_string().contains("already"));

        let names: Vec<_> = store.list().unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["Apple pie", "zebra"]);

        // The engine queues a ticket; a save from the window keeps it.
        store
            .update(&z.id, |f| {
                f.queue.push(Ticket {
                    key: "LIN-1".into(),
                    ..Ticket::default()
                })
            })
            .unwrap();
        let mut edited = z.clone();
        edited.parts.push(part(Kind::Check, "Tests"));
        let saved = store.save(&edited).unwrap();
        assert_eq!(saved.queue.len(), 1);
        assert_eq!(store.get("zebra").unwrap().parts.len(), 2);

        let moved = store.remove("zebra").unwrap();
        assert!(moved.join("factory.json").is_file());
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn slugs() {
        assert_eq!(
            slug("LIN-92 · Rounding in totals"),
            "lin-92-rounding-in-totals"
        );
        assert_eq!(slug("  ***  "), "factory");
        assert_eq!(slug("invoice_app"), "invoice-app");
    }
}
