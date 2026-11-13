//! The local network filter: an HTTP proxy that only reaches allowed hosts.
//!
//! TLS goes through `CONNECT` and is never opened. Plain-HTTP absolute-URI requests, which some package
//! managers send, are filtered and forwarded too. Every connection gets its own thread, up to a cap, and
//! request contents are never logged.

mod conn;
mod hosts;
mod http;
#[cfg(all(test, unix))]
mod tests;

use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use conn::{Conn, HeadError, read_head, refuse, tunnel};
use hosts::AllowList;
use http::{Kind, parse_head};

/// How long a connect to an allowed host may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long one write may stall before the connection is given up.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// Blocked requests queued for the callback before further ones are dropped. A flood must not grow
/// without bound while a callback waits on a person.
const DENIED_QUEUE: usize = 1024;

/// One request the filter refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Denied {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub head_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_connections: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            head_timeout: Duration::from_secs(30),
            // Long enough for a model to think without sending a byte.
            idle_timeout: Duration::from_secs(600),
            max_connections: 256,
        }
    }
}

pub(crate) type Connector = Arc<dyn Fn(&str, u16) -> io::Result<TcpStream> + Send + Sync>;

struct Inner {
    allow: Mutex<AllowList>,
    denied: SyncSender<Denied>,
    connect: Connector,
    limits: Limits,
    stopped: AtomicBool,
    active: AtomicUsize,
}

impl Inner {
    fn allow(&self) -> MutexGuard<'_, AllowList> {
        self.allow.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A running filter. Dropping it stops it.
pub struct Filter {
    inner: Arc<Inner>,
    port: u16,
    socket: PathBuf,
    accepting: Vec<JoinHandle<()>>,
}

impl Filter {
    /// Binds 127.0.0.1:0 and, on unix, a Unix socket at `socket`. `allow` are domains; a domain also
    /// matches its subdomains. `on_denied` runs once per blocked request on a thread of its own, so it
    /// may block, for example to ask a person.
    pub fn start(
        allow: Vec<String>,
        socket: PathBuf,
        on_denied: Arc<dyn Fn(Denied) + Send + Sync>,
    ) -> anyhow::Result<Filter> {
        Filter::start_with(
            allow,
            socket,
            on_denied,
            Arc::new(connect_direct),
            Limits::default(),
        )
    }

    /// `start`, with the upstream connection and the limits replaceable for tests.
    pub(crate) fn start_with(
        allow: Vec<String>,
        socket: PathBuf,
        on_denied: Arc<dyn Fn(Denied) + Send + Sync>,
        connect: Connector,
        limits: Limits,
    ) -> anyhow::Result<Filter> {
        let listener = TcpListener::bind("127.0.0.1:0").context("binding the filter port")?;
        let port = listener.local_addr()?.port();

        let (denied, queue) = sync_channel::<Denied>(DENIED_QUEUE);
        thread::Builder::new()
            .name("sandbox-denied".into())
            .spawn(move || {
                for request in queue {
                    let callback = &on_denied;
                    // One bad callback must not stop later requests being reported.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        callback(request)
                    }));
                }
            })?;

        let inner = Arc::new(Inner {
            allow: Mutex::new(AllowList::new(allow)),
            denied,
            connect,
            limits,
            stopped: AtomicBool::new(false),
            active: AtomicUsize::new(0),
        });

        let mut accepting = Vec::new();
        let tcp_inner = inner.clone();
        accepting.push(
            thread::Builder::new()
                .name("sandbox-filter".into())
                .spawn(move || serve_tcp(listener, tcp_inner))?,
        );
        #[cfg(unix)]
        {
            let unix_listener = unix::bind(&socket)
                .with_context(|| format!("binding the filter socket {}", socket.display()))?;
            let unix_inner = inner.clone();
            accepting.push(
                thread::Builder::new()
                    .name("sandbox-filter-unix".into())
                    .spawn(move || unix::serve(unix_listener, unix_inner))?,
            );
        }

        Ok(Filter {
            inner,
            port,
            socket,
            accepting,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// The next request to this host passes, once.
    pub fn allow_once(&self, host: &str) {
        self.inner.allow().add_once(host);
    }

    /// Adds to the allow list for as long as this filter lives.
    pub fn allow_always(&self, host: &str) {
        self.inner.allow().add(host);
    }

    pub fn allowed(&self) -> Vec<String> {
        self.inner.allow().domains()
    }

    pub fn stop(self) {
        drop(self);
    }

    /// Unblocks the accept threads with a connection each. Returns whether every one was reached.
    fn wake_acceptors(&self) -> bool {
        let address = SocketAddr::from(([127, 0, 0, 1], self.port));
        let tcp = TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_ok();
        #[cfg(unix)]
        let unix = std::os::unix::net::UnixStream::connect(&self.socket).is_ok();
        #[cfg(not(unix))]
        let unix = true;
        tcp && unix
    }
}

impl Drop for Filter {
    fn drop(&mut self) {
        self.inner.stopped.store(true, Ordering::SeqCst);
        // If a wake failed, an accept thread can stay blocked, so joining could hang the caller.
        if self.wake_acceptors() {
            for thread in self.accepting.drain(..) {
                let _ = thread.join();
            }
        }
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn serve_tcp(listener: TcpListener, inner: Arc<Inner>) {
    loop {
        let accepted = listener.accept();
        if inner.stopped.load(Ordering::SeqCst) {
            return;
        }
        match accepted {
            Ok((stream, _)) => dispatch(&inner, Conn::Tcp(stream)),
            // Usually out of file descriptors; pausing stops this loop spinning until some free up.
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// Holds a slot of the connection cap, released when the connection's thread ends.
struct Slot(Arc<Inner>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

fn dispatch(inner: &Arc<Inner>, mut client: Conn) {
    if inner.active.fetch_add(1, Ordering::SeqCst) >= inner.limits.max_connections {
        inner.active.fetch_sub(1, Ordering::SeqCst);
        let _ = client.set_write_timeout(Some(Duration::from_secs(1)));
        let _ = client.write_all(
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 28\r\nConnection: close\r\n\r\nstopcord: too many requests\n",
        );
        return;
    }
    let slot = Slot(inner.clone());
    // A failed spawn drops the closure, which frees the slot and closes the client.
    let _ = thread::Builder::new()
        .name("sandbox-conn".into())
        .spawn(move || {
            let inner = slot.0.clone();
            let _slot = slot;
            handle(&inner, client);
        });
}

fn handle(inner: &Inner, mut client: Conn) {
    let _ = client.set_write_timeout(Some(WRITE_TIMEOUT));
    let (head, rest) = match read_head(&mut client, inner.limits.head_timeout, &inner.stopped) {
        Ok(read) => read,
        Err(HeadError::TooLarge) => {
            refuse(
                &mut client,
                "431 Request Header Fields Too Large",
                "stopcord: request header too large",
            );
            return;
        }
        Err(HeadError::Gone) => return,
    };
    let Some(target) = parse_head(&head) else {
        refuse(
            &mut client,
            "400 Bad Request",
            "stopcord: not a proxy request this filter can read",
        );
        return;
    };

    if !inner.allow().permits(&target.host) {
        // Reported before the reply, so a client that hangs up can't lose the report. A full queue
        // drops the report, not the block.
        let _ = inner.denied.try_send(Denied {
            host: target.host.clone(),
            port: target.port,
        });
        refuse(
            &mut client,
            "403 Forbidden",
            &format!("stopcord: {} is not on the allow list", target.host),
        );
        return;
    }

    let mut upstream = match (inner.connect)(&target.host, target.port) {
        Ok(stream) => stream,
        Err(_) => {
            refuse(
                &mut client,
                "502 Bad Gateway",
                &format!("stopcord: could not reach {}", target.host),
            );
            return;
        }
    };
    let _ = upstream.set_write_timeout(Some(WRITE_TIMEOUT));
    let sent = match &target.kind {
        Kind::Connect => client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n"),
        Kind::Plain { head } => upstream.write_all(head),
    };
    if sent.is_err() || upstream.write_all(&rest).is_err() {
        return;
    }
    tunnel(
        client,
        Conn::Tcp(upstream),
        inner.limits.idle_timeout,
        &inner.stopped,
    );
}

fn connect_direct(host: &str, port: u16) -> io::Result<TcpStream> {
    let mut last = io::Error::from(io::ErrorKind::AddrNotAvailable);
    for address in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = error,
        }
    }
    Err(last)
}

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::thread;
    use std::time::Duration;

    use anyhow::bail;

    use super::{Conn, Inner, dispatch};

    /// Binds the socket, replacing one left by a crashed run but never a live socket or another file.
    pub(super) fn bind(path: &Path) -> anyhow::Result<UnixListener> {
        if let Ok(meta) = fs::symlink_metadata(path) {
            if !meta.file_type().is_socket() {
                bail!("{} exists and is not a socket", path.display());
            }
            if UnixStream::connect(path).is_ok() {
                bail!("{} is in use", path.display());
            }
            fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(listener)
    }

    pub(super) fn serve(listener: UnixListener, inner: Arc<Inner>) {
        loop {
            let accepted = listener.accept();
            if inner.stopped.load(Ordering::SeqCst) {
                return;
            }
            match accepted {
                Ok((stream, _)) => dispatch(&inner, Conn::Unix(stream)),
                Err(_) => thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}
