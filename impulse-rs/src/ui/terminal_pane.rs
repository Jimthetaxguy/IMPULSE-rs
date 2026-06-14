use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

/// Default scrollback buffer size (10,000 lines - 10x the previous default)
pub const DEFAULT_SCROLLBACK_LINES: usize = 10_000;

/// Minimum allowed scrollback lines
pub const MIN_SCROLLBACK_LINES: usize = 100;

/// Maximum allowed scrollback lines (prevent memory exhaustion)
pub const MAX_SCROLLBACK_LINES: usize = 100_000;

/// Default PTY buffer size for reading
pub const PTY_READ_BUFFER_SIZE: usize = 4096;

/// Default respawn max attempts
pub const DEFAULT_MAX_RESPAWN_ATTEMPTS: usize = 5;

/// Default respawn base delay in milliseconds
pub const DEFAULT_RESPAWN_BASE_DELAY_MS: u64 = 100;

/// Default respawn max delay in milliseconds
pub const DEFAULT_RESPAWN_MAX_DELAY_MS: u64 = 30_000;

/// Terminal pane health check interval in seconds
pub const HEALTH_CHECK_INTERVAL_SECS: u64 = 5;

/// Activity threshold in seconds (2 seconds)
pub const ACTIVITY_THRESHOLD_SECS: u64 = 2;

pub struct TerminalPane {
    pub id: usize,
    pub name: String,
    pub project_index: usize,
    // Spawn parameters (for respawn)
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    // Scrollback configuration
    pub scrollback_lines: usize,
    // Output statistics
    output_bytes: Arc<AtomicU64>,
    output_lines: Arc<AtomicU64>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    screen: Arc<Mutex<vt100::Parser>>,
    child: Arc<Mutex<Box<dyn Child + Send>>>,
    alive: Arc<AtomicBool>,
    last_output_time: Arc<Mutex<Instant>>,
    _output_thread: Option<JoinHandle<()>>,
}

pub struct TerminalSpawnRequest<'a> {
    pub id: usize,
    pub name: String,
    pub command: &'a str,
    pub args: &'a [&'a str],
    pub working_dir: Option<&'a Path>,
    pub size: PtySize,
    pub project_index: usize,
    pub impulse_home: Option<&'a Path>,
    pub scrollback_lines: Option<usize>,
}

impl TerminalPane {
    pub fn spawn(request: TerminalSpawnRequest<'_>) -> anyhow::Result<Self> {
        let TerminalSpawnRequest {
            id,
            name,
            command,
            args,
            working_dir,
            size,
            project_index,
            impulse_home,
            scrollback_lines,
        } = request;

        // Clamp scrollback to valid range
        let scrollback = scrollback_lines
            .unwrap_or(DEFAULT_SCROLLBACK_LINES)
            .clamp(MIN_SCROLLBACK_LINES, MAX_SCROLLBACK_LINES);

        let pty_system = native_pty_system();
        let pair = pty_system.openpty(size)?;

        let mut cmd = CommandBuilder::new(command);
        for arg in args {
            cmd.arg(arg);
        }
        if let Some(cwd) = working_dir {
            cmd.cwd(cwd);
            cmd.env("IMPULSE_PROJECT", cwd.to_string_lossy().as_ref());
        }

        // Inject Impulse environment variables
        // Core identifiers
        cmd.env("IMPULSE_PANE_ID", id.to_string());
        cmd.env("IMPULSE_PANE_NAME", &name);
        cmd.env("IMPULSE_SCROLLBACK_LINES", scrollback.to_string());

        if let Some(home) = impulse_home {
            cmd.env("IMPULSE_HOME", home.to_string_lossy().as_ref());
            // Point agents to the capabilities manifest for tool discovery
            let manifest_path = home.join("impulse-capabilities.json");
            if manifest_path.exists() {
                cmd.env(
                    "IMPULSE_CAPABILITIES_PATH",
                    manifest_path.to_string_lossy().as_ref(),
                );
            }
        }

        let session_id = format!(
            "{}-{}-{}",
            name,
            chrono::Local::now().format("%H%M%S"),
            &uuid::Uuid::new_v4().to_string()[..8]
        );
        cmd.env("IMPULSE_SESSION_ID", &session_id);

        // Extended environment variables for better tooling integration
        cmd.env("IMPULSE_TERM", "xterm-256color");
        cmd.env("IMPULSE_TERM_PROGRAM", "impulse-rs");
        cmd.env("IMPULSE_VERSION", env!("CARGO_PKG_VERSION"));

        // Pane configuration for tooling
        cmd.env("IMPULSE_COMMAND", command);
        if !args.is_empty() {
            cmd.env("IMPULSE_ARGS", args.join(" "));
        }

        // Terminal dimensions
        cmd.env("IMPULSE_TERM_ROWS", size.rows.to_string());
        cmd.env("IMPULSE_TERM_COLS", size.cols.to_string());

        // Project context
        cmd.env("IMPULSE_PROJECT_INDEX", project_index.to_string());

        // Timestamp for session tracking
        cmd.env("IMPULSE_STARTED_AT", chrono::Utc::now().to_rfc3339());

        let child = pair.slave.spawn_command(cmd)?;
        // Drop the slave side — the child process owns it now
        drop(pair.slave);

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let screen = Arc::new(Mutex::new(vt100::Parser::new(
            size.rows, size.cols, scrollback, // scrollback lines
        )));
        let alive = Arc::new(AtomicBool::new(true));
        let last_output_time = Arc::new(Mutex::new(Instant::now()));
        let output_bytes = Arc::new(AtomicU64::new(0));
        let output_lines = Arc::new(AtomicU64::new(0));

        // Background thread: read PTY output → feed to vt100 parser
        let screen_clone = Arc::clone(&screen);
        let alive_clone = Arc::clone(&alive);
        let output_time_clone = Arc::clone(&last_output_time);
        let output_bytes_clone = Arc::clone(&output_bytes);
        let output_lines_clone = Arc::clone(&output_lines);
        let output_thread = std::thread::Builder::new()
            .name(format!("pty-reader-{}", id))
            .spawn(move || {
                pty_reader_loop(
                    reader,
                    screen_clone,
                    alive_clone,
                    output_time_clone,
                    output_bytes_clone,
                    output_lines_clone,
                );
            })?;

        Ok(Self {
            id,
            name,
            project_index,
            command: command.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            working_dir: working_dir.map(|p| p.to_path_buf()),
            scrollback_lines: scrollback,
            output_bytes,
            output_lines,
            master: Arc::new(Mutex::new(pair.master)),
            writer: Arc::new(Mutex::new(writer)),
            screen,
            child: Arc::new(Mutex::new(child)),
            alive,
            last_output_time,
            _output_thread: Some(output_thread),
        })
    }

    /// Send a startup message to the PTY to inform the agent about Impulse's capabilities.
    /// Returns Ok(()) if the message was sent successfully.
    pub fn send_startup_message(&self, session_id: &str, platform: &str) -> anyhow::Result<()> {
        let startup_msg = format!(
            "\r\n\x1b[32m╔════════════════════════════════════════════════════════════════╗\r\n\
            ║  IMPULSE SIDEKAR - Session Active                      ║\r\n\
            ╠════════════════════════════════════════════════════════════════╣\r\n\
            ║  Session ID: {}                          ║\r\n\
            ║  Platform:   {}                           ║\r\n\
            ╠════════════════════════════════════════════════════════════════╣\r\n\
            ║  Capabilities:                                          ║\r\
            ║    • Cross-session memory & multi-agent awareness        ║\r\
            ║    • Context injection with review-first workflow       ║\r\
            ║    • File & tool tracking across sessions              ║\r\
            ║    • Semantic retrieval with FTS5 + embeddings         ║\r\
            ║    • Session orchestration & handoff                     ║\r\n\
            ╚════════════════════════════════════════════════════════════════╝\x1b[0m\r\n\r\n",
            session_id, platform
        );

        self.write_input(startup_msg.as_bytes())?;
        Ok(())
    }

    pub fn write_input(&self, data: &[u8]) -> anyhow::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|e| anyhow::anyhow!("writer lock poisoned: {}", e))?;
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, size: PtySize) -> anyhow::Result<()> {
        let master = self
            .master
            .lock()
            .map_err(|e| anyhow::anyhow!("master lock poisoned: {}", e))?;
        master.resize(size)?;
        drop(master);
        let mut screen = self
            .screen
            .lock()
            .map_err(|e| anyhow::anyhow!("screen lock poisoned: {}", e))?;
        screen.set_size(size.rows, size.cols);
        Ok(())
    }

    #[must_use]
    pub fn is_alive(&self) -> bool {
        // Fast path: check cached flag
        if !self.alive.load(Ordering::Relaxed) {
            return false;
        }
        // Slow path: poll child process
        if let Ok(mut child) = self.child.lock() {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    self.alive.store(false, Ordering::Relaxed);
                    false
                }
                Ok(None) => true, // Still running
                Err(_) => {
                    self.alive.store(false, Ordering::Relaxed);
                    false
                }
            }
        } else {
            false
        }
    }

    pub fn screen_snapshot(&self) -> vt100::Screen {
        self.screen
            .lock()
            .map(|parser| parser.screen().clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().screen().clone())
    }

    /// Returns a screen snapshot with scrollback offset applied.
    /// After cloning, restores the parser to live view (offset 0).
    pub fn screen_snapshot_at_offset(&self, scroll_offset: usize) -> vt100::Screen {
        self.screen
            .lock()
            .map(|mut parser| {
                parser.set_scrollback(scroll_offset);
                let snapshot = parser.screen().clone();
                parser.set_scrollback(0); // restore live view
                snapshot
            })
            .unwrap_or_else(|poisoned| {
                let mut parser = poisoned.into_inner();
                parser.set_scrollback(0);
                parser.screen().clone()
            })
    }

    /// Returns the current number of scrollback rows available.
    /// Uses the probe-and-restore pattern since vt100 clamps to actual length.
    pub fn scrollback_len(&self) -> usize {
        self.screen
            .lock()
            .map(|mut parser| {
                parser.set_scrollback(usize::MAX);
                let max = parser.screen().scrollback();
                parser.set_scrollback(0); // restore live view
                max
            })
            .unwrap_or(0)
    }

    /// Returns true if this pane has received output within the last 2 seconds.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.last_output_time
            .lock()
            .map(|t| t.elapsed() < std::time::Duration::from_secs(ACTIVITY_THRESHOLD_SECS))
            .unwrap_or(false)
    }

    /// Returns true if this pane had output within the last `secs` seconds.
    /// Useful for detecting "recently fired" state (output stopped within a wider window).
    #[must_use]
    pub fn had_recent_output(&self, secs: u64) -> bool {
        self.last_output_time
            .lock()
            .map(|t| t.elapsed() < std::time::Duration::from_secs(secs))
            .unwrap_or(false)
    }

    /// Returns the number of output bytes received since spawn.
    pub fn output_bytes(&self) -> u64 {
        self.output_bytes.load(Ordering::Relaxed)
    }

    /// Returns the estimated number of output lines received since spawn.
    pub fn output_lines(&self) -> u64 {
        self.output_lines.load(Ordering::Relaxed)
    }

    /// Returns a summary of output statistics.
    pub fn output_stats(&self) -> PaneOutputStats {
        PaneOutputStats {
            bytes: self.output_bytes(),
            lines: self.output_lines(),
            scrollback_used: self.scrollback_len(),
            scrollback_capacity: self.scrollback_lines,
        }
    }

    /// Update scrollback buffer size (re-creates the parser - use sparingly)
    pub fn set_scrollback(&mut self, lines: usize) -> anyhow::Result<()> {
        let new_size = lines.clamp(MIN_SCROLLBACK_LINES, MAX_SCROLLBACK_LINES);

        let (rows, cols) = self
            .screen
            .lock()
            .map(|p| p.screen().size())
            .unwrap_or((24, 80));

        // Create new parser with new scrollback size
        let new_screen = vt100::Parser::new(rows, cols, new_size);

        // Replace the screen
        if let Ok(mut screen) = self.screen.lock() {
            *screen = new_screen;
            self.scrollback_lines = new_size;
        }

        Ok(())
    }

    pub fn kill(&self) -> anyhow::Result<()> {
        if let Ok(mut child) = self.child.lock() {
            child.kill()?;
        }
        self.alive.store(false, Ordering::Relaxed);
        Ok(())
    }
}

/// Output statistics for a terminal pane
#[derive(Debug, Clone, Default)]
pub struct PaneOutputStats {
    pub bytes: u64,
    pub lines: u64,
    pub scrollback_used: usize,
    pub scrollback_capacity: usize,
}

impl PaneOutputStats {
    /// Get scrollback usage as a percentage (0.0 to 1.0)
    pub fn scrollback_usage_pct(&self) -> f64 {
        if self.scrollback_capacity == 0 {
            return 0.0;
        }
        (self.scrollback_used as f64 / self.scrollback_capacity as f64).min(1.0)
    }

    /// Get human-readable bytes (KB, MB, GB)
    pub fn bytes_formatted(&self) -> String {
        format_bytes(self.bytes)
    }

    /// Get human-readable lines
    pub fn lines_formatted(&self) -> String {
        if self.lines < 1000 {
            format!("{} lines", self.lines)
        } else if self.lines < 1_000_000 {
            format!("{:.1}K lines", self.lines as f64 / 1000.0)
        } else {
            format!("{:.1}M lines", self.lines as f64 / 1_000_000.0)
        }
    }
}

/// Format bytes to human-readable string (KB, MB, GB)
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn pty_reader_loop(
    mut reader: Box<dyn Read + Send>,
    screen: Arc<Mutex<vt100::Parser>>,
    alive: Arc<AtomicBool>,
    last_output_time: Arc<Mutex<Instant>>,
    output_bytes: Arc<AtomicU64>,
    output_lines: Arc<AtomicU64>,
) {
    let mut buf = [0u8; PTY_READ_BUFFER_SIZE];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                // EOF — child closed its end
                alive.store(false, Ordering::Relaxed);
                break;
            }
            Ok(n) => {
                // Track bytes
                output_bytes.fetch_add(n as u64, Ordering::Relaxed);

                // Track newlines for line count estimate
                let newlines = buf[..n].iter().filter(|&&b| b == b'\n').count();
                if newlines > 0 {
                    output_lines.fetch_add(newlines as u64, Ordering::Relaxed);
                }

                if let Ok(mut parser) = screen.lock() {
                    parser.process(&buf[..n]);
                }
                if let Ok(mut t) = last_output_time.lock() {
                    *t = Instant::now();
                }
            }
            Err(_) => {
                alive.store(false, Ordering::Relaxed);
                break;
            }
        }
    }
}
