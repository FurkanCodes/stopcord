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
use crate::settings::{self, SlackSettings};

const API: &str = "https://slack.com/api";
/// How far back to look for mentions missed while asleep. Ones already seen are left out.
const CATCH_UP_SECS: u64 = 24 * 60 * 60;
const READ_TICK: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
pub const SOURCE: &str = "slack";

fn call(method: &str, token: &str, form: &[(&str, &str)]) -> Result<Value> {
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
    let title: String = body.lines().next().unwrap_or_default().chars().take(80).collect();
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
    fn slack_proposal(self: &Arc<Self>, s: &SlackSettings, event: &Value) {
        if let Some(ticket) = ticket_from_event(event, &s.allow_users) {
            let part = Part { id: SOURCE.to_string(), name: "Slack".to_string(), kind: Kind::Tickets, ..Part::default() };
            self.propose(&s.factory, &part, vec![ticket]);
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
                    self.slack_proposal(s, &event);
                }
            }
        }
        Ok(())
    }

    fn slack_connection(self: &Arc<Self>, s: &SlackSettings, stop: &AtomicBool) -> Result<()> {
        let home = self.store.home();
        let app = settings::get_secret(home, &s.app_token_secret).ok_or_else(|| anyhow!("the Slack app token isn't set"))?;
        let bot = settings::get_secret(home, &s.bot_token_secret).ok_or_else(|| anyhow!("the Slack bot token isn't set"))?;
        let opened = call("apps.connections.open", &app, &[])?;
        let url = opened["url"].as_str().ok_or_else(|| anyhow!("Slack didn't give a socket address"))?;
        let (mut ws, _) = tungstenite::connect(url)?;
        match ws.get_mut() {
            MaybeTlsStream::Plain(t) => t.set_read_timeout(Some(READ_TICK))?,
            MaybeTlsStream::Rustls(t) => t.get_mut().set_read_timeout(Some(READ_TICK))?,
            _ => {}
        }
        // A catch-up that fails, say for a missing scope, shouldn't cost the live connection.
        if let Err(e) = self.slack_catch_up(s, &bot) {
            self.log(&s.factory, Run::new("source", format!("Slack couldn't catch up on missed mentions: {e:#}")).ok(false));
        }
        serve(&mut ws, stop, &mut |event| self.slack_proposal(s, event))
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
                    if !told {
                        self.log(&s.factory, Run::new("source", format!("Slack isn't connected: {e:#}")).ok(false));
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

    /// Tells the Slack thread a piece of work came from what happened to it.
    pub(crate) fn slack_reply(&self, url: &str, text: &str) {
        let Some((channel, thread)) = thread_of(url) else { return };
        let home = self.store.home();
        let Some(bot) = settings::load(home).ok().and_then(|s| settings::get_secret(home, &s.slack.bot_token_secret)) else {
            return;
        };
        let text = text.to_string();
        std::thread::spawn(move || {
            let _ = call("chat.postMessage", &bot, &[("channel", &channel), ("thread_ts", &thread), ("text", &text)]);
        });
    }
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
}
