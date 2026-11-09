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

pub const DEFAULT_WEBHOOK_PORT: u16 = 38715;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TunnelKind {
    /// You run the tunnel yourself, or there isn't one.
    #[default]
    None,
    /// A temporary address from Cloudflare. No account, and it changes each time.
    CloudflaredQuick,
    /// Your own Cloudflare tunnel, using a token.
    CloudflaredNamed,
    TailscaleFunnel,
    Ngrok,
    /// Any command. `{port}` is the listener's port.
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct WebhookSettings {
    /// Off by default: a tunnel puts a public address on the machine that holds your logins.
    pub enabled: bool,
    pub port: u16,
}

impl Default for WebhookSettings {
    fn default() -> Self {
        WebhookSettings {
            enabled: false,
            port: DEFAULT_WEBHOOK_PORT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct TunnelSettings {
    pub kind: TunnelKind,
    /// `Custom`: the command to run.
    pub command: String,
    /// The public address, when the tunnel can't tell Sawtooth what it is.
    pub public_url: String,
    /// `CloudflaredNamed`: the name of the secret holding its token.
    pub token_secret: String,
    /// Start the tunnel when Sawtooth starts.
    pub auto_start: bool,
}

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
    pub webhooks: WebhookSettings,
    pub tunnel: TunnelSettings,
    pub slack: SlackSettings,
    /// Keep the machine awake while work is running.
    pub keep_awake: bool,
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

pub fn get_secret(home: &Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(secret_file(home, name).ok()?).ok()?;
    let text = text.trim_end_matches(['\n', '\r']).to_string();
    (!text.is_empty()).then_some(text)
}

pub fn has_secret(home: &Path, name: &str) -> bool {
    get_secret(home, name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_public_is_on_until_someone_turns_it_on() {
        let home = tempfile::tempdir().unwrap();
        let s = load(home.path()).unwrap();
        assert!(!s.webhooks.enabled);
        assert_eq!(s.tunnel.kind, TunnelKind::None);
        assert!(!s.slack.enabled);
        assert!(s.slack.allow_users.is_empty());
    }

    #[test]
    fn settings_come_back_the_same() {
        let home = tempfile::tempdir().unwrap();
        let mut s = Settings::default();
        s.webhooks.enabled = true;
        s.tunnel.kind = TunnelKind::TailscaleFunnel;
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
        assert!(!has_secret(home.path(), "SENTRY_HOOK"));
    }

    #[test]
    fn a_secret_name_cant_climb_out_of_the_folder() {
        let home = tempfile::tempdir().unwrap();
        assert!(set_secret(home.path(), "../evil", "x").is_err());
        assert!(!has_secret(home.path(), "../evil"));
    }
}
