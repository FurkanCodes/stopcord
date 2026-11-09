//! The Linux forwarder that runs inside the sandbox.
//!
//! The sandbox's network namespace is empty apart from loopback, so tools talk to a port on loopback
//! here, and this carries each connection out through the filter's Unix socket.

/// The Linux forwarder: listens on 127.0.0.1:port, forwards each connection to the Unix socket, runs
/// argv, exits with its status.
///
/// Takes `<port> <socket> -- argv...`, with or without a leading `sandbox-bridge`. Port 0 starts no
/// forwarder. Exits 2 on bad arguments, 125 if the port can't be bound, and 126 or 127 if the command can't be started.
pub fn bridge_main(args: &[String]) -> i32 {
    #[cfg(unix)]
    {
        imp::run(args)
    }
    #[cfg(not(unix))]
    {
        let _ = args;
        eprintln!("sandbox-bridge: not supported on this platform");
        125
    }
}

#[cfg(unix)]
mod imp {
    use std::io;
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::Command;
    use std::thread;

    struct Parsed {
        port: u16,
        socket: PathBuf,
        argv: Vec<String>,
    }

    fn parse(args: &[String]) -> Option<Parsed> {
        let args = match args.first() {
            Some(first) if first == "sandbox-bridge" => &args[1..],
            _ => args,
        };
        let [port, socket, separator, argv @ ..] = args else {
            return None;
        };
        if separator != "--" || argv.is_empty() {
            return None;
        }
        Some(Parsed {
            port: port.parse().ok()?,
            socket: PathBuf::from(socket),
            argv: argv.to_vec(),
        })
    }

    pub(super) fn run(args: &[String]) -> i32 {
        let Some(parsed) = parse(args) else {
            eprintln!("usage: sandbox-bridge <port> <socket> -- <command> [args...]");
            return 2;
        };
        // Port 0 means no filter: this process is here only for its signal handling.
        if parsed.port != 0 {
            // Bound before the command starts, so its first request can't find the port closed.
            let listener = match TcpListener::bind(("127.0.0.1", parsed.port)) {
                Ok(listener) => listener,
                Err(error) => {
                    eprintln!("sandbox-bridge: port {}: {error}", parsed.port);
                    return 125;
                }
            };
            let socket = parsed.socket;
            thread::spawn(move || accept(listener, socket));
        }
        survive_terminal_interrupts();

        match Command::new(&parsed.argv[0])
            .args(&parsed.argv[1..])
            .status()
        {
            Ok(status) => status
                .code()
                .or_else(|| status.signal().map(|signal| 128 + signal))
                .unwrap_or(1),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                eprintln!("sandbox-bridge: {}: not found", parsed.argv[0]);
                127
            }
            Err(error) => {
                eprintln!("sandbox-bridge: {}: {error}", parsed.argv[0]);
                126
            }
        }
    }

    extern "C" fn do_nothing(_signal: libc::c_int) {}

    /// Ctrl-C goes to the whole foreground process group, this process included. If this process died
    /// the sandbox would go with it, ending an agent that handles Ctrl-C itself. A handler is used
    /// rather than ignoring the signal because exec resets handlers but keeps ignored signals, and the
    /// command must start with normal signal behaviour. This also replaces the ignore set up by `wrap`.
    fn survive_terminal_interrupts() {
        let handler = do_nothing as extern "C" fn(libc::c_int) as libc::sighandler_t;
        // Safety: installing a handler that does nothing, which is async-signal-safe.
        unsafe {
            libc::signal(libc::SIGINT, handler);
            libc::signal(libc::SIGQUIT, handler);
        }
    }

    fn accept(listener: TcpListener, socket: PathBuf) {
        for stream in listener.incoming().flatten() {
            let socket = socket.clone();
            thread::spawn(move || forward(stream, socket));
        }
    }

    fn forward(tcp: TcpStream, socket: PathBuf) {
        // Dropping the TCP connection tells the tool the proxy failed, as a refused connect would.
        let Ok(unix) = UnixStream::connect(&socket) else {
            return;
        };
        let (Ok(tcp_back), Ok(unix_back)) = (tcp.try_clone(), unix.try_clone()) else {
            return;
        };
        thread::spawn(move || {
            copy(&tcp_back, &unix_back);
        });
        copy(&unix, &tcp);
    }

    /// Copies one direction, then passes on the end of input without closing the other direction.
    fn copy<R, W>(mut from: R, mut to: W)
    where
        R: io::Read,
        W: io::Write + Shut,
    {
        let _ = io::copy(&mut from, &mut to);
        to.shut_write();
    }

    /// A stream whose write side can be closed on its own.
    trait Shut {
        fn shut_write(&self);
    }

    impl Shut for &TcpStream {
        fn shut_write(&self) {
            let _ = self.shutdown(Shutdown::Write);
        }
    }

    impl Shut for &UnixStream {
        fn shut_write(&self) {
            let _ = self.shutdown(Shutdown::Write);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::net::UnixListener;
    use std::thread;
    use std::time::{Duration, Instant};

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    #[test]
    fn bad_arguments_exit_2() {
        assert_eq!(bridge_main(&args(&[])), 2);
        assert_eq!(bridge_main(&args(&["notaport", "/s", "--", "x"])), 2);
        assert_eq!(bridge_main(&args(&["80", "/s", "x"])), 2);
        assert_eq!(bridge_main(&args(&["80", "/s", "--"])), 2);
    }

    #[test]
    fn exits_with_the_command_status() {
        let code = bridge_main(&args(&[
            "0",
            "/nonexistent.sock",
            "--",
            "/bin/sh",
            "-c",
            "exit 7",
        ]));
        assert_eq!(code, 7);
    }

    #[test]
    fn accepts_the_subcommand_name_as_first_argument() {
        let code = bridge_main(&args(&[
            "sandbox-bridge",
            "0",
            "/nonexistent.sock",
            "--",
            "/bin/sh",
            "-c",
            "exit 3",
        ]));
        assert_eq!(code, 3);
    }

    #[test]
    fn a_command_killed_by_a_signal_exits_128_plus_the_signal() {
        let code = bridge_main(&args(&[
            "0",
            "/nonexistent.sock",
            "--",
            "/bin/sh",
            "-c",
            "kill -9 $$",
        ]));
        assert_eq!(code, 137);
    }

    #[test]
    fn port_zero_runs_the_command_with_no_forwarder() {
        let code = bridge_main(&args(&["0", "/dev/null", "--", "/bin/sh", "-c", "exit 4"]));
        assert_eq!(code, 4);
    }

    #[test]
    fn a_missing_command_exits_127() {
        assert_eq!(
            bridge_main(&args(&[
                "0",
                "/nonexistent.sock",
                "--",
                "/nonexistent/program"
            ])),
            127
        );
    }

    #[test]
    fn the_command_keeps_default_signal_handling() {
        // The bridge survives Ctrl-C with a handler, not SIG_IGN, because an ignored signal would be
        // inherited across exec and the command could no longer be interrupted.
        let code = bridge_main(&args(&[
            "0",
            "/nonexistent.sock",
            "--",
            "/bin/sh",
            "-c",
            "kill -INT $$; exit 5",
        ]));
        assert_eq!(code, 130);
    }

    #[test]
    fn connections_are_forwarded_to_the_unix_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.sock");
        let unix = UnixListener::bind(&path).unwrap();
        thread::spawn(move || {
            for mut stream in unix.incoming().flatten() {
                thread::spawn(move || {
                    let mut buf = [0u8; 64];
                    while let Ok(n) = stream.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                        stream.write_all(&buf[..n]).unwrap();
                    }
                });
            }
        });

        let port = free_port();
        let argv = args(&[
            &port.to_string(),
            path.to_str().unwrap(),
            "--",
            "/bin/sleep",
            "2",
        ]);
        let bridge = thread::spawn(move || bridge_main(&argv));

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut client = loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => break stream,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Err(e) => panic!("{e}"),
            }
        };
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client.write_all(b"hello").unwrap();
        let mut back = [0u8; 5];
        client.read_exact(&mut back).unwrap();
        assert_eq!(&back, b"hello");
        assert_eq!(bridge.join().unwrap(), 0);
    }
}
