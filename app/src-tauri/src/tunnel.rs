//! Starting the tunnel a person chose, watching its output for the public
//! address, and stopping it again. Sawtooth runs the person's tool and hosts nothing.

use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Result, bail};
use serde::Serialize;

use crate::settings::{self, TunnelKind, TunnelSettings};
use crate::shell;

const KEEP_LINES: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub running: bool,
    /// The public address, once it's known.
    pub url: String,
    /// The last thing that went wrong.
    pub error: String,
    pub output: Vec<String>,
}

/// The shell command for a tunnel. `home` is where a token secret is read from.
pub fn command_for(t: &TunnelSettings, port: u16, home: &Path) -> Result<String> {
    Ok(match t.kind {
        TunnelKind::None => bail!("no tunnel is set up"),
        TunnelKind::CloudflaredQuick => format!("cloudflared tunnel --url http://127.0.0.1:{port}"),
        TunnelKind::CloudflaredNamed => {
            let token = settings::get_secret(home, &t.token_secret)
                .ok_or_else(|| anyhow::anyhow!("the tunnel's token isn't set"))?;
            format!("cloudflared tunnel run --token '{token}' --url http://127.0.0.1:{port}")
        }
        TunnelKind::TailscaleFunnel => format!("tailscale funnel {port}"),
        TunnelKind::Ngrok => format!("ngrok http {port} --log stdout"),
        TunnelKind::Custom => {
            if t.command.trim().is_empty() {
                bail!("the custom tunnel has no command");
            }
            t.command.replace("{port}", &port.to_string())
        }
    })
}

/// The first public https address in a line of a tunnel's output.
pub fn find_url(line: &str) -> Option<String> {
    let at = line.find("https://")?;
    let url: String = line[at..]
        .chars()
        .take_while(|c| !c.is_whitespace() && !matches!(c, '|' | '"' | '\'' | '<' | '>' | ')' | ','))
        .collect();
    let host = url.strip_prefix("https://")?.split('/').next()?;
    // Cloudflare and others print links to their own docs and terms in the same output.
    let noise = ["cloudflare.com", "developers.", "github.com", "ngrok.com/docs", "dashboard.ngrok", "www."];
    let looks_like_tunnel = host.contains('.') && !noise.iter().any(|n| host.contains(n) && !host.ends_with("trycloudflare.com"));
    looks_like_tunnel.then_some(url)
}

/// Removes a token from text before it's shown.
fn hide(line: &str, command: &str) -> String {
    match command.split("--token '").nth(1).and_then(|r| r.split('\'').next()) {
        Some(token) if !token.is_empty() => line.replace(token, "••••"),
        _ => line.to_string(),
    }
}

#[derive(Default)]
pub struct Tunnel {
    child: Mutex<Option<Child>>,
    status: Arc<Mutex<Status>>,
}

impl Tunnel {
    pub fn status(&self) -> Status {
        let mut status = self.status.lock().unwrap().clone();
        if let Some(child) = self.child.lock().unwrap().as_mut()
            && matches!(child.try_wait(), Ok(Some(_)))
        {
            status.running = false;
        }
        status
    }

    pub fn start(&self, command: &str, known_url: &str) -> Result<()> {
        self.stop();
        let mut child = Command::new(shell::user_shell())
            .args(["-l", "-c", &format!("exec {command}")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()?;
        *self.status.lock().unwrap() = Status {
            running: true,
            url: known_url.to_string(),
            ..Status::default()
        };
        for pipe in [
            child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
            child.stderr.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let (status, command) = (Arc::clone(&self.status), command.to_string());
            thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    let line = hide(&line, &command);
                    let mut s = status.lock().unwrap();
                    if s.url.is_empty()
                        && let Some(url) = find_url(&line)
                    {
                        s.url = url;
                    }
                    s.output.push(line);
                    let extra = s.output.len().saturating_sub(KEEP_LINES);
                    s.output.drain(..extra);
                }
                let mut s = status.lock().unwrap();
                s.running = false;
                if s.url.is_empty() && s.error.is_empty() {
                    s.error = s.output.last().cloned().unwrap_or_else(|| "the tunnel stopped".into());
                }
            });
        }
        *self.child.lock().unwrap() = Some(child);
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            // SAFETY: only signals the process group this tunnel started.
            unsafe { libc::kill(-(child.id() as i32), libc::SIGTERM) };
            let _ = child.wait();
        }
        let mut s = self.status.lock().unwrap();
        s.running = false;
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn settings_for(kind: TunnelKind) -> TunnelSettings {
        TunnelSettings {
            kind,
            ..TunnelSettings::default()
        }
    }

    #[test]
    fn each_kind_has_its_command() {
        let home = tempfile::tempdir().unwrap();
        let cmd = |k| command_for(&settings_for(k), 9000, home.path()).unwrap();
        assert_eq!(cmd(TunnelKind::CloudflaredQuick), "cloudflared tunnel --url http://127.0.0.1:9000");
        assert_eq!(cmd(TunnelKind::TailscaleFunnel), "tailscale funnel 9000");
        assert_eq!(cmd(TunnelKind::Ngrok), "ngrok http 9000 --log stdout");
        assert!(command_for(&settings_for(TunnelKind::None), 9000, home.path()).is_err());
        let custom = TunnelSettings {
            kind: TunnelKind::Custom,
            command: "mytunnel --to {port}".into(),
            ..TunnelSettings::default()
        };
        assert_eq!(command_for(&custom, 9000, home.path()).unwrap(), "mytunnel --to 9000");
        assert!(command_for(&settings_for(TunnelKind::Custom), 9000, home.path()).is_err());
    }

    #[test]
    fn a_named_tunnel_needs_its_token_and_never_shows_it() {
        let home = tempfile::tempdir().unwrap();
        let named = TunnelSettings {
            kind: TunnelKind::CloudflaredNamed,
            token_secret: "CF".into(),
            ..TunnelSettings::default()
        };
        assert!(command_for(&named, 1, home.path()).is_err());
        settings::set_secret(home.path(), "CF", "tok123").unwrap();
        let command = command_for(&named, 1, home.path()).unwrap();
        assert!(command.contains("tok123"));
        assert_eq!(hide("failed with tok123 here", &command), "failed with •••• here");
    }

    #[test]
    fn it_finds_the_address_and_skips_links_to_docs() {
        assert_eq!(
            find_url("INF |  https://calm-river-12.trycloudflare.com  |").as_deref(),
            Some("https://calm-river-12.trycloudflare.com")
        );
        assert_eq!(find_url("https://m.tail1234.ts.net/ proxy").as_deref(), Some("https://m.tail1234.ts.net/"));
        assert_eq!(find_url("msg=started url=https://ab12.ngrok.app").as_deref(), Some("https://ab12.ngrok.app"));
        assert_eq!(find_url("see https://developers.cloudflare.com/x for more"), None);
        assert_eq!(find_url("https://www.cloudflare.com/terms"), None);
        assert_eq!(find_url("nothing here"), None);
    }

    #[test]
    fn it_runs_a_tunnel_finds_its_address_and_stops_it() {
        let t = Tunnel::default();
        t.start("sh -c 'echo ready at https://x1.example.test; sleep 30'", "").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while t.status().url.is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        let s = t.status();
        assert_eq!(s.url, "https://x1.example.test");
        assert!(s.running);
        t.stop();
        assert!(!t.status().running);
    }

    #[test]
    fn a_tunnel_that_dies_says_why() {
        let t = Tunnel::default();
        t.start("sh -c 'echo no such account >&2; exit 3'", "").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while t.status().error.is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        let s = t.status();
        assert!(!s.running);
        assert!(s.error.contains("no such account"), "{s:?}");
    }
}
