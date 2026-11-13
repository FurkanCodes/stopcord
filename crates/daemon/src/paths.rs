//! Where the daemon keeps its socket, log and rule overrides.

use std::path::PathBuf;

/// `$STOPCORD_HOME`, or `~/.stopcord`.
pub fn home() -> PathBuf {
    if let Some(dir) = std::env::var_os("STOPCORD_HOME") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let home = base.join(".stopcord");
    carry_over(&base.join(".sawtooth"), &home);
    home
}

/// Before it was called Stopcord, the app kept everything in `~/.sawtooth`. Repos' worktree records
/// and installed hooks name paths in there, so the new name becomes a link to the old folder rather
/// than a move, and everything that points at the old paths keeps working.
fn carry_over(old: &std::path::Path, new: &std::path::Path) {
    if new.symlink_metadata().is_err() && old.is_dir() {
        let _ = std::os::unix::fs::symlink(old, new);
    }
}

/// `$STOPCORD_SOCKET`, or `<home>/stopcord.sock`.
pub fn socket() -> PathBuf {
    std::env::var_os("STOPCORD_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("stopcord.sock"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_folder_is_carried_over_by_a_link_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let (old, new) = (dir.path().join(".sawtooth"), dir.path().join(".stopcord"));
        carry_over(&old, &new);
        assert!(new.symlink_metadata().is_err(), "nothing to carry over, nothing made");

        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("settings.yaml"), "keepAwake: true\n").unwrap();
        carry_over(&old, &new);
        assert_eq!(std::fs::read_to_string(new.join("settings.yaml")).unwrap(), "keepAwake: true\n");

        let other = tempfile::tempdir().unwrap();
        let mine = other.path().join(".stopcord");
        std::fs::create_dir(&mine).unwrap();
        carry_over(&old, &mine);
        assert!(!mine.symlink_metadata().unwrap().file_type().is_symlink(), "an existing folder is left alone");
    }
}
