//! A factory's definition: the files a person edits.
//!
//! The layout follows Warp's factory tree, so anyone who knows one can read the
//! other. There's no promise that a Warp file loads unchanged.
//!
//! ```text
//! definition/
//!   factory.yaml                       repos, defaults, checks, sources, the line
//!   agents/<name>/agent.md             frontmatter and a role prompt
//!   agents/<name>/skills/<s>/SKILL.md  for that agent only
//!   skills/<s>/SKILL.md                for every agent
//!   automations/<name>/automation.md   triggers and a prompt
//!   runners/<name>.yaml                how an agent is confined
//!   knowledge.md
//! ```
//!
//! The folder is its own git repository, so every change has a diff.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::factories::{
    Agent, Defaults, Factory, Kind, LineStage, Part, Report, Runner, RunsIn, RunsOn, Skill,
    StageKind, Suite, When,
};

pub const SCHEMA: &str = "sawtooth/v1";
const FACTORY_FILE: &str = "factory.yaml";

fn is_false(v: &bool) -> bool {
    !*v
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KnowledgeDoc {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    read_as: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FolderDoc {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    folder: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckDoc {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    runs_on: RunsOn,
    #[serde(default, skip_serializing_if = "is_false")]
    network: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SourceDoc {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    command: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    harness: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    at_once: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    every: String,
    #[serde(default, skip_serializing_if = "is_false")]
    auto_start: bool,
}

/// `factory.yaml`.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FactoryDoc {
    schema_version: String,
    name: String,
    project: String,
    #[serde(default)]
    agent_defaults: Defaults,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    knowledge: Option<KnowledgeDoc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    folders: Vec<FolderDoc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    checks: Vec<CheckDoc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sources: Vec<SourceDoc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    line: Vec<LineStage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    reports: Vec<Report>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AgentMeta {
    #[serde(default)]
    description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    harness: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    runner: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkillMeta {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(default)]
    description: String,
}

/// One thing that sets off an automation.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TriggerDoc {
    provider: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    event: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    schedule: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutomationMeta {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    triggers: Vec<TriggerDoc>,
    #[serde(default)]
    runs_in: RunsIn,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    harness: String,
    #[serde(default, skip_serializing_if = "is_false")]
    disabled: bool,
}

fn trigger_doc(p: &Part) -> TriggerDoc {
    let pair = |provider: &str, event: &str| TriggerDoc {
        provider: provider.into(),
        event: event.into(),
        schedule: String::new(),
    };
    match p.when {
        When::Quiet => pair("terminal", "quiet"),
        When::CheckFails => pair("check", "failed"),
        When::TicketFed => pair("ticket", "fed"),
        When::Schedule => TriggerDoc {
            provider: "schedule".into(),
            event: String::new(),
            schedule: p.schedule.clone(),
        },
    }
}

fn when_of(t: &TriggerDoc) -> Result<When> {
    Ok(match (t.provider.as_str(), t.event.as_str()) {
        ("terminal", "quiet") => When::Quiet,
        ("check", "failed") => When::CheckFails,
        ("ticket", "fed") => When::TicketFed,
        ("schedule", _) => When::Schedule,
        (p, e) => bail!("unknown trigger: provider {p:?}, event {e:?}"),
    })
}

/// Splits `---` frontmatter from the Markdown after it.
fn split_frontmatter(text: &str) -> Result<(&str, &str)> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .context("it has to start with a --- line")?;
    if let Some(body) = rest.strip_prefix("---\n") {
        return Ok(("", body));
    }
    let end = rest.find("\n---").context("its frontmatter never ends with a --- line")?;
    let body = rest[end + 4..].trim_start_matches('\r');
    let body = body.strip_prefix('\n').unwrap_or(body);
    Ok((&rest[..end + 1], body))
}

fn with_frontmatter<T: Serialize>(meta: &T, body: &str) -> Result<String> {
    let body = body.trim_end();
    let yaml = serde_yaml_ng::to_string(meta)?;
    Ok(if body.is_empty() {
        format!("---\n{yaml}---\n")
    } else {
        format!("---\n{yaml}---\n{body}\n")
    })
}

fn read_md<T: DeserializeOwned>(file: &Path) -> Result<(T, String)> {
    let text = read(file)?;
    let (head, body) = split_frontmatter(&text).with_context(|| format!("{} isn't valid", file.display()))?;
    let meta = serde_yaml_ng::from_str(head).with_context(|| format!("{} isn't valid", file.display()))?;
    Ok((meta, body.trim_end().to_string()))
}

/// Reads a definition file. A symlink could point an agent's prompt anywhere, so it's refused.
fn read(file: &Path) -> Result<String> {
    let meta = std::fs::symlink_metadata(file).with_context(|| format!("couldn't read {}", file.display()))?;
    if meta.file_type().is_symlink() {
        bail!("{} is a symlink, and definition files can't be", file.display());
    }
    std::fs::read_to_string(file).with_context(|| format!("couldn't read {}", file.display()))
}

/// Folder names directly under `dir`, sorted. A missing folder has none.
fn subdirs(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
}

fn read_skills(dir: &Path, agent: &str, out: &mut Vec<Skill>) -> Result<()> {
    for id in subdirs(dir) {
        let file = dir.join(&id).join("SKILL.md");
        if !file.exists() {
            continue;
        }
        let (meta, body): (SkillMeta, String) = read_md(&file)?;
        out.push(Skill {
            id,
            agent: agent.to_string(),
            description: meta.description,
            body,
        });
    }
    Ok(())
}

/// Reads a definition folder into a factory. Engine state isn't here; see `Store`.
pub fn load(dir: &Path) -> Result<Factory> {
    let file = dir.join(FACTORY_FILE);
    let text = read(&file)?;
    let doc: FactoryDoc = serde_yaml_ng::from_str(&text).with_context(|| format!("{} isn't valid", file.display()))?;
    if doc.schema_version != SCHEMA {
        bail!("{} isn't valid: schemaVersion must be {SCHEMA}", file.display());
    }
    let mut parts = Vec::new();
    if let Some(k) = doc.knowledge {
        parts.push(Part {
            id: k.id,
            kind: Kind::Knowledge,
            name: k.name,
            read_as: k.read_as,
            ..Part::default()
        });
    }
    parts.extend(doc.folders.into_iter().map(|d| Part {
        id: d.id,
        kind: Kind::Folder,
        name: d.name,
        folder: d.folder,
        ..Part::default()
    }));
    parts.extend(doc.checks.into_iter().map(|c| Part {
        id: c.id,
        kind: Kind::Check,
        name: c.name,
        command: c.command,
        runs_on: c.runs_on,
        network: c.network,
        ..Part::default()
    }));
    parts.extend(doc.sources.into_iter().map(|s| Part {
        id: s.id,
        kind: Kind::Tickets,
        name: s.name,
        command: s.command,
        harness: s.harness,
        at_once: s.at_once,
        every: s.every,
        auto_start: s.auto_start,
        ..Part::default()
    }));
    let autos = dir.join("automations");
    for id in subdirs(&autos) {
        let file = autos.join(&id).join("automation.md");
        if !file.exists() {
            continue;
        }
        let (meta, prompt): (AutomationMeta, String) = read_md(&file)?;
        let trigger = meta
            .triggers
            .first()
            .with_context(|| format!("{} isn't valid: it has no triggers", file.display()))?;
        let when = when_of(trigger).with_context(|| format!("{} isn't valid", file.display()))?;
        parts.push(Part {
            kind: Kind::Trigger,
            name: if meta.name.is_empty() { id.clone() } else { meta.name.clone() },
            when,
            schedule: trigger.schedule.clone(),
            runs_in: meta.runs_in,
            harness: meta.harness.clone(),
            prompt,
            id,
            ..Part::default()
        });
    }

    let mut agents = Vec::new();
    let mut skills = Vec::new();
    read_skills(&dir.join("skills"), "", &mut skills)?;
    let agents_dir = dir.join("agents");
    for id in subdirs(&agents_dir) {
        let file = agents_dir.join(&id).join("agent.md");
        if !file.exists() {
            continue;
        }
        let (meta, prompt): (AgentMeta, String) = read_md(&file)?;
        read_skills(&agents_dir.join(&id).join("skills"), &id, &mut skills)?;
        agents.push(Agent {
            id,
            description: meta.description,
            harness: meta.harness,
            runner: meta.runner,
            prompt,
        });
    }

    let mut scorers = Vec::new();
    for id in subdirs(&dir.join("scorers")) {
        let file = dir.join("scorers").join(&id).join("scorer.md");
        if file.exists() {
            let (meta, prompt): (AgentMeta, String) = read_md(&file)?;
            scorers.push(Agent { id, description: meta.description, harness: meta.harness, runner: meta.runner, prompt });
        }
    }

    let mut benchmarks = Vec::new();
    for id in subdirs(&dir.join("benchmarks")) {
        let file = dir.join("benchmarks").join(&id).join("suite.yaml");
        if file.exists() {
            let mut suite: Suite = serde_yaml_ng::from_str(&read(&file)?).with_context(|| format!("{} isn't valid", file.display()))?;
            suite.id = id;
            benchmarks.push(suite);
        }
    }

    let mut runners = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir.join("runners")) {
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "yaml"))
            .collect();
        files.sort();
        for file in files {
            let text = read(&file)?;
            let mut runner: Runner =
                serde_yaml_ng::from_str(&text).with_context(|| format!("{} isn't valid", file.display()))?;
            runner.id = file.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            runners.push(runner);
        }
    }

    let factory = Factory {
        name: doc.name,
        path: doc.project,
        parts,
        agents,
        skills,
        runners,
        scorers,
        benchmarks,
        line: doc.line,
        reports: doc.reports,
        defaults: doc.agent_defaults,
        ..Factory::default()
    };
    let problems = validate(&factory);
    if !problems.is_empty() {
        bail!("{} isn't valid: {}", file.display(), problems.join("; "));
    }
    Ok(factory)
}

fn safe_name(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// What's wrong with a factory, in words a person can act on. Empty means it's fine.
pub fn validate(f: &Factory) -> Vec<String> {
    let mut out = Vec::new();
    let named = f
        .agents
        .iter()
        .map(|a| ("agent", a.id.as_str()))
        .chain(f.runners.iter().map(|r| ("runner", r.id.as_str())))
        .chain(f.skills.iter().map(|s| ("skill", s.id.as_str())))
        .chain(f.scorers.iter().map(|s| ("scorer", s.id.as_str())))
        .chain(f.benchmarks.iter().map(|s| ("benchmark", s.id.as_str())))
        .chain(
            f.parts
                .iter()
                .filter(|p| p.kind == Kind::Trigger)
                .map(|p| ("automation", p.id.as_str())),
        );
    for (what, id) in named {
        if !safe_name(id) {
            out.push(format!("{what} {id:?} needs a name of letters, digits, dashes and dots"));
        }
    }
    let agents: BTreeSet<&str> = f.agents.iter().chain(&f.scorers).map(|a| a.id.as_str()).collect();
    let runners: BTreeSet<&str> = f.runners.iter().map(|r| r.id.as_str()).collect();
    let checks: BTreeSet<&str> = f
        .parts
        .iter()
        .filter(|p| p.kind == Kind::Check)
        .map(|p| p.id.as_str())
        .collect();
    if !f.defaults.runner.is_empty() && !runners.contains(f.defaults.runner.as_str()) {
        out.push(format!("agentDefaults names a runner that doesn't exist: {}", f.defaults.runner));
    }
    for s in &f.benchmarks {
        if s.tasks.iter().any(|t| t.key.trim().is_empty() || t.check.trim().is_empty()) {
            out.push(format!("benchmark {} has a task with no key or no check", s.id));
        }
        if !s.candidates.is_empty() && !agents.contains(s.agent.as_str()) {
            out.push(format!("benchmark {} compares candidates but names no agent to swap", s.id));
        }
        if !s.start_at.is_empty() && !f.line.iter().any(|l| l.id == s.start_at && l.kind == StageKind::Agent) {
            out.push(format!("benchmark {} starts at {}, which isn't an agent stage", s.id, s.start_at));
        }
    }
    for a in f.agents.iter().chain(&f.scorers) {
        if !a.runner.is_empty() && !runners.contains(a.runner.as_str()) {
            out.push(format!("agent {} names a runner that doesn't exist: {}", a.id, a.runner));
        }
    }
    for s in &f.skills {
        if !s.agent.is_empty() && !agents.contains(s.agent.as_str()) {
            out.push(format!("skill {} belongs to an agent that doesn't exist: {}", s.id, s.agent));
        }
    }
    out.extend(validate_line(&f.line, &agents, &checks));
    out
}

fn validate_line(line: &[LineStage], agents: &BTreeSet<&str>, checks: &BTreeSet<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if line.is_empty() {
        return out;
    }
    let mut by_id: BTreeMap<&str, &LineStage> = BTreeMap::new();
    for s in line {
        if by_id.insert(s.id.as_str(), s).is_some() {
            out.push(format!("the line has two stages called {}", s.id));
        }
        match s.kind {
            StageKind::Agent if !agents.contains(s.agent.as_str()) => {
                out.push(format!("stage {} needs an agent that exists, and has {:?}", s.id, s.agent));
            }
            StageKind::Gate if s.checks.is_empty() => {
                out.push(format!("gate {} has no checks", s.id));
            }
            StageKind::Gate => {
                for c in &s.checks {
                    if !checks.contains(c.as_str()) {
                        out.push(format!("gate {} names a check that doesn't exist: {c}", s.id));
                    }
                }
            }
            _ => {}
        }
        if s.isolated && s.kind != StageKind::Agent {
            out.push(format!("stage {} is isolated, and only agent stages can be", s.id));
        }
        if !s.isolated && !s.keep.is_empty() {
            out.push(format!("stage {} has keep, which only means something on an isolated stage", s.id));
        }
        for name in &s.keep {
            if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
                out.push(format!("stage {} keeps {name:?}, and keep takes file names in the top folder", s.id));
            }
        }
        for (label, to) in [("then", &s.then), ("onFail", &s.on_fail)] {
            if !to.is_empty() && !line.iter().any(|t| &t.id == to) {
                out.push(format!("stage {} has {label}: {to}, and there's no such stage", s.id));
            }
        }
        if !s.on_fail.is_empty() && s.max_loops == 0 {
            out.push(format!("stage {} can loop back to {} with no maxLoops", s.id, s.on_fail));
        }
    }
    if !out.is_empty() {
        return out;
    }
    // Every stage has to be able to reach a stage with nowhere further to go.
    let ends: BTreeSet<&str> = line.iter().filter(|s| s.then.is_empty()).map(|s| s.id.as_str()).collect();
    if ends.is_empty() {
        out.push("the line has no end: every stage goes on to another".to_string());
        return out;
    }
    for s in line {
        let mut seen = BTreeSet::new();
        let mut at = s.id.as_str();
        while !ends.contains(at) {
            if !seen.insert(at) {
                out.push(format!("stage {} goes round in a circle and never reaches an end", s.id));
                break;
            }
            at = by_id[at].then.as_str();
        }
    }
    out
}

/// Every file the factory is made of, by path relative to the definition folder.
fn render(f: &Factory) -> Result<BTreeMap<PathBuf, String>> {
    let parts = |kind: Kind| f.parts.iter().filter(move |p| p.kind == kind);
    let doc = FactoryDoc {
        schema_version: SCHEMA.into(),
        name: f.name.clone(),
        project: f.path.clone(),
        agent_defaults: f.defaults.clone(),
        knowledge: f.knowledge().map(|k| KnowledgeDoc {
            id: k.id.clone(),
            name: k.name.clone(),
            read_as: k.read_as.clone(),
        }),
        folders: parts(Kind::Folder)
            .map(|p| FolderDoc {
                id: p.id.clone(),
                name: p.name.clone(),
                folder: p.folder.clone(),
            })
            .collect(),
        checks: parts(Kind::Check)
            .map(|p| CheckDoc {
                id: p.id.clone(),
                name: p.name.clone(),
                command: p.command.clone(),
                runs_on: p.runs_on,
                network: p.network,
            })
            .collect(),
        sources: parts(Kind::Tickets)
            .map(|p| SourceDoc {
                id: p.id.clone(),
                name: p.name.clone(),
                command: p.command.clone(),
                harness: p.harness.clone(),
                at_once: p.at_once,
                every: p.every.clone(),
                auto_start: p.auto_start,
            })
            .collect(),
        line: f.line.clone(),
        reports: f.reports.clone(),
    };
    let mut files = BTreeMap::new();
    files.insert(PathBuf::from(FACTORY_FILE), serde_yaml_ng::to_string(&doc)?);
    for p in parts(Kind::Trigger) {
        let meta = AutomationMeta {
            name: p.name.clone(),
            triggers: vec![trigger_doc(p)],
            runs_in: p.runs_in,
            harness: p.harness.clone(),
            disabled: false,
        };
        files.insert(
            Path::new("automations").join(&p.id).join("automation.md"),
            with_frontmatter(&meta, &p.prompt)?,
        );
    }
    for a in &f.agents {
        let meta = AgentMeta {
            description: a.description.clone(),
            harness: a.harness.clone(),
            runner: a.runner.clone(),
        };
        files.insert(
            Path::new("agents").join(&a.id).join("agent.md"),
            with_frontmatter(&meta, &a.prompt)?,
        );
    }
    for a in &f.scorers {
        let meta = AgentMeta {
            description: a.description.clone(),
            harness: a.harness.clone(),
            runner: a.runner.clone(),
        };
        files.insert(Path::new("scorers").join(&a.id).join("scorer.md"), with_frontmatter(&meta, &a.prompt)?);
    }
    for s in &f.benchmarks {
        files.insert(Path::new("benchmarks").join(&s.id).join("suite.yaml"), serde_yaml_ng::to_string(s)?);
    }
    for s in &f.skills {
        let meta = SkillMeta {
            name: s.id.clone(),
            description: s.description.clone(),
        };
        let base = if s.agent.is_empty() {
            PathBuf::from("skills")
        } else {
            Path::new("agents").join(&s.agent).join("skills")
        };
        files.insert(base.join(&s.id).join("SKILL.md"), with_frontmatter(&meta, &s.body)?);
    }
    for r in &f.runners {
        files.insert(
            Path::new("runners").join(format!("{}.yaml", r.id)),
            serde_yaml_ng::to_string(r)?,
        );
    }
    Ok(files)
}

/// Files in the managed folders that the factory no longer has. Anything else a person
/// put in the folder stays.
fn stale(dir: &Path, wanted: &BTreeMap<PathBuf, String>) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut note = |rel: PathBuf| {
        if dir.join(&rel).is_file() && !wanted.contains_key(&rel) {
            found.push(rel);
        }
    };
    for id in subdirs(&dir.join("automations")) {
        note(Path::new("automations").join(id).join("automation.md"));
    }
    for id in subdirs(&dir.join("skills")) {
        note(Path::new("skills").join(id).join("SKILL.md"));
    }
    for id in subdirs(&dir.join("scorers")) {
        note(Path::new("scorers").join(id).join("scorer.md"));
    }
    for id in subdirs(&dir.join("benchmarks")) {
        note(Path::new("benchmarks").join(id).join("suite.yaml"));
    }
    for id in subdirs(&dir.join("agents")) {
        note(Path::new("agents").join(&id).join("agent.md"));
        for skill in subdirs(&dir.join("agents").join(&id).join("skills")) {
            note(Path::new("agents").join(&id).join("skills").join(skill).join("SKILL.md"));
        }
    }
    if let Ok(entries) = std::fs::read_dir(dir.join("runners")) {
        for e in entries.flatten() {
            if e.path().extension().is_some_and(|x| x == "yaml") {
                note(Path::new("runners").join(e.file_name()));
            }
        }
    }
    found
}

fn remove_empty_parents(dir: &Path, rel: &Path) {
    let mut at = rel.parent();
    while let Some(parent) = at {
        if parent.as_os_str().is_empty() || std::fs::remove_dir(dir.join(parent)).is_err() {
            break;
        }
        at = parent.parent();
    }
}

fn write_atomic(file: &Path, text: &str) -> Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = file.with_extension("tmp-write");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, file)?;
    Ok(())
}

/// Writes the factory's files, leaving unchanged ones alone. Returns true if anything changed.
/// `factory.yaml` goes last: it's what marks a definition as complete.
pub fn save(dir: &Path, f: &Factory) -> Result<bool> {
    let problems = validate(f);
    if !problems.is_empty() {
        bail!("{}", problems.join("; "));
    }
    let wanted = render(f)?;
    let mut changed = false;
    let top = Path::new(FACTORY_FILE);
    for (rel, text) in wanted.iter().filter(|(rel, _)| rel.as_path() != top) {
        changed |= write_if_changed(&dir.join(rel), text)?;
    }
    for rel in stale(dir, &wanted) {
        std::fs::remove_file(dir.join(&rel))?;
        remove_empty_parents(dir, &rel);
        changed = true;
    }
    changed |= write_if_changed(&dir.join(top), &wanted[top])?;
    Ok(changed)
}

fn write_if_changed(file: &Path, text: &str) -> Result<bool> {
    if std::fs::read_to_string(file).is_ok_and(|now| now == text) {
        return Ok(false);
    }
    write_atomic(file, text)?;
    Ok(true)
}

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=Sawtooth", "-c", "user.email=sawtooth@localhost"])
        .args(["-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null"])
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Records the folder's current state in its own git repository. Without git the
/// factory still works; it only loses the history.
pub fn commit(dir: &Path, message: &str) {
    if !dir.join(".git").exists() && !git(dir, &["init", "-q", "-b", "main"]) {
        return;
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
}

/// Newest modification time in the folder, so a hand edit is noticed.
pub fn newest(dir: &Path) -> Option<std::time::SystemTime> {
    let mut best = None;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&at) else { continue };
        for e in entries.flatten() {
            if e.file_name() == ".git" {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                stack.push(e.path());
            } else if let Ok(t) = meta.modified() {
                best = Some(best.map_or(t, |b: std::time::SystemTime| b.max(t)));
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::Level;

    fn sample() -> Factory {
        Factory {
            name: "Payments".into(),
            path: "/code/payments".into(),
            defaults: Defaults {
                harness: "claude".into(),
                runner: "contained".into(),
                at_once: 2,
            },
            parts: vec![
                Part {
                    id: "kb".into(),
                    kind: Kind::Knowledge,
                    name: "Knowledge".into(),
                    read_as: vec!["AGENTS.md".into()],
                    ..Part::default()
                },
                Part {
                    id: "specs".into(),
                    kind: Kind::Folder,
                    name: "Specs".into(),
                    folder: "specs/".into(),
                    ..Part::default()
                },
                Part {
                    id: "tests".into(),
                    kind: Kind::Check,
                    name: "Tests".into(),
                    command: "npm test".into(),
                    runs_on: RunsOn::Merge,
                    ..Part::default()
                },
                Part {
                    id: "gh".into(),
                    kind: Kind::Tickets,
                    name: "GitHub".into(),
                    command: "gh issue list --json number".into(),
                    harness: "codex".into(),
                    at_once: 3,
                    ..Part::default()
                },
                Part {
                    id: "nightly".into(),
                    kind: Kind::Trigger,
                    name: "Nightly tidy".into(),
                    when: When::Schedule,
                    schedule: "every day 09:00".into(),
                    runs_in: RunsIn::Own,
                    prompt: "Tidy the knowledge base.\n\nKeep it short.".into(),
                    harness: "claude".into(),
                    ..Part::default()
                },
            ],
            agents: vec![Agent {
                id: "builder".into(),
                description: "Builds it".into(),
                harness: "codex".into(),
                runner: "contained".into(),
                prompt: "You build the change.".into(),
            }],
            skills: vec![
                Skill {
                    id: "crash".into(),
                    agent: "".into(),
                    description: "Investigate a crash".into(),
                    body: "Start from the stack trace.".into(),
                },
                Skill {
                    id: "tests".into(),
                    agent: "builder".into(),
                    description: "Testing standards".into(),
                    body: "Failing test first.".into(),
                },
            ],
            runners: vec![Runner {
                id: "contained".into(),
                level: Level::Contained,
                network_allow: vec!["registry.npmjs.org".into()],
                ..Runner::default()
            }],
            line: vec![
                LineStage {
                    id: "build".into(),
                    kind: StageKind::Agent,
                    agent: "builder".into(),
                    then: "verify".into(),
                    ..LineStage::default()
                },
                LineStage {
                    id: "verify".into(),
                    kind: StageKind::Gate,
                    checks: vec!["tests".into()],
                    then: "handoff".into(),
                    on_fail: "build".into(),
                    max_loops: 3,
                    ..LineStage::default()
                },
                LineStage {
                    id: "handoff".into(),
                    kind: StageKind::Human,
                    ..LineStage::default()
                },
            ],
            ..Factory::default()
        }
    }

    #[test]
    fn a_factory_survives_being_written_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let f = sample();
        assert!(save(dir.path(), &f).unwrap());
        assert_eq!(load(dir.path()).unwrap(), f);
    }

    #[test]
    fn the_files_are_where_the_layout_says() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &sample()).unwrap();
        for rel in [
            "factory.yaml",
            "agents/builder/agent.md",
            "agents/builder/skills/tests/SKILL.md",
            "skills/crash/SKILL.md",
            "automations/nightly/automation.md",
            "runners/contained.yaml",
        ] {
            assert!(dir.path().join(rel).is_file(), "{rel} is missing");
        }
        let yaml = std::fs::read_to_string(dir.path().join("factory.yaml")).unwrap();
        assert!(yaml.contains("schemaVersion: sawtooth/v1"), "{yaml}");
        assert!(yaml.contains("onFail: build"), "{yaml}");
        let auto = std::fs::read_to_string(dir.path().join("automations/nightly/automation.md")).unwrap();
        assert!(auto.starts_with("---\n"), "{auto}");
        assert!(auto.contains("provider: schedule"), "{auto}");
        assert!(auto.ends_with("Keep it short.\n"), "{auto}");
    }

    #[test]
    fn saving_the_same_thing_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &sample()).unwrap();
        assert!(!save(dir.path(), &sample()).unwrap());
    }

    #[test]
    fn a_removed_agent_loses_its_files_and_other_files_stay() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = sample();
        save(dir.path(), &f).unwrap();
        std::fs::write(dir.path().join("agents/builder/notes.txt"), "mine").unwrap();
        f.parts.retain(|p| p.id != "nightly");
        f.skills.retain(|s| s.id != "crash");
        save(dir.path(), &f).unwrap();
        assert!(!dir.path().join("automations/nightly").exists());
        assert!(!dir.path().join("skills/crash").exists());
        assert!(dir.path().join("agents/builder/notes.txt").exists());
    }

    #[test]
    fn a_hand_edit_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &sample()).unwrap();
        let file = dir.path().join("agents/builder/agent.md");
        let text = std::fs::read_to_string(&file).unwrap().replace("You build the change.", "Build it small.");
        std::fs::write(&file, text).unwrap();
        assert_eq!(load(dir.path()).unwrap().agents[0].prompt, "Build it small.");
    }

    #[test]
    fn a_typo_in_a_field_is_refused_rather_than_ignored() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &sample()).unwrap();
        let file = dir.path().join("factory.yaml");
        let text = std::fs::read_to_string(&file).unwrap().replace("onFail:", "onfail:");
        std::fs::write(&file, text).unwrap();
        let e = format!("{:#}", load(dir.path()).unwrap_err());
        assert!(e.contains("isn't valid") && e.contains("onfail"), "{e}");
    }

    #[test]
    fn a_symlinked_prompt_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &sample()).unwrap();
        let file = dir.path().join("agents/builder/agent.md");
        std::fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink("/etc/hosts", &file).unwrap();
        assert!(format!("{:#}", load(dir.path()).unwrap_err()).contains("symlink"));
    }

    fn line_problems(change: impl FnOnce(&mut Factory)) -> String {
        let mut f = sample();
        change(&mut f);
        validate(&f).join("; ")
    }

    #[test]
    fn a_sound_line_has_no_problems() {
        assert_eq!(line_problems(|_| ()), "");
    }

    #[test]
    fn a_line_must_have_real_stages_agents_and_checks() {
        assert!(line_problems(|f| f.line[0].then = "nope".into()).contains("no such stage"));
        assert!(line_problems(|f| f.line[0].agent = "ghost".into()).contains("needs an agent"));
        assert!(line_problems(|f| f.line[1].checks = vec!["ghost".into()]).contains("check that doesn't exist"));
        assert!(line_problems(|f| f.line[1].checks.clear()).contains("no checks"));
        assert!(line_problems(|f| f.line[1].id = "build".into()).contains("two stages"));
    }

    #[test]
    fn a_line_must_end_and_its_loops_must_be_limited() {
        assert!(line_problems(|f| f.line[1].max_loops = 0).contains("no maxLoops"));
        assert!(line_problems(|f| f.line[1].keep = vec!["PLAN.md".into()]).contains("only means something"));
        assert!(line_problems(|f| {
            f.line[1].isolated = true;
            f.line[1].kind = StageKind::Human;
        })
        .contains("only agent stages"));
        assert!(line_problems(|f| f.line[2].then = "build".into()).contains("no end"));
        let circle = line_problems(|f| {
            f.line[0].then = "verify".into();
            f.line[1].then = "build".into();
        });
        assert!(circle.contains("circle"), "{circle}");
    }

    #[test]
    fn references_to_runners_must_exist() {
        assert!(line_problems(|f| f.defaults.runner = "ghost".into()).contains("runner that doesn't exist"));
        assert!(line_problems(|f| f.agents[0].runner = "ghost".into()).contains("runner that doesn't exist"));
    }

    #[test]
    fn a_name_cant_climb_out_of_its_folder() {
        assert!(line_problems(|f| f.agents[0].id = "../evil".into()).contains("needs a name"));
    }
}
