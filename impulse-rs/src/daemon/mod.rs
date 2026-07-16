//! Daemon IPC server — long-running Unix socket process.
//!
//! Accepts JSON-line messages over [`DaemonRequest`] / [`DaemonResponse`] protocol.
//! Owns in-memory [`crate::state::State`] with dirty-flag sync. Handles session
//! lifecycle, file tracking, conflict detection, chat, and tool invocation.
//!
//! ## Module layout
//!
//! - [`protocol`] — Wire-format types: `DaemonRequest`, `DaemonResponse`, helpers
//! - [`handlers`] — Request dispatch and grouped sub-handlers
//! - This file — `Daemon` struct, startup, socket accept loop, shutdown

pub mod handlers;
pub mod protocol;

// Re-export protocol types so existing `crate::daemon::DaemonRequest` paths keep working.
pub use protocol::*;

use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinSet;

use crate::state::SharedState;

/// Per-request line size cap, shared by the daemon's own read loop and (via
/// [`read_bounded_line`]) reusable by other JSON-line protocol readers in
/// this codebase (see `mcp::server`).
pub const MAX_REQUEST_SIZE: usize = 10 * 1024 * 1024; // 10MB limit per request

/// Give requests that were already being processed a bounded opportunity to
/// finish before shutdown cancels them. Idle connections are woken
/// immediately by [`Daemon::begin_shutdown`].
const CONNECTION_SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// Desktop companions verify their direct parent before publishing runtime
/// files and then poll the parent relationship while serving. A PID alone is
/// not sufficient because it can be reused after the desktop exits.
const OWNER_PARENT_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Process-level serialization for one daemon socket lifecycle.
///
/// This is deliberately distinct from the desktop host's `*.desktop.lock`:
/// the desktop lease serializes desktop owners, while this lock serializes
/// every daemon process (desktop-spawned or operator-started) before any stale
/// socket check or removal. The lock file is retained after release because
/// unlinking a flocked inode would let another process lock a replacement inode
/// and defeat mutual exclusion.
struct DaemonInstanceLock {
    _file: File,
}

impl DaemonInstanceLock {
    fn acquire(socket_path: &Path) -> Result<Self> {
        let lock_path = socket_path.with_extension("daemon.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&lock_path)
            .with_context(|| {
                format!(
                    "Failed to open daemon lifecycle lock {}",
                    lock_path.display()
                )
            })?;
        if !file
            .metadata()
            .with_context(|| {
                format!(
                    "Failed to inspect daemon lifecycle lock {}",
                    lock_path.display()
                )
            })?
            .is_file()
        {
            anyhow::bail!(
                "Daemon lifecycle lock {} must be a regular file",
                lock_path.display()
            );
        }
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .with_context(|| {
                format!(
                    "Failed to restrict daemon lifecycle lock {}",
                    lock_path.display()
                )
            })?;

        loop {
            // SAFETY: `file` remains alive in the returned guard for the
            // complete lock lifetime, so its raw descriptor is valid here.
            let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if result == 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if error.kind() == std::io::ErrorKind::WouldBlock {
                anyhow::bail!(
                    "Another daemon owns lifecycle lock {} for socket {}",
                    lock_path.display(),
                    socket_path.display()
                );
            }
            return Err(error).with_context(|| {
                format!(
                    "Failed to acquire daemon lifecycle lock {}",
                    lock_path.display()
                )
            });
        }

        Ok(Self { _file: file })
    }
}

/// Result of a single bounded line read. See [`read_bounded_line`].
pub enum BoundedLine {
    /// A complete line was read (trailing `\n`/`\r\n` stripped), and its
    /// byte length did not exceed `max_bytes`.
    Line(String),
    /// The stream reached EOF with no more data to read (connection closed
    /// cleanly between requests).
    Eof,
    /// More than `max_bytes` bytes were read without finding a `\n` (or
    /// EOF). The caller must not attempt to keep reading from this
    /// connection to "recover" the rest of the line -- the remaining bytes
    /// on the wire are themselves unbounded and of unknown length, so
    /// draining them would reintroduce the exact unbounded-read problem
    /// this type exists to prevent. The only bounded response is to close
    /// the connection.
    TooLarge,
}

/// Read a single `\n`-delimited line from `reader`, but never buffer more
/// than `max_bytes + 1` bytes into memory regardless of how much data the
/// peer sends before the next newline (or EOF).
///
/// **Bug being fixed:** the daemon's connection loop previously called
/// `reader.read_line(&mut line).await?` — which has no upper bound — and
/// only checked `line.len() > MAX_REQUEST_SIZE` *after* `read_line` had
/// already appended the full, arbitrarily large line into `line`. A local
/// client with socket access could send many gigabytes of non-newline bytes
/// on one connection and OOM the daemon before the size check ever fired;
/// the guard existed but ran too late to bound memory. Wrapping the reader
/// in [`tokio::io::AsyncReadExt::take`] (`max_bytes + 1` per call) bounds
/// how many bytes the inner `read_until` will ever pull from the stream
/// before giving up, so peak memory for a single line read is capped at
/// `max_bytes + 1` bytes no matter what the peer sends -- turning "eventual
/// check after unbounded growth" into an actually-bounded read.
///
/// `reader` must already be a buffered reader (e.g. `BufReader`), since
/// `Take<R>` only implements `AsyncBufRead` when `R: AsyncBufRead`.
pub async fn read_bounded_line<R>(reader: &mut R, max_bytes: usize) -> std::io::Result<BoundedLine>
where
    R: AsyncBufRead + Unpin,
{
    let mut buf: Vec<u8> = Vec::new();
    // `+1` so we can distinguish "exactly max_bytes bytes then a newline"
    // (allowed) from "more than max_bytes bytes with no newline yet"
    // (rejected) using only the length of what came back.
    let mut limited = reader.take(max_bytes as u64 + 1);
    let n = limited.read_until(b'\n', &mut buf).await?;

    if n == 0 {
        return Ok(BoundedLine::Eof);
    }

    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        if buf.len() > max_bytes {
            return Ok(BoundedLine::TooLarge);
        }
        return Ok(BoundedLine::Line(
            String::from_utf8_lossy(&buf).into_owned(),
        ));
    }

    // No newline was found within `max_bytes + 1` bytes: either the peer
    // sent more than the cap without a delimiter (reject), or the
    // underlying stream hit EOF with a final, unterminated line at or under
    // the cap (accept, matching `read_line`'s existing EOF-without-newline
    // behavior).
    if buf.len() > max_bytes {
        Ok(BoundedLine::TooLarge)
    } else {
        Ok(BoundedLine::Line(
            String::from_utf8_lossy(&buf).into_owned(),
        ))
    }
}

fn build_remote_tool_context(
    impulse_dir: &std::path::Path,
    config: &crate::state::Config,
) -> crate::tooling::ToolContext {
    crate::handlers::build_tool_context(
        impulse_dir,
        config,
        crate::tooling::ExecutionOrigin::Daemon,
        false,
        None,
    )
}

pub struct DaemonConfig {
    pub socket_path: PathBuf,
    pub state: SharedState,
    owner_pid: Option<u32>,
}

pub struct Daemon {
    config: DaemonConfig,
    shutdown_flag: Arc<AtomicBool>,
    shutdown_notify: Arc<Notify>,
    tool_registry: Arc<crate::tooling::ToolRegistry>,
    tool_context: crate::tooling::ToolContext,
    terminal_telemetry: Arc<RwLock<crate::ops_workbench::TerminalOpsTelemetryStore>>,
    supervisor_session_override: Arc<RwLock<Option<impulse_ops::SupervisorPermissionPolicy>>>,
    conflict_resolver: Arc<RwLock<crate::agent::coordinator::ConflictResolver>>,
    /// Cross-agent delegation tracker (Phase 1B). Backs the
    /// RegisterDelegation/CompleteDelegation/ListDelegations endpoints.
    delegation_tracker: Arc<RwLock<crate::delegation::DelegationTracker>>,
    /// One cached ImpulseAgent for the daemon session. Agent handlers retain
    /// this async mutex for a complete, bounded logical turn so history,
    /// recommendations, and pane summaries cannot fork under concurrency.
    /// Concurrent agent requests receive typed Busy; unrelated daemon request
    /// groups never acquire it.
    cached_agent: Arc<tokio::sync::Mutex<Option<crate::agent::ImpulseAgent>>>,
}

impl Daemon {
    pub fn new(state: SharedState) -> Self {
        let socket_path = state
            .storage()
            .base_path()
            .join("sockets")
            .join(protocol::SOCKET_NAME);
        let project_root = state
            .storage()
            .base_path()
            .parent()
            .unwrap_or_else(|| state.storage().base_path());
        let config_snapshot = state.config_snapshot().unwrap_or_default();
        let external_tools_dir = config_snapshot.resolved_external_tools_dir_from(project_root);
        let tool_registry = crate::tooling::ToolRegistry::with_runtime(
            state.storage().base_path(),
            &external_tools_dir,
        )
        .unwrap_or_else(|_| crate::tooling::ToolRegistry::with_defaults());
        if let Err(err) = crate::agent_discovery::write_capabilities_manifest(
            state.storage().base_path(),
            &tool_registry,
        ) {
            tracing::warn!("failed to refresh capabilities manifest: {}", err);
        }
        let tool_context = build_remote_tool_context(state.storage().base_path(), &config_snapshot);

        // Initialize the global plugin registry so ListPlugins/InvokePlugin work.
        crate::plugin::registry::init_global_registry();

        Self {
            config: DaemonConfig {
                socket_path: socket_path.clone(),
                state,
                owner_pid: None,
            },
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            shutdown_notify: Arc::new(Notify::new()),
            tool_registry: Arc::new(tool_registry),
            tool_context,
            terminal_telemetry: Arc::new(RwLock::new(
                crate::ops_workbench::TerminalOpsTelemetryStore::default(),
            )),
            supervisor_session_override: Arc::new(RwLock::new(None)),
            conflict_resolver: Arc::new(RwLock::new(
                crate::agent::coordinator::ConflictResolver::new(),
            )),
            delegation_tracker: Arc::new(RwLock::new(crate::delegation::DelegationTracker::new())),
            cached_agent: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    #[cfg(test)]
    pub fn socket_path(&self) -> &PathBuf {
        &self.config.socket_path
    }

    /// Bind this daemon to the exact process that launched it.
    ///
    /// Desktop uses this for companions it owns. Operator daemons leave the
    /// value unset and retain their independent lifetime.
    pub fn with_owner_pid(mut self, owner_pid: Option<u32>) -> Self {
        self.config.owner_pid = owner_pid;
        self
    }

    fn begin_shutdown(&self) {
        self.shutdown_flag.store(true, Ordering::SeqCst);
        // Wake every connection currently blocked on socket input. `notify_one`
        // also leaves a permit for the accept loop if a test or future internal
        // caller requests shutdown just before that loop begins polling.
        self.shutdown_notify.notify_waiters();
        self.shutdown_notify.notify_one();
    }

    pub async fn start(&self) -> Result<()> {
        validate_direct_parent(self.config.owner_pid)?;

        // Initialize structured logging for daemon mode.
        // RUST_LOG=impulse_rs=debug for verbose output; default is info.
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("impulse_rs=info")),
            )
            .with_target(false)
            .compact()
            .try_init();

        // Register both Unix signals before creating the socket/PID files so
        // there is no startup window in which Ctrl-C or SIGTERM retains its
        // default process-killing behavior and skips daemon cleanup.
        let mut interrupt_signal =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                .context("Failed to install daemon Ctrl-C handler")?;
        let mut terminate_signal =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context("Failed to install daemon SIGTERM handler")?;

        let socket_dir = self
            .config
            .socket_path
            .parent()
            .context("Invalid socket path")?;
        tokio::fs::create_dir_all(socket_dir)
            .await
            .context("Failed to create socket directory")?;
        tokio::fs::set_permissions(socket_dir, std::fs::Permissions::from_mode(0o700))
            .await
            .context("Failed to restrict socket directory permissions")?;

        // Hold this through bind, service, final sync, and socket/PID cleanup.
        // A competing daemon therefore cannot replace either runtime file in
        // the check-then-remove windows below.
        let _daemon_instance_lock = DaemonInstanceLock::acquire(&self.config.socket_path)?;

        // Stale socket detection: try connecting to distinguish crash residue from running daemon
        let pid_path = self.config.socket_path.with_extension("pid");
        if self.config.socket_path.exists() {
            let is_alive = tokio::net::UnixStream::connect(&self.config.socket_path)
                .await
                .is_ok();

            if is_alive {
                anyhow::bail!(
                    "Another daemon is already running (socket: {})",
                    self.config.socket_path.display()
                );
            }

            // Socket exists but nobody's listening → stale from crash
            let _ = tokio::fs::remove_file(&self.config.socket_path).await;
            let _ = tokio::fs::remove_file(&pid_path).await;
        }

        let listener =
            UnixListener::bind(&self.config.socket_path).context("Failed to bind socket")?;
        tokio::fs::set_permissions(
            &self.config.socket_path,
            std::fs::Permissions::from_mode(0o600),
        )
        .await
        .context("Failed to restrict daemon socket permissions")?;

        // Write the PID marker without following a repository-controlled
        // symlink. The descriptor-level regular-file check closes the gap
        // between project activation preflight and daemon startup.
        let mut pid_file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&pid_path)
            .with_context(|| format!("Failed to open daemon PID file {}", pid_path.display()))?;
        if !pid_file
            .metadata()
            .with_context(|| format!("Failed to inspect daemon PID file {}", pid_path.display()))?
            .is_file()
        {
            anyhow::bail!(
                "Daemon PID file {} must be a regular file",
                pid_path.display()
            );
        }
        pid_file
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .with_context(|| {
                format!("Failed to restrict daemon PID file {}", pid_path.display())
            })?;
        std::io::Write::write_all(&mut pid_file, std::process::id().to_string().as_bytes())
            .context("Failed to write daemon PID file")?;
        pid_file
            .sync_all()
            .context("Failed to sync daemon PID file")?;

        println!("Daemon listening on {}", self.config.socket_path.display());

        let mut connections = JoinSet::new();
        let mut owner_parent_poll = tokio::time::interval(OWNER_PARENT_POLL_INTERVAL);
        owner_parent_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let accept_result: Result<()> = loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            let state = self.config.state.clone();
                            let shutdown = self.shutdown_flag.clone();
                            let shutdown_notify = self.shutdown_notify.clone();
                            let registry = self.tool_registry.clone();
                            let tool_context = self.tool_context.clone();
                            let terminal_telemetry = self.terminal_telemetry.clone();
                            let supervisor_session_override =
                                self.supervisor_session_override.clone();
                            let conflict_resolver = self.conflict_resolver.clone();
                            let delegation_tracker = self.delegation_tracker.clone();
                            let cached_agent = self.cached_agent.clone();
                            connections.spawn(async move {
                                handle_connection(
                                    stream,
                                    ConnectionContext {
                                        state,
                                        shutdown,
                                        shutdown_notify,
                                        registry,
                                        tool_context,
                                        terminal_telemetry,
                                        supervisor_session_override,
                                        conflict_resolver,
                                        delegation_tracker,
                                        cached_agent,
                                    },
                                )
                                .await
                            });
                        }
                        Err(e) => {
                            eprintln!("Accept error: {}", e);
                        }
                    }
                }
                completed = connections.join_next(), if !connections.is_empty() => {
                    if let Some(completed) = completed {
                        log_connection_completion(completed);
                    }
                }
                _ = self.shutdown_notify.notified() => {
                    println!("Shutting down daemon (internal request)...");
                    break Ok(());
                }
                signal = interrupt_signal.recv() => {
                    match signal {
                        Some(()) => {
                            println!("Shutting down daemon (Ctrl-C)...");
                            break Ok(());
                        }
                        None => break Err(anyhow::anyhow!("daemon Ctrl-C signal stream closed unexpectedly")),
                    }
                }
                signal = terminate_signal.recv() => {
                    match signal {
                        Some(()) => {
                            println!("Shutting down daemon (SIGTERM)...");
                            break Ok(());
                        }
                        None => break Err(anyhow::anyhow!("daemon SIGTERM signal stream closed unexpectedly")),
                    }
                }
                _ = owner_parent_poll.tick(), if self.config.owner_pid.is_some() => {
                    let expected_owner = self
                        .config
                        .owner_pid
                        .expect("owner poll branch requires an owner PID");
                    let observed_parent = direct_parent_pid();
                    if observed_parent != expected_owner {
                        println!(
                            "Shutting down daemon (desktop owner PID {expected_owner} exited; current parent PID is {observed_parent})..."
                        );
                        break Ok(());
                    }
                }
            }
        };

        // Stop accepting before waking clients, then let any request already
        // inside a handler finish within the bounded grace period.
        self.begin_shutdown();
        drop(listener);
        drain_connections(&mut connections).await;

        // Always attempt both persistence and runtime-file cleanup, even when
        // signal handling or one shutdown operation reports an error.
        let sync_result = self
            .config
            .state
            .sync_immediate()
            .await
            .context("Failed to persist daemon state during shutdown");
        let cleanup_result =
            remove_owned_daemon_files(&self.config.socket_path, &pid_path, std::process::id())
                .await;

        combine_shutdown_results(accept_result, sync_result, cleanup_result)
    }
}

fn direct_parent_pid() -> u32 {
    // SAFETY: `getppid` has no preconditions and returns the caller's current
    // parent process ID.
    unsafe { libc::getppid() as u32 }
}

fn validate_direct_parent(owner_pid: Option<u32>) -> Result<()> {
    let Some(expected_owner) = owner_pid else {
        return Ok(());
    };
    let observed_parent = direct_parent_pid();
    if observed_parent != expected_owner {
        anyhow::bail!(
            "daemon owner PID {expected_owner} is not its direct parent (observed parent PID {observed_parent})"
        );
    }
    Ok(())
}

fn log_connection_completion(completed: std::result::Result<Result<()>, tokio::task::JoinError>) {
    match completed {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!("daemon connection ended with an error: {error:#}"),
        Err(error) if error.is_cancelled() => {
            tracing::debug!("daemon connection cancelled during shutdown")
        }
        Err(error) => tracing::error!("daemon connection task failed: {error}"),
    }
}

async fn drain_connections(connections: &mut JoinSet<Result<()>>) {
    let drain = async {
        while let Some(completed) = connections.join_next().await {
            log_connection_completion(completed);
        }
    };

    if tokio::time::timeout(CONNECTION_SHUTDOWN_GRACE, drain)
        .await
        .is_err()
    {
        tracing::warn!(
            grace_ms = CONNECTION_SHUTDOWN_GRACE.as_millis() as u64,
            remaining_connections = connections.len(),
            "daemon connection drain timed out; cancelling remaining requests"
        );
        connections.abort_all();
        while let Some(completed) = connections.join_next().await {
            log_connection_completion(completed);
        }
    }
}

async fn remove_file_if_present(path: &Path) -> Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Failed to remove {}", path.display())),
    }
}

async fn remove_owned_daemon_files(
    socket_path: &Path,
    pid_path: &Path,
    expected_pid: u32,
) -> Result<()> {
    match tokio::fs::read_to_string(pid_path).await {
        Ok(contents) if contents.trim() != expected_pid.to_string() => {
            anyhow::bail!(
                "Refusing to remove daemon runtime files because {} is owned by PID {}, not {}",
                pid_path.display(),
                contents.trim(),
                expected_pid
            );
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(
                pid_path = %pid_path.display(),
                "daemon PID file disappeared before shutdown cleanup"
            );
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!("Failed to verify daemon PID file {}", pid_path.display())
            });
        }
    }

    remove_file_if_present(socket_path).await?;
    remove_file_if_present(pid_path).await?;
    Ok(())
}

fn combine_shutdown_results(
    accept_result: Result<()>,
    sync_result: Result<()>,
    cleanup_result: Result<()>,
) -> Result<()> {
    let mut failures = Vec::new();
    if let Err(error) = accept_result {
        failures.push(format!("accept loop: {error:#}"));
    }
    if let Err(error) = sync_result {
        failures.push(format!("state sync: {error:#}"));
    }
    if let Err(error) = cleanup_result {
        failures.push(format!("runtime cleanup: {error:#}"));
    }

    if failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("daemon shutdown failed: {}", failures.join("; "))
    }
}

struct ConnectionContext {
    state: SharedState,
    shutdown: Arc<AtomicBool>,
    shutdown_notify: Arc<Notify>,
    registry: Arc<crate::tooling::ToolRegistry>,
    tool_context: crate::tooling::ToolContext,
    terminal_telemetry: Arc<RwLock<crate::ops_workbench::TerminalOpsTelemetryStore>>,
    supervisor_session_override: Arc<RwLock<Option<impulse_ops::SupervisorPermissionPolicy>>>,
    conflict_resolver: Arc<RwLock<crate::agent::coordinator::ConflictResolver>>,
    delegation_tracker: Arc<RwLock<crate::delegation::DelegationTracker>>,
    cached_agent: Arc<tokio::sync::Mutex<Option<crate::agent::ImpulseAgent>>>,
}

async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    context: ConnectionContext,
) -> Result<()> {
    let ConnectionContext {
        state,
        shutdown,
        shutdown_notify,
        registry,
        tool_context,
        terminal_telemetry,
        supervisor_session_override,
        conflict_resolver,
        delegation_tracker,
        cached_agent,
    } = context;

    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);

    loop {
        // Register the notification before checking the flag so shutdown
        // cannot slip between those two operations and leave this connection
        // blocked forever waiting for another request.
        let shutdown_requested = shutdown_notify.notified();
        tokio::pin!(shutdown_requested);
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        let read_result = tokio::select! {
            result = read_bounded_line(&mut reader, MAX_REQUEST_SIZE) => result?,
            _ = &mut shutdown_requested => break,
        };

        let line = match read_result {
            BoundedLine::Eof => break,
            BoundedLine::TooLarge => {
                // The oversized line was never fully buffered (bounded read
                // capped memory at MAX_REQUEST_SIZE + 1 bytes), but the
                // remaining bytes still on the wire for this "line" are of
                // unknown length -- there is no bounded way to skip past
                // them and resynchronize on the next `\n`. Reject this
                // request and close the connection rather than attempt an
                // unbounded drain, matching the size guard's original
                // intent (reject oversized requests) without reintroducing
                // the unbounded-read bug.
                let err_response = DaemonResponse::Error {
                    message: format!("Request too large (max {} bytes)", MAX_REQUEST_SIZE),
                };
                let response_json = serde_json::to_string(&err_response)?;
                writer.write_all(response_json.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
                break;
            }
            BoundedLine::Line(line) => line,
        };

        let request: DaemonRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let response = DaemonResponse::Error {
                    message: format!("Failed to parse request: {}", e),
                };
                writer
                    .write_all(serde_json::to_string(&response)?.as_bytes())
                    .await?;
                writer.write_all(b"\n").await?;
                continue;
            }
        };

        let req_type = protocol::request_type_name(&request);
        let start = std::time::Instant::now();
        let response = handlers::process_request(
            request,
            handlers::ProcessRequestContext {
                state: state.clone(),
                registry: &registry,
                tool_context: &tool_context,
                terminal_telemetry: &terminal_telemetry,
                supervisor_session_override: &supervisor_session_override,
                conflict_resolver: &conflict_resolver,
                delegation_tracker: &delegation_tracker,
                cached_agent: &cached_agent,
            },
        )
        .await;
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 100 {
            tracing::warn!(
                request_type = req_type,
                elapsed_ms = elapsed.as_millis() as u64,
                "slow request"
            );
        } else {
            tracing::debug!(
                request_type = req_type,
                elapsed_ms = elapsed.as_millis() as u64,
                "request processed"
            );
        }

        writer
            .write_all(serde_json::to_string(&response)?.as_bytes())
            .await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        if shutdown.load(Ordering::SeqCst) {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod bounded_line_tests {
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll};

    use tokio::io::{AsyncBufRead, AsyncRead, ReadBuf};

    use super::{read_bounded_line, BoundedLine};

    #[tokio::test]
    async fn test_read_bounded_line_reads_lines_under_cap_normally() {
        let data = b"hello world\nsecond line\n".to_vec();
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(data));

        match read_bounded_line(&mut reader, 1024).await.unwrap() {
            BoundedLine::Line(line) => assert_eq!(line, "hello world"),
            _ => panic!("expected first line to be read normally"),
        }
        match read_bounded_line(&mut reader, 1024).await.unwrap() {
            BoundedLine::Line(line) => assert_eq!(line, "second line"),
            _ => panic!("expected second line to be read normally"),
        }
        match read_bounded_line(&mut reader, 1024).await.unwrap() {
            BoundedLine::Eof => {}
            _ => panic!("expected EOF after both lines consumed"),
        }
    }

    #[tokio::test]
    async fn test_read_bounded_line_accepts_a_line_exactly_at_the_cap() {
        // A line whose payload (excluding the trailing \n) is exactly
        // `max_bytes` long must be accepted, not rejected -- the cap is
        // inclusive.
        let payload = "a".repeat(64);
        let data = format!("{payload}\n").into_bytes();
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(data));

        match read_bounded_line(&mut reader, 64).await.unwrap() {
            BoundedLine::Line(line) => assert_eq!(line, payload),
            BoundedLine::TooLarge => {
                panic!("an exactly-at-cap line must be accepted, not rejected")
            }
            BoundedLine::Eof => panic!("expected a line, got EOF"),
        }
    }

    #[tokio::test]
    async fn test_read_bounded_line_rejects_a_line_one_byte_over_the_cap() {
        let payload = "a".repeat(65);
        let data = format!("{payload}\n").into_bytes();
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(data));

        let result = read_bounded_line(&mut reader, 64).await.unwrap();
        assert!(matches!(result, BoundedLine::TooLarge));
    }

    /// An `AsyncRead + AsyncBufRead` source that never produces a `\n` and
    /// never reaches EOF -- standing in for an adversarial client sending
    /// gigabytes of non-newline bytes on one connection. Counts every byte
    /// actually consumed through the `poll_fill_buf`/`consume` protocol so
    /// the test can assert `read_bounded_line` only ever pulls a bounded
    /// amount from it, even though the source itself is unbounded.
    struct InfiniteByteSource {
        bytes_consumed: Arc<AtomicUsize>,
        chunk: [u8; 64],
    }

    impl InfiniteByteSource {
        fn new(bytes_consumed: Arc<AtomicUsize>) -> Self {
            Self {
                bytes_consumed,
                chunk: [b'a'; 64],
            }
        }
    }

    impl AsyncRead for InfiniteByteSource {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            // Not exercised by `read_bounded_line` (which only uses the
            // AsyncBufRead half via `read_until`), but required to satisfy
            // `AsyncReadExt::take`'s bound on the wrapped reader.
            let this = self.get_mut();
            let n = buf.remaining().min(this.chunk.len());
            buf.put_slice(&this.chunk[..n]);
            this.bytes_consumed.fetch_add(n, Ordering::SeqCst);
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncBufRead for InfiniteByteSource {
        fn poll_fill_buf(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<&[u8]>> {
            let this = self.get_mut();
            Poll::Ready(Ok(&this.chunk[..]))
        }

        fn consume(self: Pin<&mut Self>, amt: usize) {
            let this = self.get_mut();
            this.bytes_consumed.fetch_add(amt, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn test_read_bounded_line_rejects_oversized_input_without_unbounded_memory_use() {
        let bytes_consumed = Arc::new(AtomicUsize::new(0));
        let mut source = InfiniteByteSource::new(bytes_consumed.clone());
        let max_bytes = 4096;

        let result = read_bounded_line(&mut source, max_bytes).await.unwrap();
        assert!(
            matches!(result, BoundedLine::TooLarge),
            "an infinite non-newline source must be rejected as too-large, not read forever"
        );

        let pulled = bytes_consumed.load(Ordering::SeqCst);
        // Bounded by max_bytes + 1 (the read_bounded_line cap) plus at most
        // one extra chunk's worth of slop from the source's internal
        // fill_buf granularity -- nowhere close to "as much as the source
        // would provide" (which is unbounded).
        assert!(
            pulled <= max_bytes + 1 + 64,
            "expected a bounded number of bytes pulled from an infinite source, got {pulled}"
        );
    }
}

#[cfg(test)]
mod tests;
