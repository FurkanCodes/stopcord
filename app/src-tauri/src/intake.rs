//! Work coming in from outside: sources pulled on a timer, webhook deliveries,
//! the tunnel, and keeping the machine awake.
//!
//! Outside text never starts an agent by itself. Whatever arrives becomes a
//! proposal for a person, unless they turned that off for that one source.

use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::engine::{Due, Engine, due, parse_schedule};
use crate::factories::{Factory, Kind, Part, Stage, Ticket, Waiting};
use crate::runs::{self, Run};
use crate::settings::{self, Settings, TunnelKind};
use crate::tickets;
use crate::webhook::{self, Listener, Request};
use crate::work;

/// A burst of deliveries becomes one pull.
const MIN_PULL_GAP: Duration = Duration::from_secs(5);

/// What the engine keeps for intake.
#[derive(Default)]
pub struct Intake {
    pub listener: Option<Listener>,
    /// The port the listener is on, for anything that needs to keep agents away from it.
    pub port: u16,
    pub problem: String,
    awake: Option<Child>,
    pulls: std::collections::HashMap<(String, String), Instant>,
    pulling: std::collections::HashSet<(String, String)>,
}

fn field<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| v.get(*k).and_then(Value::as_str)).filter(|s| !s.is_empty())
}

/// Reads a ticket straight out of a delivery, for a source with no pull command.
pub fn ticket_from_payload(scheme: &str, body: &[u8]) -> Option<Ticket> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let make = |key: String, title: &str, text: &str, url: &str| Ticket {
        key,
        title: title.to_string(),
        body: text.to_string(),
        url: url.to_string(),
    };
    match scheme {
        "sentry" => {
            let data = v.get("data")?;
            let issue = data.get("issue").or_else(|| data.get("event"))?;
            let key = field(issue, &["shortId", "short_id", "id", "event_id"]).map(String::from).or_else(|| {
                issue.get("id").and_then(Value::as_u64).map(|n| n.to_string())
            })?;
            Some(make(
                key,
                field(issue, &["title", "message"]).unwrap_or("Sentry issue"),
                field(issue, &["culprit"]).unwrap_or_default(),
                field(issue, &["web_url", "permalink", "url"]).unwrap_or_default(),
            ))
        }
        "github" => {
            let item = v.get("issue").or_else(|| v.get("pull_request"))?;
            let number = item.get("number").and_then(Value::as_u64)?;
            Some(make(
                format!("#{number}"),
                field(item, &["title"]).unwrap_or_default(),
                field(item, &["body"]).unwrap_or_default(),
                field(item, &["html_url"]).unwrap_or_default(),
            ))
        }
        "linear" => {
            let data = v.get("data")?;
            Some(make(
                field(data, &["identifier", "id"])?.to_string(),
                field(data, &["title"]).unwrap_or_default(),
                field(data, &["description"]).unwrap_or_default(),
                field(data, &["url"]).or(field(&v, &["url"])).unwrap_or_default(),
            ))
        }
        _ => {
            let key = field(&v, &["key", "id", "identifier"])?;
            Some(make(
                key.to_string(),
                field(&v, &["title", "message", "summary"]).unwrap_or_default(),
                field(&v, &["body", "description", "text"]).unwrap_or_default(),
                field(&v, &["url", "link"]).unwrap_or_default(),
            ))
        }
    }
}

impl Engine {
    /// Applies the machine's settings: turns the listener on or off, and starts the tunnel if asked.
    pub fn apply_settings(self: &Arc<Self>) {
        let settings = settings::load(self.store.home()).unwrap_or_default();
        let mut intake = self.intake.lock().unwrap();
        intake.problem.clear();
        let wanted = settings.webhooks.enabled.then_some(settings.webhooks.port);
        let current = intake.listener.as_ref().map(Listener::port);
        if wanted != current {
            intake.listener = None;
            intake.port = 0;
            if let Some(port) = wanted {
                let me = Arc::clone(self);
                match Listener::start(port, Arc::new(move |req: &Request| me.on_delivery(req))) {
                    Ok(l) => {
                        intake.port = l.port();
                        intake.listener = Some(l);
                    }
                    Err(e) => intake.problem = format!("{e:#}"),
                }
            }
        }
        let port = intake.port;
        drop(intake);
        if settings.tunnel.auto_start && settings.tunnel.kind != TunnelKind::None && port != 0 && !self.tunnel.status().running {
            let _ = self.start_tunnel(&settings);
        }
    }

    pub fn start_tunnel(self: &Arc<Self>, settings: &Settings) -> Result<(), String> {
        let port = self.intake.lock().unwrap().port;
        if port == 0 {
            return Err("Turn webhooks on first, so the tunnel has something to reach.".to_string());
        }
        let command = crate::tunnel::command_for(&settings.tunnel, port, self.store.home()).map_err(|e| format!("{e:#}"))?;
        self.tunnel.start(&command, &settings.tunnel.public_url).map_err(|e| format!("{e:#}"))
    }

    fn on_delivery(self: &Arc<Self>, req: &Request) -> u16 {
        let Some((factory, source)) = webhook::route(&req.path) else {
            return 404;
        };
        let Ok(f) = self.store.get(factory) else { return 404 };
        let Some(part) = f.parts.iter().find(|p| p.kind == Kind::Tickets && p.id == source && !p.webhook.is_empty()).cloned() else {
            return 404;
        };
        let secret = settings::get_secret(self.store.home(), &part.secret).unwrap_or_default();
        if !webhook::verify(&part.webhook, &secret, req) {
            self.log(&f.id, Run::new("webhook", format!("{} refused a delivery with a bad signature", part.name)).ok(false).part(&part.id));
            return 401;
        }
        self.log(&f.id, Run::new("webhook", format!("{} got a delivery", part.name)).ok(true).part(&part.id));
        if part.command.trim().is_empty() {
            if let Some(ticket) = ticket_from_payload(&part.webhook, &req.body) {
                self.propose(&f.id, &part, vec![ticket]);
            }
        } else {
            self.pull_soon(&f.id, &part.id);
        }
        202
    }

    /// Asks for a pull, unless one is running or just ran.
    fn pull_soon(self: &Arc<Self>, factory: &str, part: &str) {
        let key = (factory.to_string(), part.to_string());
        {
            let mut intake = self.intake.lock().unwrap();
            let recent = intake.pulls.get(&key).is_some_and(|t| t.elapsed() < MIN_PULL_GAP);
            if recent || !intake.pulling.insert(key.clone()) {
                return;
            }
            intake.pulls.insert(key.clone(), Instant::now());
        }
        let me = Arc::clone(self);
        std::thread::spawn(move || {
            let _ = me.pull(&key.0, &key.1);
            me.intake.lock().unwrap().pulling.remove(&key);
        });
    }

    /// Runs a source's command now. Returns how many proposals were new.
    pub fn pull(self: &Arc<Self>, factory: &str, part_id: &str) -> Result<usize, String> {
        let f = self.store.get(factory).map_err(|e| format!("{e:#}"))?;
        let part = f
            .parts
            .iter()
            .find(|p| p.kind == Kind::Tickets && p.id == part_id)
            .cloned()
            .ok_or_else(|| format!("there's no source {part_id}"))?;
        match tickets::run_part(&f, &part) {
            Ok(tickets) => {
                self.clear_alert(factory, &format!("pull:{}", part.id));
                Ok(self.propose(factory, &part, tickets))
            }
            Err(e) => {
                let text = format!("{} couldn't pull: {e:#}", part.name);
                self.store
                    .update(factory, |fa| work::raise(fa, &format!("pull:{}", part.id), &text, None, runs::now_ms()))
                    .ok();
                self.log(factory, Run::new("source", text.clone()).ok(false).part(&part.id));
                Err(text)
            }
        }
    }

    fn propose(self: &Arc<Self>, factory: &str, part: &Part, tickets: Vec<Ticket>) -> usize {
        let added = self
            .store
            .update(factory, |f| work::propose(f, &part.id, tickets, part.auto_start))
            .unwrap_or(0);
        if added > 0 {
            self.log(
                factory,
                Run::new("source", format!("{} brought in {added} new item{}", part.name, if added == 1 { "" } else { "s" }))
                    .ok(true)
                    .part(&part.id),
            );
        }
        added
    }

    /// Pulls every source that's due. A laptop that slept catches up on waking.
    pub(crate) fn pull_due(self: &Arc<Self>, f: &Factory) {
        let now_ms = runs::now_ms();
        for part in f.parts.iter().filter(|p| p.kind == Kind::Tickets && !p.every.trim().is_empty()) {
            let Some(schedule) = parse_schedule(&part.every) else { continue };
            let key = format!("source:{}", part.id);
            let Some(last) = f.fired.get(&key).copied() else {
                let _ = self.store.update(&f.id, |fa| {
                    fa.fired.entry(key.clone()).or_insert(now_ms);
                });
                continue;
            };
            if due(schedule, crate::engine::local(last), crate::engine::local(now_ms)) == Due::No {
                continue;
            }
            if self.store.update(&f.id, |fa| fa.fired.insert(key.clone(), now_ms)).is_ok() {
                self.pull_soon(&f.id, &part.id);
            }
        }
    }

    /// Holds the machine awake while work is running, if the person asked for that.
    pub(crate) fn keep_awake(&self, factories: &[Factory]) {
        let wanted = settings::load(self.store.home()).map(|s| s.keep_awake).unwrap_or(false)
            && factories
                .iter()
                .any(|f| f.work.iter().any(|w| w.stage == Stage::Running && w.waiting == Waiting::No));
        let mut intake = self.intake.lock().unwrap();
        match (&mut intake.awake, wanted) {
            (None, true) => {
                let mut cmd = if cfg!(target_os = "macos") {
                    // -s only applies on mains power, so a laptop on battery still sleeps.
                    let mut c = Command::new("caffeinate");
                    c.args(["-i", "-s"]);
                    c
                } else {
                    let mut c = Command::new("systemd-inhibit");
                    c.args(["--what=idle:sleep", "--who=Sawtooth", "--why=A factory is working", "sleep", "infinity"]);
                    c
                };
                intake.awake = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok();
            }
            (Some(child), false) => {
                let _ = child.kill();
                let _ = child.wait();
                intake.awake = None;
            }
            _ => {}
        }
    }

    pub fn hook_url(&self, factory: &str, part: &str) -> String {
        let public = self.tunnel.status().url;
        let settings = settings::load(self.store.home()).unwrap_or_default();
        let base = if !public.is_empty() {
            public
        } else if !settings.tunnel.public_url.is_empty() {
            settings.tunnel.public_url
        } else {
            format!("http://127.0.0.1:{}", settings.webhooks.port)
        };
        format!("{}/hook/{factory}/{part}", base.trim_end_matches('/'))
    }

    pub fn intake_problem(&self) -> String {
        self.intake.lock().unwrap().problem.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sentry_delivery_becomes_a_ticket() {
        let body = br#"{"action":"created","data":{"issue":{"id":"7","shortId":"PAY-41","title":"TypeError","culprit":"pay.js","web_url":"https://sentry.io/x"}}}"#;
        let t = ticket_from_payload("sentry", body).unwrap();
        assert_eq!(t.key, "PAY-41");
        assert_eq!(t.title, "TypeError");
        assert_eq!(t.url, "https://sentry.io/x");
    }

    #[test]
    fn a_github_issue_becomes_a_ticket() {
        let body = br#"{"action":"opened","issue":{"number":12,"title":"Crash","body":"it broke","html_url":"https://github.com/a/b/issues/12"}}"#;
        let t = ticket_from_payload("github", body).unwrap();
        assert_eq!(t.key, "#12");
        assert_eq!(t.body, "it broke");
    }

    #[test]
    fn a_linear_issue_becomes_a_ticket() {
        let body = br#"{"data":{"identifier":"LIN-9","title":"Bug","description":"d"},"url":"https://linear.app/x"}"#;
        let t = ticket_from_payload("linear", body).unwrap();
        assert_eq!(t.key, "LIN-9");
        assert_eq!(t.url, "https://linear.app/x");
    }

    #[test]
    fn anything_else_needs_an_id_or_is_ignored() {
        assert_eq!(ticket_from_payload("hmac", br#"{"id":"a","title":"T"}"#).unwrap().key, "a");
        assert!(ticket_from_payload("hmac", br#"{"title":"no id"}"#).is_none());
        assert!(ticket_from_payload("sentry", b"not json").is_none());
    }
}
