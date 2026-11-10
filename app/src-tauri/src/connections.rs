//! Connections: the outside services a person signs in to once, for every factory on this
//! machine. A factory's source or report names a connection and says only what's its own, such
//! as which Sentry project. Secret fields live in Sawtooth's secrets, plain ones in settings.yaml.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::settings;

/// One value a connection needs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    /// The environment variable a command gets it as.
    pub name: &'static str,
    pub label: &'static str,
    /// Kept in Sawtooth's secrets and never shown again, or kept plain in settings.yaml.
    pub secret: bool,
    pub required: bool,
    pub placeholder: &'static str,
    pub hint: &'static str,
}

/// A kind of service.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: &'static str,
    pub name: &'static str,
    pub fields: &'static [Field],
    /// Where the person gets what the fields ask for.
    pub how: &'static str,
    pub link: &'static str,
}

const fn secret(name: &'static str, label: &'static str, placeholder: &'static str, hint: &'static str) -> Field {
    Field { name, label, secret: true, required: true, placeholder, hint }
}

pub const SLACK: &str = "slack";

pub fn all() -> &'static [Connection] {
    const ALL: &[Connection] = &[
        Connection {
            id: SLACK,
            name: "Slack",
            fields: &[
                secret("SLACK_BOT_TOKEN", "bot token", "xoxb-…", "OAuth & Permissions, after installing the app"),
                secret("SLACK_APP_TOKEN", "app token", "xapp-…", "Basic Information → App-Level Tokens, with connections:write"),
            ],
            how: "create the app from the manifest below, install it to your workspace, then paste its two tokens",
            link: "https://api.slack.com/apps?new_app=1",
        },
        Connection {
            id: "sentry",
            name: "Sentry",
            fields: &[
                secret("SENTRY_AUTH_TOKEN", "auth token", "sntryu_…", "a user auth token with event:read, project:read and org:read"),
                Field { name: "SENTRY_URL", label: "address", secret: false, required: false, placeholder: "https://sentry.io", hint: "only for self-hosted Sentry" },
            ],
            how: "make a user auth token in Sentry's settings",
            link: "https://sentry.io/settings/account/api/auth-tokens/",
        },
        Connection {
            id: "linear",
            name: "Linear",
            fields: &[secret("LINEAR_API_KEY", "api key", "lin_api_…", "a personal API key")],
            how: "make a personal API key in Linear's settings",
            link: "https://linear.app/settings/account/security",
        },
        Connection {
            id: "github",
            name: "GitHub",
            fields: &[],
            how: "sign in once with gh auth login in a terminal. Sawtooth uses gh as you",
            link: "https://cli.github.com",
        },
    ];
    ALL
}

pub fn find(id: &str) -> Option<&'static Connection> {
    all().iter().find(|c| c.id == id)
}

/// Where a connection's secret is kept.
pub fn secret_key(connection: &str, name: &str) -> String {
    format!("connection.{connection}.{name}")
}

/// A field's value, when it has one. Slack's tokens were kept under their own names before
/// connections existed, and those still count.
pub fn value(home: &Path, connection: &str, field: &Field) -> Option<String> {
    if field.secret {
        settings::get_secret(home, &secret_key(connection, field.name)).or_else(|| {
            let old = settings::load(home).ok()?.slack;
            let legacy = match field.name {
                "SLACK_BOT_TOKEN" if connection == SLACK => old.bot_token_secret,
                "SLACK_APP_TOKEN" if connection == SLACK => old.app_token_secret,
                _ => return None,
            };
            (!legacy.is_empty()).then(|| settings::get_secret(home, &legacy)).flatten()
        })
    } else {
        settings::load(home).ok()?.connections.get(connection)?.get(field.name).filter(|v| !v.trim().is_empty()).cloned()
    }
}

/// Stores a field. An empty value clears it.
pub fn set(home: &Path, connection: &str, name: &str, value: &str) -> Result<(), String> {
    let c = find(connection).ok_or_else(|| format!("there's no connection called {connection}"))?;
    let field = c.fields.iter().find(|f| f.name == name).ok_or_else(|| format!("{} has no field {name}", c.name))?;
    let value = value.trim();
    if field.secret {
        return settings::set_secret(home, &secret_key(connection, name), value).map_err(|e| format!("{e:#}"));
    }
    let mut s = settings::load(home).map_err(|e| format!("{e:#}"))?;
    let fields = s.connections.entry(connection.to_string()).or_default();
    if value.is_empty() {
        fields.remove(name);
    } else {
        fields.insert(name.to_string(), value.to_string());
    }
    s.connections.retain(|_, f| !f.is_empty());
    settings::save(home, &s).map_err(|e| format!("{e:#}"))
}

/// What a command using the connection gets in its environment.
pub fn env(home: &Path, connection: &str) -> Vec<(String, String)> {
    let Some(c) = find(connection) else { return Vec::new() };
    c.fields.iter().filter_map(|f| value(home, connection, f).map(|v| (f.name.to_string(), v))).collect()
}

/// The required fields that have no value yet.
pub fn missing(home: &Path, connection: &str) -> Vec<&'static Field> {
    find(connection).map_or_else(Vec::new, |c| c.fields.iter().filter(|f| f.required && value(home, connection, f).is_none()).collect())
}

/// What the window shows for one connection: the plain values, and which secrets are set.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    #[serde(flatten)]
    pub connection: Connection,
    /// Plain fields' values.
    pub values: BTreeMap<String, String>,
    /// Secret fields that have a value. Never the values.
    pub set: Vec<String>,
    /// Required fields with no value.
    pub missing: Vec<String>,
}

pub fn views(home: &Path) -> Vec<View> {
    all()
        .iter()
        .map(|c| {
            let mut values = BTreeMap::new();
            let mut set = Vec::new();
            for f in c.fields {
                match (f.secret, value(home, c.id, f)) {
                    (true, Some(_)) => set.push(f.name.to_string()),
                    (false, Some(v)) => {
                        values.insert(f.name.to_string(), v);
                    }
                    _ => {}
                }
            }
            View { connection: c.clone(), values, set, missing: missing(home, c.id).iter().map(|f| f.name.to_string()).collect() }
        })
        .collect()
}

/// A sentence saying what's missing, or nothing when the connection is ready to use.
pub fn not_ready(home: &Path, connection: &str) -> Option<String> {
    let c = find(connection)?;
    let gone = missing(home, connection);
    (!gone.is_empty()).then(|| {
        let names: Vec<&str> = gone.iter().map(|f| f.label).collect();
        format!("{} isn't connected: set its {} in settings → connections", c.name, names.join(" and "))
    })
}

fn http_error(service: &str, e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let said = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v["detail"].as_str().or(v["error"].as_str()).or(v["errors"][0]["message"].as_str()).map(str::to_string))
                .unwrap_or_else(|| body.chars().take(160).collect());
            match code {
                401 | 403 => format!("{service} refused the token ({code}): {said}"),
                _ => format!("{service} answered {code}: {said}"),
            }
        }
        ureq::Error::Transport(t) => format!("couldn't reach {service}: {t}"),
    }
}

/// Tries the connection for real and says what it found: who it signs in as, or why it can't.
pub fn test(home: &Path, connection: &str) -> Result<String, String> {
    // Said on the settings screen itself, so it names the fields and not where they are.
    let gone = missing(home, connection);
    if !gone.is_empty() {
        let names: Vec<&str> = gone.iter().map(|f| f.label).collect();
        return Err(format!("not set up yet: paste its {} first", names.join(" and ")));
    }
    let get = |name: &str| env(home, connection).into_iter().find(|(k, _)| k == name).map(|(_, v)| v).unwrap_or_default();
    let timeout = Duration::from_secs(15);
    match connection {
        SLACK => {
            let me = crate::slack::call("auth.test", &get("SLACK_BOT_TOKEN"), &[]).map_err(|e| format!("the bot token didn't work: {e:#}"))?;
            crate::slack::call("apps.connections.open", &get("SLACK_APP_TOKEN"), &[]).map_err(|e| format!("the bot token works, but the app token didn't: {e:#}"))?;
            Ok(format!("signed in as @{} in {}", me["user"].as_str().unwrap_or("?"), me["team"].as_str().unwrap_or("your workspace")))
        }
        "sentry" => {
            let base = env(home, connection).into_iter().find(|(k, _)| k == "SENTRY_URL").map_or_else(|| "https://sentry.io".to_string(), |(_, v)| v.trim_end_matches('/').to_string());
            let orgs: Value = ureq::get(&format!("{base}/api/0/organizations/"))
                .set("Authorization", &format!("Bearer {}", get("SENTRY_AUTH_TOKEN")))
                .timeout(timeout)
                .call()
                .map_err(|e| http_error("Sentry", e))?
                .into_json()
                .map_err(|e| format!("Sentry's answer wasn't JSON: {e}"))?;
            let slugs: Vec<&str> = orgs.as_array().into_iter().flatten().filter_map(|o| o["slug"].as_str()).collect();
            Ok(match slugs.as_slice() {
                [] => "the token works, but it sees no organizations".to_string(),
                s => format!("the token works and sees {}", s.join(", ")),
            })
        }
        "linear" => {
            let v: Value = ureq::post("https://api.linear.app/graphql")
                .set("Authorization", &get("LINEAR_API_KEY"))
                .timeout(timeout)
                .send_json(serde_json::json!({ "query": "{ viewer { name email } }" }))
                .map_err(|e| http_error("Linear", e))?
                .into_json()
                .map_err(|e| format!("Linear's answer wasn't JSON: {e}"))?;
            match v["data"]["viewer"]["name"].as_str() {
                Some(name) => Ok(format!("signed in as {name}")),
                None => Err(format!("Linear said no: {}", v["errors"][0]["message"].as_str().unwrap_or("no viewer came back"))),
            }
        }
        "github" => {
            let out = crate::tickets::run_in_login_shell("gh api user --jq .login", home, &[], Duration::from_secs(20)).map_err(|e| format!("{e:#}").replace("the pull command failed", "gh failed"))?;
            Ok(format!("signed in to gh as {}", out.trim()))
        }
        _ => Err(format!("there's no connection called {connection}")),
    }
}

/// A Slack app manifest with everything Sawtooth needs, so creating the app is one paste.
pub fn slack_manifest() -> String {
    let manifest = serde_json::json!({
        "display_information": { "name": "Sawtooth", "description": "Turns @mentions into work for your factories" },
        "features": { "bot_user": { "display_name": "sawtooth", "always_online": true } },
        "oauth_config": { "scopes": { "bot": ["app_mentions:read", "chat:write", "channels:read", "groups:read", "channels:history", "groups:history"] } },
        "settings": {
            "event_subscriptions": { "bot_events": ["app_mention"] },
            "interactivity": { "is_enabled": false },
            "org_deploy_enabled": false,
            "socket_mode_enabled": true,
            "token_rotation_enabled": false
        }
    });
    serde_json::to_string_pretty(&manifest).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connection_gives_its_values_to_commands_and_says_what_is_missing() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        assert!(not_ready(h, "sentry").unwrap().contains("auth token"), "nothing set yet");
        set(h, "sentry", "SENTRY_AUTH_TOKEN", "tok").unwrap();
        assert!(not_ready(h, "sentry").is_none(), "the address is optional");
        set(h, "sentry", "SENTRY_URL", " https://sentry.example ").unwrap();
        assert_eq!(env(h, "sentry"), vec![("SENTRY_AUTH_TOKEN".into(), "tok".into()), ("SENTRY_URL".into(), "https://sentry.example".into())]);
        let text = std::fs::read_to_string(settings::file(h)).unwrap();
        assert!(text.contains("sentry.example") && !text.contains("tok"), "plain values in settings, secrets not: {text}");
        let v = views(h).into_iter().find(|v| v.connection.id == "sentry").unwrap();
        assert_eq!((v.set, v.values.get("SENTRY_URL").cloned(), v.missing.len()), (vec!["SENTRY_AUTH_TOKEN".to_string()], Some("https://sentry.example".to_string()), 0));
        set(h, "sentry", "SENTRY_URL", "").unwrap();
        assert_eq!(env(h, "sentry").len(), 1, "an empty value clears it");
        assert!(set(h, "sentry", "PATH", "x").is_err(), "only a connection's own fields");
        assert!(set(h, "nope", "X", "x").is_err());
    }

    #[test]
    fn slack_tokens_kept_before_connections_still_count() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let mut s = settings::Settings::default();
        s.slack.bot_token_secret = "SLACK_BOT_TOKEN".into();
        settings::save(h, &s).unwrap();
        settings::set_secret(h, "SLACK_BOT_TOKEN", "old").unwrap();
        assert_eq!(value(h, SLACK, &find(SLACK).unwrap().fields[0]).as_deref(), Some("old"));
        assert_eq!(missing(h, SLACK).len(), 1, "the app token is still missing");
        set(h, SLACK, "SLACK_BOT_TOKEN", "new").unwrap();
        assert_eq!(value(h, SLACK, &find(SLACK).unwrap().fields[0]).as_deref(), Some("new"), "a token set here wins");
    }

    #[test]
    fn a_test_says_what_is_missing_before_calling_anyone() {
        let home = tempfile::tempdir().unwrap();
        assert!(test(home.path(), "linear").unwrap_err().contains("api key"));
        assert!(test(home.path(), SLACK).unwrap_err().contains("bot token and app token"));
    }

    #[test]
    fn the_manifest_asks_for_what_the_code_uses() {
        let m: Value = serde_json::from_str(&slack_manifest()).unwrap();
        assert_eq!(m["settings"]["socket_mode_enabled"], true);
        assert_eq!(m["settings"]["event_subscriptions"]["bot_events"][0], "app_mention");
        let scopes = m["oauth_config"]["scopes"]["bot"].to_string();
        for s in ["app_mentions:read", "chat:write", "channels:history", "groups:history", "channels:read", "groups:read"] {
            assert!(scopes.contains(s), "{s}");
        }
    }
}
