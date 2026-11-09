//! A client connection that may arrive by TCP or Unix socket, and the tunnel that carries it.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// How often a blocked read wakes to check for a timeout or a stop.
const POLL: Duration = Duration::from_secs(1);

/// The longest a request head may be.
pub(super) const MAX_HEAD: usize = 64 * 1024;

/// One end of a connection the filter holds.
pub(super) enum Conn {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(UnixStream),
}

impl Conn {
    fn try_clone(&self) -> io::Result<Conn> {
        match self {
            Conn::Tcp(s) => s.try_clone().map(Conn::Tcp),
            #[cfg(unix)]
            Conn::Unix(s) => s.try_clone().map(Conn::Unix),
        }
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            Conn::Tcp(s) => s.set_read_timeout(timeout),
            #[cfg(unix)]
            Conn::Unix(s) => s.set_read_timeout(timeout),
        }
    }

    pub(super) fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            Conn::Tcp(s) => s.set_write_timeout(timeout),
            #[cfg(unix)]
            Conn::Unix(s) => s.set_write_timeout(timeout),
        }
    }

    fn shutdown(&self, how: Shutdown) {
        let _ = match self {
            Conn::Tcp(s) => s.shutdown(how),
            #[cfg(unix)]
            Conn::Unix(s) => s.shutdown(how),
        };
    }
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Conn::Tcp(s) => s.read(buf),
            #[cfg(unix)]
            Conn::Unix(s) => s.read(buf),
        }
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Conn::Tcp(s) => s.write(buf),
            #[cfg(unix)]
            Conn::Unix(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn is_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// Why no request head could be read.
pub(super) enum HeadError {
    TooLarge,
    /// The client went away, stalled past the deadline, or the filter is stopping.
    Gone,
}

/// Reads up to and including the blank line that ends a request head. Returns the head and any bytes
/// that arrived after it. The deadline covers the whole head, so sending a byte at a time gains nothing.
pub(super) fn read_head(
    client: &mut Conn,
    timeout: Duration,
    stopped: &AtomicBool,
) -> Result<(Vec<u8>, Vec<u8>), HeadError> {
    let deadline = Instant::now() + timeout;
    let mut head = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(index) = head.windows(4).position(|w| w == b"\r\n\r\n") {
            let end = index + 4;
            if end > MAX_HEAD {
                return Err(HeadError::TooLarge);
            }
            let rest = head.split_off(end);
            return Ok((head, rest));
        }
        if head.len() > MAX_HEAD {
            return Err(HeadError::TooLarge);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || stopped.load(Ordering::Relaxed) {
            return Err(HeadError::Gone);
        }
        client
            .set_read_timeout(Some(left.min(POLL)))
            .map_err(|_| HeadError::Gone)?;
        match client.read(&mut chunk) {
            Ok(0) => return Err(HeadError::Gone),
            Ok(n) => head.extend_from_slice(&chunk[..n]),
            Err(e) if is_timeout(&e) || e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Err(HeadError::Gone),
        }
    }
}

/// Sends a short plain-text response and closes. Reads and discards a little first-in-flight data,
/// because closing with unread bytes resets the connection and the client could lose the response.
pub(super) fn refuse(client: &mut Conn, status: &str, message: &str) {
    let body = format!("{message}\n");
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    if client.write_all(response.as_bytes()).is_err() {
        return;
    }
    client.shutdown(Shutdown::Write);
    let deadline = Instant::now() + POLL;
    let mut drained = 0usize;
    let mut chunk = [0u8; 4096];
    while drained < 1024 * 1024 && Instant::now() < deadline {
        if client.set_read_timeout(Some(POLL)).is_err() {
            return;
        }
        match client.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => drained += n,
        }
    }
}

/// What the two directions of a tunnel share.
struct Tunnel<'a> {
    started: Instant,
    last_activity_ms: AtomicU64,
    idle_timeout: Duration,
    stopped: &'a AtomicBool,
}

impl Tunnel<'_> {
    fn touch(&self) {
        let now = self.started.elapsed().as_millis() as u64;
        self.last_activity_ms.store(now, Ordering::Relaxed);
    }

    fn should_end(&self) -> bool {
        let last = Duration::from_millis(self.last_activity_ms.load(Ordering::Relaxed));
        self.stopped.load(Ordering::Relaxed)
            || self.started.elapsed().saturating_sub(last) > self.idle_timeout
    }
}

/// Carries bytes both ways until both sides finish, either side fails, the connection has been idle
/// for `idle_timeout`, or the filter stops. Activity in either direction counts, so a long download
/// with nothing to send isn't cut off.
pub(super) fn tunnel(client: Conn, upstream: Conn, idle_timeout: Duration, stopped: &AtomicBool) {
    let (Ok(client2), Ok(upstream2)) = (client.try_clone(), upstream.try_clone()) else {
        return;
    };
    if client.set_read_timeout(Some(POLL)).is_err()
        || upstream.set_read_timeout(Some(POLL)).is_err()
    {
        return;
    }
    let shared = Tunnel {
        started: Instant::now(),
        last_activity_ms: AtomicU64::new(0),
        idle_timeout,
        stopped,
    };
    thread::scope(|scope| {
        let shared = &shared;
        scope.spawn(move || pump(client, upstream, shared));
        pump(upstream2, client2, shared);
    });
}

/// Copies one direction. A clean end of input is passed on as a half-close so the other direction can
/// finish; anything else tears down both.
fn pump(mut from: Conn, mut to: Conn, shared: &Tunnel) {
    let mut buffer = vec![0u8; 16 * 1024];
    loop {
        match from.read(&mut buffer) {
            Ok(0) => {
                to.shutdown(Shutdown::Write);
                return;
            }
            Ok(n) => {
                if to.write_all(&buffer[..n]).is_err() {
                    break;
                }
                shared.touch();
            }
            Err(e) if is_timeout(&e) || e.kind() == io::ErrorKind::Interrupted => {
                if shared.should_end() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    from.shutdown(Shutdown::Both);
    to.shutdown(Shutdown::Both);
}
