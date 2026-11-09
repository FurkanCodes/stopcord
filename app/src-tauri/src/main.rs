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
mod engine;
mod factories;
mod git;
mod hooks;
mod kb;
mod leave_key;
mod project;
mod runs;
mod shell;
#[cfg(debug_assertions)]
mod testbridge;
mod tickets;
mod typing;
mod work;
mod worktree;

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bridge::{Bridge, Failure};
use engine::{Engine, StartRequest};
use factories::{Factory, Kind, Part, RunsOn, Store, Ticket};
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
        StartRequest {
            factory_id,
            cwd,
            command,
            name,
            branch,
            rows,
            cols,
            task,
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

#[tauri::command(async)]
fn create_factory(
    app: State<'_, App>,
    path: String,
    name: String,
    template: String,
    read_as: Vec<String>,
) -> Result<FactoryInfo, Failure> {
    let folder = Path::new(&path);
    let mut parts = vec![Part {
        id: "kb".into(),
        kind: Kind::Knowledge,
        name: "Knowledge base".into(),
        read_as,
        ..Part::default()
    }];
    if template == "reviewed" {
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
    let factory = app.store.create(folder, &name, parts).map_err(fail)?;
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
fn list_tickets(app: State<'_, App>, id: String) -> Result<Vec<Ticket>, Failure> {
    let factory = app.store.get(&id).map_err(fail)?;
    tickets::run(&factory).map_err(|e| Failure::new("tickets", format!("{e:#}")))
}

#[tauri::command(async)]
fn feed_tickets(
    app: State<'_, App>,
    engine: State<'_, Arc<Engine>>,
    id: String,
    tickets: Vec<Ticket>,
) -> Result<FactoryInfo, Failure> {
    engine.feed(&id, tickets).map_err(fail)?;
    Ok(app.info(app.store.get(&id).map_err(fail)?))
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
        _ => {}
    }

    let program = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("sawtooth-app"));
    let store = Arc::new(Store::new(paths::home()));
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
            feed_tickets,
            nearby_folders,
            detect_project,
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
