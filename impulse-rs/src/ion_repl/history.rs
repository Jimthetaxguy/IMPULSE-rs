//! `.impulse/ion_history` persistence for the ion REPL (TUI_SPEC.md T6).
//!
//! Resolves the `.impulse/` directory: explicit `IMPULSE_HOME`, else
//! `$HOME/.impulse` (`%USERPROFILE%` on Windows), else `.impulse` under the
//! current directory. This is character-for-character identical to
//! `impulse-desktop`'s `resolve_memory_root` (`impulse-desktop/src/bin/impulse_desktop.rs`).
//! Reimplemented here (rather than imported) because `impulse-desktop` is not
//! a dependency of `impulse-rs` — `src/lib.rs`'s `resolve_impulse_dir` is a
//! separate, currently-passthrough seam scoped to the `--impulse-dir` CLI
//! flag, not this env-var convention.
//!
//! **Not the same convention as `impulse-gui`'s `agent_panel::persistence`**
//! (a prior version of this doc comment incorrectly claimed it was): that
//! module accepts a blank `IMPULSE_HOME` as valid, then walks up from the
//! current directory looking for an *existing* `.impulse/` dir before ever
//! consulting `HOME`/`USERPROFILE`. The three implementations are not
//! interchangeable; do not assume they'd resolve to the same path for a
//! given `cwd`/env combination.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rustyline::DefaultEditor;

/// Resolve the `.impulse/` directory: explicit `IMPULSE_HOME`, else
/// `$HOME/.impulse` (or `%USERPROFILE%/.impulse`), else `.impulse` under the
/// current directory.
pub fn impulse_home() -> PathBuf {
    if let Ok(home) = std::env::var("IMPULSE_HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home);
        }
    }
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .filter(|value| !value.trim().is_empty());
    match home {
        Some(home) => PathBuf::from(home).join(".impulse"),
        None => PathBuf::from(".impulse"),
    }
}

/// Path to the ion REPL's persistent history file.
pub fn history_path() -> PathBuf {
    impulse_home().join("ion_history")
}

/// Load history from `path` into `editor`. A missing file is not an error
/// (first run) — other I/O/parse errors are returned so the caller can warn
/// without failing the session.
pub fn load(editor: &mut DefaultEditor, path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    editor
        .load_history(path)
        .with_context(|| format!("Failed to load ion history from {}", path.display()))
}

/// Append this session's new history entries to `path`, creating the parent
/// directory (and the file itself) if needed.
///
/// Uses rustyline's `append_history` rather than `save_history`
/// (overwrite-the-whole-file) for two reasons: `save_history` would silently
/// lose every line typed if the process is killed or panics before this call
/// runs (nothing is durable until exit), and with two concurrent `ion`
/// sessions the last one to exit would clobber the other's entries entirely.
/// Appending only the entries added since `load()` avoids both — prior
/// sessions' history is preserved regardless of exit order or crashes.
pub fn save(editor: &mut DefaultEditor, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create ion history directory {}",
                parent.display()
            )
        })?;
    }
    editor
        .append_history(path)
        .with_context(|| format!("Failed to append ion history to {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustyline::history::History;

    /// Serializes tests that mutate process-global env vars (`IMPULSE_HOME`,
    /// `HOME`, `USERPROFILE`), since `cargo test` runs unit tests in the
    /// same process on multiple threads by default. Mirrors the identical
    /// helper in `handlers/ion.rs` and `impulse-ion/src/pi_adapter.rs`.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn test_impulse_home_prefers_impulse_home_env_var() {
        let _guard = env_lock();
        let prev = std::env::var("IMPULSE_HOME").ok();
        std::env::set_var("IMPULSE_HOME", "/tmp/custom-impulse-home");
        let home = impulse_home();
        match prev {
            Some(value) => std::env::set_var("IMPULSE_HOME", value),
            None => std::env::remove_var("IMPULSE_HOME"),
        }
        assert_eq!(home, PathBuf::from("/tmp/custom-impulse-home"));
    }

    #[test]
    fn test_impulse_home_ignores_blank_impulse_home_env_var() {
        let _guard = env_lock();
        let prev_home_var = std::env::var("IMPULSE_HOME").ok();
        let prev_home = std::env::var("HOME").ok();
        std::env::set_var("IMPULSE_HOME", "   ");
        std::env::set_var("HOME", "/tmp/fallback-home");

        let home = impulse_home();

        match prev_home_var {
            Some(value) => std::env::set_var("IMPULSE_HOME", value),
            None => std::env::remove_var("IMPULSE_HOME"),
        }
        match prev_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }

        assert_eq!(home, PathBuf::from("/tmp/fallback-home/.impulse"));
    }

    #[test]
    fn test_impulse_home_falls_back_to_home_dir() {
        let _guard = env_lock();
        let prev_home_var = std::env::var("IMPULSE_HOME").ok();
        let prev_home = std::env::var("HOME").ok();
        std::env::remove_var("IMPULSE_HOME");
        std::env::set_var("HOME", "/tmp/fallback-home-2");

        let home = impulse_home();

        match prev_home_var {
            Some(value) => std::env::set_var("IMPULSE_HOME", value),
            None => std::env::remove_var("IMPULSE_HOME"),
        }
        match prev_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }

        assert_eq!(home, PathBuf::from("/tmp/fallback-home-2/.impulse"));
    }

    #[test]
    fn test_history_path_appends_ion_history_filename() {
        let _guard = env_lock();
        let prev = std::env::var("IMPULSE_HOME").ok();
        std::env::set_var("IMPULSE_HOME", "/tmp/custom-impulse-home-3");
        let path = history_path();
        match prev {
            Some(value) => std::env::set_var("IMPULSE_HOME", value),
            None => std::env::remove_var("IMPULSE_HOME"),
        }
        assert_eq!(
            path,
            PathBuf::from("/tmp/custom-impulse-home-3/ion_history")
        );
    }

    #[test]
    fn test_load_missing_file_is_not_an_error() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("does-not-exist");
        let mut editor = DefaultEditor::new().expect("editor");
        assert!(load(&mut editor, &path).is_ok());
    }

    #[test]
    fn test_save_then_load_round_trips_history_entries() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("nested").join("ion_history");

        let mut writer = DefaultEditor::new().expect("editor");
        writer
            .add_history_entry("/help")
            .expect("add_history_entry");
        writer
            .add_history_entry("/verify --repo .")
            .expect("add_history_entry");
        save(&mut writer, &path).expect("save should create parent dir and write history");
        assert!(path.exists());

        let mut reader = DefaultEditor::new().expect("editor");
        load(&mut reader, &path).expect("load should succeed for a file save() just wrote");
        assert_eq!(reader.history().len(), 2);
    }

    #[test]
    fn test_save_appends_across_sessions_instead_of_overwriting() {
        // Regression test: an earlier version used rustyline's save_history,
        // which overwrites the whole file -- a second session's save() would
        // silently erase everything a first (still-open or already-exited)
        // session had written. append_history must preserve prior entries.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("ion_history");

        let mut session_one = DefaultEditor::new().expect("editor");
        session_one
            .add_history_entry("/help")
            .expect("add_history_entry");
        save(&mut session_one, &path).expect("session one save");

        let mut session_two = DefaultEditor::new().expect("editor");
        session_two
            .add_history_entry("/tools")
            .expect("add_history_entry");
        save(&mut session_two, &path).expect("session two save");

        let mut reader = DefaultEditor::new().expect("editor");
        load(&mut reader, &path).expect("load should see both sessions' entries");
        assert_eq!(
            reader.history().len(),
            2,
            "session two's save() must not have erased session one's entry"
        );
    }

    #[test]
    fn test_save_returns_err_when_path_has_no_writable_parent() {
        // A path under a file (not a directory) can never have its parent
        // created, so save() must surface an Err rather than panicking.
        // Setup must not panic if the filesystem refuses the blocking write:
        // PermissionDenied is the same class of failure save() should return.
        // Still invoke save() and require Err — a successful save must not pass.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let blocking_file = dir.path().join("not-a-dir");
        let bad_path = match std::fs::write(&blocking_file, b"x") {
            Ok(()) => blocking_file.join("ion_history"),
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                // Could not plant a file-as-parent in the temp dir. Fall back
                // to an existing file (this process executable) so the parent
                // is still "not a writable directory" without another write.
                match std::env::current_exe() {
                    Ok(exe) if exe.is_file() => exe.join("ion_history"),
                    Ok(exe) => panic!(
                        "could not create unwritable-parent fixture: blocking write was PermissionDenied and current_exe is not a file ({})",
                        exe.display()
                    ),
                    Err(exe_err) => panic!(
                        "could not create unwritable-parent fixture: blocking write was PermissionDenied ({err}) and current_exe failed ({exe_err})"
                    ),
                }
            }
            Err(err) => {
                panic!("could not create blocking-file fixture for unwritable-parent test: {err}")
            }
        };

        let mut editor = DefaultEditor::new().expect("editor");
        assert!(
            save(&mut editor, &bad_path).is_err(),
            "save() must return Err when the parent is not a writable directory"
        );
    }
}
