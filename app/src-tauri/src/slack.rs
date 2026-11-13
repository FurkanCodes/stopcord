//! Slack in, over an outbound socket, so a desktop app with no public address can still hear it.
//!
//! An @mention from someone on the allow list becomes a proposal. It never starts an agent,
//! and nothing from anyone else gets past the first check.

use std::io::{ErrorKind, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::engine::Engine;
use crate::factories::{Kind, Part, Ticket};
use crate::runs::{self, Run};
use crate::connections;
use crate::settings::SlackSettings;

const API: &str = "https://slack.com/api";

/// Someone who @mentioned the bot but isn't allowed to send work yet. Kept so the person running
/// Stopcord can allow them with one press, instead of looking up a member id.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Knock {
    pub user: String,
    /// Their name in Slack, or their id when Slack won't say.
    pub name: String,
    /// What they asked for, so it's clear who they are.
    pub text: String,
    pub at: u64,
}

/// Knocks kept at most, newest first.
const KNOCKS: usize = 20;
/// How far back to look for mentions missed while asleep. Ones already seen are left out.
const CATCH_UP_SECS: u64 = 24 * 60 * 60;
const READ_TICK: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
pub const SOURCE: &str = "slack";

pub(crate) fn call(method: &str, token: &str, form: &[(&str, &str)]) -> Result<Value> {
    let v: Value = ureq::post(&format!("{API}/{method}"))
        .set("Authorization", &format!("Bearer {token}"))
        .timeout(Duration::from_secs(20))
        .send_form(form)?
        .into_json()?;
    if v["ok"] != true {
        bail!("Slack said no to {method}: {}", v["error"].as_str().unwrap_or("unknown error"));
    }
    Ok(v)
}

/// Takes out `<@U123>` mentions, so the text reads as what the person asked.
fn without_mentions(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<@") {
        out.push_str(&rest[..start]);
        match rest[start..].find('>') {
            Some(end) => rest = &rest[start + end + 1..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// A ticket from a mention, or nothing if the sender isn't on the list.
pub fn ticket_from_event(event: &Value, allow: &[String]) -> Option<Ticket> {
    let user = event["user"].as_str()?;
    if event["type"] != "app_mention" || !allow.iter().any(|a| a == user) {
        return None;
    }
    let (ts, channel) = (event["ts"].as_str()?, event["channel"].as_str()?);
    let thread = event["thread_ts"].as_str().unwrap_or(ts);
    let body = without_mentions(event["text"].as_str()?);
    let title = title_of(&body);
    if title.is_empty() {
        return None;
    }
    Some(Ticket {
        key: format!("slack-{}", ts.replace('.', "-")),
        title,
        body,
        url: format!("https://slack.com/archives/{channel}/p{}?thread_ts={thread}", ts.replace('.', "")),
    })
}

fn title_of(body: &str) -> String {
    body.lines().next().unwrap_or_default().chars().take(80).collect()
}

/// Where a mention's work goes.
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// To this factory, with the factory's name taken off the front.
    To { factory: String, body: String },
    /// Nowhere yet: this goes back to the thread.
    Ask(String),
}

/// What's left of `body` after `label` at its start, ignoring case, when the label is a whole word.
fn after_label<'a>(body: &'a str, label: &str) -> Option<&'a str> {
    let n = label.chars().count();
    if n == 0 {
        return None;
    }
    let end = match body.char_indices().nth(n) {
        Some((i, _)) => i,
        None if body.chars().count() == n => body.len(),
        None => return None,
    };
    // Lowercase "İ" is "i" with a combining dot, so it's dropped: "istanbul" finds "İstanbul".
    let fold = |s: &str| s.to_lowercase().replace('\u{307}', "");
    if fold(&body[..end]) != fold(label) {
        return None;
    }
    let rest = &body[end..];
    let apart = |c: char| c.is_whitespace() || c == ':' || c == ',';
    match rest.chars().next() {
        None => Some(""),
        Some(c) if apart(c) => Some(rest.trim_start_matches(apart)),
        Some(_) => None,
    }
}

/// A message that starts with a factory's name or id, as in "stopcord: fix the login", goes to that
/// factory. Anything else goes to `default`. Case doesn't matter, and the longest name that fits
/// wins, so "web app: …" doesn't go to a factory called "web".
pub fn route(body: &str, factories: &[(String, String)], default: &str) -> Route {
    let mut best: Option<(&str, &str, &str, usize)> = None;
    for (id, name) in factories {
        for label in [id.trim(), name.trim()] {
            if let Some(rest) = after_label(body, label)
                && best.is_none_or(|(.., len)| label.chars().count() > len)
            {
                best = Some((id, name, rest, label.chars().count()));
            }
        }
    }
    if let Some((id, name, rest, _)) = best {
        if rest.trim().is_empty() {
            return Route::Ask(format!("What should {name} do? Write it after the factory's name."));
        }
        return Route::To { factory: id.to_string(), body: rest.trim().to_string() };
    }
    if !default.is_empty() && factories.iter().any(|(id, _)| id == default) {
        return Route::To { factory: default.to_string(), body: body.trim().to_string() };
    }
    if factories.is_empty() {
        return Route::Ask("There's no factory to send this to yet.".into());
    }
    let names: Vec<&str> = factories.iter().map(|(_, n)| n.as_str()).collect();
    Route::Ask(format!("Which factory is this for? Start the message with its name, such as \"{}: {}\". The factories are {}.", names[0], title_of(body), names.join(", ")))
}

/// Where to reply, read back from a ticket's link.
pub fn thread_of(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("https://slack.com/archives/")?;
    let (channel, tail) = rest.split_once('/')?;
    let (_, thread) = tail.split_once("?thread_ts=")?;
    Some((channel.to_string(), thread.to_string()))
}

/// Reads one connection until Slack asks to reconnect or `stop` is set. Every envelope is acknowledged.
fn serve<S: Read + Write>(ws: &mut WebSocket<S>, stop: &AtomicBool, on_event: &mut dyn FnMut(&Value)) -> Result<()> {
    while !stop.load(Ordering::Relaxed) {
        let text = match ws.read() {
            Ok(Message::Text(t)) => t,
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => continue,
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => continue,
            Err(e) => return Err(e.into()),
        };
        let v: Value = serde_json::from_str(text.as_str())?;
        if let Some(id) = v["envelope_id"].as_str() {
            ws.send(Message::text(json!({ "envelope_id": id }).to_string()))?;
        }
        match v["type"].as_str() {
            Some("events_api") => on_event(&v["payload"]["event"]),
            Some("disconnect") => return Ok(()),
            _ => {}
        }
    }
    Ok(())
}

impl Engine {
    /// Proposes a mention's work in the factory it names, or the default, and says where in its
    /// thread. A question only goes back for a mention heard live: catching up after a restart
    /// would ask again about every one of them.
    fn slack_proposal(self: &Arc<Self>, s: &SlackSettings, event: &Value, live: bool) {
        if let Some(user) = event["user"].as_str()
            && event["type"] == "app_mention"
            && !s.allow_users.iter().any(|a| a == user)
        {
            if live {
                self.slack_knock(event, user);
            }
            return;
        }
        let Some(ticket) = ticket_from_event(event, &s.allow_users) else { return };
        let factories: Vec<(String, String)> = self.store.list().unwrap_or_default().into_iter().map(|f| (f.id, f.name)).collect();
        match route(&ticket.body, &factories, &s.factory) {
            Route::To { factory, body } => {
                let ticket = Ticket { title: title_of(&body), body, ..ticket };
                let part = Part { id: SOURCE.to_string(), name: "Slack".to_string(), kind: Kind::Tickets, ..Part::default() };
                // Nothing added means it was proposed before, so it was answered before too.
                if self.propose(&factory, &part, vec![ticket.clone()]) > 0 {
                    let name = factories.iter().find(|(id, _)| *id == factory).map_or(factory.as_str(), |(_, n)| n.as_str());
                    self.slack_reply(&ticket.url, &format!("Proposed in {name}. Accept it on its Work screen to start it."));
                }
            }
            Route::Ask(question) if live => self.slack_reply(&ticket.url, &question),
            Route::Ask(_) => {}
        }
    }

    /// Finds mentions from while the socket was down. Slack keeps no queue for a closed socket.
    fn slack_catch_up(self: &Arc<Self>, s: &SlackSettings, bot: &str) -> Result<()> {
        let me = call("auth.test", bot, &[])?;
        let mention = format!("<@{}>", me["user_id"].as_str().unwrap_or_default());
        let oldest = (runs::now_ms() / 1000 - CATCH_UP_SECS).to_string();
        let joined = call("users.conversations", bot, &[("types", "public_channel,private_channel"), ("limit", "200")])?;
        for channel in joined["channels"].as_array().into_iter().flatten().filter_map(|c| c["id"].as_str()) {
            let history = call("conversations.history", bot, &[("channel", channel), ("oldest", &oldest), ("limit", "200")])?;
            for m in history["messages"].as_array().into_iter().flatten() {
                if m["text"].as_str().is_some_and(|t| t.contains(&mention)) {
                    let mut event = m.clone();
                    event["type"] = json!("app_mention");
                    event["channel"] = json!(channel);
                    self.slack_proposal(s, &event, false);
                }
            }
        }
        Ok(())
    }

    fn slack_connection(self: &Arc<Self>, s: &SlackSettings, stop: &AtomicBool) -> Result<()> {
        let home = self.store.home();
        let token = |name: &str| connections::env(home, connections::SLACK).into_iter().find(|(k, _)| k == name).map(|(_, v)| v);
        let app = token("SLACK_APP_TOKEN").ok_or_else(|| anyhow!("the app token isn't set"))?;
        let bot = token("SLACK_BOT_TOKEN").ok_or_else(|| anyhow!("the bot token isn't set"))?;
        let me = call("auth.test", &bot, &[]).map_err(|e| anyhow!("the bot token didn't work: {e:#}"))?;
        let opened = call("apps.connections.open", &app, &[]).map_err(|e| anyhow!("the app token didn't work: {e:#}"))?;
        let url = opened["url"].as_str().ok_or_else(|| anyhow!("Slack didn't give a socket address"))?;
        let (mut ws, _) = tungstenite::connect(url)?;
        match ws.get_mut() {
            MaybeTlsStream::Plain(t) => t.set_read_timeout(Some(READ_TICK))?,
            MaybeTlsStream::Rustls(t) => t.get_mut().set_read_timeout(Some(READ_TICK))?,
            _ => {}
        }
        self.set_slack_state(true, format!("listening as @{} in {}", me["user"].as_str().unwrap_or("?"), me["team"].as_str().unwrap_or("your workspace")));
        // A catch-up that fails, say for a missing scope, shouldn't cost the live connection.
        if let Err(e) = self.slack_catch_up(s, &bot) {
            self.slack_log(s, Run::new("source", format!("Slack couldn't catch up on missed mentions: {e:#}")).ok(false));
        }
        serve(&mut ws, stop, &mut |event| self.slack_proposal(s, event, true))
    }

    pub(crate) fn run_slack(self: Arc<Self>, s: SlackSettings, stop: Arc<AtomicBool>) {
        let mut wait = Duration::from_secs(1);
        let mut told = false;
        while !stop.load(Ordering::Relaxed) {
            match self.slack_connection(&s, &stop) {
                Ok(()) => {
                    wait = Duration::from_secs(1);
                    told = false;
                }
                Err(e) => {
                    self.set_slack_state(false, format!("{e:#}"));
                    if !told {
                        self.slack_log(&s, Run::new("source", format!("Slack isn't connected: {e:#}")).ok(false));
                        told = true;
                    }
                    let mut slept = Duration::ZERO;
                    while slept < wait && !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(READ_TICK.min(wait));
                        slept += READ_TICK.min(wait);
                    }
                    wait = (wait * 2).min(MAX_BACKOFF);
                }
            }
        }
    }

    /// What the window shows about the live connection.
    fn set_slack_state(&self, ok: bool, text: String) {
        self.intake.lock().unwrap().slack_state = Some((ok, text));
    }

    /// Slack's own trouble goes in the default factory's log, or every factory's when there's no
    /// default, since a mention can go to any of them.
    fn slack_log(&self, s: &SlackSettings, run: Run) {
        let factories: Vec<String> = if s.factory.is_empty() { self.store.list().unwrap_or_default().into_iter().map(|f| f.id).collect() } else { vec![s.factory.clone()] };
        for f in factories {
            self.log(&f, run.clone());
        }
    }

    /// Keeps a mention from someone not allowed yet, and tells them once, in its thread, why
    /// nothing happened.
    fn slack_knock(self: &Arc<Self>, event: &Value, user: &str) {
        let (Some(ts), Some(channel)) = (event["ts"].as_str(), event["channel"].as_str()) else { return };
        let text = without_mentions(event["text"].as_str().unwrap_or_default());
        let first = {
            let mut intake = self.intake.lock().unwrap();
            let knocks = &mut intake.slack_knocks;
            let first = !knocks.iter().any(|k| k.user == user);
            knocks.retain(|k| k.user != user);
            knocks.insert(0, Knock { user: user.to_string(), name: user.to_string(), text: title_of(&text), at: runs::now_ms() });
            knocks.truncate(KNOCKS);
            first
        };
        let home = self.store.home().to_path_buf();
        let me = Arc::clone(self);
        let user = user.to_string();
        let thread = event["thread_ts"].as_str().unwrap_or(ts).to_string();
        let url = format!("https://slack.com/archives/{channel}/p{}?thread_ts={thread}", ts.replace('.', ""));
        std::thread::spawn(move || {
            // The name needs users:read. Without it the id stands in, which still works.
            let Some((_, bot)) = connections::env(&home, connections::SLACK).into_iter().find(|(k, _)| k == "SLACK_BOT_TOKEN") else { return };
            if let Ok(v) = call("users.info", &bot, &[("user", &user)]) {
                let name = [&v["user"]["profile"]["real_name"], &v["user"]["real_name"], &v["user"]["name"]].into_iter().find_map(|n| n.as_str().filter(|n| !n.is_empty())).map(str::to_string);
                if let Some(name) = name {
                    for k in me.intake.lock().unwrap().slack_knocks.iter_mut().filter(|k| k.user == user) {
                        k.name = name.clone();
                    }
                }
            }
            if first {
                me.slack_reply(&url, "Stopcord doesn't take work from you yet. Whoever runs it can allow you under integrations, then mention me again.");
            }
        });
    }

    /// The people who asked and aren't allowed yet, newest first.
    pub fn slack_knocks(&self) -> Vec<Knock> {
        self.intake.lock().unwrap().slack_knocks.clone()
    }

    /// Lets a person send work, and forgets their knock.
    pub fn allow_slack_user(self: &Arc<Self>, user: &str) -> Result<()> {
        let home = self.store.home();
        let mut s = crate::settings::load(home)?;
        if !s.slack.allow_users.iter().any(|u| u == user) {
            s.slack.allow_users.push(user.to_string());
        }
        crate::settings::save(home, &s)?;
        self.intake.lock().unwrap().slack_knocks.retain(|k| k.user != user);
        self.apply_settings();
        Ok(())
    }

    /// Tells the Slack thread a piece of work came from what happened to it.
    pub(crate) fn slack_reply(&self, url: &str, text: &str) {
        let Some((channel, thread)) = thread_of(url) else { return };
        let home = self.store.home();
        let Some((_, bot)) = connections::env(home, connections::SLACK).into_iter().find(|(k, _)| k == "SLACK_BOT_TOKEN") else {
            return;
        };
        let text = text.to_string();
        std::thread::spawn(move || {
            let _ = call("chat.postMessage", &bot, &[("channel", &channel), ("thread_ts", &thread), ("text", &text)]);
        });
    }
}

/// Posts to a channel, such as #eng, as the connected bot. Says how to fix the usual mistakes.
pub(crate) fn post(home: &std::path::Path, channel: &str, text: &str) -> std::result::Result<(), String> {
    let Some((_, bot)) = connections::env(home, connections::SLACK).into_iter().find(|(k, _)| k == "SLACK_BOT_TOKEN") else {
        return Err("Slack isn't connected: set its bot token in integrations".into());
    };
    call("chat.postMessage", &bot, &[("channel", channel), ("text", text)]).map(|_| ()).map_err(|e| {
        let e = format!("{e:#}");
        if e.contains("not_in_channel") {
            format!("the bot isn't in {channel}. invite it there with /invite @stopcord")
        } else if e.contains("channel_not_found") {
            format!("Slack has no channel {channel} the bot can see")
        } else {
            e
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn mention(user: &str) -> Value {
        json!({ "type": "app_mention", "user": user, "text": "<@UBOT> fix the login\nit 500s", "ts": "1700.42", "channel": "C1" })
    }

    #[test]
    fn a_socket_delivers_mentions_and_only_listed_people_become_tickets() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let slack = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            ws.send(Message::text(json!({ "type": "hello" }).to_string())).unwrap();
            for (id, user) in [("e1", "UOK"), ("e2", "USTRANGER")] {
                let envelope = json!({ "type": "events_api", "envelope_id": id, "payload": { "event": mention(user) } });
                ws.send(Message::text(envelope.to_string())).unwrap();
                let ack: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(ack["envelope_id"], id);
            }
            ws.send(Message::text(json!({ "type": "disconnect" }).to_string())).unwrap();
        });
        let (mut ws, _) = tungstenite::connect(format!("ws://127.0.0.1:{port}")).unwrap();
        let mut events = Vec::new();
        serve(&mut ws, &AtomicBool::new(false), &mut |e| events.push(e.clone())).unwrap();
        slack.join().unwrap();

        let allow = vec!["UOK".to_string()];
        let tickets: Vec<Ticket> = events.iter().filter_map(|e| ticket_from_event(e, &allow)).collect();
        assert_eq!(tickets.len(), 1, "two arrived, and only one sender is listed");
        assert_eq!(tickets[0].key, "slack-1700-42");
        assert_eq!(tickets[0].title, "fix the login");
        assert_eq!(thread_of(&tickets[0].url), Some(("C1".to_string(), "1700.42".to_string())));
        assert!(ticket_from_event(&mention("UOK"), &[]).is_none(), "an empty list means nobody");
    }

    fn factories() -> Vec<(String, String)> {
        [("stopcord", "Stopcord"), ("web", "Web"), ("web-app", "Web app"), ("turkish-stt", "İstanbul STT")].map(|(i, n)| (i.to_string(), n.to_string())).to_vec()
    }

    fn to(factory: &str, body: &str) -> Route {
        Route::To { factory: factory.into(), body: body.into() }
    }

    #[test]
    fn a_message_picks_its_factory_by_starting_with_its_name_or_id() {
        let f = factories();
        assert_eq!(route("stopcord: fix the login", &f, ""), to("stopcord", "fix the login"));
        assert_eq!(route("Stopcord fix the login", &f, "web"), to("stopcord", "fix the login"), "case and the colon don't matter");
        assert_eq!(route("web-app, add dark mode\nfor settings", &f, ""), to("web-app", "add dark mode\nfor settings"), "by id, keeping later lines");
        assert_eq!(route("web app: add dark mode", &f, ""), to("web-app", "add dark mode"), "the longest name wins");
        assert_eq!(route("web: add dark mode", &f, ""), to("web", "add dark mode"));
        assert_eq!(route("istanbul stt: retrain", &f, ""), to("turkish-stt", "retrain"), "a name whose lowercase is longer still fits");
        assert_eq!(route("website is down", &f, "stopcord"), to("stopcord", "website is down"), "a name has to be a whole word");
    }

    #[test]
    fn a_message_with_no_factory_goes_to_the_default_or_asks() {
        let f = factories();
        assert_eq!(route("fix the login", &f, "web"), to("web", "fix the login"));
        let Route::Ask(q) = route("fix the login", &f, "") else { panic!("it guessed") };
        assert!(q.contains("Which factory") && q.contains("Stopcord, Web, Web app") && q.contains("\"Stopcord: fix the login\""), "{q}");
        assert!(matches!(route("fix the login", &f, "gone"), Route::Ask(_)), "a default that no longer exists isn't used");
        let Route::Ask(q) = route("stopcord", &f, "web") else { panic!("an empty ticket was proposed") };
        assert!(q.contains("What should Stopcord do"), "{q}");
        assert!(matches!(route("fix it", &[], ""), Route::Ask(q) if q.contains("no factory")));
    }
}
