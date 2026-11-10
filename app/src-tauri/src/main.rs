//! The Sawtooth app: a window onto the terminal daemon, and the engine that
//! makes each factory's parts act.
//!
//! The same program has two other jobs, picked by its first argument:
//! `sawtooth-app daemon` runs the terminal daemon, so the app can start one
//! without shipping a second program, and `sawtooth-app hook <factory> <hook>`
//! runs a factory's checks from a git hook. Terminals belong to the daemon and
//! keep running when the window closes.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod connections;
mod definition;
mod engine;
mod factories;
mod flow;
mod gate;
mod git;
mod harness;
mod hooks;
mod bench;
mod intake;
mod learn;
mod oneshot;
mod probe;
mod kb;
mod leave_key;
mod line;
mod project;
mod runner;
mod runs;
mod settings;
mod shell;
mod slack;
mod st;
mod templates;
#[cfg(debug_assertions)]
mod testbridge;
mod tickets;
mod typing;
mod vm;
mod work;
mod worktree;

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bridge::{Bridge, Failure};
use engine::{Engine, StartRequest};
use factories::{Factory, Kind, Level, Part, RunsOn, Store, Ticket};
use project::{FolderSuggestion, ProjectInfo};
use runs::Run;
use sawtooth_daemon::{Config, Daemon, paths};
use serde::Serialize;
use serde_json::{Value, json};
use tauri::menu::{Menu, MenuItem, Submenu};
use tauri::{Emitter, Manager, State};

/// Daemon methods the window may call directly. Terminals start through
/// `start_terminal`, which builds the command line here.
const PASSTHROUGH: &[&str] = &[
    "daemon.info",
    "terminal.list",
    "terminal.get",
    "terminal.read",
    "terminal.send",
    "terminal.seen",
    "terminal.resize",
    "terminal.explain",
    "terminal.close",
    "terminal.rename",
];

/// Likely secrets in staged changes, for the "No secrets" check. Needs no extra tools.
const NO_SECRETS: &str = "if git diff --cached -U0 | grep -qE 'AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY-----|ghp_[A-Za-z0-9]{36}|sk-[A-Za-z0-9_-]{20,}|xox[baprs]-[A-Za-z0-9-]{10,}'; then echo 'A staged change looks like it holds a key or token. Take it out of the commit, then commit again.'; exit 1; fi";

/// What the app shares between commands.
struct App {
    store: Arc<Store>,
    program: PathBuf,
    /// Problems from the last time each factory's settings were applied, by id.
    problems: Mutex<HashMap<String, Vec<String>>>,
}

/// A factory as the window sees it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FactoryInfo {
    #[serde(flatten)]
    factory: Factory,
    worktrees_dir: String,
    knowledge_path: String,
    knowledge_changed: bool,
    problems: Vec<String>,
}

fn fail(e: anyhow::Error) -> Failure {
    Failure::new("factory", format!("{e:#}"))
}

impl App {
    fn info(&self, factory: Factory) -> FactoryInfo {
        let id = factory.id.clone();
        FactoryInfo {
            worktrees_dir: self.store.worktrees_dir(&id).display().to_string(),
            knowledge_path: self.store.knowledge_file(&id).display().to_string(),
            knowledge_changed: engine::knowledge_differs(&self.store, &id),
            problems: self
                .problems
                .lock()
                .unwrap()
                .get(&id)
                .cloned()
                .unwrap_or_default(),
            factory,
        }
    }

    /// The project and every worktree the factory has made.
    fn checkouts(&self, factory: &Factory) -> Vec<PathBuf> {
        let mut dirs = vec![PathBuf::from(&factory.path)];
        if let Ok(entries) = std::fs::read_dir(self.store.worktrees_dir(&factory.id)) {
            dirs.extend(
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.join(".git").exists()),
            );
        }
        dirs
    }

    /// Makes the project match the factory's settings: knowledge base links and git hooks.
    /// `before` is the factory as it was, so links it no longer wants come out.
    fn apply(&self, factory: &Factory, before: Option<&Factory>) -> Vec<String> {
        let kb_file = self.store.knowledge_file(&factory.id);
        let now: Vec<String> = factory
            .knowledge()
            .map(|p| p.read_as.clone())
            .unwrap_or_default();
        let was: Vec<String> = before
            .and_then(|f| f.knowledge())
            .map(|p| p.read_as.clone())
            .unwrap_or_default();
        let dropped: Vec<String> = was.into_iter().filter(|n| !now.contains(n)).collect();
        let mut problems = Vec::new();
        // Keep the Library section in step with the folder parts, in the approved copy too,
        // so the app's own edit never shows up as a change to review.
        for file in [kb_file.clone(), self.store.approved_knowledge_file(&factory.id)] {
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            let next = kb::with_library(&text, factory);
            if next != text
                && let Err(e) = std::fs::write(&file, next)
            {
                problems.push(format!("couldn't update {}: {e}", file.display()));
            }
        }
        for dir in self.checkouts(factory) {
            problems.extend(kb::unlink(&kb_file, &dir, &dropped));
            problems.extend(kb::link(&kb_file, &dir, &now));
        }
        problems.extend(hooks::install(factory, &self.program, self.store.home()));
        problems.dedup();
        self.problems
            .lock()
            .unwrap()
            .insert(factory.id.clone(), problems.clone());
        problems
    }

    /// A path inside the factory's project, from one relative to it.
    fn inside(&self, id: &str, path: &str) -> Result<PathBuf, Failure> {
        let factory = self.store.get(id).map_err(fail)?;
        let rel = Path::new(path);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(Failure::new(
                "invalid_params",
                format!("{path} isn't inside the project"),
            ));
        }
        Ok(Path::new(&factory.path).join(rel))
    }
}

// ---- Daemon and terminals ---------------------------------------------------

#[tauri::command(async)]
fn daemon(
    bridge: State<'_, Bridge>,
    engine: State<'_, Arc<Engine>>,
    method: String,
    params: Option<Value>,
) -> Result<Value, Failure> {
    if !PASSTHROUGH.contains(&method.as_str()) {
        return Err(Failure::new(
            "not_allowed",
            format!("the window can't call {method}"),
        ));
    }
    let params = params.unwrap_or_else(|| json!({}));
    if method == "terminal.send" {
        engine.person_typed(&params);
    }
    bridge.call(&method, params)
}

#[tauri::command(async)]
fn view(bridge: State<'_, Bridge>, id: String) -> Result<Value, Failure> {
    bridge.view(&id)
}

#[tauri::command]
fn unview(bridge: State<'_, Bridge>, id: String) {
    bridge.unview(&id);
}

/// Starts a terminal: in a factory (on its own worktree when `branch` is given), or in `cwd`.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn start_terminal(
    app: State<'_, App>,
    bridge: State<'_, Bridge>,
    factory_id: Option<String>,
    cwd: Option<String>,
    command: Option<String>,
    name: Option<String>,
    branch: Option<String>,
    rows: Option<u16>,
    cols: Option<u16>,
    task: Option<String>,
) -> Result<Value, Failure> {
    engine::start_terminal(
        &app.store,
        &bridge,
        &app.program,
        StartRequest {
            factory_id,
            cwd,
            command,
            name,
            branch,
            rows,
            cols,
            task,
            runner: None,
            workspace: None,
        },
    )
}

/// Which of these commands the login shell can run.
#[tauri::command(async)]
fn installed(names: Vec<String>) -> Vec<String> {
    shell::installed(&shell::user_shell(), &names, Duration::from_secs(8))
}

#[tauri::command(async)]
fn branch(path: String) -> Option<String> {
    git::branch(Path::new(&path))
}

/// What a terminal's checkout changed since it left the project's checkout in `base`.
#[tauri::command(async)]
fn changes(path: String, base: String) -> Option<git::Changes> {
    git::changes(Path::new(&path), Path::new(&base))
}

/// Files under `path` changed since `since` (ms since 1970), newest first: what a terminal
/// changed in a folder that isn't a git checkout, where nothing else can tell.
#[tauri::command(async)]
fn touched(path: String, since: u64) -> Vec<git::Touched> {
    git::touched(Path::new(&path), since)
}

// ---- Factories --------------------------------------------------------------

#[tauri::command(async)]
fn factories(app: State<'_, App>) -> Result<Vec<FactoryInfo>, Failure> {
    let list = app.store.list().map_err(fail)?;
    Ok(list.into_iter().map(|f| app.info(f)).collect())
}

/// The person has looked at an alert.
#[tauri::command(async)]
fn dismiss_alert(app: State<'_, App>, id: String, key: String) -> Result<(), Failure> {
    app.store
        .update(&id, |f| work::clear(f, &key))
        .map_err(fail)
}

/// Factories whose settings can't be read, in words.
#[tauri::command(async)]
fn broken_factories(app: State<'_, App>) -> Vec<String> {
    app.store
        .broken()
        .into_iter()
        .map(|(id, problem)| format!("The factory {id} can't be read, so it's switched off: {problem}"))
        .collect()
}

#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn create_factory(
    app: State<'_, App>,
    path: String,
    name: String,
    template: String,
    read_as: Vec<String>,
    harness: Option<String>,
    reviewer: Option<String>,
    level: Option<String>,
) -> Result<FactoryInfo, Failure> {
    let folder = Path::new(&path);
    let mut parts = vec![Part {
        id: "kb".into(),
        kind: Kind::Knowledge,
        name: "Knowledge base".into(),
        read_as,
        ..Part::default()
    }];
    if template == "reviewed" || template == "line" {
        if let Some(test) = project::test_command(folder) {
            parts.push(Part {
                id: "tests".into(),
                kind: Kind::Check,
                name: "Tests".into(),
                command: test,
                runs_on: RunsOn::Commit,
                ..Part::default()
            });
        }
        parts.push(Part {
            id: "no-secrets".into(),
            kind: Kind::Check,
            name: "No secrets".into(),
            command: NO_SECRETS.into(),
            runs_on: RunsOn::Commit,
            ..Part::default()
        });
    }
    let test = parts.iter().find(|p| p.id == "tests").map(|p| p.command.clone());
    let mut factory = app.store.create(folder, &name, parts).map_err(fail)?;
    if template == "line" {
        let harness = harness.filter(|h| !h.trim().is_empty()).unwrap_or_else(|| "claude".to_string());
        let reviewer = reviewer.filter(|h| !h.trim().is_empty()).unwrap_or_else(|| harness.clone());
        let level = match level.as_deref() {
            Some("contained") => Level::Contained,
            Some("vm") => Level::Vm,
            Some("harness") => Level::Harness,
            _ => Level::Worktree,
        };
        templates::standard_line(&mut factory, &harness, &reviewer, test.as_deref(), level);
        factory = app.store.save(&factory).map_err(fail)?;
    }
    let seed = kb::seed(&factory.name);
    std::fs::write(app.store.knowledge_file(&factory.id), &seed).map_err(|e| fail(e.into()))?;
    std::fs::write(app.store.approved_knowledge_file(&factory.id), &seed)
        .map_err(|e| fail(e.into()))?;
    app.apply(&factory, None);
    let _ = runs::append(
        &app.store,
        &factory.id,
        &Run::new("factory", format!("{} became a factory", factory.name)),
    );
    Ok(app.info(factory))
}

#[tauri::command(async)]
fn save_factory(app: State<'_, App>, factory: Factory) -> Result<FactoryInfo, Failure> {
    let before = app.store.get(&factory.id).map_err(fail)?;
    let saved = app.store.save(&factory).map_err(fail)?;
    app.apply(&saved, Some(&before));
    Ok(app.info(saved))
}

/// Forgets a factory: takes its links and hooks out of the project. Terminals keep running.
#[tauri::command(async)]
fn remove_factory(app: State<'_, App>, id: String) -> Result<(), Failure> {
    let factory = app.store.get(&id).map_err(fail)?;
    if let Some(part) = factory.knowledge() {
        let kb_file = app.store.knowledge_file(&id);
        for dir in app.checkouts(&factory) {
            kb::unlink(&kb_file, &dir, &part.read_as);
        }
    }
    hooks::uninstall(&factory);
    app.store.remove(&id).map_err(fail)?;
    app.problems.lock().unwrap().remove(&id);
    Ok(())
}

#[tauri::command(async)]
fn runs(app: State<'_, App>, id: String, limit: Option<usize>) -> Result<Vec<Run>, Failure> {
    runs::read(&app.store, &id, limit.unwrap_or(200)).map_err(fail)
}

// ---- Knowledge --------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderFiles {
    part_id: String,
    name: String,
    folder: String,
    exists: bool,
    files: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Knowledge {
    path: String,
    text: String,
    changed_by_agent: bool,
    approved_text: String,
    folders: Vec<FolderFiles>,
}

/// Markdown files under `dir`, relative to it, skipping hidden folders and dependencies.
fn markdown_files(dir: &Path, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if name.to_lowercase().ends_with(".md") {
                if let Ok(rel) = path.strip_prefix(dir) {
                    out.push(rel.display().to_string());
                }
                if out.len() >= max {
                    out.sort();
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

#[tauri::command(async)]
fn knowledge(app: State<'_, App>, id: String) -> Result<Knowledge, Failure> {
    let factory = app.store.get(&id).map_err(fail)?;
    let file = app.store.knowledge_file(&id);
    let folders = factory
        .parts
        .iter()
        .filter(|p| p.kind == Kind::Folder)
        .map(|p| {
            let dir = Path::new(&factory.path).join(p.folder.trim().trim_start_matches('/'));
            FolderFiles {
                part_id: p.id.clone(),
                name: p.name.clone(),
                folder: p.folder.clone(),
                exists: dir.is_dir() && !p.folder.trim().is_empty(),
                files: if p.folder.trim().is_empty() {
                    Vec::new()
                } else {
                    markdown_files(&dir, 500)
                },
            }
        })
        .collect();
    Ok(Knowledge {
        path: file.display().to_string(),
        text: std::fs::read_to_string(&file).unwrap_or_default(),
        changed_by_agent: engine::knowledge_differs(&app.store, &id),
        approved_text: std::fs::read_to_string(app.store.approved_knowledge_file(&id))
            .unwrap_or_default(),
        folders,
    })
}

fn write(path: &Path, text: &str) -> Result<(), Failure> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| fail(e.into()))?;
    }
    std::fs::write(path, text).map_err(|e| fail(e.into()))
}

/// Saves the knowledge base as the person wrote it, which also approves it.
#[tauri::command(async)]
fn save_knowledge(app: State<'_, App>, id: String, text: String) -> Result<(), Failure> {
    app.store.get(&id).map_err(fail)?;
    write(&app.store.knowledge_file(&id), &text)?;
    write(&app.store.approved_knowledge_file(&id), &text)
}

/// Keeps what an agent changed in the knowledge base.
#[tauri::command(async)]
fn approve_knowledge(app: State<'_, App>, id: String) -> Result<(), Failure> {
    let text =
        std::fs::read_to_string(app.store.knowledge_file(&id)).map_err(|e| fail(e.into()))?;
    write(&app.store.approved_knowledge_file(&id), &text)?;
    let _ = runs::append(
        &app.store,
        &id,
        &Run::new("knowledge", "Changes to the knowledge base were approved").ok(true),
    );
    Ok(())
}

/// Puts the knowledge base back as it was last approved.
#[tauri::command(async)]
fn undo_knowledge(app: State<'_, App>, id: String) -> Result<(), Failure> {
    let text = std::fs::read_to_string(app.store.approved_knowledge_file(&id))
        .map_err(|e| fail(e.into()))?;
    write(&app.store.knowledge_file(&id), &text)?;
    let _ = runs::append(
        &app.store,
        &id,
        &Run::new("knowledge", "Changes to the knowledge base were undone"),
    );
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileEntry {
    /// Relative to the folder.
    path: String,
    title: String,
    /// The first line of text after the title, if any.
    summary: String,
    /// Last change, in ms since the epoch.
    modified: u64,
}

/// A Markdown file's title (its first heading, or its name) and its first line of text.
fn describe(text: &str, path: &str) -> (String, String) {
    let name = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().replace(['-', '_'], " "))
        .unwrap_or_default();
    let mut title = None;
    let mut summary = String::new();
    let mut in_code = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code || line.is_empty() || line.starts_with("<!--") || line.starts_with("---") {
            continue;
        }
        if let Some(h) = line.strip_prefix('#') {
            if title.is_none() {
                title = Some(h.trim_start_matches('#').trim().to_string());
            }
            continue;
        }
        let plain: String = line
            .trim_start_matches(['-', '*', '>', ' '])
            .chars()
            .filter(|c| !matches!(c, '*' | '`' | '_'))
            .collect();
        summary = plain.chars().take(160).collect();
        break;
    }
    (title.filter(|t| !t.is_empty()).unwrap_or(name), summary)
}

/// The Markdown files in a folder part, each with its title and first line.
#[tauri::command(async)]
fn folder_files(app: State<'_, App>, id: String, part_id: String) -> Result<Vec<FileEntry>, Failure> {
    let factory = app.store.get(&id).map_err(fail)?;
    let part = factory
        .parts
        .iter()
        .find(|p| p.id == part_id)
        .ok_or_else(|| Failure::new("invalid_params", "there's no such part"))?;
    let dir = app.inside(&id, part.folder.trim())?;
    let mut out = Vec::new();
    for rel in markdown_files(&dir, 500) {
        let file = dir.join(&rel);
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let (title, summary) = describe(&text, &rel);
        let modified = std::fs::metadata(&file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as u64);
        out.push(FileEntry { path: rel, title, summary, modified });
    }
    Ok(out)
}

#[tauri::command(async)]
fn read_file(app: State<'_, App>, id: String, path: String) -> Result<String, Failure> {
    let file = app.inside(&id, &path)?;
    std::fs::read_to_string(&file)
        .map_err(|e| Failure::new("factory", format!("couldn't read {path}: {e}")))
}

#[tauri::command(async)]
fn write_file(app: State<'_, App>, id: String, path: String, text: String) -> Result<(), Failure> {
    let file = app.inside(&id, &path)?;
    write(&file, &text)
}

// ---- Tickets and projects ---------------------------------------------------

#[tauri::command(async)]
fn list_tickets(app: State<'_, App>, id: String, source: Option<String>) -> Result<Vec<Ticket>, Failure> {
    let factory = app.store.get(&id).map_err(fail)?;
    tickets::run(app.store.home(), &factory, source.as_deref()).map_err(|e| Failure::new("tickets", format!("{e:#}")))
}

#[tauri::command(async)]
fn feed_tickets(
    app: State<'_, App>,
    engine: State<'_, Arc<Engine>>,
    id: String,
    tickets: Vec<Ticket>,
    source: Option<String>,
) -> Result<FactoryInfo, Failure> {
    engine.feed(&id, tickets, source.as_deref()).map_err(fail)?;
    Ok(app.info(app.store.get(&id).map_err(fail)?))
}

/// Tries a runner's sandbox from the inside and says what held.
#[tauri::command(async)]
fn test_runner(app: State<'_, App>, bridge: State<'_, Bridge>, id: String, runner: String) -> Result<runner::TestReport, Failure> {
    let f = app.store.get(&id).map_err(fail)?;
    let r = f.runners.iter().find(|r| r.id == runner).ok_or_else(|| Failure::new("runner", format!("there's no runner {runner}")))?;
    runner::test(&app.store, &bridge, &f, r, &app.program)
}

/// What the sandbox settings screen shows that isn't in the factory: built-in blocks, locked escapes, harness sites.
#[tauri::command]
fn sandbox_info(app: State<'_, App>) -> serde_json::Value {
    runner::info(&app.store)
}

#[tauri::command(async)]
fn run_benchmark(engine: State<'_, Arc<Engine>>, id: String, suite: String) -> Result<String, Failure> {
    engine.run_benchmark(&id, &suite).map_err(|e| Failure::new("benchmark", e))
}

#[tauri::command(async)]
fn benchmark_results(app: State<'_, App>, id: String) -> Vec<bench::BenchResult> {
    bench::results(&app.store, &id)
}

#[tauri::command(async)]
fn answer_proposal(app: State<'_, App>, engine: State<'_, Arc<Engine>>, id: String, proposal: String, apply: bool) -> Result<FactoryInfo, Failure> {
    engine.answer_proposal(&id, &proposal, apply).map_err(|e| Failure::new("proposal", e))?;
    Ok(app.info(app.store.get(&id).map_err(fail)?))
}

#[tauri::command(async)]
fn suggest_start(engine: State<'_, Arc<Engine>>, id: String, key: String) -> Result<String, Failure> {
    engine.suggest_start(&id, &key).map_err(|e| Failure::new("suggest", e))
}

#[tauri::command(async)]
fn work_action(
    app: State<'_, App>,
    engine: State<'_, Arc<Engine>>,
    id: String,
    key: String,
    action: String,
    note: Option<String>,
) -> Result<FactoryInfo, Failure> {
    engine
        .work_action(&id, &key, &action, note.as_deref().unwrap_or_default())
        .map_err(|e| Failure::new("work", e))?;
    Ok(app.info(app.store.get(&id).map_err(fail)?))
}

/// The plan and diff of a piece of work, for deciding about it.
#[tauri::command(async)]
fn work_evidence(engine: State<'_, Arc<Engine>>, id: String, key: String) -> Result<git::Evidence, Failure> {
    engine.evidence(&id, &key).map_err(|e| Failure::new("work", e))
}

#[tauri::command(async)]
fn accept_proposal(
    app: State<'_, App>,
    engine: State<'_, Arc<Engine>>,
    id: String,
    key: String,
    start_at: String,
) -> Result<FactoryInfo, Failure> {
    app.store.update(&id, |f| work::accept(f, &key, &start_at)).map_err(fail)?;
    engine.pump();
    Ok(app.info(app.store.get(&id).map_err(fail)?))
}

#[tauri::command(async)]
fn reject_proposal(app: State<'_, App>, id: String, key: String) -> Result<FactoryInfo, Failure> {
    app.store.update(&id, |f| work::reject(f, &key)).map_err(fail)?;
    Ok(app.info(app.store.get(&id).map_err(fail)?))
}

#[tauri::command(async)]
fn pull_source(engine: State<'_, Arc<Engine>>, id: String, part: String) -> Result<usize, Failure> {
    engine.pull(&id, &part).map_err(|e| Failure::new("source", e))
}

#[tauri::command(async)]
fn answer_denied(
    engine: State<'_, Arc<Engine>>,
    id: String,
    key: String,
    allow: bool,
    always: bool,
) -> Result<(), Failure> {
    engine.answer_denied(&id, &key, allow, always).map_err(|e| Failure::new("network", e))
}

#[tauri::command]
fn source_templates() -> Vec<templates::SourceTemplate> {
    templates::source_templates()
}

/// The machine's settings, and how the things they control are doing.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Machine {
    settings: settings::Settings,
    sandbox: Option<String>,
}

fn machine_of(app: &App) -> Result<Machine, Failure> {
    Ok(Machine {
        settings: settings::load(app.store.home()).map_err(fail)?,
        sandbox: sawtooth_sandbox::available().err(),
    })
}

#[tauri::command(async)]
fn machine(app: State<'_, App>) -> Result<Machine, Failure> {
    machine_of(&app)
}

#[tauri::command(async)]
fn save_machine(
    app: State<'_, App>,
    engine: State<'_, Arc<Engine>>,
    settings: settings::Settings,
) -> Result<Machine, Failure> {
    settings::save(app.store.home(), &settings).map_err(fail)?;
    engine.apply_settings();
    machine_of(&app)
}

#[tauri::command(async)]
fn set_secret(app: State<'_, App>, name: String, value: String) -> Result<(), Failure> {
    settings::set_secret(app.store.home(), &name, &value).map_err(fail)
}

/// Opens Docker's install page in the browser. Only that page: the window can't open others.
#[tauri::command(async)]
fn open_docker_page() -> Result<(), Failure> {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(opener).arg(vm::install_page()).spawn().map(|_| ()).map_err(|e| Failure::new("docker", format!("couldn't open {}: {e}", vm::install_page())))
}

/// Starts Docker Desktop, which can take a minute before its engine answers.
#[tauri::command(async)]
fn start_docker() -> Result<(), Failure> {
    if !cfg!(target_os = "macos") {
        return Err(Failure::new("docker", "Start Docker's engine yourself, for example with `sudo systemctl start docker`."));
    }
    let ok = std::process::Command::new("open").args(["-a", "Docker"]).status().is_ok_and(|s| s.success());
    if ok { Ok(()) } else { Err(Failure::new("docker", "Docker Desktop couldn't be started. Open it from Applications.")) }
}

/// Installs Docker Desktop with Homebrew in a terminal of its own, on the Mac, where the person
/// watches it and types their password if it asks. Then starts it.
#[tauri::command(async)]
fn install_docker(app: State<'_, App>, bridge: State<'_, Bridge>) -> Result<Value, Failure> {
    let brew = vm::homebrew().ok_or_else(|| Failure::new("docker", "Homebrew isn't installed, so install Docker Desktop from its page."))?;
    let home = std::env::var("HOME").map_err(|_| Failure::new("docker", "couldn't find the home folder"))?;
    let brew = shell::quote(&brew.display().to_string());
    engine::start_terminal(
        &app.store,
        &bridge,
        &app.program,
        engine::StartRequest {
            cwd: Some(home),
            command: Some(format!("{brew} install --cask docker && open -a Docker")),
            name: Some("install docker".into()),
            ..engine::StartRequest::default()
        },
    )
}

/// Which of these secrets have a value. Never the values: the window can't read a secret back.
#[tauri::command(async)]
fn secrets_set(app: State<'_, App>, names: Vec<String>) -> Vec<String> {
    names.into_iter().filter(|n| settings::get_secret(app.store.home(), n).is_some()).collect()
}

/// Stores one of a VM sandbox's secrets, for that sandbox only. An empty value removes it.
#[tauri::command(async)]
fn set_vm_secret(app: State<'_, App>, factory: String, runner: String, name: String, value: String) -> Result<(), Failure> {
    vm::secret_name(&name).map_err(|why| Failure::new("secret", why))?;
    settings::set_secret(app.store.home(), &settings::vm_key(&factory, &runner, &name), value.trim()).map_err(fail)
}

/// Which of a VM sandbox's secrets have a value. Never the values themselves.
#[tauri::command(async)]
fn vm_secrets_set(app: State<'_, App>, factory: String, runner: String, names: Vec<String>) -> Vec<String> {
    names.into_iter().filter(|n| settings::get_secret(app.store.home(), &settings::vm_key(&factory, &runner, n)).is_some()).collect()
}

/// Every connection, its plain values and which secrets are set. Never a secret's value.
#[tauri::command(async)]
fn connections(app: State<'_, App>) -> Vec<connections::View> {
    connections::views(app.store.home())
}

/// Stores one of a connection's fields. An empty value clears it.
#[tauri::command(async)]
fn set_connection_field(app: State<'_, App>, engine: State<'_, Arc<Engine>>, connection: String, name: String, value: String) -> Result<(), Failure> {
    connections::set(app.store.home(), &connection, &name, &value).map_err(|why| Failure::new("connection", why))?;
    if connection == connections::SLACK {
        engine.restart_slack();
    }
    Ok(())
}

/// Tries a connection for real: who it signs in as, or why it can't.
#[tauri::command(async)]
fn test_connection(app: State<'_, App>, connection: String) -> Result<String, Failure> {
    connections::test(app.store.home(), &connection).map_err(|why| Failure::new("connection", why))
}

#[tauri::command(async)]
fn slack_manifest() -> String {
    connections::slack_manifest()
}

/// Whether the Slack connection is up, and what it last said. Nothing while it's off.
#[tauri::command(async)]
fn slack_state(engine: State<'_, Arc<Engine>>) -> Option<serde_json::Value> {
    engine.slack_state().map(|(ok, text)| json!({ "ok": ok, "text": text }))
}

/// Why a source can't pull yet, or nothing when it can.
#[tauri::command(async)]
fn source_ready(app: State<'_, App>, id: String, part: String) -> Result<Option<String>, Failure> {
    let f = app.store.get(&id).map_err(fail)?;
    let p = f.parts.iter().find(|p| p.id == part).ok_or_else(|| Failure::new("source", format!("there's no source {part}")))?;
    Ok(tickets::not_ready(app.store.home(), &f, p))
}

/// Sends a report with made-up details and says what happened.
#[tauri::command(async)]
fn test_report(app: State<'_, App>, id: String, report: String) -> Result<serde_json::Value, Failure> {
    let f = app.store.get(&id).map_err(fail)?;
    let r = f.reports.iter().find(|r| r.id == report).ok_or_else(|| Failure::new("report", format!("there's no report {report}")))?;
    let (ok, said) = flow::test_report(app.store.home(), &f, r);
    Ok(json!({ "ok": ok, "said": said }))
}

/// Stores one of a report's keys, for that report only. An empty value removes it.
#[tauri::command(async)]
fn set_report_key(app: State<'_, App>, factory: String, report: String, name: String, value: String) -> Result<(), Failure> {
    vm::secret_name(&name).map_err(|why| Failure::new("secret", why))?;
    settings::set_secret(app.store.home(), &settings::report_key(&factory, &report, &name), value.trim()).map_err(fail)
}

/// Which of a report's keys have a value. Never the values themselves.
#[tauri::command(async)]
fn report_keys_set(app: State<'_, App>, factory: String, report: String, names: Vec<String>) -> Vec<String> {
    names.into_iter().filter(|n| settings::get_secret(app.store.home(), &settings::report_key(&factory, &report, n)).is_some()).collect()
}

/// Stores one of a ticket source's keys. An empty value removes it.
#[tauri::command(async)]
fn set_source_key(app: State<'_, App>, factory: String, source: String, name: String, value: String) -> Result<(), Failure> {
    vm::secret_name(&name).map_err(|why| Failure::new("secret", why))?;
    settings::set_secret(app.store.home(), &settings::source_key(&factory, &source, &name), value.trim()).map_err(fail)
}

/// Which of a ticket source's keys have a value. Never the values themselves.
#[tauri::command(async)]
fn source_keys_set(app: State<'_, App>, factory: String, source: String, names: Vec<String>) -> Vec<String> {
    names.into_iter().filter(|n| settings::get_secret(app.store.home(), &settings::source_key(&factory, &source, n)).is_some()).collect()
}

#[tauri::command(async)]
fn harnesses(app: State<'_, App>) -> Vec<harness::Harness> {
    harness::load(&app.store.home().join("harnesses")).0
}

#[tauri::command(async)]
fn save_harness(app: State<'_, App>, harness: harness::Harness) -> Result<(), Failure> {
    let dir = app.store.home().join("harnesses");
    std::fs::create_dir_all(&dir).map_err(|e| fail(e.into()))?;
    let text = serde_yaml_ng::to_string(&harness).map_err(|e| fail(e.into()))?;
    std::fs::write(dir.join(format!("{}.yaml", factories::slug(&harness.id))), text).map_err(|e| fail(e.into()))
}

#[tauri::command(async)]
fn nearby_folders(app: State<'_, App>) -> Vec<FolderSuggestion> {
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    let taken: Vec<(String, String)> = app
        .store
        .list()
        .unwrap_or_default()
        .into_iter()
        .map(|f| (f.path, f.name))
        .collect();
    project::nearby(Path::new(&home), &taken)
}

#[tauri::command(async)]
fn detect_project(path: String) -> ProjectInfo {
    project::detect(Path::new(&path))
}

// ---- Start-up ---------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("daemon") => {
            if let Err(e) = Daemon::new(Config::default()).serve() {
                eprintln!("sawtooth: {e:#}");
                std::process::exit(1);
            }
            return;
        }
        Some("hook") => {
            let (Some(id), Some(hook)) = (args.get(2), args.get(3)) else {
                eprintln!("sawtooth: usage: sawtooth-app hook <factory> <pre-commit|pre-push>");
                std::process::exit(0);
            };
            let store = Store::new(paths::home());
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            std::process::exit(hooks::run_hook(&store, id, hook, &cwd));
        }
        Some("st") => std::process::exit(st::main(&args[2..])),
        Some("sandbox-bridge") => std::process::exit(sawtooth_sandbox::bridge_main(&args[2..])),
        Some("probe") => std::process::exit(probe::main(&args[2..])),
        Some("vm-run") => std::process::exit(vm::main(&args[2..])),
        _ => {}
    }

    let program = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("sawtooth-app"));
    let store = Arc::new(Store::new(paths::home()));
    if let Err(e) = st::install(store.home(), &program) {
        eprintln!("sawtooth: couldn't install st: {e}");
    }
    let app = App {
        store: Arc::clone(&store),
        program: program.clone(),
        problems: Mutex::new(HashMap::new()),
    };
    // Put links and hooks right at start-up: the app may have moved since.
    for factory in store.list().unwrap_or_default() {
        app.apply(&factory, None);
    }

    let built = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Bridge::new(program.clone()))
        .manage(app)
        .setup(move |tauri_app| {
            tauri_app
                .state::<Bridge>()
                .watch(tauri_app.handle().clone());
            let handle = tauri_app.handle().clone();
            let emit: engine::Emit = Arc::new(move |name: &str, payload: Value| {
                let _ = handle.emit(name, payload);
            });
            let engine = Engine::new(Arc::clone(&store), program.clone(), emit);
            engine.start();
            tauri_app.manage(engine);
            #[cfg(debug_assertions)]
            testbridge::start(tauri_app.handle().clone(), &paths::home());
            leave_key::watch(tauri_app.handle());
            // The menu shows the key, and clicking it does the same.
            let menu = Menu::default(tauri_app.handle())?;
            let leave = MenuItem::with_id(
                tauri_app,
                "leave-terminal",
                "Back to Sawtooth",
                true,
                Some("CmdOrCtrl+Up"),
            )?;
            menu.append(&Submenu::with_items(tauri_app, "Terminal", true, &[&leave])?)?;
            tauri_app.set_menu(menu)?;
            tauri_app.on_menu_event(|app, event| {
                if event.id() == "leave-terminal" {
                    let _ = app.emit(leave_key::EVENT, ());
                }
            });
            Ok(())
        })
        // Closing the window hides it, so the engine keeps working. Cmd+Q quits.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            broken_factories,
            dismiss_alert,
            leave_key::in_terminal,
            daemon,
            view,
            unview,
            start_terminal,
            installed,
            branch,
            changes,
            touched,
            factories,
            create_factory,
            save_factory,
            remove_factory,
            runs,
            knowledge,
            save_knowledge,
            approve_knowledge,
            undo_knowledge,
            read_file,
            write_file,
            folder_files,
            list_tickets,
            run_benchmark,
            test_runner,
            sandbox_info,
            secrets_set,
            set_vm_secret,
            vm_secrets_set,
            set_source_key,
            connections,
            set_connection_field,
            test_connection,
            slack_manifest,
            slack_state,
            source_ready,
            test_report,
            set_report_key,
            report_keys_set,
            source_keys_set,
            open_docker_page,
            start_docker,
            install_docker,
            benchmark_results,
            suggest_start,
            answer_proposal,
            feed_tickets,
            nearby_folders,
            detect_project,
            work_action,
            work_evidence,
            accept_proposal,
            reject_proposal,
            pull_source,
            answer_denied,
            source_templates,
            machine,
            save_machine,
            set_secret,
            harnesses,
            save_harness,
        ])
        .build(tauri::generate_context!())
        .expect("Sawtooth couldn't open its window");

    built.run(|handle, event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = event
            && let Some(window) = handle.get_webview_window("main")
        {
            let _ = window.show();
            let _ = window.set_focus();
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (handle, event);
    });
}

#[cfg(test)]
mod tests {
    use super::describe;

    #[test]
    fn describes_a_markdown_file() {
        assert_eq!(
            describe("# Tax\n\nCanadian invoices show **GST** as its own line.\n", "tax.md"),
            ("Tax".to_string(), "Canadian invoices show GST as its own line.".to_string())
        );
        assert_eq!(
            describe("<!-- note -->\n- first point\n", "partial-refunds.md"),
            ("partial refunds".to_string(), "first point".to_string())
        );
        assert_eq!(describe("", "x.md"), ("x".to_string(), String::new()));
    }
}
