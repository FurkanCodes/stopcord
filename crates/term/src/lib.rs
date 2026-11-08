//! A terminal: a pseudo-terminal, the program running in it, and an in-memory
//! copy of its screen.
//!
//! Output is read on a background thread, applied to the screen, then handed
//! to the caller as raw bytes so a viewer can draw it too. Terminal queries
//! (cursor position, device attributes, colours) are answered here, because a
//! terminal often runs with nobody watching it.

mod process;
mod replies;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result, anyhow};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub use process::{Process, process_info};

/// Environment variables that mark a process as running inside another agent's session.
pub const SESSION_MARKERS: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
    "AI_AGENT",
    "CODEX_SANDBOX",
    "CODEX_SANDBOX_NETWORK_DISABLED",
];

/// What to run and how big the terminal starts.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    /// The program and its arguments. The first one is looked up on `PATH`.
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Extra environment variables, on top of the caller's own.
    pub env: Vec<(String, String)>,
    pub rows: u16,
    pub cols: u16,
    /// Lines of scrollback kept in memory.
    pub scrollback: usize,
}

impl SpawnSpec {
    pub fn new(argv: Vec<String>) -> Self {
        SpawnSpec {
            argv,
            cwd: None,
            env: Vec::new(),
            rows: 40,
            cols: 120,
            scrollback: 2000,
        }
    }
}

/// Something that happened in a terminal.
#[derive(Debug, Clone)]
pub enum Event {
    /// Bytes the program wrote, already applied to the screen. `seq` counts chunks
    /// from 1, so a viewer can tell which ones a [`Redraw`] already holds.
    Output { data: Vec<u8>, seq: u64 },
    /// The program ended. The exit code, when there was one.
    Exited(Option<u32>),
}

/// The screen as text.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Visible rows top to bottom, trailing spaces and empty trailing rows removed.
    pub text: String,
    /// The last title the program set.
    pub title: String,
    /// The last OSC 9;4 progress report, such as `4;0`.
    pub progress: String,
    pub rows: u16,
    pub cols: u16,
    /// True while a full-screen program uses the alternate screen.
    pub alternate: bool,
}

/// The current screen as escape sequences, and the last output chunk it includes.
#[derive(Debug, Clone)]
pub struct Redraw {
    pub data: Vec<u8>,
    pub seq: u64,
}

/// Captures what the screen itself doesn't keep.
#[derive(Default)]
struct Sink {
    title: String,
    progress: String,
    /// Replies to terminal queries, waiting to be written back to the program.
    replies: Vec<u8>,
    /// The last output chunk applied to the screen. Lives under the parser's lock,
    /// so a redraw and its `seq` always agree.
    seq: u64,
    /// Recent raw output, replayed into a screen of the new width on resize, so lines
    /// rewrap the way a real terminal reflows them instead of being cut off.
    log: Vec<u8>,
}

/// How much raw output a terminal keeps for reflowing on resize.
const LOG_LIMIT: usize = 512 * 1024;

impl Sink {
    fn record(&mut self, bytes: &[u8]) {
        self.log.extend_from_slice(bytes);
        if self.log.len() > LOG_LIMIT {
            // Drop the older half, starting the rest at a line so it begins cleanly.
            let cut = self.log.len() - LOG_LIMIT / 2;
            let start = self.log[cut..].iter().position(|&b| b == b'\n').map_or(cut, |i| cut + i + 1);
            self.log.drain(..start);
        }
    }
}

impl vt100::Callbacks for Sink {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = String::from_utf8_lossy(title).into_owned();
    }

    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        if let Some(reply) = replies::csi(screen, i1, params, c) {
            self.replies.extend_from_slice(&reply);
        }
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        if let [b"9", rest @ ..] = params {
            if rest.first() == Some(&&b"4"[..]) {
                self.progress = rest
                    .iter()
                    .map(|p| String::from_utf8_lossy(p).into_owned())
                    .collect::<Vec<_>>()
                    .join(";");
            }
            return;
        }
        if let Some(reply) = replies::osc(params) {
            self.replies.extend_from_slice(&reply);
        }
    }
}

pub struct Terminal {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    parser: Mutex<vt100::Parser<Sink>>,
    scrollback: usize,
    pid: Option<u32>,
}

impl Terminal {
    /// Starts `spec.argv` in a new pseudo-terminal. `on_event` is called from a
    /// background thread for every chunk of output and once when the program ends.
    pub fn spawn(
        spec: SpawnSpec,
        on_event: impl Fn(Event) + Send + 'static,
    ) -> Result<Arc<Terminal>> {
        let program = spec.argv.first().ok_or_else(|| anyhow!("nothing to run"))?;
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: spec.rows,
                cols: spec.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("couldn't open a pseudo-terminal")?;

        let mut cmd = CommandBuilder::new(program);
        cmd.args(&spec.argv[1..]);
        if let Some(cwd) = &spec.cwd {
            cmd.cwd(cwd);
        }
        // An agent's session markers tell the agents started here that they run inside
        // that session, so they act like sub-agents (Claude Code stops saving transcripts).
        // Settings such as CLAUDE_CODE_USE_BEDROCK still pass through.
        for marker in SESSION_MARKERS {
            cmd.env_remove(marker);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (key, value) in &spec.env {
            cmd.env(key, value);
        }
        let child = pty
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("couldn't start {program}"))?;
        drop(pty.slave);

        let mut reader = pty.master.try_clone_reader()?;
        let writer = pty.master.take_writer()?;
        let pid = child.process_id();
        let term = Arc::new(Terminal {
            master: Mutex::new(pty.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            parser: Mutex::new(vt100::Parser::new_with_callbacks(
                spec.rows,
                spec.cols,
                spec.scrollback,
                Sink::default(),
            )),
            scrollback: spec.scrollback,
            pid,
        });

        let reading = Arc::clone(&term);
        thread::Builder::new()
            .name("sawtooth-term-read".into())
            .spawn(move || {
                let mut buf = vec![0u8; 16 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let (replies, seq) = {
                                let mut parser = reading.parser.lock().unwrap();
                                parser.callbacks_mut().record(&buf[..n]);
                                parser.process(&buf[..n]);
                                let sink = parser.callbacks_mut();
                                sink.seq += 1;
                                (std::mem::take(&mut sink.replies), sink.seq)
                            };
                            if !replies.is_empty() {
                                let _ = reading.write(&replies);
                            }
                            on_event(Event::Output {
                                data: buf[..n].to_vec(),
                                seq,
                            });
                        }
                    }
                }
                let code = reading
                    .child
                    .lock()
                    .unwrap()
                    .wait()
                    .ok()
                    .map(|s| s.exit_code());
                on_event(Event::Exited(code));
            })?;
        Ok(term)
    }

    /// The program's process id.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Sends bytes to the program, as if typed.
    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        if rows == 0 || cols == 0 {
            return Err(anyhow!("a terminal needs at least one row and one column"));
        }
        {
            let mut parser = self.parser.lock().unwrap();
            if parser.screen().size().1 == cols {
                parser.screen_mut().set_size(rows, cols);
            } else {
                *parser = reflowed(&mut parser, rows, cols, self.scrollback);
            }
        }
        self.master.lock().unwrap().resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    /// The screen as plain text, with the title.
    pub fn snapshot(&self) -> Snapshot {
        let parser = self.parser.lock().unwrap();
        let screen = parser.screen();
        let (rows, cols) = screen.size();
        let mut lines: Vec<String> = screen
            .rows(0, cols)
            .map(|row| row.trim_end().to_string())
            .collect();
        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        Snapshot {
            text: lines.join("\n"),
            title: parser.callbacks().title.clone(),
            progress: parser.callbacks().progress.clone(),
            rows,
            cols,
            alternate: screen.alternate_screen(),
        }
    }

    /// Escape sequences that redraw the current screen from blank, for a viewer that
    /// connects late. Draw this first, then only the output chunks after `seq`.
    pub fn redraw(&self) -> Redraw {
        let parser = self.parser.lock().unwrap();
        Redraw {
            data: parser.screen().state_formatted(),
            seq: parser.callbacks().seq,
        }
    }

    /// The process in the foreground right now: the shell, or whatever it is running.
    pub fn foreground(&self) -> Option<Process> {
        let leader = self.master.lock().unwrap().process_group_leader()?;
        process_info(leader as u32)
    }

    /// Ends the program.
    pub fn kill(&self) -> Result<()> {
        self.child.lock().unwrap().kill()?;
        Ok(())
    }
}

/// A screen of the new size holding the same output, rewrapped.
fn reflowed(old: &mut vt100::Parser<Sink>, rows: u16, cols: u16, scrollback: usize) -> vt100::Parser<Sink> {
    let before = std::mem::take(old.callbacks_mut());
    let mut fresh = vt100::Parser::new_with_callbacks(rows, cols, scrollback, Sink::default());
    fresh.process(&before.log);
    let sink = fresh.callbacks_mut();
    // The program already got its answers the first time round.
    sink.replies.clear();
    sink.seq = before.seq;
    sink.title = before.title;
    sink.progress = before.progress;
    sink.log = before.log;
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn sh(script: &str) -> SpawnSpec {
        SpawnSpec::new(vec!["/bin/sh".into(), "-c".into(), script.into()])
    }

    fn wait_exit(rx: &mpsc::Receiver<Event>) -> Option<u32> {
        loop {
            match rx
                .recv_timeout(Duration::from_secs(10))
                .expect("the program never ended")
            {
                Event::Exited(code) => return code,
                Event::Output { .. } => {}
            }
        }
    }

    #[test]
    fn resizing_rewraps_long_lines() {
        let (tx, rx) = mpsc::channel();
        let mut spec = sh("printf 'a%.0s' $(seq 150); printf 'b\\n'; sleep 1");
        spec.cols = 120;
        let term = Terminal::spawn(spec, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(400));
        term.resize(24, 99).unwrap();
        let text = term.snapshot().text.replace('\n', "");
        assert!(text.contains(&format!("{}b", "a".repeat(150))), "{text:?}");
        assert_eq!(term.snapshot().cols, 99);
        wait_exit(&rx);
    }

    #[test]
    fn another_agents_session_markers_stay_out() {
        // This test process may itself run inside an agent's session; if not, pretend it does.
        // SAFETY: no other test reads or writes these variables.
        unsafe { std::env::set_var("CLAUDE_CODE_CHILD_SESSION", "1") };
        let (tx, rx) = mpsc::channel();
        let mut spec = sh("echo \"[${CLAUDE_CODE_CHILD_SESSION}:${HOME:+home}]\"");
        spec.env.push(("SAWTOOTH_PROBE".into(), "1".into()));
        let term = Terminal::spawn(spec, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        wait_exit(&rx);
        let text = term.snapshot().text;
        assert!(text.contains("[:home]"), "{text:?}");
    }

    #[test]
    fn screen_title_and_exit_code() {
        let (tx, rx) = mpsc::channel();
        let term = Terminal::spawn(
            sh("printf '\\033]2;fixing tax\\007'; echo hello; exit 3"),
            move |e| {
                let _ = tx.send(e);
            },
        )
        .unwrap();
        assert_eq!(wait_exit(&rx), Some(3));
        let snap = term.snapshot();
        assert!(snap.text.contains("hello"), "{:?}", snap.text);
        assert_eq!(snap.title, "fixing tax");
    }

    #[test]
    fn answers_cursor_position_queries() {
        // The program asks where the cursor is and prints the reply it reads back.
        let script =
            "stty raw -echo; printf 'ab\\033[6n'; dd bs=1 count=6 2>/dev/null | od -c | head -1";
        let (tx, rx) = mpsc::channel();
        let term = Terminal::spawn(sh(script), move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        wait_exit(&rx);
        let text = term.snapshot().text;
        // Row 1, column 3: ESC [ 1 ; 3 R
        assert!(text.contains("033   [   1   ;   3   R"), "{text}");
    }

    #[test]
    fn finds_the_foreground_process() {
        let (tx, _rx) = mpsc::channel();
        let term = Terminal::spawn(sh("exec sleep 5"), move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        let mut found = None;
        for _ in 0..50 {
            found = term.foreground();
            if found.as_ref().is_some_and(|p| p.name == "sleep") {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let process = found.expect("no foreground process");
        assert_eq!(process.name, "sleep");
        assert_eq!(process.argv.first().map(String::as_str), Some("sleep"));
        term.kill().unwrap();
    }

    #[test]
    fn resize_changes_the_screen() {
        let (tx, _rx) = mpsc::channel();
        let term = Terminal::spawn(sh("sleep 5"), move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        term.resize(10, 30).unwrap();
        let snap = term.snapshot();
        assert_eq!((snap.rows, snap.cols), (10, 30));
        term.kill().unwrap();
    }
}
