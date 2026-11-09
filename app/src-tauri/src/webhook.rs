//! The local webhook listener.
//!
//! It listens on 127.0.0.1 only, and a tunnel the person runs gives it a public
//! address. It's in the engine and not the daemon, so a bug in code that faces the
//! internet can't take terminals down.
//!
//! A delivery only makes an item show up sooner. It's checked against the source's
//! signature and then asks for the same pull a timer would. The timer stays on, so
//! a sleeping machine or a dropped tunnel delays work and doesn't lose it.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use sha2::Sha256;

const MAX_HEADERS: usize = 16 * 1024;
const MAX_BODY: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONNECTIONS: usize = 32;

/// A request that got this far has only proved it's well-formed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// How a source's deliveries prove where they came from.
pub fn verify(scheme: &str, secret: &str, req: &Request) -> bool {
    let header = |name: &str| req.headers.get(name).map(String::as_str);
    let signed = |given: Option<&str>| -> bool {
        let Some(given) = given else { return false };
        let given = given.trim();
        let given = given.strip_prefix("sha256=").unwrap_or(given);
        let Ok(given) = hex::decode(given) else { return false };
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
            return false;
        };
        mac.update(&req.body);
        mac.verify_slice(&given).is_ok()
    };
    if secret.is_empty() {
        return false;
    }
    match scheme {
        "sentry" => signed(header("sentry-hook-signature")),
        "github" => signed(header("x-hub-signature-256")),
        "linear" => signed(header("linear-signature")),
        "hmac" => signed(header("x-signature-256")),
        "token" => {
            let given = header("authorization")
                .and_then(|a| a.strip_prefix("Bearer "))
                .or(header("x-webhook-token"))
                .unwrap_or_default();
            // Compared through a MAC so the time taken says nothing about the secret.
            let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(b"sawtooth-token-compare") else {
                return false;
            };
            mac.update(secret.as_bytes());
            let want = mac.finalize().into_bytes();
            let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(b"sawtooth-token-compare") else {
                return false;
            };
            mac.update(given.as_bytes());
            mac.verify_slice(&want).is_ok()
        }
        _ => false,
    }
}

/// `/hook/<factory>/<source>`, as (factory, source).
pub fn route(path: &str) -> Option<(&str, &str)> {
    let path = path.split('?').next()?;
    let mut parts = path.trim_start_matches('/').split('/');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("hook"), Some(f), Some(s), None) if !f.is_empty() && !s.is_empty() => Some((f, s)),
        _ => None,
    }
}

fn read_request(stream: &mut TcpStream) -> Result<Request, u16> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        if buf.len() > MAX_HEADERS {
            return Err(431);
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Err(400),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split(' ');
    let (method, path) = (first.next().unwrap_or_default(), first.next().unwrap_or_default());
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let length: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if length > MAX_BODY {
        return Err(413);
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Err(400),
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    body.truncate(length);
    Ok(Request {
        method: method.to_string(),
        path: path.to_string(),
        headers,
        body,
    })
}

fn respond(stream: &mut TcpStream, code: u16) {
    let text = match code {
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    };
    let _ = write!(stream, "HTTP/1.1 {code} {text}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
}

/// Decides what a well-formed request gets. Returns the status code.
pub type Handler = Arc<dyn Fn(&Request) -> u16 + Send + Sync>;

pub struct Listener {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl Listener {
    /// Starts listening on loopback. Port 0 picks a free one.
    pub fn start(port: u16, handler: Handler) -> Result<Listener> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .with_context(|| format!("couldn't listen on port {port}. Is something else using it?"))?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let active = Arc::new(AtomicUsize::new(0));
        thread::Builder::new().name("sawtooth-webhooks".into()).spawn(move || {
            for stream in listener.incoming() {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut stream) = stream else { continue };
                if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    active.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let (handler, active) = (Arc::clone(&handler), Arc::clone(&active));
                thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(TIMEOUT));
                    let _ = stream.set_write_timeout(Some(TIMEOUT));
                    let code = match read_request(&mut stream) {
                        Ok(req) if req.method != "POST" => {
                            let _ = req;
                            405
                        }
                        Ok(req) => handler(&req),
                        Err(code) => code,
                    };
                    respond(&mut stream, code);
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        })?;
        Ok(Listener { port, stop })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop so it sees the flag.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn sign(secret: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        hex::encode(mac.finalize().into_bytes())
    }

    fn request(headers: &[(&str, String)], body: &[u8]) -> Request {
        Request {
            method: "POST".into(),
            path: "/hook/f/s".into(),
            headers: headers.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
            body: body.to_vec(),
        }
    }

    #[test]
    fn each_scheme_accepts_a_real_signature_and_refuses_a_wrong_one() {
        let body = b"{\"action\":\"created\"}";
        let good = sign("topsecret", body);
        let bad = sign("wrong", body);
        for (scheme, header, prefix) in [
            ("sentry", "sentry-hook-signature", ""),
            ("github", "x-hub-signature-256", "sha256="),
            ("linear", "linear-signature", ""),
            ("hmac", "x-signature-256", "sha256="),
        ] {
            let ok = request(&[(header, format!("{prefix}{good}"))], body);
            assert!(verify(scheme, "topsecret", &ok), "{scheme}");
            let no = request(&[(header, format!("{prefix}{bad}"))], body);
            assert!(!verify(scheme, "topsecret", &no), "{scheme}");
            assert!(!verify(scheme, "topsecret", &request(&[], body)), "{scheme} with no header");
        }
    }

    #[test]
    fn a_body_changed_on_the_way_is_refused() {
        let good = sign("s", b"original");
        assert!(!verify("sentry", "s", &request(&[("sentry-hook-signature", good)], b"tampered")));
    }

    #[test]
    fn a_token_has_to_match() {
        let ok = request(&[("authorization", "Bearer abc".to_string())], b"");
        assert!(verify("token", "abc", &ok));
        assert!(!verify("token", "abd", &ok));
        assert!(verify("token", "abc", &request(&[("x-webhook-token", "abc".to_string())], b"")));
    }

    #[test]
    fn no_secret_or_an_unknown_scheme_lets_nothing_through() {
        let any = request(&[("sentry-hook-signature", sign("", b""))], b"");
        assert!(!verify("sentry", "", &any));
        assert!(!verify("none", "x", &any));
    }

    #[test]
    fn routes() {
        assert_eq!(route("/hook/pay/sentry"), Some(("pay", "sentry")));
        assert_eq!(route("/hook/pay/sentry?x=1"), Some(("pay", "sentry")));
        assert_eq!(route("/hook/pay"), None);
        assert_eq!(route("/hook/pay/sentry/more"), None);
        assert_eq!(route("/other/a/b"), None);
    }

    fn send(port: u16, raw: &[u8]) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(raw).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn the_listener_takes_a_post_and_refuses_the_rest() {
        let seen: Arc<Mutex<Vec<Request>>> = Arc::default();
        let log = Arc::clone(&seen);
        let l = Listener::start(
            0,
            Arc::new(move |r: &Request| {
                log.lock().unwrap().push(r.clone());
                202
            }),
        )
        .unwrap();
        let reply = send(l.port(), b"POST /hook/a/b HTTP/1.1\r\nContent-Length: 5\r\nX-Thing: 1\r\n\r\nhello");
        assert!(reply.starts_with("HTTP/1.1 202"), "{reply}");
        {
            let seen = seen.lock().unwrap();
            assert_eq!(seen[0].body, b"hello");
            assert_eq!(seen[0].headers["x-thing"], "1");
        }
        assert!(send(l.port(), b"GET /hook/a/b HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 405"));
        let huge = format!("POST /x HTTP/1.1\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1);
        assert!(send(l.port(), huge.as_bytes()).starts_with("HTTP/1.1 413"));
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn it_only_listens_on_this_machine() {
        let l = Listener::start(0, Arc::new(|_| 202)).unwrap();
        assert!(TcpStream::connect(("127.0.0.1", l.port())).is_ok());
    }
}
