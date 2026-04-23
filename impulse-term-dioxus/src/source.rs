//! `PtySource` — bridges `TerminalBackend` (the PTY-driven parser) and
//! `LiveGrid` (the damage tracker).
//!
//! # What this owns
//!
//! - One `TerminalBackend` (the PTY child process + reader thread + parser)
//! - One `LiveGrid` (the damage tracker)
//!
//! # What this exposes
//!
//! A synchronous `tick()` that:
//! 1. Builds a `GridSnapshot` from the backend's current `vt100::Screen`
//! 2. Feeds it to `LiveGrid::update_from_snapshot`
//! 3. Returns the `UpdateReport` so the caller knows which rows changed
//!
//! Plus pass-throughs for input writing, output stats, and lifecycle
//! (`is_alive`, `kill`, `resize`).
//!
//! # Why synchronous, not async
//!
//! Dioxus 0.6 desktop runs on Tokio internally, but the PTY backend is
//! thread-based (its reader thread is a `std::thread`). A synchronous
//! `tick()` API lets the Dioxus side wrap the source in `use_future` /
//! `use_coroutine` with whatever cadence makes sense (16ms for high
//! responsiveness, 100ms for lower idle CPU). It also makes `PtySource`
//! testable without a Dioxus runtime — spawn `echo`, tick once, assert the
//! output is in the live grid.
//!
//! L165 will add the Dioxus-side wrapper component (`PtyTerminalView`)
//! that owns a `PtySource` and per-row `Signal<RowSnapshot>`s.

use std::path::Path;

use impulse_term_core::{GridSnapshot, TerminalBackend};

use crate::live::{LiveGrid, UpdateReport};

/// Errors that can occur during `PtySource` lifecycle.
#[derive(Debug, thiserror::Error)]
pub enum PtySourceError {
    #[error("failed to spawn PTY: {0}")]
    Spawn(String),
    #[error("failed to write to PTY: {0}")]
    Write(String),
    #[error("failed to resize PTY: {0}")]
    Resize(String),
}

/// Configuration for spawning a PTY-backed terminal source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtySpec {
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: Option<std::path::PathBuf>,
    pub env_vars: Vec<(String, String)>,
    pub rows: u16,
    pub cols: u16,
    pub scrollback_lines: Option<usize>,
}

impl PtySpec {
    /// Spec for a basic shell with default sizing.
    pub fn shell(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            working_dir: None,
            env_vars: Vec::new(),
            rows: 24,
            cols: 80,
            scrollback_lines: None,
        }
    }
}

/// PTY-backed terminal source. Owns the backend and the live grid.
pub struct PtySource {
    backend: TerminalBackend,
    live_grid: LiveGrid,
}

impl PtySource {
    /// Spawn a PTY child process and create a paired `LiveGrid`.
    ///
    /// The reader thread starts immediately (inside `TerminalBackend`); the
    /// `LiveGrid` is empty until the first `tick()`.
    pub fn spawn(spec: &PtySpec) -> Result<Self, PtySourceError> {
        let env_borrow: Vec<(&str, String)> = spec
            .env_vars
            .iter()
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect();

        let backend = TerminalBackend::spawn(
            &spec.command,
            &spec.args,
            spec.working_dir.as_deref().map(|p: &Path| p),
            &env_borrow,
            spec.rows,
            spec.cols,
            spec.scrollback_lines,
        )
        .map_err(|e| PtySourceError::Spawn(e.to_string()))?;

        let live_grid = LiveGrid::new(spec.rows, spec.cols);

        Ok(Self { backend, live_grid })
    }

    /// Build a fresh snapshot from the parser and update the live grid.
    ///
    /// Returns the `UpdateReport` so the caller can decide whether to
    /// notify the UI layer. Cheap to call repeatedly — when nothing has
    /// changed, returns `UpdateReport::is_clean() == true` and does no
    /// allocation beyond the report itself.
    pub fn tick(&mut self) -> UpdateReport {
        let snapshot = self
            .backend
            .with_parser(|p| GridSnapshot::from_screen(p.screen()));
        self.live_grid.update_from_snapshot(&snapshot)
    }

    /// Borrow the live grid (read-only).
    pub fn live_grid(&self) -> &LiveGrid {
        &self.live_grid
    }

    /// Write user-typed input (key bytes from `key_to_pty_bytes`) to the PTY.
    pub fn write_input(&self, data: &[u8]) -> Result<(), PtySourceError> {
        self.backend
            .write_input(data)
            .map_err(|e| PtySourceError::Write(e.to_string()))
    }

    /// Whether the PTY child process is still alive.
    pub fn is_alive(&self) -> bool {
        self.backend.is_alive()
    }

    /// Total bytes the reader has consumed since spawn.
    pub fn output_bytes(&self) -> u64 {
        self.backend.output_bytes()
    }

    /// Resize both the PTY and the live grid.
    ///
    /// Re-tick after resize so the live grid picks up the new dimensions.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), PtySourceError> {
        self.backend
            .resize(cols, rows)
            .map_err(|e| PtySourceError::Resize(e.to_string()))?;
        // The next tick will detect the resize via the snapshot's new dims.
        Ok(())
    }

    /// Kill the child process. The reader thread will exit shortly after.
    pub fn kill(&self) {
        self.backend.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Wait up to `max` for the PTY reader to consume some bytes.
    /// Returns true if any output appeared, false on timeout.
    fn wait_for_output(source: &PtySource, max: Duration) -> bool {
        let deadline = Instant::now() + max;
        while Instant::now() < deadline {
            if source.output_bytes() > 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn test_pty_spec_shell_defaults() {
        let spec = PtySpec::shell("/bin/sh");
        assert_eq!(spec.command, "/bin/sh");
        assert_eq!(spec.rows, 24);
        assert_eq!(spec.cols, 80);
        assert!(spec.args.is_empty());
        assert!(spec.env_vars.is_empty());
    }

    #[test]
    fn test_spawn_echo_produces_output() {
        // Use /bin/echo (POSIX, present on macOS and Linux). Spawn, wait
        // briefly for output, tick, assert the live grid has changed rows.
        let spec = PtySpec {
            command: "/bin/echo".into(),
            args: vec!["impulse-tick-test".into()],
            working_dir: None,
            env_vars: Vec::new(),
            rows: 5,
            cols: 40,
            scrollback_lines: Some(100),
        };

        let mut source = PtySource::spawn(&spec).expect("echo should spawn");
        assert!(wait_for_output(&source, Duration::from_secs(2)));
        let report = source.tick();
        assert!(
            !report.is_clean(),
            "first tick after output should report changes, got {report:?}"
        );

        // The echo'd text should appear on row 0 of the live grid.
        let row0 = source.live_grid().row(0).expect("row 0 exists");
        let row_text: String = row0.runs.iter().map(|r| r.text.as_str()).collect();
        assert!(
            row_text.contains("impulse-tick-test"),
            "expected echoed text in row 0, got {row_text:?}"
        );
    }

    #[test]
    fn test_idle_tick_is_clean() {
        // Spawn /bin/sleep 5 — produces no output. After enough time, the
        // first tick still reports clean (parser screen is all spaces; live
        // grid was initialized empty; spaces vs empty differ on the first
        // tick BUT subsequent ticks should be clean).
        let spec = PtySpec {
            command: "/bin/sleep".into(),
            args: vec!["5".into()],
            working_dir: None,
            env_vars: Vec::new(),
            rows: 3,
            cols: 10,
            scrollback_lines: Some(50),
        };
        let mut source = PtySource::spawn(&spec).expect("sleep should spawn");

        // Drain initial parser-empty-grid vs live-grid-empty difference.
        let _first = source.tick();

        // Wait briefly to ensure no PTY output races in.
        std::thread::sleep(Duration::from_millis(100));
        let second = source.tick();
        assert!(
            second.is_clean(),
            "second tick on idle PTY should be clean, got {second:?}"
        );

        source.kill();
    }

    #[test]
    fn test_is_alive_transitions_to_false_after_exit() {
        let spec = PtySpec {
            command: "/bin/echo".into(),
            args: vec!["bye".into()],
            working_dir: None,
            env_vars: Vec::new(),
            rows: 3,
            cols: 10,
            scrollback_lines: None,
        };
        let source = PtySource::spawn(&spec).expect("echo should spawn");
        // Echo exits immediately. is_alive() flips false within ~100ms.
        let deadline = Instant::now() + Duration::from_secs(2);
        while source.is_alive() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!source.is_alive(), "echo should have exited");
    }

    #[test]
    fn test_write_input_to_cat_round_trips() {
        // Spawn `cat` (echoes stdin to stdout). Write "hello\n", read back.
        let spec = PtySpec {
            command: "/bin/cat".into(),
            args: vec![],
            working_dir: None,
            env_vars: Vec::new(),
            rows: 5,
            cols: 40,
            scrollback_lines: None,
        };
        let mut source = PtySource::spawn(&spec).expect("cat should spawn");

        source
            .write_input(b"hello\n")
            .expect("write should succeed");

        // Wait for cat to echo back.
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut saw_hello = false;
        while Instant::now() < deadline {
            source.tick();
            let row0 = source.live_grid().row(0);
            let row1 = source.live_grid().row(1);
            for row in [row0, row1].iter().flatten() {
                let text: String = row.runs.iter().map(|r| r.text.as_str()).collect();
                if text.contains("hello") {
                    saw_hello = true;
                    break;
                }
            }
            if saw_hello {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        source.kill();
        assert!(saw_hello, "expected cat to echo 'hello' back");
    }
}
