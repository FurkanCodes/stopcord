//! Turns a policy into a macOS Seatbelt profile.
//!
//! The profile denies by default and later rules win, so every narrowing `deny` comes after the broad
//! `allow` it narrows. Seatbelt compares real paths, which is why every path goes through [`resolve`].

use std::path::{Path, PathBuf};

use crate::paths::{GIT_POINTERS, GIT_WRITES, protected_git_paths, resolve, resolve_parent};
use crate::policy::{Network, Policy, Reads};

/// Everything a normal program needs that isn't a path or the network.
const BASE: &str = r#"(version 1)
(deny default)
(allow process-exec process-fork)
(allow process-info*)
(allow signal (target same-sandbox))
(allow sysctl-read)
(allow file-ioctl)
(allow pseudo-tty)
(allow ipc-posix-shm* ipc-posix-sem)
(allow iokit-open)
(allow mach-lookup)
(allow mach-register)
(allow system-socket)
(allow user-preference-read)
"#;

/// Device nodes programs write to, kept separate from the policy's own paths.
const DEVICE_WRITES: &str = r#"  (literal "/dev/null") (literal "/dev/zero") (literal "/dev/random") (literal "/dev/urandom") (literal "/dev/dtracehelper")
  (regex #"^/dev/tty") (regex #"^/dev/ptmx") (regex #"^/dev/pts/") (regex #"^/dev/fd")
"#;

/// App launching runs outside the sandbox, so LaunchServices is closed. Everything else in `mach-lookup`
/// stays open because harness logins reach the Keychain through it.
const NO_APP_LAUNCHING: &str = r##"(deny mach-lookup (global-name "com.apple.coreservices.launchservicesd") (global-name "com.apple.coreservices.quarantine-resolver") (global-name-regex #"^com\.apple\.lsd\."))
(deny process-exec (literal "/usr/bin/open") (literal "/usr/bin/osascript"))
"##;

/// System folders every program reads to start, for a policy that reads only what's listed.
const SYSTEM_READS: &[&str] = &[
    "/System",
    "/usr",
    "/bin",
    "/sbin",
    "/Library",
    "/opt",
    "/etc",
    "/private/etc",
    "/var/db",
    "/private/var/db",
    "/private/var/select",
    "/dev",
    "/Applications/Xcode.app",
];

/// Everything, or only the listed paths. A listed policy still sees every file's metadata,
/// because finding a path means looking at each folder on the way to it.
fn read_allows(policy: &Policy) -> String {
    let Reads::Listed(listed) = &policy.reads else {
        return "(allow file-read*)\n".to_string();
    };
    let mut paths: Vec<String> = SYSTEM_READS.iter().map(|p| real(Path::new(p))).collect();
    paths.push(real(&policy.worktree));
    if let Some(common) = &policy.git_common_dir {
        paths.push(real(common));
    }
    paths.extend(policy.write.iter().map(|p| real(p)));
    paths.extend(listed.iter().map(|p| real(p)));
    paths.push(real(&policy.program));
    if let Some(report) = &policy.report_socket {
        paths.push(real(report));
    }
    dedupe(&mut paths);
    let mut out = String::from("(allow file-read-metadata)\n(allow file-read*\n  (literal \"/\")\n");
    for path in paths {
        out.push_str(&format!("  {}\n", subpath(&path)));
    }
    for prefix in &policy.write_prefix {
        out.push_str(&format!("  (regex #\"^{}\")\n", regex_escape(&real(prefix))));
    }
    out.push_str(")\n");
    out
}

/// Renders the Seatbelt profile for a policy.
pub fn seatbelt_profile(policy: &Policy) -> String {
    let mut out = String::from(BASE);
    out.push_str(&read_allows(policy));
    out.push_str(&network_allows(policy));
    out.push_str(&write_allows(policy));

    let deny_read = policy.deny_read.iter().flat_map(|p| both_forms(p));
    push_deny(
        &mut out,
        "file-read*",
        deny_read.map(|p| subpath(&p)).collect(),
    );
    push_deny(&mut out, "file-write*", write_denies(policy));

    out.push_str(NO_APP_LAUNCHING);
    out.push_str(&network_denies(policy));
    if let Some(report) = &policy.report_socket {
        out.push_str(&format!(
            "(allow network-outbound (literal {}))\n",
            quote(&real(report))
        ));
    }
    out
}

/// `Off` opens nothing, not even loopback, because macOS can't scope loopback to this sandbox.
fn network_allows(policy: &Policy) -> String {
    match policy.network {
        Network::Off => String::new(),
        Network::Open => "(allow network*)\n".to_string(),
        Network::Filter { port, .. } => format!(
            concat!(
                "(allow network-outbound (remote ip \"localhost:{port}\"))\n",
                "(allow network-bind (local ip \"localhost:*\"))\n",
                "(allow network-inbound (local ip \"localhost:*\"))\n",
                "(allow network-outbound (remote ip \"localhost:*\"))\n",
                "(allow network-outbound (literal \"/private/var/run/mDNSResponder\"))\n",
            ),
            port = port
        ),
    }
}

fn network_denies(policy: &Policy) -> String {
    if policy.network == Network::Off {
        return String::new();
    }
    let mut out = String::new();
    for socket in policy.masked_sockets.iter().flat_map(|p| both_forms(p)) {
        out.push_str(&format!(
            "(deny network-outbound (literal {}))\n",
            quote(&socket)
        ));
    }
    for port in &policy.deny_ports {
        out.push_str(&format!(
            "(deny network-outbound (remote ip \"localhost:{port}\"))\n"
        ));
    }
    out
}

fn write_allows(policy: &Policy) -> String {
    let mut paths = vec![real(&policy.worktree)];
    if let Some(common) = &policy.git_common_dir {
        let common = resolve(common);
        paths.extend(GIT_WRITES.iter().map(|g| private(&common.join(g))));
    }
    paths.extend(policy.write.iter().map(|p| real(p)));
    dedupe(&mut paths);

    let mut out = String::from("(allow file-write*\n");
    out.push_str(DEVICE_WRITES);
    for path in paths {
        out.push_str(&format!("  {}\n", subpath(&path)));
    }
    for prefix in &policy.write_prefix {
        out.push_str(&format!("  (regex #\"^{}\")\n", regex_escape(&real(prefix))));
    }
    out.push_str(")\n");
    out
}

/// Filters for everything that must stay unwritable under the writable paths.
fn write_denies(policy: &Policy) -> Vec<String> {
    let mut paths: Vec<PathBuf> = policy.read_only.clone();
    paths.extend(protected_git_paths(policy));
    let mut filters: Vec<String> = paths
        .iter()
        .flat_map(|p| both_forms(p))
        .map(|p| subpath(&p))
        .collect();
    // `.git` in a linked worktree is a one-line file saying where the repository is; rewriting it would
    // redirect git run later outside the sandbox. Literal, so a real `.git` directory's contents stay open.
    let dot_git = private(&resolve_parent(&policy.worktree.join(".git")));
    filters.push(format!("(literal {})", quote(&dot_git)));
    if let Some(common) = &policy.git_common_dir {
        let names = GIT_POINTERS
            .iter()
            .map(|n| regex_escape(n))
            .collect::<Vec<_>>()
            .join("|");
        let root = regex_escape(&private(&resolve(common)));
        filters.push(format!(
            "(regex {})",
            quote(&format!("^{root}/worktrees/[^/]+/({names})$"))
        ));
    }
    filters
}

/// Appends a deny rule, unless there is nothing to deny: a `deny` with no filter matches everything.
fn push_deny(out: &mut String, operation: &str, mut filters: Vec<String>) {
    dedupe(&mut filters);
    if !filters.is_empty() {
        out.push_str(&format!("(deny {operation} {})\n", filters.join(" ")));
    }
}

fn subpath(path: &str) -> String {
    format!("(subpath {})", quote(path))
}

/// Both the real path and the path as named. Protecting only the real path would leave a symlink under
/// a writable folder free to be replaced, and a program reading through it would then see the new file.
fn both_forms(path: &Path) -> Vec<String> {
    let mut forms = vec![real(path), private(&resolve_parent(path))];
    dedupe(&mut forms);
    forms
}

fn real(path: &Path) -> String {
    private(&resolve(path))
}

/// `/tmp`, `/var` and `/etc` are symlinks into `/private` on macOS. A real macOS path has been through
/// `canonicalize` already, so this matters only when the profile is rendered on another OS.
fn private(path: &Path) -> String {
    let text = path.to_string_lossy();
    for root in ["/tmp", "/var", "/etc"] {
        if text == root || text.starts_with(&format!("{root}/")) {
            return format!("/private{text}");
        }
    }
    text.into_owned()
}

/// A double-quoted SBPL string.
fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Escapes regular-expression metacharacters so a path matches only itself.
fn regex_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if r"\.+*?()|[]{}^$".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn dedupe(items: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    items.retain(|item| seen.insert(item.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Network;
    use std::path::PathBuf;

    fn policy() -> Policy {
        Policy::new(
            "/nowhere-sawtooth/wt".into(),
            "/nowhere-sawtooth/home".into(),
            "/bin/app".into(),
        )
    }

    fn filter(port: u16) -> Network {
        Network::Filter {
            port,
            socket: "/nowhere-sawtooth/f.sock".into(),
        }
    }

    fn pos(profile: &str, needle: &str) -> usize {
        profile
            .find(needle)
            .unwrap_or_else(|| panic!("{needle} not in profile:\n{profile}"))
    }

    fn last(profile: &str, needle: &str) -> usize {
        profile
            .rfind(needle)
            .unwrap_or_else(|| panic!("{needle} not in profile:\n{profile}"))
    }

    #[test]
    fn reads_everything_unless_told_otherwise() {
        assert!(seatbelt_profile(&policy()).contains("(allow file-read*)\n"));
    }

    #[test]
    fn a_listed_policy_reads_only_its_paths_and_the_system() {
        let mut p = policy();
        p.reads = Reads::Listed(vec!["/opt/tools".into()]);
        let s = seatbelt_profile(&p);
        assert!(!s.contains("(allow file-read*)\n"), "{s}");
        assert!(s.contains("(allow file-read-metadata)"), "{s}");
        let allow = &s[pos(&s, "(allow file-read*\n")..pos(&s, "(allow file-write*")];
        for want in ["/usr", "/System", "/opt/tools"] {
            assert!(allow.contains(&format!("(subpath \"{}\")", real(Path::new(want)))), "missing {want} in {allow}");
        }
        assert!(allow.contains(&format!("(subpath \"{}\")", real(&p.worktree))), "its own folder: {allow}");
        assert!(pos(&s, "(allow file-read*\n") < pos(&s, "(deny file-read*"), "denies still win: {s}");
    }

    #[test]
    fn starts_default_deny() {
        assert!(seatbelt_profile(&policy()).starts_with("(version 1)\n(deny default)\n"));
    }

    #[test]
    fn worktree_and_extra_writes_are_writable() {
        let mut p = policy();
        p.write.push("/nowhere-sawtooth/state".into());
        let s = seatbelt_profile(&p);
        assert!(s.contains(r#"(subpath "/nowhere-sawtooth/wt")"#));
        assert!(s.contains(r#"(subpath "/nowhere-sawtooth/state")"#));
        assert!(s.contains(r#"(subpath "/nowhere-sawtooth/home/.npm")"#));
    }

    #[test]
    fn tmp_and_var_become_their_real_paths() {
        let mut p = policy();
        p.write.push("/var/folders/zz/x".into());
        let s = seatbelt_profile(&p);
        assert!(s.contains(r#"(subpath "/private/tmp")"#), "{s}");
        assert!(
            s.contains(r#"(subpath "/private/var/folders/zz/x")"#),
            "{s}"
        );
        assert!(!s.contains(r#"(subpath "/tmp")"#), "{s}");
    }

    #[test]
    fn symlinks_resolve_and_denies_cover_the_link_too() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let target = root.join("real");
        std::fs::create_dir(&target).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let mut p = policy();
        p.write.push(link.join("sub"));
        p.read_only.push(link.clone());
        let s = seatbelt_profile(&p);
        let real = target.to_str().unwrap();
        let named = link.to_str().unwrap();
        assert!(s.contains(&format!(r#"(subpath "{real}/sub")"#)), "{s}");
        let deny = &s[pos(&s, "(deny file-write*")..];
        assert!(deny.contains(&format!(r#"(subpath "{real}")"#)), "{s}");
        assert!(deny.contains(&format!(r#"(subpath "{named}")"#)), "{s}");
    }

    #[test]
    fn a_write_prefix_becomes_an_anchored_regex() {
        let mut p = policy();
        p.write_prefix.push(PathBuf::from("/nowhere-sawtooth/.claude.json"));
        let s = seatbelt_profile(&p);
        assert!(s.contains(r#"(regex #"^/nowhere-sawtooth/\.claude\.json")"#), "{s}");
    }

    #[test]
    fn quotes_and_backslashes_are_escaped() {
        let mut p = policy();
        p.write.push(PathBuf::from("/nowhere-sawtooth/a\"b\\c"));
        let s = seatbelt_profile(&p);
        assert!(
            s.contains(r#"(subpath "/nowhere-sawtooth/a\"b\\c")"#),
            "{s}"
        );
    }

    #[test]
    fn denies_come_after_the_broad_allows() {
        let mut p = policy();
        p.read_only.push("/nowhere-sawtooth/wt/keep".into());
        p.masked_sockets.push("/nowhere-sawtooth/d.sock".into());
        p.deny_ports.push(7777);
        p.network = filter(9000);
        let s = seatbelt_profile(&p);
        let writes = pos(&s, "(allow file-write*");
        let denied_read = pos(&s, "(deny file-read*");
        let denied_write = pos(&s, "(deny file-write*");
        assert!(pos(&s, "(allow file-read*)") < denied_read);
        assert!(writes < denied_read && writes < denied_write);
        assert!(pos(&s, "(allow mach-lookup)") < pos(&s, "(deny mach-lookup"));
        assert!(pos(&s, "(allow process-exec") < pos(&s, "(deny process-exec"));
        let loopback = last(&s, r#"(allow network-outbound (remote ip "localhost:*"))"#);
        assert!(
            loopback
                < pos(
                    &s,
                    r#"(deny network-outbound (remote ip "localhost:7777"))"#
                )
        );
        assert!(
            loopback
                < pos(
                    &s,
                    r#"(deny network-outbound (literal "/nowhere-sawtooth/d.sock"))"#
                )
        );
    }

    #[test]
    fn read_denials_and_read_only_paths_are_listed() {
        let mut p = policy();
        p.read_only
            .push("/nowhere-sawtooth/home/.claude/settings.json".into());
        let s = seatbelt_profile(&p);
        let deny_read = &s[pos(&s, "(deny file-read*")..];
        assert!(deny_read.contains(r#"(subpath "/nowhere-sawtooth/home/.ssh")"#));
        assert!(deny_read.contains(r#"(subpath "/nowhere-sawtooth/home/.sawtooth/secrets")"#));
        let deny_write = &s[pos(&s, "(deny file-write*")..];
        assert!(deny_write.contains(r#"(subpath "/nowhere-sawtooth/home/.claude/settings.json")"#));
    }

    #[test]
    fn git_common_dir_opens_objects_but_not_hooks_or_config() {
        let mut p = policy();
        p.git_common_dir = Some("/nowhere-sawtooth/proj/.git".into());
        let s = seatbelt_profile(&p);
        let (allows, denies) = s.split_at(pos(&s, "(deny file-write*"));
        for w in [
            "objects",
            "refs",
            "logs",
            "worktrees",
            "packed-refs",
            "index",
        ] {
            assert!(
                allows.contains(&format!(r#"(subpath "/nowhere-sawtooth/proj/.git/{w}")"#)),
                "{w}"
            );
        }
        for r in ["hooks", "config"] {
            let rule = format!(r#"(subpath "/nowhere-sawtooth/proj/.git/{r}")"#);
            assert!(!allows.contains(&rule), "{r} allowed");
            assert!(denies.contains(&rule), "{r} not denied");
        }
    }

    #[test]
    fn git_pointers_that_redirect_hooks_stay_read_only() {
        let mut p = policy();
        p.git_common_dir = Some("/nowhere-sawtooth/pr+oj/.git".into());
        let s = seatbelt_profile(&p);
        let denies = &s[pos(&s, "(deny file-write*")..];
        assert!(
            denies.contains(r#"(literal "/nowhere-sawtooth/wt/.git")"#),
            "{s}"
        );
        assert!(
            denies.contains(r#"(subpath "/nowhere-sawtooth/wt/.git/hooks")"#),
            "{s}"
        );
        assert!(
            denies.contains(r#"(subpath "/nowhere-sawtooth/wt/.git/config")"#),
            "{s}"
        );
        // The `+` must be escaped as a regex and its backslash escaped again for the SBPL string.
        assert!(
            denies.contains(
                r#"(regex "^/nowhere-sawtooth/pr\\+oj/\\.git/worktrees/[^/]+/(commondir|gitdir|config\\.worktree)$")"#
            ),
            "{s}"
        );
    }

    #[test]
    fn report_socket_is_allowed_by_literal_after_the_masks() {
        let mut p = policy();
        p.network = filter(9000);
        p.masked_sockets.push("/nowhere-sawtooth/r.sock".into());
        p.report_socket = Some("/nowhere-sawtooth/r.sock".into());
        let s = seatbelt_profile(&p);
        let deny = pos(
            &s,
            r#"(deny network-outbound (literal "/nowhere-sawtooth/r.sock"))"#,
        );
        let allow = pos(
            &s,
            r#"(allow network-outbound (literal "/nowhere-sawtooth/r.sock"))"#,
        );
        assert!(deny < allow, "{s}");
    }

    #[test]
    fn app_launching_is_closed_and_mach_lookup_stays_broad() {
        let s = seatbelt_profile(&policy());
        assert!(s.contains("(allow mach-lookup)\n"), "{s}");
        assert!(s.contains(r#"(global-name "com.apple.coreservices.launchservicesd")"#));
        assert!(s.contains(r#"(global-name "com.apple.coreservices.quarantine-resolver")"#));
        assert!(s.contains(r##"(global-name-regex #"^com\.apple\.lsd\.")"##));
        assert!(s.contains(
            r#"(deny process-exec (literal "/usr/bin/open") (literal "/usr/bin/osascript"))"#
        ));
    }

    #[test]
    fn off_has_no_network_rules_at_all() {
        let mut p = policy();
        p.deny_ports.push(7777);
        let s = seatbelt_profile(&p);
        assert!(!s.contains("network"), "{s}");
    }

    #[test]
    fn off_still_allows_the_report_socket() {
        let mut p = policy();
        p.report_socket = Some("/nowhere-sawtooth/r.sock".into());
        let s = seatbelt_profile(&p);
        assert!(s.contains(r#"(allow network-outbound (literal "/nowhere-sawtooth/r.sock"))"#));
        assert!(!s.contains("localhost"), "{s}");
    }

    #[test]
    fn filter_opens_the_filter_port_loopback_and_dns_only() {
        let mut p = policy();
        p.network = filter(9000);
        let s = seatbelt_profile(&p);
        assert!(s.contains(r#"(allow network-outbound (remote ip "localhost:9000"))"#));
        assert!(s.contains(r#"(allow network-outbound (remote ip "localhost:*"))"#));
        assert!(s.contains(r#"(allow network-bind (local ip "localhost:*"))"#));
        assert!(
            s.contains(r#"(allow network-outbound (literal "/private/var/run/mDNSResponder"))"#)
        );
        assert!(!s.contains("(allow network*)"), "{s}");
        assert!(!s.contains(r#"(remote ip "*:*")"#), "{s}");
    }

    #[test]
    fn open_allows_all_network_but_keeps_denied_ports_and_masks() {
        let mut p = policy();
        p.network = Network::Open;
        p.deny_ports.push(7777);
        p.masked_sockets.push("/nowhere-sawtooth/d.sock".into());
        let s = seatbelt_profile(&p);
        let allow = pos(&s, "(allow network*)");
        assert!(
            allow
                < pos(
                    &s,
                    r#"(deny network-outbound (remote ip "localhost:7777"))"#
                )
        );
        assert!(
            allow
                < pos(
                    &s,
                    r#"(deny network-outbound (literal "/nowhere-sawtooth/d.sock"))"#
                )
        );
    }

    #[test]
    fn empty_lists_never_become_a_blanket_deny() {
        let mut p = policy();
        p.deny_read.clear();
        p.read_only.clear();
        let s = seatbelt_profile(&p);
        assert!(!s.contains("(deny file-read*"), "{s}");
        assert!(!s.contains("(deny file-write* )"), "{s}");
        assert!(!s.contains("(deny file-write*)"), "{s}");
    }

    #[test]
    fn regex_metacharacters_in_a_path_are_escaped() {
        assert_eq!(regex_escape("a.b+c(d)"), r"a\.b\+c\(d\)");
    }

    #[test]
    fn paths_are_not_repeated() {
        let mut p = policy();
        p.write.push("/nowhere-sawtooth/wt".into());
        let s = seatbelt_profile(&p);
        assert_eq!(
            s.matches(r#"(subpath "/nowhere-sawtooth/wt")"#).count(),
            1,
            "{s}"
        );
    }
}
