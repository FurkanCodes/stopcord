//! Settings that belong to the machine rather than to one factory, and the
//! secrets that integrations need.
//!
//! `~/.sawtooth/settings.yaml` is plain and can be edited by hand. Secrets are one
//! file each in `~/.sawtooth/secrets/`, readable only by this user and denied to
//! every sandbox. The window can set a secret or ask whether it exists, but it
//! can never read one back.

use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct SlackSettings {
    pub enabled: bool,
    /// Secret names, never the tokens themselves.
    pub app_token_secret: String,
    pub bot_token_secret: String,
    /// Slack user ids allowed to send work. Empty means nobody.
    pub allow_users: Vec<String>,
    /// Which factory an @mention feeds.
    pub factory: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Settings {
    pub slack: SlackSettings,
    /// Keep the machine awake while work is running.
    pub keep_awake: bool,
    /// Each connection's plain fields, such as a self-hosted Sentry's address. Secrets are never here.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub connections: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
}

pub fn file(home: &Path) -> PathBuf {
    home.join("settings.yaml")
}

/// Missing means defaults. A file that can't be read is an error the window can show.
pub fn load(home: &Path) -> Result<Settings> {
    let path = file(home);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_yaml_ng::from_str(&text).with_context(|| format!("{} isn't valid", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(e).with_context(|| format!("couldn't read {}", path.display())),
    }
}

pub fn save(home: &Path, settings: &Settings) -> Result<()> {
    std::fs::create_dir_all(home)?;
    let path = file(home);
    let tmp = path.with_extension("tmp-write");
    std::fs::write(&tmp, serde_yaml_ng::to_string(settings)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

fn secrets_dir(home: &Path) -> PathBuf {
    home.join("secrets")
}

fn secret_file(home: &Path, name: &str) -> Result<PathBuf> {
    let safe = !name.is_empty()
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !safe {
        bail!("{name:?} can't be a secret's name. Use letters, digits, dashes and underscores.");
    }
    Ok(secrets_dir(home).join(name))
}

/// Stores a secret. An empty value removes it.
pub fn set_secret(home: &Path, name: &str, value: &str) -> Result<()> {
    let path = secret_file(home, name)?;
    if value.is_empty() {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => return Ok(()),
        }
    }
    let dir = secrets_dir(home);
    std::fs::create_dir_all(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    let _ = std::fs::remove_file(&path);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path)?;
    f.write_all(value.as_bytes())?;
    Ok(())
}

/// Where a ticket source's key is kept: per factory and source, so two factories can each have
/// their own SENTRY_PROJECT.
pub fn source_key(factory: &str, source: &str, name: &str) -> String {
    format!("source.{factory}.{source}.{name}")
}

/// Where a VM sandbox's secret is kept: per factory and sandbox. A bare name would be shared with
/// every other sandbox, and with Sawtooth's own secrets such as the Slack tokens, so an untrusted
/// sandbox could ask for those by name.
pub fn vm_key(factory: &str, runner: &str, name: &str) -> String {
    format!("vm.{factory}.{runner}.{name}")
}

/// Where a report's key is kept: per factory and report.
pub fn report_key(factory: &str, report: &str, name: &str) -> String {
    format!("report.{factory}.{report}.{name}")
}

/// Keeps a report's keys when it's renamed, since they're kept under its name. Reports are matched
/// by place, so keys move only when the list kept its length, the old name is gone and the new one
/// is new: adding, removing or reordering reports moves nothing.
pub fn move_report_keys(home: &Path, factory: &str, before: &[crate::factories::Report], after: &[crate::factories::Report]) -> Result<()> {
    if before.len() != after.len() {
        return Ok(());
    }
    for (old, new) in before.iter().zip(after) {
        if old.id == new.id || after.iter().any(|r| r.id == old.id) || before.iter().any(|r| r.id == new.id) {
            continue;
        }
        for name in old.keys.iter().chain(&new.keys) {
            if let Some(value) = get_secret(home, &report_key(factory, &old.id, name)) {
                set_secret(home, &report_key(factory, &new.id, name), &value)?;
                set_secret(home, &report_key(factory, &old.id, name), "")?;
            }
        }
    }
    Ok(())
}

/// What a report's command gets in its environment: each of its keys that has a value.
pub fn report_env(home: &Path, factory: &str, report: &str, names: &[String]) -> Vec<(String, String)> {
    names.iter().filter_map(|n| get_secret(home, &report_key(factory, report, n)).map(|v| (n.clone(), v))).collect()
}

/// What a source's command gets in its environment: each of its keys that has a value.
pub fn source_env(home: &Path, factory: &str, source: &str, names: &[String]) -> Vec<(String, String)> {
    names.iter().filter_map(|n| get_secret(home, &source_key(factory, source, n)).map(|v| (n.clone(), v))).collect()
}

pub fn get_secret(home: &Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(secret_file(home, name).ok()?).ok()?;
    let text = text.trim_end_matches(['\n', '\r']).to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_renamed_report_keeps_its_keys_and_others_keep_theirs() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let report = |id: &str| crate::factories::Report { id: id.into(), keys: vec!["HOOK".into()], ..Default::default() };
        set_secret(h, &report_key("f", "a", "HOOK"), "one").unwrap();
        set_secret(h, &report_key("f", "b", "HOOK"), "two").unwrap();

        move_report_keys(h, "f", &[report("a"), report("b")], &[report("alerts"), report("b")]).unwrap();
        assert_eq!(get_secret(h, &report_key("f", "alerts", "HOOK")).as_deref(), Some("one"));
        assert_eq!(get_secret(h, &report_key("f", "a", "HOOK")), None);
        assert_eq!(get_secret(h, &report_key("f", "b", "HOOK")).as_deref(), Some("two"));

        // Removing one, or swapping two, moves nothing.
        move_report_keys(h, "f", &[report("alerts"), report("b")], &[report("b")]).unwrap();
        move_report_keys(h, "f", &[report("alerts"), report("b")], &[report("b"), report("alerts")]).unwrap();
        assert_eq!(get_secret(h, &report_key("f", "alerts", "HOOK")).as_deref(), Some("one"));
        assert_eq!(get_secret(h, &report_key("f", "b", "HOOK")).as_deref(), Some("two"));
    }

    #[test]
    fn nothing_public_is_on_until_someone_turns_it_on() {
        let home = tempfile::tempdir().unwrap();
        let s = load(home.path()).unwrap();
        assert!(!s.slack.enabled);
        assert!(s.slack.allow_users.is_empty());
    }

    #[test]
    fn settings_come_back_the_same() {
        let home = tempfile::tempdir().unwrap();
        let s = Settings::default();
        save(home.path(), &s).unwrap();
        assert_eq!(load(home.path()).unwrap(), s);
    }

    #[test]
    fn a_bad_settings_file_says_so() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(file(home.path()), "tunnels: {}\n").unwrap();
        assert!(format!("{:#}", load(home.path()).unwrap_err()).contains("isn't valid"));
    }

    #[test]
    fn a_secret_is_private_and_can_be_removed() {
        let home = tempfile::tempdir().unwrap();
        set_secret(home.path(), "SENTRY_HOOK", "s3cret").unwrap();
        assert_eq!(get_secret(home.path(), "SENTRY_HOOK").as_deref(), Some("s3cret"));
        let path = secret_file(home.path(), "SENTRY_HOOK").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        set_secret(home.path(), "SENTRY_HOOK", "").unwrap();
        assert!(get_secret(home.path(), "SENTRY_HOOK").is_none());
    }

    #[test]
    fn a_sources_keys_are_its_own() {
        let home = tempfile::tempdir().unwrap();
        set_secret(home.path(), &source_key("web", "sentry", "SENTRY_PROJECT"), "web").unwrap();
        set_secret(home.path(), &source_key("api", "sentry", "SENTRY_PROJECT"), "api").unwrap();
        let names = vec!["SENTRY_PROJECT".to_string(), "SENTRY_URL".to_string()];
        assert_eq!(source_env(home.path(), "web", "sentry", &names), vec![("SENTRY_PROJECT".to_string(), "web".to_string())], "a key with no value is left out");
        assert_eq!(source_env(home.path(), "api", "sentry", &names)[0].1, "api");
    }

    #[test]
    fn a_secret_name_cant_climb_out_of_the_folder() {
        let home = tempfile::tempdir().unwrap();
        assert!(set_secret(home.path(), "../evil", "x").is_err());
        assert!(get_secret(home.path(), "../evil").is_none());
    }
}
