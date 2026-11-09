//! Filter behaviour against a local upstream server. Every host resolves to that server.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use super::*;

/// Accepts connections, records what each one sent, and answers with a fixed response.
struct Upstream {
    port: u16,
    seen: Arc<Mutex<Vec<Vec<u8>>>>,
    connections: Arc<AtomicUsize>,
}

impl Upstream {
    fn start() -> Upstream {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(AtomicUsize::new(0));
        let (seen2, connections2) = (seen.clone(), connections.clone());
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                connections2.fetch_add(1, Ordering::SeqCst);
                let seen = seen2.clone();
                thread::spawn(move || serve_upstream(stream, seen));
            }
        });
        Upstream {
            port,
            seen,
            connections,
        }
    }

    fn connector(&self) -> Connector {
        let port = self.port;
        Arc::new(move |_host: &str, _port: u16| TcpStream::connect(("127.0.0.1", port)))
    }

    fn requests(&self) -> Vec<String> {
        wait_until(|| !self.seen.lock().unwrap().is_empty());
        let seen = self.seen.lock().unwrap();
        seen.iter()
            .map(|r| String::from_utf8_lossy(r).into_owned())
            .collect()
    }

    fn connection_count(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

fn serve_upstream(mut stream: TcpStream, seen: Arc<Mutex<Vec<Vec<u8>>>>) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut got = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let end = got.windows(4).position(|w| w == b"\r\n\r\n");
        if let Some(i) = end {
            let head = String::from_utf8_lossy(&got[..i]).to_ascii_lowercase();
            let want: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length: ")?.trim().parse().ok())
                .unwrap_or(0);
            if got.len() >= i + 4 + want {
                break;
            }
        }
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
        }
    }
    seen.lock().unwrap().push(got);
    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
}

fn wait_until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting");
        thread::sleep(Duration::from_millis(10));
    }
}

type Sink = Arc<Mutex<Vec<Denied>>>;

struct Fix {
    upstream: Upstream,
    filter: Filter,
    denied: Sink,
    _dir: tempfile::TempDir,
}

fn start(allow: &[&str]) -> Fix {
    start_limited(allow, Limits::default())
}

fn start_limited(allow: &[&str], limits: Limits) -> Fix {
    let upstream = Upstream::start();
    let dir = tempfile::tempdir().unwrap();
    let denied: Sink = Arc::new(Mutex::new(Vec::new()));
    let sink = denied.clone();
    let filter = Filter::start_with(
        allow.iter().map(|s| s.to_string()).collect(),
        dir.path().join("f.sock"),
        Arc::new(move |d| sink.lock().unwrap().push(d)),
        upstream.connector(),
        limits,
    )
    .unwrap();
    Fix {
        upstream,
        filter,
        denied,
        _dir: dir,
    }
}

fn connect(port: u16) -> TcpStream {
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
}

/// Reads the whole response, tolerating a reset once something has arrived.
fn read_all(stream: &mut impl Read) -> String {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    while let Ok(n) = stream.read(&mut buf) {
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn request(port: u16, raw: &[u8]) -> String {
    let mut stream = connect(port);
    stream.write_all(raw).unwrap();
    read_all(&mut stream)
}

/// Sends a CONNECT and returns the status line with the open stream.
fn tunnel(port: u16, target: &str) -> (String, TcpStream) {
    let mut stream = connect(port);
    write!(
        stream,
        "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n"
    )
    .unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => break,
        }
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    (text.lines().next().unwrap_or("").to_string(), stream)
}

fn tunnel_status(port: u16, target: &str) -> String {
    tunnel(port, target).0
}

#[test]
fn an_allowed_connect_reaches_upstream_and_carries_bytes() {
    let f = start(&["example.com"]);
    let (status, mut stream) = tunnel(f.filter.port(), "example.com:443");
    assert!(status.contains("200"), "{status}");
    stream.write_all(b"ping\r\n\r\n").unwrap();
    assert!(read_all(&mut stream).ends_with("ok"));
    assert_eq!(f.upstream.requests(), ["ping\r\n\r\n"]);
    assert!(f.denied.lock().unwrap().is_empty());
}

#[test]
fn a_blocked_connect_gets_403_and_is_reported_once() {
    let f = start(&["example.com"]);
    let response = request(f.filter.port(), b"CONNECT other.net:443 HTTP/1.1\r\n\r\n");
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    let body = response.split("\r\n\r\n").nth(1).unwrap();
    assert_eq!(body.trim_end().lines().count(), 1, "{body:?}");
    assert!(body.contains("other.net"));
    wait_until(|| !f.denied.lock().unwrap().is_empty());
    thread::sleep(Duration::from_millis(50));
    assert_eq!(
        *f.denied.lock().unwrap(),
        [Denied {
            host: "other.net".into(),
            port: 443
        }]
    );
    assert_eq!(f.upstream.connection_count(), 0);
}

#[test]
fn a_subdomain_passes() {
    let f = start(&["example.com"]);
    assert!(tunnel_status(f.filter.port(), "api.example.com:443").contains("200"));
}

#[test]
fn a_look_alike_domain_is_blocked() {
    let f = start(&["example.com"]);
    assert!(tunnel_status(f.filter.port(), "evil-example.com:443").contains("403"));
    assert!(tunnel_status(f.filter.port(), "example.com.evil.net:443").contains("403"));
    assert_eq!(f.upstream.connection_count(), 0);
}

#[test]
fn matching_ignores_case() {
    let f = start(&["example.com"]);
    assert!(tunnel_status(f.filter.port(), "API.EXAMPLE.com:443").contains("200"));
}

#[test]
fn an_ip_literal_passes_only_when_listed_exactly() {
    let f = start(&["10.1.2.3"]);
    assert!(tunnel_status(f.filter.port(), "10.1.2.3:443").contains("200"));
    assert!(tunnel_status(f.filter.port(), "10.1.2.4:443").contains("403"));
}

#[test]
fn allow_once_passes_exactly_once() {
    let f = start(&[]);
    let port = f.filter.port();
    assert!(tunnel_status(port, "once.net:443").contains("403"));
    f.filter.allow_once("once.net");
    assert!(tunnel_status(port, "once.net:443").contains("200"));
    assert!(tunnel_status(port, "once.net:443").contains("403"));
    assert_eq!(f.upstream.connection_count(), 1);
    wait_until(|| f.denied.lock().unwrap().len() == 2);
}

#[test]
fn allow_always_adds_to_the_list() {
    let f = start(&["a.com"]);
    let port = f.filter.port();
    assert!(tunnel_status(port, "b.com:443").contains("403"));
    f.filter.allow_always("B.com");
    assert!(tunnel_status(port, "b.com:443").contains("200"));
    assert!(tunnel_status(port, "x.b.com:443").contains("200"));
    assert_eq!(f.filter.allowed(), ["a.com", "b.com"]);
}

#[test]
fn a_plain_http_request_is_filtered_and_forwarded() {
    let f = start(&["example.com"]);
    let response = request(
        f.filter.port(),
        b"GET http://example.com/pkg?x=1 HTTP/1.1\r\nHost: example.com\r\nProxy-Connection: keep-alive\r\nAccept: */*\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with("ok"));
    assert_eq!(
        f.upstream.requests(),
        ["GET /pkg?x=1 HTTP/1.1\r\nHost: example.com\r\nAccept: */*\r\nConnection: close\r\n\r\n"]
    );
}

#[test]
fn a_plain_http_request_to_a_blocked_host_gets_403() {
    let f = start(&["example.com"]);
    let response = request(
        f.filter.port(),
        b"GET http://evil-example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    assert_eq!(f.upstream.connection_count(), 0);
    wait_until(|| f.denied.lock().unwrap().len() == 1);
    assert_eq!(f.denied.lock().unwrap()[0].port, 80);
}

#[test]
fn a_plain_http_body_reaches_upstream() {
    let f = start(&["example.com"]);
    let response = request(
        f.filter.port(),
        b"POST http://example.com/up HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello",
    );
    assert!(response.ends_with("ok"), "{response}");
    assert!(f.upstream.requests()[0].ends_with("\r\n\r\nhello"));
}

#[test]
fn an_oversized_header_is_refused_without_reaching_upstream() {
    let f = start(&["example.com"]);
    let mut raw = b"GET http://example.com/ HTTP/1.1\r\nX-Big: ".to_vec();
    raw.resize(raw.len() + 70 * 1024, b'a');
    raw.extend_from_slice(b"\r\n\r\n");
    let response = request(f.filter.port(), &raw);
    assert!(response.starts_with("HTTP/1.1 431"), "{response}");
    assert_eq!(f.upstream.connection_count(), 0);
}

#[test]
fn a_header_just_inside_the_limit_is_accepted() {
    let f = start(&["example.com"]);
    let mut raw = b"GET http://example.com/ HTTP/1.1\r\nX-Big: ".to_vec();
    raw.resize(60 * 1024, b'a');
    raw.extend_from_slice(b"\r\n\r\n");
    assert!(request(f.filter.port(), &raw).ends_with("ok"));
}

#[test]
fn a_malformed_request_gets_400() {
    let f = start(&["example.com"]);
    let response = request(
        f.filter.port(),
        b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    let response = request(
        f.filter.port(),
        b"CONNECT example.com@evil.net:443 HTTP/1.1\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    assert_eq!(f.upstream.connection_count(), 0);
}

#[test]
fn an_unreachable_upstream_gets_502() {
    let denied: Sink = Arc::new(Mutex::new(Vec::new()));
    let sink = denied.clone();
    let dir = tempfile::tempdir().unwrap();
    let filter = Filter::start_with(
        vec!["example.com".into()],
        dir.path().join("f.sock"),
        Arc::new(move |d| sink.lock().unwrap().push(d)),
        Arc::new(|_: &str, _: u16| Err(io::Error::from(io::ErrorKind::ConnectionRefused))),
        Limits::default(),
    )
    .unwrap();
    let response = request(filter.port(), b"CONNECT example.com:443 HTTP/1.1\r\n\r\n");
    assert!(response.starts_with("HTTP/1.1 502"), "{response}");
    assert!(denied.lock().unwrap().is_empty());
}

#[test]
fn the_unix_socket_is_filtered_the_same_way() {
    let f = start(&["example.com"]);
    let mut stream = std::os::unix::net::UnixStream::connect(f.filter.socket()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(b"GET http://example.com/ HTTP/1.1\r\n\r\n")
        .unwrap();
    assert!(read_all(&mut stream).ends_with("ok"));

    let mut stream = std::os::unix::net::UnixStream::connect(f.filter.socket()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(b"CONNECT evil.net:443 HTTP/1.1\r\n\r\n")
        .unwrap();
    assert!(read_all(&mut stream).starts_with("HTTP/1.1 403"));
}

#[test]
fn a_stuck_client_is_dropped_after_the_head_timeout() {
    let f = start_limited(
        &[],
        Limits {
            head_timeout: Duration::from_millis(150),
            ..Limits::default()
        },
    );
    let mut stream = connect(f.filter.port());
    stream.write_all(b"GET http://exa").unwrap();
    let started = Instant::now();
    let _ = read_all(&mut stream);
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn an_idle_tunnel_is_closed_after_the_idle_timeout() {
    let f = start_limited(
        &["example.com"],
        Limits {
            idle_timeout: Duration::from_millis(100),
            ..Limits::default()
        },
    );
    let (status, mut stream) = tunnel(f.filter.port(), "example.com:443");
    assert!(status.contains("200"));
    let started = Instant::now();
    let _ = read_all(&mut stream);
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[test]
fn connections_over_the_cap_are_turned_away_until_a_slot_frees() {
    let f = start_limited(
        &["example.com"],
        Limits {
            max_connections: 1,
            ..Limits::default()
        },
    );
    let port = f.filter.port();
    let held = connect(port);
    // The first connection takes the slot once it has been accepted.
    wait_until(|| f.filter.inner.active.load(Ordering::SeqCst) == 1);
    let response = request(port, b"GET http://example.com/ HTTP/1.1\r\n\r\n");
    assert!(response.starts_with("HTTP/1.1 503"), "{response}");

    drop(held);
    wait_until(|| f.filter.inner.active.load(Ordering::SeqCst) == 0);
    assert!(request(port, b"GET http://example.com/ HTTP/1.1\r\n\r\n").ends_with("ok"));
}

#[test]
fn a_blocking_callback_does_not_hold_up_the_proxy() {
    let upstream = Upstream::start();
    let dir = tempfile::tempdir().unwrap();
    let (release, wait) = mpsc::channel::<()>();
    let wait = Mutex::new(wait);
    let filter = Filter::start_with(
        vec![],
        dir.path().join("f.sock"),
        Arc::new(move |_| {
            let _ = wait.lock().unwrap().recv();
        }),
        upstream.connector(),
        Limits::default(),
    )
    .unwrap();
    let started = Instant::now();
    for _ in 0..3 {
        let response = request(filter.port(), b"CONNECT a.net:443 HTTP/1.1\r\n\r\n");
        assert!(response.starts_with("HTTP/1.1 403"));
    }
    assert!(started.elapsed() < Duration::from_secs(3));
    drop(release);
}

#[test]
fn stop_closes_the_port_and_removes_the_socket() {
    let f = start(&[]);
    let port = f.filter.port();
    let socket = f.filter.socket().to_path_buf();
    assert!(socket.exists());
    let started = Instant::now();
    f.filter.stop();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(!socket.exists());
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
}

#[test]
fn stop_ends_open_tunnels() {
    let f = start(&["example.com"]);
    let (status, mut stream) = tunnel(f.filter.port(), "example.com:443");
    assert!(status.contains("200"));
    let started = Instant::now();
    f.filter.stop();
    let _ = read_all(&mut stream);
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[test]
fn a_stale_socket_file_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.sock");
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    assert!(path.exists());
    let filter = Filter::start(vec![], path.clone(), Arc::new(|_| {})).unwrap();
    assert!(std::os::unix::net::UnixStream::connect(&path).is_ok());
    filter.stop();
}

#[test]
fn a_live_socket_or_a_regular_file_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("live.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&live).unwrap();
    assert!(Filter::start(vec![], live.clone(), Arc::new(|_| {})).is_err());
    assert!(live.exists());

    let file = dir.path().join("file");
    std::fs::write(&file, "keep").unwrap();
    assert!(Filter::start(vec![], file.clone(), Arc::new(|_| {})).is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "keep");
}

#[test]
fn the_socket_is_private_to_this_user() {
    use std::os::unix::fs::PermissionsExt;
    let f = start(&[]);
    let mode = std::fs::metadata(f.filter.socket())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}
