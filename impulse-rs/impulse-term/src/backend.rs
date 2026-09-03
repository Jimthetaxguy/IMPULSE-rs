//! Terminal backend — PTY spawn, vt100 parser, and reader thread.
//!
//! Ported from `impulse-rs/src/ui/terminal_pane.rs` with key differences:
//! - Uses `parking_lot::FairMutex` to prevent reader-thread starvation
//! - Exposes `screen_text()` and `scrollback_text()` for context extraction
//! - No TUI (ratatui) dependency — rendering is done by `renderer.rs`

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use parking_lot::{FairMutex, Mutex};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

pub type OutputCallback = Arc<dyn Fn(&[u8]) + Send + Sync + 'static>;
pub type ExitCallback = Arc<dyn Fn() + Send + Sync + 'static>;

#[derive(Default, Clone)]
pub struct TerminalCallbacks {
    pub output: Option<OutputCallback>,
    pub exit: Option<ExitCallback>,
}

/// Default scrollback buffer size.
const DEFAULT_SCROLLBACK_LINES: usize = 10_000;

/// PTY read buffer size (bytes).
const PTY_READ_BUFFER_SIZE: usize = 4096;

/// Remove every inherited Impulse control-plane variable before a PTY launch.
///
/// `portable_pty::CommandBuilder` starts from the parent process environment.
/// Omitting a key from a launch request therefore does not isolate it: an
/// ordinary pane could otherwise inherit a daemon socket, producer CLI, task,
/// or role identity that belongs to the Desktop process. Explicit launch
/// metadata is overlaid again after this scrub.
/// Strip every inherited `IMPULSE_*` key from a command before the caller
/// overlays the variables a governed pane is *meant* to receive.
///
/// The prefix rule is deliberately an allowlist-by-omission: a launched runtime
/// gets exactly the keys the harness sets afterwards and nothing the harness
/// process happens to be carrying. That is what keeps `IMPULSE_OPERATOR_CAPABILITY`
/// (ADR-0018) out of governed panes — a runtime is never handed the capability
/// that raises a socket connection to operator class, so it cannot approve its
/// own governed task even though it holds `IMPULSE_SOCKET_PATH`. Callers that
/// build the explicit overlay must never add that key back.
fn remove_inherited_impulse_env<I>(command: &mut CommandBuilder, inherited_keys: I)
where
    I: IntoIterator<Item = OsString>,
{
    for key in inherited_keys {
        let rendered = key.to_string_lossy();
        if rendered
            .get(.."IMPULSE_".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("IMPULSE_"))
        {
            command.env_remove(&key);
        }
    }
}

/// Owns a PTY process and its vt100 parser. The reader thread runs in the
/// background, continuously feeding PTY output into the parser.
pub struct TerminalBackend {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    write_queue: WriteQueue,
    parser: Arc<FairMutex<vt100::Parser>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    output_bytes: Arc<AtomicU64>,
    output_lines: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
    /// Counts consecutive PTY read errors for diagnostics.
    read_errors: Arc<AtomicU64>,
    _reader_thread: JoinHandle<()>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    cols: AtomicU16,
    rows: AtomicU16,
    command: String,
    working_dir: Option<PathBuf>,
}

/// Owns a child during fallible PTY setup. `portable_pty::Child` does not
/// promise kill-on-drop, so every error after `spawn_command` must explicitly
/// terminate and reap the process before returning control to the harness.
struct SpawnedChildGuard {
    child: Option<Box<dyn Child + Send + Sync>>,
}

impl SpawnedChildGuard {
    fn new(child: Box<dyn Child + Send + Sync>) -> Self {
        Self { child: Some(child) }
    }

    fn into_inner(mut self) -> Box<dyn Child + Send + Sync> {
        self.child
            .take()
            .expect("spawned child guard must own a child until disarmed")
    }
}

impl Drop for SpawnedChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl TerminalBackend {
    /// Spawn a child process in a new PTY.
    ///
    /// # Arguments
    /// * `command` — executable name or path
    /// * `args` — command-line arguments
    /// * `working_dir` — working directory for the child
    /// * `env_vars` — extra environment variables to set
    /// * `rows`, `cols` — initial terminal dimensions
    /// * `scrollback` — scrollback buffer lines (default: 10,000)
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        command: &str,
        args: &[String],
        working_dir: Option<&Path>,
        env_vars: &[(&str, String)],
        rows: u16,
        cols: u16,
        scrollback: Option<usize>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::spawn_with_callbacks(
            command,
            args,
            working_dir,
            env_vars,
            rows,
            cols,
            scrollback,
            None,
            None,
        )
    }

    /// Spawn a child process in a new PTY and report raw output/exit events.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_with_callbacks(
        command: &str,
        args: &[String],
        working_dir: Option<&Path>,
        env_vars: &[(&str, String)],
        rows: u16,
        cols: u16,
        scrollback: Option<usize>,
        output_callback: Option<OutputCallback>,
        exit_callback: Option<ExitCallback>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let scrollback = scrollback.unwrap_or(DEFAULT_SCROLLBACK_LINES);

        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(command);
        for arg in args {
            cmd.arg(arg);
        }
        if let Some(cwd) = working_dir {
            cmd.cwd(cwd);
        }

        // CommandBuilder inherits the Desktop environment. Remove all Impulse
        // routing/identity first, then add only the values authorized for this
        // specific launch.
        remove_inherited_impulse_env(&mut cmd, std::env::vars_os().map(|(key, _value)| key));
        for (key, value) in env_vars {
            cmd.env(key, value);
        }

        let child = pair.slave.spawn_command(cmd)?;
        let child_guard = SpawnedChildGuard::new(child);
        // Drop the slave side — the child process owns it now.
        drop(pair.slave);

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        // Store the master handle for resize() — try_clone_reader() clones
        // (doesn't consume), take_writer() takes but leaves the handle alive.
        let master: Arc<Mutex<Box<dyn MasterPty + Send>>> = Arc::new(Mutex::new(pair.master));

        let parser = Arc::new(FairMutex::new(vt100::Parser::new(rows, cols, scrollback)));
        let alive = Arc::new(AtomicBool::new(true));
        let output_bytes = Arc::new(AtomicU64::new(0));
        let output_lines = Arc::new(AtomicU64::new(0));
        let read_errors = Arc::new(AtomicU64::new(0));

        // Background thread: read PTY output → feed to vt100 parser.
        let reader_thread = {
            let parser = Arc::clone(&parser);
            let alive = Arc::clone(&alive);
            let output_bytes = Arc::clone(&output_bytes);
            let output_lines = Arc::clone(&output_lines);
            let read_errors = Arc::clone(&read_errors);
            let callbacks = TerminalCallbacks {
                output: output_callback,
                exit: exit_callback,
            };

            std::thread::Builder::new()
                .name(format!("pty-reader-{}", command))
                .spawn(move || {
                    pty_reader_loop(
                        reader,
                        parser,
                        alive,
                        output_bytes,
                        output_lines,
                        read_errors,
                        callbacks,
                    );
                })?
        };

        let writer_arc = Arc::new(Mutex::new(writer));
        let write_queue = WriteQueue::new(Arc::clone(&writer_arc));
        let child = child_guard.into_inner();

        Ok(Self {
            writer: writer_arc,
            write_queue,
            parser,
            master,
            output_bytes,
            output_lines,
            alive,
            read_errors,
            _reader_thread: reader_thread,
            child: Arc::new(Mutex::new(child)),
            cols: AtomicU16::new(cols),
            rows: AtomicU16::new(rows),
            command: command.to_string(),
            working_dir: working_dir.map(|p| p.to_path_buf()),
        })
    }

    /// Get the full visible screen text (no scrollback).
    pub fn screen_text(&self) -> String {
        let parser = self.parser.lock();
        parser.screen().contents()
    }

    /// Count visible characters on screen (ANSI-stripped by vt100).
    ///
    /// Uses `screen().contents()` which returns only visible text —
    /// no escape sequences, no control chars. Suitable for token estimation.
    pub fn visible_char_count(&self) -> usize {
        let parser = self.parser.lock();
        parser.screen().contents().len()
    }

    /// Get screen text including scrollback lines.
    pub fn scrollback_text(&self, lines: usize) -> String {
        let mut parser = self.parser.lock();
        parser.set_scrollback(lines);
        let text = parser.screen().contents();
        parser.set_scrollback(0);
        text
    }

    /// Write raw bytes to the PTY.
    ///
    /// **Prefer `write_queue().write_user_input()` or `write_queue().write_injection()`**
    /// for proper serialization. Retained for backwards compat during migration.
    pub fn write_input(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let mut writer = self.writer.lock();
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    /// Access the write queue for serialized PTY writes.
    pub fn write_queue(&self) -> &WriteQueue {
        &self.write_queue
    }

    /// Total output bytes received from the PTY since spawn.
    pub fn output_bytes(&self) -> u64 {
        self.output_bytes.load(Ordering::Relaxed)
    }

    /// Total newline-delimited output lines since spawn.
    pub fn output_lines(&self) -> u64 {
        self.output_lines.load(Ordering::Relaxed)
    }

    /// Whether the child process is still alive.
    pub fn is_alive(&self) -> bool {
        // Fast path: check cached flag.
        if !self.alive.load(Ordering::Relaxed) {
            return false;
        }
        // Slow path: poll child process.
        let mut child = self.child.lock();
        match child.try_wait() {
            Ok(Some(_)) => {
                self.alive.store(false, Ordering::Relaxed);
                false
            }
            Ok(None) => true,
            Err(error) => {
                log::error!(
                    "failed to poll PTY child for '{}'; retaining last live state: {}",
                    self.command,
                    error
                );
                true
            }
        }
    }

    /// Current terminal dimensions.
    pub fn size(&self) -> (u16, u16) {
        (
            self.cols.load(Ordering::Relaxed),
            self.rows.load(Ordering::Relaxed),
        )
    }

    /// Resize the PTY and update the parser dimensions.
    ///
    /// Locks the parser FIRST to block the reader thread, then resizes the PTY
    /// master and updates parser dimensions atomically. Without this ordering,
    /// the reader could process output formatted for the new PTY size while the
    /// parser still has the old dimensions, causing text wrapping corruption.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), Box<dyn std::error::Error>> {
        // Lock parser first — blocks reader thread during the resize window.
        let mut parser = self.parser.lock();

        // Resize the PTY master (sends SIGWINCH to child process).
        {
            let master = self.master.lock();
            master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })?;
        }

        // Update parser dimensions while still holding the lock.
        parser.set_size(rows, cols);

        self.cols.store(cols, Ordering::Relaxed);
        self.rows.store(rows, Ordering::Relaxed);
        Ok(())
    }

    /// Lock the parser and call `f` with it. Used by the renderer.
    pub fn with_parser<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&vt100::Parser) -> R,
    {
        let parser = self.parser.lock();
        f(&parser)
    }

    /// Lock the parser mutably (e.g., for scrollback adjustment).
    pub fn with_parser_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut vt100::Parser) -> R,
    {
        let mut parser = self.parser.lock();
        f(&mut parser)
    }

    /// Terminate and reap the child process.
    ///
    /// The backend is marked dead only after `try_wait` or `wait` confirms a
    /// terminal process state. Kill failures for a still-running process are
    /// returned to the caller so lifecycle truth cannot claim an exit that did
    /// not happen.
    pub fn kill(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut child = self.child.lock();
        if child.try_wait()?.is_some() {
            self.alive.store(false, Ordering::Relaxed);
            return Ok(());
        }
        if let Err(kill_error) = child.kill() {
            // The child may have exited between `try_wait` and `kill`. Confirm
            // that race before treating the kill error as a live orphan.
            if child.try_wait()?.is_some() {
                self.alive.store(false, Ordering::Relaxed);
                return Ok(());
            }
            self.alive.store(true, Ordering::Relaxed);
            return Err(Box::new(kill_error));
        }
        child.wait()?;
        self.alive.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// The command that was spawned.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// The working directory.
    pub fn working_dir(&self) -> Option<&Path> {
        self.working_dir.as_deref()
    }

    /// Number of scrollback lines available above the visible screen.
    pub fn scrollback_len(&self) -> usize {
        let parser = self.parser.lock();
        parser.screen().scrollback()
    }

    /// Number of visible rows in the terminal.
    pub fn visible_rows(&self) -> usize {
        self.rows.load(Ordering::Relaxed) as usize
    }

    /// Check if there's been new output since a given byte count.
    pub fn has_new_output_since(&self, previous_bytes: u64) -> bool {
        self.output_bytes.load(Ordering::Relaxed) > previous_bytes
    }

    /// Total count of PTY read errors encountered since spawn.
    /// A non-zero value after the terminal is dead indicates the PTY broke unexpectedly.
    pub fn read_error_count(&self) -> u64 {
        self.read_errors.load(Ordering::Relaxed)
    }
}

impl Drop for TerminalBackend {
    fn drop(&mut self) {
        if let Err(error) = self.kill() {
            log::error!(
                "failed to terminate and reap PTY child for '{}': {}",
                self.command,
                error
            );
        }
    }
}

// ---------------------------------------------------------------------------
// WriteQueue — serialized PTY writes
// ---------------------------------------------------------------------------

/// Minimum quiet period (ms) after user input before injection is allowed.
const INJECTION_QUIET_MS: u64 = 500;

/// Serializes all writes to the PTY, preventing message-level interleaving
/// between user input, context injection, and lifecycle writes.
///
/// All code paths that write to the PTY must go through this queue.
/// `write_user_input()` always succeeds and records a timestamp.
/// `write_injection()` is skipped if user input occurred recently.
pub struct WriteQueue {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    /// Epoch millis of the last user input write.
    last_user_input: Arc<AtomicU64>,
}

impl WriteQueue {
    /// Create a new WriteQueue wrapping the given PTY writer.
    pub fn new(writer: Arc<Mutex<Box<dyn Write + Send>>>) -> Self {
        Self {
            writer,
            last_user_input: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Write user keyboard/paste input — always succeeds, updates last-input timestamp.
    pub fn write_user_input(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let mut writer = self.writer.lock();
        writer.write_all(data)?;
        writer.flush()?;
        let now = epoch_millis();
        self.last_user_input.store(now, Ordering::Relaxed);
        Ok(())
    }

    /// Write injected context — skipped if user typed within INJECTION_QUIET_MS.
    /// Writes the entire buffer in a single lock acquisition to prevent interleaving.
    /// Returns `true` if the write happened, `false` if skipped.
    pub fn write_injection(&self, data: &[u8]) -> Result<bool, Box<dyn std::error::Error>> {
        let now = epoch_millis();
        let last = self.last_user_input.load(Ordering::Relaxed);
        if now.saturating_sub(last) < INJECTION_QUIET_MS {
            return Ok(false);
        }
        let mut writer = self.writer.lock();
        writer.write_all(data)?;
        writer.flush()?;
        Ok(true)
    }
}

fn epoch_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Background reader thread — reads PTY output and feeds it to the vt100 parser.
fn pty_reader_loop(
    mut reader: Box<dyn Read + Send>,
    parser: Arc<FairMutex<vt100::Parser>>,
    alive: Arc<AtomicBool>,
    output_bytes: Arc<AtomicU64>,
    output_lines: Arc<AtomicU64>,
    read_errors: Arc<AtomicU64>,
    callbacks: TerminalCallbacks,
) {
    let mut buf = [0u8; PTY_READ_BUFFER_SIZE];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                // EOF — child closed its end.
                alive.store(false, Ordering::Relaxed);
                if let Some(callback) = &callbacks.exit {
                    callback();
                }
                break;
            }
            Ok(n) => {
                output_bytes.fetch_add(n as u64, Ordering::Relaxed);
                if let Some(callback) = &callbacks.output {
                    callback(&buf[..n]);
                }

                let newlines = buf[..n].iter().filter(|&&b| b == b'\n').count();
                if newlines > 0 {
                    output_lines.fetch_add(newlines as u64, Ordering::Relaxed);
                }

                let mut parser = parser.lock();
                parser.process(&buf[..n]);
            }
            Err(e) => {
                read_errors.fetch_add(1, Ordering::Relaxed);
                log::error!(
                    "PTY read error on '{}': {} — marking terminal as dead",
                    std::thread::current().name().unwrap_or("?"),
                    e
                );
                alive.store(false, Ordering::Relaxed);
                if let Some(callback) = &callbacks.exit {
                    callback();
                }
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shared buffer helper for testing WriteQueue without a real PTY.
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn test_write_queue() -> (WriteQueue, Arc<Mutex<Vec<u8>>>) {
        let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(SharedBuf(Arc::clone(&buf)))));
        (WriteQueue::new(writer), buf)
    }

    #[test]
    fn test_write_queue_user_input_succeeds() {
        let (wq, buf) = test_write_queue();
        wq.write_user_input(b"hello").unwrap();
        assert_eq!(&*buf.lock(), b"hello");
    }

    #[test]
    fn test_write_queue_user_input_multiple_writes() {
        let (wq, buf) = test_write_queue();
        wq.write_user_input(b"abc").unwrap();
        wq.write_user_input(b"def").unwrap();
        assert_eq!(&*buf.lock(), b"abcdef");
    }

    #[test]
    fn test_write_queue_injection_blocked_after_input() {
        let (wq, buf) = test_write_queue();
        // Simulate recent user input.
        wq.write_user_input(b"x").unwrap();
        // Injection should be blocked (within 500ms).
        let injected = wq.write_injection(b"context").unwrap();
        assert!(
            !injected,
            "injection should be blocked right after user input"
        );
        // Only user input should be in the buffer.
        assert_eq!(&*buf.lock(), b"x");
    }

    #[test]
    fn test_write_queue_injection_succeeds_when_idle() {
        let (wq, buf) = test_write_queue();
        // No user input — last_user_input is 0, so the gap is huge.
        let injected = wq.write_injection(b"context").unwrap();
        assert!(
            injected,
            "injection should succeed when no recent user input"
        );
        assert_eq!(&*buf.lock(), b"context");
    }

    #[test]
    fn test_write_queue_injection_atomic_write() {
        let (wq, buf) = test_write_queue();
        // Write a multi-part injection (simulating bracketed paste).
        let paste = b"\x1b[200~injected content\x1b[201~";
        let injected = wq.write_injection(paste).unwrap();
        assert!(injected);
        assert_eq!(&*buf.lock(), paste.as_slice());
    }

    #[test]
    fn test_epoch_millis_nonzero() {
        let ms = epoch_millis();
        assert!(ms > 0, "epoch millis should be nonzero");
    }

    #[test]
    fn test_write_queue_user_input_updates_timestamp() {
        let (wq, _buf) = test_write_queue();
        let before = wq.last_user_input.load(Ordering::Relaxed);
        assert_eq!(before, 0, "initial timestamp should be 0");
        wq.write_user_input(b"x").unwrap();
        let after = wq.last_user_input.load(Ordering::Relaxed);
        assert!(after > 0, "timestamp should be updated after user input");
    }

    #[test]
    fn test_write_queue_injection_does_not_update_timestamp() {
        let (wq, _buf) = test_write_queue();
        wq.write_injection(b"ctx").unwrap();
        let ts = wq.last_user_input.load(Ordering::Relaxed);
        assert_eq!(ts, 0, "injection should not update user input timestamp");
    }

    #[test]
    fn test_write_queue_concurrent_user_writes_no_interleave() {
        let (wq, buf) = test_write_queue();
        let wq = Arc::new(wq);

        let mut handles = Vec::new();
        for i in 0..10 {
            let wq = Arc::clone(&wq);
            let msg = format!("msg{:02}", i);
            handles.push(std::thread::spawn(move || {
                wq.write_user_input(msg.as_bytes()).unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let data = buf.lock().clone();
        // All 10 messages should be present (each is 5 bytes "msgNN").
        assert_eq!(data.len(), 50, "all 10 messages should be written");
        // Each "msg" prefix should appear exactly 10 times (no byte interleaving).
        let as_str = String::from_utf8(data).unwrap();
        assert_eq!(
            as_str.matches("msg").count(),
            10,
            "no message should be interleaved"
        );
    }

    #[test]
    fn inherited_impulse_environment_is_removed_before_explicit_overlay() {
        let mut command = CommandBuilder::new("true");
        command.env("IMPULSE_SOCKET_PATH", "/tmp/parent.sock");
        command.env("impulse_control_cli", "/tmp/parent-cli");
        command.env("PATH", "/usr/bin");

        remove_inherited_impulse_env(
            &mut command,
            [
                OsString::from("IMPULSE_SOCKET_PATH"),
                OsString::from("impulse_control_cli"),
                OsString::from("PATH"),
            ],
        );

        assert!(command.get_env("IMPULSE_SOCKET_PATH").is_none());
        assert!(command.get_env("IMPULSE_CONTROL_CLI").is_none());
        assert_eq!(
            command.get_env("PATH"),
            Some(std::ffi::OsStr::new("/usr/bin"))
        );

        command.env("IMPULSE_SOCKET_PATH", "/tmp/profiled.sock");
        assert_eq!(
            command.get_env("IMPULSE_SOCKET_PATH"),
            Some(std::ffi::OsStr::new("/tmp/profiled.sock"))
        );
    }

    #[test]
    fn operator_capability_never_reaches_a_spawned_pane() {
        // ADR-0018: a governed pane holds IMPULSE_SOCKET_PATH but must never
        // hold the capability that would let it approve its own task.
        let mut command = CommandBuilder::new("true");
        command.env("IMPULSE_OPERATOR_CAPABILITY", "a".repeat(64));
        command.env("impulse_operator_capability", "b".repeat(64));
        command.env("IMPULSE_SOCKET_PATH", "/tmp/parent.sock");

        remove_inherited_impulse_env(
            &mut command,
            [
                OsString::from("IMPULSE_OPERATOR_CAPABILITY"),
                OsString::from("impulse_operator_capability"),
                OsString::from("IMPULSE_SOCKET_PATH"),
            ],
        );

        assert!(
            command.get_env("IMPULSE_OPERATOR_CAPABILITY").is_none(),
            "the operator capability must not survive into a governed pane"
        );

        // The explicit governed overlay re-adds the socket path and nothing else.
        command.env("IMPULSE_SOCKET_PATH", "/tmp/profiled.sock");
        assert_eq!(
            command.get_env("IMPULSE_SOCKET_PATH"),
            Some(std::ffi::OsStr::new("/tmp/profiled.sock"))
        );
        assert!(command.get_env("IMPULSE_OPERATOR_CAPABILITY").is_none());
    }
}
