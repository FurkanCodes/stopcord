//! Factory templates people make: a factory's line, roles, sandboxes, skills and checks, without the
//! project. One YAML file each in `~/.stopcord/templates`, so a template can be handed to someone else.
//!
//! A factory made from a template is a copy. Changing the template later changes nothing already made.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::factories::{Agent, Defaults, Factory, Kind, LineStage, Part, Runner, Skill, slug};

pub const SCHEMA: &str = "stopcord/template/v1";
/// What templates were written as before the app was renamed from Sawtooth. Still read.
const OLD_SCHEMA: &str = "sawtooth/template/v1";

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Template {
    pub schema: String,
    pub id: String,
    pub name: String,
    pub description: String,
    pub defaults: Defaults,
    pub line: Vec<LineStage>,
    pub agents: Vec<Agent>,
    pub scorers: Vec<Agent>,
    pub skills: Vec<Skill>,
    pub runners: Vec<Runner>,
    /// The checks its gates run. A check named `tests` takes the new project's own test command.
    pub checks: Vec<Part>,
}

/// Everything about a factory that isn't its project: no folder, tickets, sources, reports or secrets.
pub fn from_factory(f: &Factory, name: &str, description: &str) -> Template {
    let name = if name.trim().is_empty() { f.name.clone() } else { name.trim().to_string() };
    Template {
        schema: SCHEMA.into(),
        id: slug(&name),
        name,
        description: description.trim().to_string(),
        defaults: f.defaults.clone(),
        line: f.line.clone(),
        agents: f.agents.clone(),
        scorers: f.scorers.clone(),
        skills: f.skills.clone(),
        runners: f.runners.clone(),
        checks: f.parts.iter().filter(|p| p.kind == Kind::Check).cloned().collect(),
    }
}

/// Makes `f` work the way the template says. Its own knowledge base and sources stay.
pub fn apply(t: &Template, f: &mut Factory, detected_tests: Option<&str>) {
    f.defaults = t.defaults.clone();
    f.line = t.line.clone();
    f.agents = t.agents.clone();
    f.scorers = t.scorers.clone();
    f.skills = t.skills.clone();
    f.runners = t.runners.clone();
    f.parts.retain(|p| p.kind != Kind::Check);
    for check in &t.checks {
        let mut c = check.clone();
        c.kind = Kind::Check;
        if c.id == "tests"
            && let Some(cmd) = detected_tests
        {
            c.command = cmd.to_string();
        }
        f.parts.push(c);
    }
}

fn dir(home: &Path) -> PathBuf {
    home.join("templates")
}

fn file(home: &Path, id: &str) -> PathBuf {
    dir(home).join(format!("{id}.yaml"))
}

fn read(path: &Path) -> Result<Template> {
    let text = std::fs::read_to_string(path).with_context(|| format!("couldn't read {}", path.display()))?;
    let mut t: Template = serde_yaml_ng::from_str(&text).with_context(|| format!("{} isn't a Stopcord template", path.display()))?;
    if !t.schema.is_empty() && t.schema != SCHEMA && t.schema != OLD_SCHEMA {
        bail!("{} is a {} file, not a Stopcord template", path.display(), t.schema);
    }
    if t.name.trim().is_empty() {
        t.name = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    }
    t.id = slug(&t.name);
    t.schema = SCHEMA.into();
    Ok(t)
}

/// The templates on this machine, by name, and the files that couldn't be read.
pub fn list(home: &Path) -> (Vec<Template>, Vec<String>) {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir(home)) else {
        return (out, problems);
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().is_some_and(|x| x == "yaml") {
            match read(&path) {
                Ok(t) => out.push(t),
                Err(e) => problems.push(format!("{e:#}")),
            }
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    (out, problems)
}

pub fn get(home: &Path, id: &str) -> Result<Template> {
    let path = file(home, &slug(id));
    if !path.exists() {
        bail!("there's no template {id}");
    }
    read(&path)
}

/// Keeps a template. One with the same name is replaced.
pub fn save(home: &Path, t: &Template) -> Result<Template> {
    check(t)?;
    let mut t = t.clone();
    t.schema = SCHEMA.into();
    t.id = slug(&t.name);
    std::fs::create_dir_all(dir(home))?;
    write(&file(home, &t.id), &t)?;
    Ok(t)
}

pub fn remove(home: &Path, id: &str) -> Result<()> {
    let path = file(home, &slug(id));
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Writes a template where someone chose, to hand on.
pub fn export(home: &Path, id: &str, to: &Path) -> Result<()> {
    write(to, &get(home, id)?)
}

/// Brings in a template someone handed over, after checking its line holds together.
pub fn import(home: &Path, from: &Path) -> Result<Template> {
    save(home, &read(from)?)
}

fn write(path: &Path, t: &Template) -> Result<()> {
    let text = format!("# A Stopcord factory template. Open it from New factory, under templates.\n{}", serde_yaml_ng::to_string(t)?);
    std::fs::write(path, text).with_context(|| format!("couldn't write {}", path.display()))
}

/// A template has to make a factory that would save: real stages, agents and checks.
fn check(t: &Template) -> Result<()> {
    if t.name.trim().is_empty() {
        bail!("a template needs a name");
    }
    let mut f = Factory { name: t.name.clone(), ..Factory::default() };
    apply(t, &mut f, None);
    let problems = crate::definition::validate(&f);
    if !problems.is_empty() {
        bail!("the template {} wouldn't make a working factory: {}", t.name, problems.join("; "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::StageKind;

    fn factory() -> Factory {
        let mut f = Factory { id: "shop".into(), name: "web shop".into(), path: "/p/shop".into(), ..Factory::default() };
        f.parts.push(Part { id: "kb".into(), kind: Kind::Knowledge, ..Part::default() });
        f.parts.push(Part { id: "sentry".into(), kind: Kind::Tickets, command: "pull".into(), ..Part::default() });
        f.parts.push(Part { id: "tests".into(), kind: Kind::Check, command: "npm test".into(), ..Part::default() });
        f.agents.push(Agent { id: "builder".into(), harness: "codex --model gpt-5".into(), ..Agent::default() });
        f.line.push(LineStage { id: "build".into(), kind: StageKind::Agent, agent: "builder".into(), then: "verify".into(), ..LineStage::default() });
        f.line.push(LineStage { id: "verify".into(), kind: StageKind::Gate, checks: vec!["tests".into()], ..LineStage::default() });
        f
    }

    #[test]
    fn a_template_keeps_the_way_of_working_and_leaves_the_project_behind() {
        let t = from_factory(&factory(), "Bug fixer", "fixes bugs");
        assert_eq!(t.id, "bug-fixer");
        assert_eq!(t.agents[0].harness, "codex --model gpt-5");
        assert_eq!(t.checks.len(), 1);
        let text = serde_yaml_ng::to_string(&t).unwrap();
        assert!(!text.contains("/p/shop") && !text.contains("sentry"), "{text}");
    }

    #[test]
    fn a_new_factory_takes_the_template_and_its_own_test_command() {
        let t = from_factory(&factory(), "bug fixer", "");
        let mut f = Factory { id: "api".into(), parts: vec![Part { id: "kb".into(), kind: Kind::Knowledge, ..Part::default() }], ..Factory::default() };
        apply(&t, &mut f, Some("cargo test"));
        assert_eq!(f.line.len(), 2);
        assert_eq!(f.parts.iter().find(|p| p.id == "tests").unwrap().command, "cargo test");
        assert!(f.parts.iter().any(|p| p.kind == Kind::Knowledge));
    }

    #[test]
    fn templates_are_kept_shared_and_brought_back() {
        let home = tempfile::tempdir().unwrap();
        let t = save(home.path(), &from_factory(&factory(), "bug fixer", "")).unwrap();
        assert_eq!(list(home.path()).0.len(), 1);
        let out = home.path().join("shared.yaml");
        export(home.path(), &t.id, &out).unwrap();
        remove(home.path(), &t.id).unwrap();
        assert!(list(home.path()).0.is_empty());
        let back = import(home.path(), &out).unwrap();
        assert_eq!(back.line, t.line);
        assert_eq!(get(home.path(), "bug fixer").unwrap().agents, t.agents);
    }

    #[test]
    fn a_template_saved_before_the_rename_is_read_and_brought_up_to_date() {
        let home = tempfile::tempdir().unwrap();
        let t = from_factory(&factory(), "bug fixer", "");
        let old = home.path().join("old.yaml");
        let text = serde_yaml_ng::to_string(&t).unwrap().replace(SCHEMA, OLD_SCHEMA);
        assert!(text.contains(OLD_SCHEMA));
        std::fs::write(&old, text).unwrap();
        assert_eq!(read(&old).unwrap().schema, SCHEMA);
    }

    #[test]
    fn a_template_whose_line_points_nowhere_is_refused() {
        let mut t = from_factory(&factory(), "broken", "");
        t.agents.clear();
        let home = tempfile::tempdir().unwrap();
        assert!(save(home.path(), &t).unwrap_err().to_string().contains("wouldn't make a working factory"));
        std::fs::write(home.path().join("other.yaml"), "schema: something/else\n").unwrap();
        assert!(import(home.path(), &home.path().join("other.yaml")).is_err());
    }
}
