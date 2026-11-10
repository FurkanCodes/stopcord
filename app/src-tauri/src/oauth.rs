//! Signing in to a connection in the browser, so nobody makes or pastes a token: Sentry with its
//! device flow (a code to approve), Linear with PKCE and a callback to this machine. Neither needs
//! a client secret, so nothing secret ships in the app. Slack can't: its desktop sign-in can't ask
//! for bot scopes, so it keeps its manifest. GitHub signs in through gh.
//!
//! The tokens are kept in Sawtooth's secrets, under the connection, and stand in for its pasted
//! token. They're refreshed before they run out, and when that fails the connection says to sign
//! in again.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::settings;

/// Sawtooth's registered OAuth apps. Public clients: an id isn't a secret. An empty id means the
/// app isn't registered yet, so the card only offers the pasted token. `SAWTOOTH_<NAME>_CLIENT_ID`
/// overrides one, for testing against another app.
const SENTRY_CLIENT_ID: &str = "840d30ca586830182349bce87ecd852480db198466cd1aead6fcffdf1a8d9943";
const LINEAR_CLIENT_ID: &str = "ee532b0420bea3bfbbe28434b8569c51";

/// Where Linear sends the browser back. It has to match the app's registered redirect exactly,
/// so the port is fixed.
pub const LINEAR_REDIRECT_PORT: u16 = 47615;
const SCOPES_SENTRY: &str = "org:read project:read event:read";
const SCOPES_LINEAR: &str = "read";

/// Refresh this long before a token runs out, so a pull never starts with one that's about to.
const EARLY: u64 = 5 * 60 * 1000;
const TIMEOUT: Duration = Duration::from_secs(20);

struct Service {
    connection: &'static str,
    /// The connection field the token stands in for.
    field: &'static str,
    /// What goes before the token in that field, since commands send it as the header's value.
    prefix: &'static str,
}

const SERVICES: &[Service] = &[
    Service { connection: "sentry", field: "SENTRY_AUTH_TOKEN", prefix: "" },
    Service { connection: "linear", field: "LINEAR_API_KEY", prefix: "Bearer " },
];

/// The services' addresses. The e2e tests point them at a fake on this machine.
fn address(var: &str, real: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| real.to_string()).trim_end_matches('/').to_string()
}
pub fn sentry_url() -> String {
    address("SAWTOOTH_SENTRY_OAUTH_URL", "https://sentry.io")
}
fn linear_authorize_url() -> String {
    address("SAWTOOTH_LINEAR_AUTHORIZE_URL", "https://linear.app/oauth/authorize")
}
pub fn linear_api() -> String {
    address("SAWTOOTH_LINEAR_API_URL", "https://api.linear.app")
}
fn token_url(connection: &str) -> String {
    if connection == "sentry" { format!("{}/oauth/token/", sentry_url()) } else { format!("{}/oauth/token", linear_api()) }
}

fn service(connection: &str) -> Option<&'static Service> {
    SERVICES.iter().find(|s| s.connection == connection)
}

/// The connection field a sign-in fills.
pub fn field(connection: &str) -> Option<&'static str> {
    service(connection).map(|s| s.field)
}

pub fn client_id(connection: &str) -> String {
    let built = match connection {
        "sentry" => SENTRY_CLIENT_ID,
        "linear" => LINEAR_CLIENT_ID,
        _ => return String::new(),
    };
    std::env::var(format!("SAWTOOTH_{}_CLIENT_ID", connection.to_uppercase())).unwrap_or_else(|_| built.to_string()).trim().to_string()
}

/// Whether the browser sign-in can be offered: the app is registered, and for Sentry, it's
/// sentry.io and not a self-hosted one, which would need its own registered app.
pub fn available(home: &Path, connection: &str) -> bool {
    if service(connection).is_none() || client_id(connection).is_empty() {
        return false;
    }
    connection != "sentry" || crate::connections::plain(home, "sentry", "SENTRY_URL").is_none_or(|u| u.trim_end_matches('/') == sentry_url())
}

fn key(connection: &str, what: &str) -> String {
    format!("connection.{connection}.oauth.{what}")
}

/// Who signed in, when someone did.
pub fn who(home: &Path, connection: &str) -> Option<String> {
    settings::get_secret(home, &key(connection, "access"))?;
    Some(settings::get_secret(home, &key(connection, "who")).unwrap_or_else(|| "you".to_string()))
}

/// Forgets the sign-in.
pub fn sign_out(home: &Path, connection: &str) -> Result<(), String> {
    for what in ["access", "refresh", "expires", "who"] {
        settings::set_secret(home, &key(connection, what), "").map_err(|e| format!("{e:#}"))?;
    }
    Ok(())
}

fn now_ms() -> u64 {
    crate::runs::now_ms()
}

/// Keeps a token, and drops any pasted one, so there's one way in at a time.
fn keep(home: &Path, connection: &str, v: &Value, who: Option<&str>) -> Result<(), String> {
    let access = v["access_token"].as_str().ok_or_else(|| format!("{connection} sent no token back"))?;
    let save = |what: &str, value: &str| settings::set_secret(home, &key(connection, what), value).map_err(|e| format!("{e:#}"));
    save("access", access)?;
    if let Some(r) = v["refresh_token"].as_str() {
        save("refresh", r)?;
    }
    let expires = v["expires_in"].as_u64().map_or(0, |s| now_ms() + s * 1000);
    save("expires", &expires.to_string())?;
    if let Some(who) = who {
        save("who", who)?;
    }
    if let Some(s) = service(connection) {
        let _ = settings::set_secret(home, &crate::connections::secret_key(connection, s.field), "");
    }
    Ok(())
}

/// Only one refresh at a time: Linear hands out a new refresh token each time, so two at once
/// would leave one of them holding a dead one.
fn refresh_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// The value a connection's token field gets from the sign-in, refreshed first when it's about to
/// run out. Nothing when nobody signed in, or the refresh failed: then it needs signing in again.
pub fn token(home: &Path, connection: &str, field: &str) -> Option<String> {
    let s = service(connection).filter(|s| s.field == field)?;
    let _guard = refresh_lock().lock().unwrap();
    let access = settings::get_secret(home, &key(connection, "access"))?;
    let expires: u64 = settings::get_secret(home, &key(connection, "expires")).and_then(|e| e.parse().ok()).unwrap_or(0);
    if expires == 0 || now_ms() + EARLY < expires {
        return Some(format!("{}{access}", s.prefix));
    }
    let refresh = settings::get_secret(home, &key(connection, "refresh"))?;
    let id = client_id(connection);
    let answer = ureq::post(&token_url(connection)).timeout(TIMEOUT).send_form(&[("grant_type", "refresh_token"), ("refresh_token", &refresh), ("client_id", &id)]);
    match answer.ok().and_then(|r| r.into_json::<Value>().ok()) {
        Some(v) if v["access_token"].is_string() => {
            keep(home, connection, &v, None).ok()?;
            v["access_token"].as_str().map(|t| format!("{}{t}", s.prefix))
        }
        // It can't be refreshed, so it's gone: the card asks to sign in again.
        _ => {
            let _ = sign_out(home, connection);
            None
        }
    }
}

/// How a sign-in in the browser is going, for the card to show.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Signing {
    /// Waiting for the person to approve it in the browser.
    pub waiting: bool,
    /// Sentry's code to check against the one in the browser.
    pub code: String,
    /// Where to approve it, to open again if the browser didn't.
    pub url: String,
    /// What went wrong. Empty while it's going or once it worked.
    pub error: String,
    /// Who it signed in as, once it worked.
    pub done: String,
}

fn signings() -> &'static Mutex<HashMap<String, Signing>> {
    static S: OnceLock<Mutex<HashMap<String, Signing>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn note(connection: &str, change: impl FnOnce(&mut Signing)) {
    let mut all = signings().lock().unwrap();
    change(all.entry(connection.to_string()).or_default());
}

pub fn signing(connection: &str) -> Signing {
    signings().lock().unwrap().get(connection).cloned().unwrap_or_default()
}

fn open_browser(url: &str) {
    // Tests answer the page themselves.
    if std::env::var_os("SAWTOOTH_NO_BROWSER").is_some() {
        return;
    }
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = std::process::Command::new(opener).arg(url).spawn();
}

/// Starts signing in. It goes on in the background; `signing` says how it's going.
pub fn start(home: &Path, connection: &str) -> Result<Signing, String> {
    if !available(home, connection) {
        return Err(format!("signing in to {connection} in the browser isn't set up in this build. paste a token instead"));
    }
    // Already waiting: open its page again, as when the browser tab was closed.
    let now = signing(connection);
    if now.waiting {
        open_browser(&now.url);
        return Ok(now);
    }
    let home = home.to_path_buf();
    match connection {
        "sentry" => sentry_start(home),
        "linear" => linear_start(home),
        _ => Err(format!("{connection} can't sign in in the browser")),
    }
}

fn error_of(v: &Value) -> String {
    v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("it said no").to_string()
}

/// Sentry's device flow: ask for a code, open the page to approve it, and poll until it's done.
fn sentry_start(home: PathBuf) -> Result<Signing, String> {
    let id = client_id("sentry");
    let v: Value = ureq::post(&format!("{}/oauth/device/code/", sentry_url()))
        .timeout(TIMEOUT)
        .send_form(&[("client_id", &id), ("scope", SCOPES_SENTRY)])
        .map_err(|e| format!("couldn't start signing in to Sentry: {e}"))?
        .into_json()
        .map_err(|e| format!("Sentry's answer wasn't JSON: {e}"))?;
    let (Some(device), Some(code)) = (v["device_code"].as_str(), v["user_code"].as_str()) else {
        return Err(format!("Sentry didn't give a code: {}", error_of(&v)));
    };
    let url = v["verification_uri_complete"].as_str().or(v["verification_uri"].as_str()).unwrap_or("https://sentry.io/oauth/device/").to_string();
    let token = token_url("sentry");
    let mut interval = v["interval"].as_u64().unwrap_or(5);
    let until = Instant::now() + Duration::from_secs(v["expires_in"].as_u64().unwrap_or(600));
    let started = Signing { waiting: true, code: code.to_string(), url: url.clone(), ..Signing::default() };
    note("sentry", |s| *s = started.clone());
    open_browser(&url);
    let device = device.to_string();
    std::thread::spawn(move || {
        let fail = |why: String| note("sentry", |s| *s = Signing { error: why, ..Signing::default() });
        while Instant::now() < until {
            std::thread::sleep(Duration::from_secs(interval));
            if !signing("sentry").waiting {
                return; // Cancelled.
            }
            let answer = ureq::post(&token)
                .timeout(TIMEOUT)
                .send_form(&[("client_id", &id), ("device_code", &device), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")]);
            // A pending answer comes back as a 400 with a JSON body.
            let v: Value = match answer {
                Ok(r) => r.into_json().unwrap_or(Value::Null),
                Err(ureq::Error::Status(_, r)) => r.into_json().unwrap_or(Value::Null),
                Err(e) => return fail(format!("couldn't reach Sentry: {e}")),
            };
            match v["error"].as_str() {
                None if v["access_token"].is_string() => {
                    let who = v["user"]["name"].as_str().or(v["user"]["email"].as_str()).map(str::to_string);
                    return match keep(&home, "sentry", &v, who.as_deref()) {
                        Ok(()) => note("sentry", |s| *s = Signing { done: who.unwrap_or_else(|| "you".into()), ..Signing::default() }),
                        Err(why) => fail(why),
                    };
                }
                Some("authorization_pending") => {}
                Some("slow_down") => interval += 5,
                Some("access_denied") => return fail("you said no in the browser, so nothing changed".into()),
                Some("expired_token") => return fail("the code ran out before it was approved. sign in again".into()),
                _ => return fail(format!("Sentry said no: {}", error_of(&v))),
            }
        }
        fail("the code ran out before it was approved. sign in again".into());
    });
    Ok(started)
}

fn random_url_safe(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).is_err() {
        // Not expected on macOS or Linux. The time still makes it unguessable enough to start.
        let t = now_ms().to_le_bytes();
        for (i, b) in buf.iter_mut().enumerate() {
            *b = t[i % t.len()] ^ (i as u8).wrapping_mul(31);
        }
    }
    B64URL.encode(buf)
}

/// The first line of an HTTP request, as in "GET /callback?code=… HTTP/1.1": the query's values.
fn query_of(line: &str) -> HashMap<String, String> {
    let target = line.split_whitespace().nth(1).unwrap_or_default();
    let query = target.split_once('?').map_or("", |(_, q)| q);
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), percent_decode(v)))
        .collect()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b[i], hex) {
            (b'+', _) => out.push(b' '),
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 2;
            }
            (c, _) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

const PAGE_OK: &str = "<!doctype html><meta charset=utf-8><title>Sawtooth</title><body style=\"font:16px system-ui;background:#111;color:#ddd;display:grid;place-items:center;height:90vh\"><p>Signed in to Linear. You can close this tab and go back to Sawtooth.</p>";
const PAGE_NO: &str = "<!doctype html><meta charset=utf-8><title>Sawtooth</title><body style=\"font:16px system-ui;background:#111;color:#ddd;display:grid;place-items:center;height:90vh\"><p>Linear didn't sign in. Go back to Sawtooth to see why.</p>";

/// Linear's PKCE flow: open its consent page, wait on this machine for the browser to come back
/// with a code, and trade the code for a token.
fn linear_start(home: PathBuf) -> Result<Signing, String> {
    let id = client_id("linear");
    let listener = TcpListener::bind(("127.0.0.1", LINEAR_REDIRECT_PORT)).map_err(|e| format!("couldn't listen on port {LINEAR_REDIRECT_PORT} for Linear's answer: {e}. close whatever uses it and try again"))?;
    listener.set_nonblocking(true).map_err(|e| format!("{e}"))?;
    let redirect = format!("http://127.0.0.1:{LINEAR_REDIRECT_PORT}/callback");
    let verifier = random_url_safe(48);
    let challenge = B64URL.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_url_safe(16);
    let url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={SCOPES_LINEAR}&state={state}&code_challenge={challenge}&code_challenge_method=S256&prompt=consent",
        linear_authorize_url(),
        percent_encode(&id),
        percent_encode(&redirect)
    );
    let started = Signing { waiting: true, url: url.clone(), ..Signing::default() };
    note("linear", |s| *s = started.clone());
    open_browser(&url);
    std::thread::spawn(move || {
        let fail = |why: String| note("linear", |s| *s = Signing { error: why, ..Signing::default() });
        let until = Instant::now() + Duration::from_secs(10 * 60);
        while Instant::now() < until {
            if !signing("linear").waiting {
                return; // Cancelled.
            }
            let (mut stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }
                Err(e) => return fail(format!("the callback broke: {e}")),
            };
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut line = String::new();
            let _ = BufReader::new(&stream).read_line(&mut line);
            if !line.contains("/callback") {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue; // A favicon, or something else knocking.
            }
            let q = query_of(&line);
            let reply = |page: &str, stream: &mut std::net::TcpStream| {
                let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len()).as_bytes());
            };
            if q.get("state") != Some(&state) {
                reply(PAGE_NO, &mut stream);
                return fail("the answer didn't come from the sign-in Sawtooth started, so it was ignored. sign in again".into());
            }
            let Some(code) = q.get("code") else {
                reply(PAGE_NO, &mut stream);
                return fail(match q.get("error").map(String::as_str) {
                    Some("access_denied") => "you said no in the browser, so nothing changed".into(),
                    Some(e) => format!("Linear said no: {}", q.get("error_description").unwrap_or(&e.to_string())),
                    None => "Linear came back without a code".into(),
                });
            };
            let answer = ureq::post(&token_url("linear"))
                .timeout(TIMEOUT)
                .send_form(&[("grant_type", "authorization_code"), ("code", code), ("redirect_uri", &redirect), ("client_id", &id), ("code_verifier", &verifier)]);
            let v: Value = match answer {
                Ok(r) => r.into_json().unwrap_or(Value::Null),
                Err(ureq::Error::Status(_, r)) => r.into_json().unwrap_or(Value::Null),
                Err(e) => {
                    reply(PAGE_NO, &mut stream);
                    return fail(format!("couldn't reach Linear: {e}"));
                }
            };
            if !v["access_token"].is_string() {
                reply(PAGE_NO, &mut stream);
                return fail(format!("Linear said no: {}", error_of(&v)));
            }
            reply(PAGE_OK, &mut stream);
            let who = ureq::post(&format!("{}/graphql", linear_api()))
                .set("Authorization", &format!("Bearer {}", v["access_token"].as_str().unwrap_or_default()))
                .timeout(TIMEOUT)
                .send_json(serde_json::json!({ "query": "{ viewer { name } }" }))
                .ok()
                .and_then(|r| r.into_json::<Value>().ok())
                .and_then(|w| w["data"]["viewer"]["name"].as_str().map(str::to_string));
            return match keep(&home, "linear", &v, who.as_deref()) {
                Ok(()) => note("linear", |s| *s = Signing { done: who.unwrap_or_else(|| "you".into()), ..Signing::default() }),
                Err(why) => fail(why),
            };
        }
        fail("nobody came back from Linear's page in 10 minutes. sign in again".into());
    });
    Ok(started)
}

/// Stops waiting for a sign-in.
pub fn cancel(connection: &str) {
    note(connection, |s| *s = Signing::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_callback_line_gives_its_query() {
        let q = query_of("GET /callback?code=abc%2Fd&state=x+y HTTP/1.1\r\n");
        assert_eq!(q.get("code").map(String::as_str), Some("abc/d"));
        assert_eq!(q.get("state").map(String::as_str), Some("x y"));
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_encode("http://127.0.0.1:1/c b"), "http%3A%2F%2F127.0.0.1%3A1%2Fc%20b");
    }

    #[test]
    fn a_signed_in_token_stands_in_for_the_pasted_one_until_it_is_signed_out() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        settings::set_secret(h, &crate::connections::secret_key("linear", "LINEAR_API_KEY"), "lin_api_pasted").unwrap();
        keep(h, "linear", &serde_json::json!({ "access_token": "tok", "refresh_token": "ref", "expires_in": 86399 }), Some("Ada")).unwrap();
        assert_eq!(token(h, "linear", "LINEAR_API_KEY").as_deref(), Some("Bearer tok"), "linear's header needs Bearer");
        assert_eq!(who(h, "linear").as_deref(), Some("Ada"));
        assert!(settings::get_secret(h, &crate::connections::secret_key("linear", "LINEAR_API_KEY")).is_none(), "one way in at a time");
        assert_eq!(token(h, "linear", "SOMETHING_ELSE"), None);
        sign_out(h, "linear").unwrap();
        assert_eq!(token(h, "linear", "LINEAR_API_KEY"), None);
        assert_eq!(who(h, "linear"), None);
    }

    #[test]
    fn pkce_challenge_is_the_verifiers_sha256() {
        // Worked out separately, with openssl dgst -sha256 -binary | base64url.
        let verifier = "dBjftJeZ4CVP-mJ92K9ffpbJ-LzMMxR5Hfd9RvEnqDo";
        assert_eq!(B64URL.encode(Sha256::digest(verifier.as_bytes())), "cjmvLo5tKhasuxkuUI8mLA28GA0Qrp5lPMtyOmYkFIM");
        assert_eq!(random_url_safe(32).len(), 43);
    }
}
