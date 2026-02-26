//! Terminal backend — PTY spawn, vt100 parser, and reader thread.
//!
//! Ported from `impulse-rs/src/ui/terminal_pane.rs` with key differences:
//! - Uses `parking_lot::FairMutex` to prevent reader-thread starvation
//! - Exposes `screen_text()` and `scrollback_text()` for context extraction
//! - No TUI (ratatui) dependency — rendering is done by `renderer.rs`

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use parking_lot::{FairMutex, Mutex};
use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};

/// Default scrollback buffer size.
const DEFAULT_SCROLLBACK_LINES: usize = 10_000;

/// PTY read buffer size (bytes).
const PTY_READ_BUFFER_SIZE: usize = 4096;

/// Owns a PTY process and its vt100 parser. The reader thread runs in the
/// background, continuously feeding PTY output into the parser.
pub struct TerminalBackend {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    parser: Arc<FairMutex<vt100::Parser>>,
    output_bytes: Arc<AtomicU64>,
    output_lines: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
    _reader_thread: JoinHandle<()>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    cols: u16,
    rows: u16,
    command: String,
    working_dir: Option<PathBuf>,
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

        // Inject environment variables.
        for (key, value) in env_vars {
            cmd.env(key, value);
        }

        let child = pair.slave.spawn_command(cmd)?;
        // Drop the slave side — the child process owns it now.
        drop(pair.slave);

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let parser = Arc::new(FairMutex::new(vt100::Parser::new(rows, cols, scrollback)));
        let alive = Arc::new(AtomicBool::new(true));
        let output_bytes = Arc::new(AtomicU64::new(0));
        let output_lines = Arc::new(AtomicU64::new(0));

        // Background thread: read PTY output → feed to vt100 parser.
        let reader_thread = {
            let parser = Arc::clone(&parser);
            let alive = Arc::clone(&alive);
            let output_bytes = Arc::clone(&output_bytes);
            let output_lines = Arc::clone(&output_lines);

            std::thread::Builder::new()
                .name(format!("pty-reader-{}", command))
                .spawn(move || {
                    pty_reader_loop(reader, parser, alive, output_bytes, output_lines);
                })?
        };

        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            parser,
            output_bytes,
            output_lines,
            alive,
            _reader_thread: reader_thread,
            child: Arc::new(Mutex::new(child)),
            cols,
            rows,
            command: command.to_string(),
            working_dir: working_dir.map(|p| p.to_path_buf()),
        })
    }

    /// Get the full visible screen text (no scrollback).
    pub fn screen_text(&self) -> String {
        let parser = self.parser.lock();
        parser.screen().contents()
    }

    /// Get screen text including scrollback lines.
    pub fn scrollback_text(&self, lines: usize) -> String {
        let mut parser = self.parser.lock();
        parser.set_scrollback(lines);
        let text = parser.screen().contents();
        parser.set_scrollback(0);
        text
    }

    /// Write raw bytes to the PTY (keyboard input, injected context, etc.).
    pub fn write_input(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let mut writer = self.writer.lock();
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
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
            Err(_) => {
                self.alive.store(false, Ordering::Relaxed);
                false
            }
        }
    }

    /// Current terminal dimensions.
    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    /// Resize the PTY and update the parser dimensions.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), Box<dyn std::error::Error>> {
        // vt100 parser resize.
        {
            let mut parser = self.parser.lock();
            parser.set_size(rows, cols);
        }
        self.cols = cols;
        self.rows = rows;
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

    /// Kill the child process.
    pub fn kill(&self) {
        let mut child = self.child.lock();
        let _ = child.kill();
        self.alive.store(false, Ordering::Relaxed);
    }

    /// The command that was spawned.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// The working directory.
    pub fn working_dir(&self) -> Option<&Path> {
        self.working_dir.as_deref()
    }

    /// Check if there's been new output since a given byte count.
    pub fn has_new_output_since(&self, previous_bytes: u64) -> bool {
        self.output_bytes.load(Ordering::Relaxed) > previous_bytes
    }
}

/// Background reader thread — reads PTY output and feeds it to the vt100 parser.
fn pty_reader_loop(
    mut reader: Box<dyn Read + Send>,
    parser: Arc<FairMutex<vt100::Parser>>,
    alive: Arc<AtomicBool>,
    output_bytes: Arc<AtomicU64>,
    output_lines: Arc<AtomicU64>,
) {
    let mut buf = [0u8; PTY_READ_BUFFER_SIZE];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                // EOF — child closed its end.
                alive.store(false, Ordering::Relaxed);
                break;
            }
            Ok(n) => {
                output_bytes.fetch_add(n as u64, Ordering::Relaxed);

                let newlines = buf[..n].iter().filter(|&&b| b == b'\n').count();
                if newlines > 0 {
                    output_lines.fetch_add(newlines as u64, Ordering::Relaxed);
                }

                let mut parser = parser.lock();
                parser.process(&buf[..n]);
            }
            Err(_) => {
                alive.store(false, Ordering::Relaxed);
                break;
            }
        }
    }
}
