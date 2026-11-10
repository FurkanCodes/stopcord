//! The VM runner: each terminal runs in a fresh Linux container in Docker.
//!
//! The container gets a copy of the work in a Docker volume and nothing from the Mac: no
//! mounts of its folders, no Docker socket, no network. It sits alone on an internal network
//! where the one thing it can reach is a relay that forwards two ports to the Mac: its web
//! filter, and the port `st` reports on.
//!
//! The copy is kept in step with the terminal's folder on the Mac by `sawtooth-sync`, which
//! runs on both sides. Whichever side changed since they last matched wins, and when both did
//! nothing is overwritten. Work comes back through git: commits that have to follow on from
//! the folder's own, and the rest as a tree git checks out. Git on the Mac never runs inside
//! anything the container wrote.

use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const FILES: &[(&str, &str)] = &[
    ("Dockerfile", include_str!("../vm/Dockerfile")),
    ("st", include_str!("../vm/st")),
    ("sawtooth-sync", include_str!("../vm/sawtooth-sync")),
    ("relay", include_str!("../vm/relay")),
    ("probe", include_str!("../vm/probe")),
];

/// The sync script, run on the Mac's side.
const SYNC: &str = include_str!("../vm/sawtooth-sync");

/// The name a person's own image starts FROM.
pub const LATEST: &str = "sawtooth-vm:latest";

/// Where harness logins live, shared by every VM agent.
pub const HOME_VOLUME: &str = "sawtooth-home";

/// Written into the terminal's git folder once the container is about to start, or when it can't.
const READY: &str = "sawtooth-vm-ready";

const PROXY: &str = "http://sawtooth:3128";

/// The environment variable a terminal's report key travels in.
pub const KEY: &str = "SAWTOOTH_KEY";

/// Names a secret can't take, because Sawtooth sets them: a secret named HTTPS_PROXY could send
/// an agent's traffic around its filter.
const RESERVED: &[&str] = &["HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy", "NO_PROXY", "no_proxy", "ALL_PROXY", "all_proxy", "HOME", "PATH", "TERM"];

/// Whether `name` can be a secret a VM agent gets: an environment variable's name that Sawtooth doesn't set itself.
pub fn secret_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let shaped = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !shaped {
        return Err(format!("{name:?} isn't an environment variable's name. Use letters, digits and underscores."));
    }
    if RESERVED.contains(&name) || name.starts_with("SAWTOOTH_") {
        return Err(format!("{name} is set by Sawtooth, so it can't be a secret."));
    }
    Ok(())
}

/// The values of a terminal's secrets, read on the Mac, and the names that have none.
pub fn secret_values(store: &Path, names: &[String]) -> (Vec<(String, String)>, Vec<String>) {
    let (mut found, mut missing) = (Vec::new(), Vec::new());
    for name in names.iter().filter(|n| secret_name(n).is_ok()) {
        match crate::settings::get_secret(store, name) {
            Some(value) => found.push((name.clone(), value)),
            None => missing.push(name.clone()),
        }
    }
    (found, missing)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// Sawtooth's image, named by what's in it, so a change to any file builds a new one.
pub fn image() -> String {
    let mut all = Vec::new();
    for (name, body) in FILES {
        all.extend_from_slice(name.as_bytes());
        all.push(0);
        all.extend_from_slice(body.as_bytes());
        all.push(0);
    }
    format!("sawtooth-vm:{}", &sha(&all)[..12])
}

/// A short name for a folder, for its volume and its record.
pub fn key(dir: &Path) -> String {
    sha(dir.display().to_string().as_bytes())[..16].to_string()
}

pub fn volume(dir: &Path) -> String {
    format!("sawtooth-work-{}", key(dir))
}

/// A terminal's name among every Sawtooth's containers. Tokens start again with each daemon,
/// and a test's daemon runs beside the real one, so the home is part of it.
fn terminal_id(spec: &Spec) -> String {
    format!("{}-{}", &key(&spec.store)[..8], spec.token)
}

/// The label every container and network of one Sawtooth home carries.
pub fn home_label(store: &Path) -> String {
    format!("sawtooth.home={}", key(store))
}

fn network(spec: &Spec) -> String {
    format!("sawtooth-net-{}", terminal_id(spec))
}

fn relay(spec: &Spec) -> String {
    format!("sawtooth-relay-{}", terminal_id(spec))
}

fn agent(spec: &Spec) -> String {
    format!("sawtooth-agent-{}", terminal_id(spec))
}

/// Docker's command line tool. An app started from the Dock has a short PATH, so the usual
/// places are looked at too.
pub fn docker() -> Option<PathBuf> {
    let mut places: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME") {
        places.push(PathBuf::from(home).join(".docker/bin"));
    }
    places.extend(["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin", "/Applications/Docker.app/Contents/Resources/bin"].map(PathBuf::from));
    places.into_iter().map(|p| p.join("docker")).find(|p| p.is_file())
}

/// Docker, if it's installed and running. Otherwise what the person has to do.
pub fn available() -> Result<PathBuf, String> {
    let docker = docker().ok_or("The VM runner needs Docker: Docker Desktop on a Mac, or Docker Engine on Linux. It isn't installed.")?;
    let out = Command::new(&docker).args(["version", "--format", "{{.Server.Version}}"]).output();
    match out {
        Ok(o) if o.status.success() => Ok(docker),
        _ => Err("Docker is installed but isn't running. Start Docker Desktop, then try again.".to_string()),
    }
}

fn run(docker: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(docker).args(args).output().map_err(|e| format!("couldn't run docker: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!("docker {} failed: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

pub fn has_image(docker: &Path, tag: &str) -> bool {
    run(docker, &["image", "inspect", "--format", "{{.Id}}", tag]).is_ok()
}

/// Builds Sawtooth's image unless it's there. Shows Docker's output unless `quiet`.
pub fn ensure_image(docker: &Path, store_home: &Path, quiet: bool) -> Result<String, String> {
    let tag = image();
    if has_image(docker, &tag) {
        return Ok(tag);
    }
    let dir = store_home.join("vm").join("build");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
    for (name, body) in FILES {
        std::fs::write(dir.join(name), body).map_err(|e| format!("couldn't write {name}: {e}"))?;
    }
    let mut build = Command::new(docker);
    build.args(["build", "-t", &tag, "-t", LATEST, "."]).current_dir(&dir);
    let ok = if quiet {
        let out = build.output().map_err(|e| format!("couldn't run docker: {e}"))?;
        if !out.status.success() {
            return Err(format!("building the VM image failed:\n{}", crate::runs::last_lines(&String::from_utf8_lossy(&out.stderr), 15)));
        }
        true
    } else {
        build.status().map_err(|e| format!("couldn't run docker: {e}"))?.success()
    };
    if !ok {
        return Err("building the VM image failed. Docker's output is above.".to_string());
    }
    Ok(tag)
}

/// One side of a mirror: somewhere `sawtooth-sync` can run.
pub trait Side {
    fn sync(&self, args: &[String], input: Option<&[u8]>) -> Result<Vec<u8>, String>;
}

fn output(mut cmd: Command, input: Option<&[u8]>) -> Result<Vec<u8>, String> {
    cmd.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("couldn't start the sync: {e}"))?;
    // Written from another thread, so a big bundle can't fill a pipe both ways.
    let writer = input.map(|data| {
        let mut stdin = child.stdin.take();
        let data = data.to_vec();
        std::thread::spawn(move || {
            if let Some(s) = stdin.as_mut() {
                let _ = s.write_all(&data);
            }
        })
    });
    let out = child.wait_with_output().map_err(|e| format!("the sync didn't finish: {e}"))?;
    if let Some(w) = writer {
        let _ = w.join();
    }
    if out.status.success() {
        Ok(out.stdout)
    } else {
        // Git's first error says why; what follows is how it gave up.
        let text = String::from_utf8_lossy(&out.stderr);
        let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("hint:")).collect();
        let why = lines.iter().find(|l| l.starts_with("error:") || l.starts_with("fatal:")).or(lines.last()).copied().unwrap_or("it failed");
        Err(why.trim_start_matches("sawtooth-sync: ").to_string())
    }
}

/// A folder on this machine.
pub struct Local<'a> {
    pub dir: &'a Path,
}

impl Side for Local<'_> {
    fn sync(&self, args: &[String], input: Option<&[u8]>) -> Result<Vec<u8>, String> {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(SYNC).arg("sawtooth-sync").args(args).current_dir(self.dir).env("GIT_TERMINAL_PROMPT", "0").env("LC_ALL", "C");
        output(cmd, input)
    }
}

/// A Docker volume, reached through a container that has no network.
pub struct Volume {
    pub docker: PathBuf,
    pub image: String,
    pub volume: String,
}

impl Side for Volume {
    fn sync(&self, args: &[String], input: Option<&[u8]>) -> Result<Vec<u8>, String> {
        let mut cmd = Command::new(&self.docker);
        cmd.args(["run", "--rm", "-i", "--network", "none", "--cap-drop", "ALL", "--security-opt", "no-new-privileges"]);
        cmd.args(["-v", &format!("{}:/work", self.volume), "-w", "/work", &self.image, "sawtooth-sync"]).args(args);
        output(cmd, input)
    }
}

/// Where a folder is: nothing committed yet, or a commit and the tree of everything in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Empty,
    At { head: String, tree: String },
}

fn state(side: &dyn Side, exclude: &[String]) -> Result<State, String> {
    let mut args = vec!["state".to_string(), "--".to_string()];
    args.extend(exclude.iter().cloned());
    let out = String::from_utf8_lossy(&side.sync(&args, None)?).trim().to_string();
    match out.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["empty"] => Ok(State::Empty),
        [head, tree] => Ok(State::At { head: head.to_string(), tree: tree.to_string() }),
        _ => Err(format!("the sync said something unexpected: {out}")),
    }
}

/// What a sync did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Synced {
    /// Both sides were already the same.
    Same,
    /// The folder's work went into the VM.
    Sent,
    /// The VM's work came back to the folder.
    Took,
}

/// A folder on the Mac and its copy in a VM, and the record of when they last matched.
pub struct Mirror<'a> {
    pub host: &'a dyn Side,
    pub vm: &'a dyn Side,
    pub record: PathBuf,
    /// The folder's branch, which the copy is on too. Empty when the folder isn't on one.
    pub branch: String,
    /// The branch work is compared with, such as main, so the copy has it too.
    pub base: String,
    /// Sawtooth's own files, which never cross.
    pub exclude: Vec<String>,
    /// The folder is a throwaway copy, whose history doesn't matter: only its kept files go anywhere.
    pub throwaway: bool,
}

impl Mirror<'_> {
    fn last(&self) -> Option<State> {
        std::fs::read_to_string(&self.record).ok().and_then(|s| serde_json::from_str(&s).ok())
    }

    fn remember(&self, s: &State) -> Result<(), String> {
        if let Some(parent) = self.record.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&self.record, serde_json::to_string(s).unwrap_or_default()).map_err(|e| format!("couldn't keep a record of the sync: {e}"))
    }

    fn with_exclude(&self, mut args: Vec<String>) -> Vec<String> {
        args.push("--".into());
        args.extend(self.exclude.iter().cloned());
        args
    }

    /// Brings whichever side changed up to the other.
    pub fn sync(&self) -> Result<Synced, String> {
        let host = state(self.host, &self.exclude)?;
        let State::At { head: host_head, .. } = &host else {
            return Err("the VM needs a folder with at least one commit".into());
        };
        let vm = state(self.vm, &self.exclude)?;
        let last = self.last();
        if host == vm {
            self.remember(&host)?;
            return Ok(Synced::Same);
        }
        // With no record, nothing says the VM's copy came from this folder, as when a path is reused,
        // so it's replaced rather than trusted.
        if vm == State::Empty || last.is_none() || last.as_ref() == Some(&vm) {
            let have = match &vm {
                State::At { head, .. } => head.clone(),
                State::Empty => "-".into(),
            };
            let bundle = self.host.sync(&self.with_exclude(vec!["pack".into(), have, self.base.clone()]), None)?;
            let mut args = vec!["unpack".to_string()];
            if !self.branch.is_empty() {
                args.extend(["--branch".into(), self.branch.clone()]);
            }
            if !self.base.is_empty() {
                args.extend(["--set-base".into(), self.base.clone()]);
            }
            self.vm.sync(&self.with_exclude(args), Some(&bundle)).map_err(|e| format!("the work couldn't be copied into the VM: {e}"))?;
            let now = state(self.vm, &self.exclude)?;
            if now != host {
                return Err("the VM's copy didn't come out the same as the folder".into());
            }
            self.remember(&host)?;
            return Ok(Synced::Sent);
        }
        if last.as_ref() == Some(&host) {
            let bundle = self.vm.sync(&self.with_exclude(vec!["pack".into(), host_head.clone(), String::new()]), None)?;
            let mut args = vec!["unpack".to_string()];
            if !self.throwaway {
                args.push("--ff-only".into());
            }
            if !self.branch.is_empty() {
                args.extend(["--branch".into(), self.branch.clone()]);
            }
            self.host.sync(&self.with_exclude(args), Some(&bundle)).map_err(|e| format!("the VM's work couldn't come back: {e}"))?;
            let now = state(self.host, &self.exclude)?;
            if now != vm {
                return Err("the folder didn't come out the same as the VM's copy".into());
            }
            self.remember(&vm)?;
            return Ok(Synced::Took);
        }
        Err("both the folder and its copy in the VM changed since they last matched, so neither was overwritten".into())
    }
}

/// What `vm-run` needs to start one terminal. Made by the runner, passed as JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Spec {
    /// The terminal's confinement token, which names its containers and network.
    pub token: String,
    /// The folder on the Mac the VM works on a copy of.
    pub dir: PathBuf,
    /// Sawtooth's home, for the build folder and the sync records.
    pub store: PathBuf,
    /// The image agents run in. Empty means Sawtooth's own.
    pub image: String,
    /// What runs in the container. Empty means a shell.
    pub command: String,
    pub setup: Vec<String>,
    pub base: String,
    pub exclude: Vec<String>,
    /// The folder is a throwaway copy for an isolated stage.
    pub throwaway: bool,
    /// The factory's knowledge base, copied in under each excluded name.
    pub knowledge: Option<PathBuf>,
    pub filter_port: u16,
    pub filter_socket: PathBuf,
    pub report_port: u16,
    pub report_socket: PathBuf,
    /// What proves a report is this terminal's. Never in a command line, where `ps` shows it to
    /// anything on the Mac: it travels in `SAWTOOTH_KEY`, and Docker takes it from there.
    #[serde(skip)]
    pub key: String,
    /// More for the container's environment, such as who commits.
    pub env: Vec<(String, String)>,
    /// Secrets the container gets, by name. Their values are read on the Mac when it starts.
    pub secrets: Vec<String>,
}

impl Spec {
    pub fn record(&self) -> PathBuf {
        record(&self.store, &self.dir)
    }
}

pub fn record(store: &Path, dir: &Path) -> PathBuf {
    store.join("vm").join("records").join(format!("{}.json", key(dir)))
}

/// The branch a folder is on, or nothing.
fn branch_of(dir: &Path) -> String {
    crate::worktree::git(dir, &["branch", "--show-current"]).unwrap_or_default()
}

/// Keeps a terminal's folder and its VM copy in step, whichever way that goes.
pub fn sync(spec: &Spec, docker: &Path) -> Result<Synced, String> {
    let host = Local { dir: &spec.dir };
    let vm = Volume { docker: docker.to_path_buf(), image: image(), volume: volume(&spec.dir) };
    Mirror {
        host: &host,
        vm: &vm,
        record: spec.record(),
        branch: branch_of(&spec.dir),
        base: spec.base.clone(),
        exclude: spec.exclude.clone(),
        throwaway: spec.throwaway,
    }
    .sync()
}

/// The markers a terminal's folder gets: ready to start, or failed.
pub fn markers(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let ready = dir.join(crate::worktree::git(dir, &["rev-parse", "--git-path", READY]).ok()?);
    let failed = PathBuf::from(format!("{}.failed", ready.display()));
    Some((ready, failed))
}

/// The relay's way to the Mac's filter and report port.
fn relay_targets(spec: &Spec) -> (String, String, Vec<String>) {
    if cfg!(target_os = "linux") {
        // Docker on Linux shares the kernel, so the Mac-side sockets can be handed straight in.
        let mounts = vec![
            "-v".into(),
            format!("{}:/relay/net", spec.filter_socket.display()),
            "-v".into(),
            format!("{}:/relay/report", spec.report_socket.display()),
            "--user".into(),
            "root".into(),
        ];
        ("UNIX-CONNECT:/relay/net".into(), "UNIX-CONNECT:/relay/report".into(), mounts)
    } else {
        (format!("TCP:host.docker.internal:{}", spec.filter_port), format!("TCP:host.docker.internal:{}", spec.report_port), Vec::new())
    }
}

/// The environment every container on an agent's network gets.
fn agent_env(spec: &Spec) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = ["HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"].iter().map(|k| (k.to_string(), PROXY.to_string())).collect();
    env.extend([
        ("NO_PROXY".into(), "localhost,127.0.0.1".into()),
        ("no_proxy".into(), "localhost,127.0.0.1".into()),
        ("SAWTOOTH_REPORT".into(), "sawtooth:3129".into()),
        ("SAWTOOTH_ST".into(), "st".into()),
        ("TERM".into(), std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".into())),
    ]);
    env.extend(spec.env.iter().cloned());
    env
}

/// The parts of `docker run` an agent's container and its setup share.
fn agent_args(spec: &Spec, name: &str, image: &str) -> Vec<String> {
    let mut a: Vec<String> = ["run", "--rm", "-it", "--name", name, "--network", &network(spec)].iter().map(|s| s.to_string()).collect();
    a.extend(["--cap-drop", "ALL", "--security-opt", "no-new-privileges", "--pids-limit", "4096"].map(String::from));
    a.extend(["--label".into(), home_label(&spec.store)]);
    a.extend(["-v".into(), format!("{}:/work", volume(&spec.dir)), "-v".into(), format!("{HOME_VOLUME}:/home/agent"), "-w".into(), "/work".into()]);
    for (k, v) in agent_env(spec) {
        a.extend(["-e".into(), format!("{k}={v}")]);
    }
    // No values, so Docker copies them from its own environment rather than its command line.
    a.extend(["-e".into(), KEY.into()]);
    for name in spec.secrets.iter().filter(|n| secret_name(n).is_ok()) {
        a.extend(["-e".into(), name.clone()]);
    }
    a.push(image.to_string());
    a
}

/// Makes the network and starts the relay for a terminal. Clears anything left from an earlier one.
fn open_network(docker: &Path, spec: &Spec, helper_image: &str) -> Result<(), String> {
    cleanup(docker, spec);
    let net = network(spec);
    run(docker, &["network", "create", "--internal", "--label", &home_label(&spec.store), &net])?;
    let (filter, report, mounts) = relay_targets(spec);
    let mut args: Vec<String> = ["run", "-d", "--rm", "--init", "--name", &relay(spec), "--network", "bridge"].iter().map(|s| s.to_string()).collect();
    args.extend(["--label".into(), home_label(&spec.store)]);
    args.extend(mounts);
    args.extend(["-e".into(), format!("FILTER={filter}"), "-e".into(), format!("REPORT={report}"), "-e".into(), format!("AGENT={}", agent(spec))]);
    args.extend([helper_image.to_string(), "relay".into()]);
    run(docker, &args.iter().map(String::as_str).collect::<Vec<_>>())?;
    run(docker, &["network", "connect", "--alias", "sawtooth", &net, &relay(spec)])?;
    Ok(())
}

/// Removes a terminal's containers and network.
pub fn cleanup(docker: &Path, spec: &Spec) {
    let _ = run(docker, &["rm", "-f", &agent(spec), &format!("{}-setup", agent(spec)), &relay(spec)]);
    let _ = run(docker, &["network", "rm", &network(spec)]);
}

/// Forgets a folder's VM copy, once its work is over.
pub fn forget(store: &Path, dir: &Path) {
    let _ = std::fs::remove_file(record(store, dir));
    if let Some(docker) = docker() {
        let _ = run(&docker, &["volume", "rm", "-f", &volume(dir)]);
    }
}

/// The command line inside the container: the command, then a shell that stays.
pub fn inner_command(command: &str) -> String {
    let command = command.trim();
    if command.is_empty() {
        return "exec bash -l".into();
    }
    // A harness the factory names by its path on the Mac is on the VM's PATH by its name.
    let (first, rest) = command.split_once(char::is_whitespace).unwrap_or((command, ""));
    let first = first.rsplit('/').next().unwrap_or(first);
    let line = if rest.is_empty() { first.to_string() } else { format!("{first} {rest}") };
    format!("{line}; exec bash -l")
}

fn say(text: &str) {
    println!("\x1b[2msawtooth: {text}\x1b[0m");
}

fn fail(markers: Option<&(PathBuf, PathBuf)>, why: &str) -> i32 {
    eprintln!("sawtooth: {why}");
    if let Some((_, failed)) = markers {
        let _ = std::fs::write(failed, why);
    }
    // Keep the message on screen rather than closing the terminal on it.
    eprintln!("sawtooth: press return to close.");
    let _ = std::io::stdin().read_line(&mut String::new());
    1
}

/// `sawtooth-app vm-run <spec>`: the terminal's own process. It readies the VM, then becomes
/// `docker run`, so the terminal's process is the agent's.
pub fn main(args: &[String]) -> i32 {
    let Some(mut spec) = args.first().and_then(|a| serde_json::from_str::<Spec>(a).ok()) else {
        eprintln!("usage: sawtooth-app vm-run '<spec as json>'");
        return 2;
    };
    spec.key = std::env::var(KEY).unwrap_or_default();
    let markers = markers(&spec.dir);
    if let Some((ready, failed)) = &markers {
        let _ = std::fs::remove_file(ready);
        let _ = std::fs::remove_file(failed);
    }
    let docker = match available() {
        Ok(d) => d,
        Err(why) => return fail(markers.as_ref(), &why),
    };
    let tag = image();
    if !has_image(&docker, &tag) {
        say("building the VM image. This happens once and takes a few minutes.");
    }
    let tag = match ensure_image(&docker, &spec.store, false) {
        Ok(t) => t,
        Err(why) => return fail(markers.as_ref(), &why),
    };
    match sync(&spec, &docker) {
        Ok(Synced::Sent) => say("copied the work into the VM"),
        Ok(Synced::Took) => say("brought back work the VM had"),
        Ok(Synced::Same) => {}
        Err(why) => return fail(markers.as_ref(), &format!("{why}. The VM's copy is in the Docker volume {}.", volume(&spec.dir))),
    }
    if let Some(kb) = &spec.knowledge
        && let Ok(content) = std::fs::read(kb)
    {
        let vm = Volume { docker: docker.clone(), image: tag.clone(), volume: volume(&spec.dir) };
        for name in &spec.exclude {
            let _ = vm.sync(&["knowledge".to_string(), name.clone()], Some(&content));
        }
    }
    if let Err(why) = open_network(&docker, &spec, &tag) {
        cleanup(&docker, &spec);
        return fail(markers.as_ref(), &why);
    }
    let image = if spec.image.trim().is_empty() { tag } else { spec.image.trim().to_string() };
    let (secrets, missing) = secret_values(&spec.store, &spec.secrets);
    if !missing.is_empty() {
        say(&format!("{} has no value in sawtooth's secrets, so the VM doesn't get it", missing.join(", ")));
    }
    let setup_marker = crate::worktree::git(&spec.dir, &["rev-parse", "--git-path", crate::runner::SETUP_DONE]).ok().map(|p| spec.dir.join(p));
    if !spec.setup.is_empty() && setup_marker.as_ref().is_some_and(|m| !m.exists()) {
        say("running setup in the VM");
        let steps = spec.setup.iter().map(|s| format!("({s})")).collect::<Vec<_>>().join(" && ");
        let mut args = agent_args(&spec, &format!("{}-setup", agent(&spec)), &image);
        args.extend(["bash".into(), "-lc".into(), steps]);
        let ok = Command::new(&docker).args(&args).envs(secrets.iter().cloned()).status().is_ok_and(|s| s.success());
        if let Some(m) = &setup_marker {
            let _ = std::fs::write(if ok { m.clone() } else { PathBuf::from(format!("{}.failed", m.display())) }, "");
        }
        if !ok {
            cleanup(&docker, &spec);
            return fail(markers.as_ref(), "Setup failed, so the agent did not start.");
        }
    }
    if let Some((ready, _)) = &markers {
        let _ = std::fs::write(ready, "");
    }
    let mut args = agent_args(&spec, &agent(&spec), &image);
    args.extend(["bash".into(), "-lc".into(), inner_command(&spec.command)]);
    let err = Command::new(&docker).args(&args).envs(secrets).exec();
    cleanup(&docker, &spec);
    fail(None, &format!("couldn't start the container: {err}"))
}

/// Tries a VM runner's container from the inside, with the same network and relay an agent gets.
pub fn probe(docker: &Path, spec: &Spec, image: &str, mac_home: &Path, allowed_site: &str) -> Result<Vec<crate::probe::Check>, String> {
    open_network(docker, spec, &self::image())?;
    let mut args = agent_args(spec, &agent(spec), image);
    // No volume of its own: a probe has no work to keep.
    args.retain(|a| a != "-it");
    args.insert(2, "-i".into());
    let work = args.iter().position(|a| a.ends_with(":/work")).map(|i| i - 1);
    if let Some(i) = work {
        args.drain(i..i + 2);
    }
    let input = serde_json::json!({ "macHome": mac_home.display().to_string(), "macPort": spec.filter_port, "allowedSite": allowed_site }).to_string();
    args.extend(["probe".into(), input]);
    let out = Command::new(docker).args(&args).env(KEY, &spec.key).stdin(Stdio::null()).output();
    cleanup(docker, spec);
    let out = out.map_err(|e| format!("couldn't run the probe: {e}"))?;
    crate::probe::parse(&String::from_utf8_lossy(&out.stdout)).ok_or_else(|| {
        format!("the probe didn't report back: {}", crate::runs::last_lines(&String::from_utf8_lossy(&out.stderr), 5))
    })
}

/// A terminal's spec, when it's a VM terminal: its start command is `vm-run`.
pub fn spec_of(terminal: &serde_json::Value) -> Option<Spec> {
    let argv = terminal["argv"].as_array()?;
    if argv.get(1)?.as_str()? != "vm-run" {
        return None;
    }
    serde_json::from_str(argv.get(2)?.as_str()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::git;

    /// A repo with one commit, standing in for the folder on the Mac.
    fn host() -> tempfile::TempDir {
        let dir = crate::worktree::tests::repo();
        std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        git(dir.path(), &["add", "-A"]).unwrap();
        git(dir.path(), &["commit", "-qm", "a"]).unwrap();
        dir
    }

    struct Pair {
        host: tempfile::TempDir,
        vm: tempfile::TempDir,
        records: tempfile::TempDir,
        throwaway: bool,
    }

    impl Pair {
        fn new() -> Pair {
            Pair { host: host(), vm: tempfile::tempdir().unwrap(), records: tempfile::tempdir().unwrap(), throwaway: false }
        }
        fn sync(&self) -> Result<Synced, String> {
            let (h, v) = (Local { dir: self.host.path() }, Local { dir: self.vm.path() });
            Mirror {
                host: &h,
                vm: &v,
                record: self.records.path().join("r.json"),
                branch: branch_of(self.host.path()),
                base: String::new(),
                exclude: vec!["AGENTS.md".into()],
                throwaway: self.throwaway,
            }
            .sync()
        }
        fn h(&self, name: &str) -> PathBuf {
            self.host.path().join(name)
        }
        fn v(&self, name: &str) -> PathBuf {
            self.vm.path().join(name)
        }
        fn commit_in_vm(&self, msg: &str) {
            git(self.vm.path(), &["add", "-A"]).unwrap();
            git(self.vm.path(), &["-c", "user.name=a", "-c", "user.email=a@b", "commit", "-qm", msg]).unwrap();
        }
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }

    #[test]
    fn the_first_sync_copies_everything_in_including_uncommitted_work() {
        let p = Pair::new();
        std::fs::write(p.h("a.txt"), "changed\n").unwrap();
        std::fs::write(p.h("new.txt"), "untracked\n").unwrap();
        assert_eq!(p.sync().unwrap(), Synced::Sent);
        assert_eq!(read(&p.v("a.txt")), "changed\n");
        assert_eq!(read(&p.v("new.txt")), "untracked\n");
        assert_eq!(git(p.vm.path(), &["rev-parse", "HEAD"]).unwrap(), git(p.host.path(), &["rev-parse", "HEAD"]).unwrap());
        assert_eq!(git(p.vm.path(), &["branch", "--show-current"]).unwrap(), branch_of(p.host.path()));
        assert_eq!(git(p.vm.path(), &["status", "--porcelain"]).unwrap(), git(p.host.path(), &["status", "--porcelain"]).unwrap(), "uncommitted stays uncommitted");
        assert_eq!(p.sync().unwrap(), Synced::Same);
    }

    #[test]
    fn the_vms_commits_and_changes_come_back() {
        let p = Pair::new();
        p.sync().unwrap();
        std::fs::write(p.v("b.txt"), "from the vm\n").unwrap();
        p.commit_in_vm("vm work");
        std::fs::write(p.v("c.txt"), "not committed\n").unwrap();
        std::fs::remove_file(p.v("a.txt")).unwrap();
        assert_eq!(p.sync().unwrap(), Synced::Took);
        assert_eq!(git(p.host.path(), &["log", "-1", "--format=%s"]).unwrap(), "vm work");
        assert_eq!(read(&p.h("c.txt")), "not committed\n");
        assert!(!p.h("a.txt").exists(), "a deletion comes back too");
        assert!(git(p.host.path(), &["for-each-ref", "refs/sawtooth"]).unwrap().is_empty(), "no refs left behind");
        assert_eq!(p.sync().unwrap(), Synced::Same);
    }

    #[test]
    fn new_work_on_the_mac_goes_in_on_the_next_sync() {
        let p = Pair::new();
        p.sync().unwrap();
        std::fs::write(p.h("plan.md"), "the plan\n").unwrap();
        git(p.host.path(), &["add", "-A"]).unwrap();
        git(p.host.path(), &["commit", "-qm", "plan"]).unwrap();
        assert_eq!(p.sync().unwrap(), Synced::Sent);
        assert_eq!(read(&p.v("plan.md")), "the plan\n");
    }

    #[test]
    fn when_both_sides_changed_neither_is_overwritten() {
        let p = Pair::new();
        p.sync().unwrap();
        std::fs::write(p.h("a.txt"), "mac\n").unwrap();
        std::fs::write(p.v("a.txt"), "vm\n").unwrap();
        assert!(p.sync().unwrap_err().contains("both"));
        assert_eq!(read(&p.h("a.txt")), "mac\n");
        assert_eq!(read(&p.v("a.txt")), "vm\n");
    }

    #[test]
    fn a_copy_with_no_record_is_replaced_not_trusted() {
        let p = Pair::new();
        p.sync().unwrap();
        std::fs::write(p.v("stale.txt"), "from an old piece of work\n").unwrap();
        std::fs::remove_dir_all(p.records.path()).unwrap();
        assert_eq!(p.sync().unwrap(), Synced::Sent);
        assert!(!p.v("stale.txt").exists());
        assert!(!p.h("stale.txt").exists());
    }

    #[test]
    fn rewritten_history_does_not_come_back() {
        let p = Pair::new();
        p.sync().unwrap();
        let tip = git(p.host.path(), &["rev-parse", "HEAD"]).unwrap();
        git(p.vm.path(), &["-c", "user.name=a", "-c", "user.email=a@b", "commit", "-q", "--amend", "-m", "rewritten"]).unwrap();
        assert!(p.sync().unwrap_err().contains("rewritten"));
        assert_eq!(git(p.host.path(), &["rev-parse", "HEAD"]).unwrap(), tip);
    }

    #[test]
    fn a_throwaway_copy_takes_whatever_the_vm_did() {
        let mut p = Pair::new();
        p.throwaway = true;
        p.sync().unwrap();
        git(p.vm.path(), &["-c", "user.name=a", "-c", "user.email=a@b", "commit", "-q", "--amend", "-m", "rewritten"]).unwrap();
        assert_eq!(p.sync().unwrap(), Synced::Took);
        assert_eq!(git(p.host.path(), &["log", "-1", "--format=%s"]).unwrap(), "rewritten");
    }

    #[test]
    fn sawtooths_own_files_never_cross() {
        let p = Pair::new();
        // As kb::link leaves it: a link kept out of git by info/exclude.
        std::fs::write(p.h("knowledge.md"), "the mac's knowledge\n").unwrap();
        std::os::unix::fs::symlink(p.h("knowledge.md"), p.h("AGENTS.md")).unwrap();
        std::fs::write(p.host.path().join(".git/info/exclude"), "/AGENTS.md\n/knowledge.md\n").unwrap();
        p.sync().unwrap();
        assert!(!p.v("AGENTS.md").exists());
        std::fs::write(p.v("AGENTS.md"), "the vm wrote this\n").unwrap();
        std::fs::write(p.v("b.txt"), "x\n").unwrap();
        assert_eq!(p.sync().unwrap(), Synced::Took);
        assert_eq!(read(&p.h("AGENTS.md")), "the mac's knowledge\n");
        assert!(p.h("b.txt").exists());
    }

    #[test]
    fn a_vm_cannot_plant_a_git_hook_on_the_mac() {
        let p = Pair::new();
        p.sync().unwrap();
        // A tree with a .git folder in it can't be made with add, so it's built by hand.
        let vm = p.vm.path();
        std::fs::write(vm.join(".hook-body"), "#!/bin/sh\ntouch /tmp/sawtooth-pwned\n").unwrap();
        let blob = git(vm, &["hash-object", "-w", ".hook-body"]).unwrap();
        std::fs::remove_file(vm.join(".hook-body")).unwrap();
        let mk = |input: String| {
            let mut c = std::process::Command::new("git");
            c.arg("mktree").current_dir(vm).stdin(Stdio::piped()).stdout(Stdio::piped());
            let mut child = c.spawn().unwrap();
            child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
            String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap().trim().to_string()
        };
        let hooks = mk(format!("100755 blob {blob}\tpost-checkout\n"));
        let dotgit = mk(format!("040000 tree {hooks}\thooks\n"));
        let head_tree = git(vm, &["rev-parse", "HEAD^{tree}"]).unwrap();
        let listing = git(vm, &["ls-tree", &head_tree]).unwrap();
        let root = mk(format!("{listing}\n040000 tree {dotgit}\t.git\n"));
        let commit = git(vm, &["-c", "user.name=a", "-c", "user.email=a@b", "commit-tree", &root, "-p", "HEAD", "-m", "evil"]).unwrap();
        git(vm, &["update-ref", "HEAD", &commit]).unwrap();
        let before = git(p.host.path(), &["rev-parse", "HEAD"]).unwrap();
        let why = p.sync().unwrap_err();
        assert!(why.contains("hasDotgit"), "refused for the right reason: {why}");
        assert!(!p.host.path().join(".git/hooks/post-checkout").exists());
        assert_eq!(git(p.host.path(), &["rev-parse", "HEAD"]).unwrap(), before);
    }

    /// The same round trip through a real Docker volume. Needs Docker, so it's run by name:
    /// `cargo test -p sawtooth-app vm::tests::docker -- --ignored`.
    #[test]
    #[ignore]
    fn docker_round_trip_through_a_real_volume() {
        let docker = available().unwrap();
        let store = tempfile::tempdir().unwrap();
        let tag = ensure_image(&docker, store.path(), true).unwrap();
        let host = host();
        let records = tempfile::tempdir().unwrap();
        let vol = format!("sawtooth-test-{}", key(host.path()));
        let side = Volume { docker: docker.clone(), image: tag, volume: vol.clone() };
        let local = Local { dir: host.path() };
        let mirror = Mirror {
            host: &local,
            vm: &side,
            record: records.path().join("r.json"),
            branch: branch_of(host.path()),
            base: String::new(),
            exclude: Vec::new(),
            throwaway: false,
        };
        std::fs::write(host.path().join("new.txt"), "in\n").unwrap();
        let first = mirror.sync();
        let script = "cd /work && echo vm > from-vm.txt && git add -A && git -c user.name=v -c user.email=v@v commit -qm 'from the vm' && echo loose > loose.txt";
        let worked = Command::new(&docker).args(["run", "--rm", "-v", &format!("{vol}:/work"), &side.image, "sh", "-c", script]).status();
        let second = mirror.sync();
        let _ = run(&docker, &["volume", "rm", "-f", &vol]);
        assert_eq!(first.unwrap(), Synced::Sent);
        assert!(worked.unwrap().success());
        assert_eq!(second.unwrap(), Synced::Took);
        assert_eq!(git(host.path(), &["log", "-1", "--format=%s"]).unwrap(), "from the vm");
        assert_eq!(read(&host.path().join("loose.txt")), "loose\n");
        assert_eq!(read(&host.path().join("new.txt")), "in\n");
    }

    #[test]
    fn a_command_runs_by_its_name_and_a_shell_stays() {
        assert_eq!(inner_command("/Users/me/.local/bin/claude --model x"), "claude --model x; exec bash -l");
        assert_eq!(inner_command("codex"), "codex; exec bash -l");
        assert_eq!(inner_command(""), "exec bash -l");
    }

    #[test]
    fn the_image_is_named_by_what_is_in_it() {
        assert!(image().starts_with("sawtooth-vm:") && image().len() == "sawtooth-vm:".len() + 12);
        assert_eq!(image(), image());
        assert_ne!(volume(Path::new("/a")), volume(Path::new("/b")));
    }

    #[test]
    fn a_secret_is_an_environment_name_sawtooth_doesnt_set() {
        assert!(secret_name("CLAUDE_CODE_OAUTH_TOKEN").is_ok());
        assert!(secret_name("_private2").is_ok());
        for bad in ["", "2FAST", "MY-KEY", "A B", "../x", "HTTPS_PROXY", "no_proxy", "PATH", "SAWTOOTH_KEY", "SAWTOOTH_ANYTHING"] {
            assert!(secret_name(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn secrets_go_to_docker_by_name_and_their_values_stay_on_the_mac() {
        let store = tempfile::tempdir().unwrap();
        crate::settings::set_secret(store.path(), "OPENAI_API_KEY", "sk-very-secret").unwrap();
        let spec = Spec {
            token: "c1".into(),
            store: store.path().into(),
            secrets: vec!["OPENAI_API_KEY".into(), "GEMINI_API_KEY".into(), "HTTPS_PROXY".into()],
            ..Spec::default()
        };
        let args = agent_args(&spec, "a", "img");
        let line = args.join(" ");
        assert!(args.windows(2).any(|w| w[0] == "-e" && w[1] == "OPENAI_API_KEY"), "{line}");
        assert!(!line.contains("sk-very-secret") && !serde_json::to_string(&spec).unwrap().contains("sk-very-secret"));
        assert!(!args.windows(2).any(|w| w[0] == "-e" && w[1] == "HTTPS_PROXY"), "a reserved name is never passed: {line}");
        let (found, missing) = secret_values(store.path(), &spec.secrets);
        assert_eq!(found, vec![("OPENAI_API_KEY".to_string(), "sk-very-secret".to_string())]);
        assert_eq!(missing, vec!["GEMINI_API_KEY".to_string()]);
    }

    #[test]
    fn a_vm_terminal_is_known_by_its_start_command() {
        let spec = Spec { token: "c3".into(), dir: "/w".into(), ..Spec::default() };
        let t = serde_json::json!({ "argv": ["/app", "vm-run", serde_json::to_string(&spec).unwrap()] });
        assert_eq!(spec_of(&t), Some(spec));
        assert_eq!(spec_of(&serde_json::json!({ "argv": ["/bin/zsh", "-l"] })), None);
    }

    #[test]
    fn every_container_is_cut_off_and_reaches_only_the_relay() {
        let spec = Spec { token: "c3".into(), dir: "/w".into(), store: "/h".into(), key: "k".into(), ..Spec::default() };
        let a = agent_args(&spec, &agent(&spec), "img").join(" ");
        assert!(a.contains(&format!("--network {}", network(&spec))), "{a}");
        let other = Spec { store: "/test-home".into(), ..spec.clone() };
        assert_ne!(network(&spec), network(&other), "two sawtooths' terminal c3 are different terminals");
        assert!(a.contains("--cap-drop ALL") && a.contains("no-new-privileges"), "{a}");
        assert!(a.contains("HTTPS_PROXY=http://sawtooth:3128") && a.contains("-e SAWTOOTH_KEY "), "{a}");
        assert!(!a.contains("=k ") && !serde_json::to_string(&spec).unwrap().contains("\"k\""), "the key is in no command line: {a}");
        assert!(!a.contains("docker.sock") && !a.contains(":/Users") && !a.contains("/w:"), "nothing from the mac is mounted: {a}");
        assert!(a.ends_with(" img"));
    }
}
