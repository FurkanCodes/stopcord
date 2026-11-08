//! Where the daemon keeps its socket, log and rule overrides.

use std::path::PathBuf;

/// `$SAWTOOTH_HOME`, or `~/.sawtooth`.
pub fn home() -> PathBuf {
    if let Some(dir) = std::env::var_os("SAWTOOTH_HOME") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".sawtooth")
}

/// `$SAWTOOTH_SOCKET`, or `<home>/sawtooth.sock`.
pub fn socket() -> PathBuf {
    std::env::var_os("SAWTOOTH_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("sawtooth.sock"))
}

/// Rule files here replace the bundled ones with the same id.
pub fn rules_dir() -> PathBuf {
    home().join("rules")
}

pub fn log_file() -> PathBuf {
    home().join("daemon.log")
}

/// Expands a leading `~/`.
pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(rest),
        None if path == "~" => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default(),
        None => PathBuf::from(path),
    }
}
