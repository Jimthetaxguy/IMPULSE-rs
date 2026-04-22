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
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{Notify, RwLock};

use crate::state::SharedState;

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
    /// Cached ImpulseAgent instance that persists across requests within the
    /// daemon session — enables session history continuity for multi-turn queries.
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
            cached_agent: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    #[cfg(test)]
    pub fn socket_path(&self) -> &PathBuf {
        &self.config.socket_path
    }

    pub async fn start(&self) -> Result<()> {
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

        let socket_dir = self
            .config
            .socket_path
            .parent()
            .context("Invalid socket path")?;
        tokio::fs::create_dir_all(socket_dir)
            .await
            .context("Failed to create socket directory")?;

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

        // Write PID file for stale socket detection on next startup
        let _ = tokio::fs::write(&pid_path, std::process::id().to_string()).await;

        println!("Daemon listening on {}", self.config.socket_path.display());

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            let state = self.config.state.clone();
                            let shutdown = self.shutdown_flag.clone();
                            let notify = self.shutdown_notify.clone();
                            let registry = self.tool_registry.clone();
                            let tool_context = self.tool_context.clone();
                            let terminal_telemetry = self.terminal_telemetry.clone();
                            let supervisor_session_override =
                                self.supervisor_session_override.clone();
                            let conflict_resolver = self.conflict_resolver.clone();
                            let cached_agent = self.cached_agent.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(
                                    stream,
                                    state,
                                    shutdown,
                                    notify,
                                    registry,
                                    tool_context,
                                    terminal_telemetry,
                                    supervisor_session_override,
                                    conflict_resolver,
                                    cached_agent,
                                )
                                .await
                                {
                                    eprintln!("Connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("Accept error: {}", e);
                        }
                    }
                }
                _ = self.shutdown_notify.notified() => {
                    println!("Shutting down daemon...");
                    break;
                }
            }
        }

        Ok(())
    }
}

// TODO(refactor): extract params into struct
#[allow(clippy::too_many_arguments)]
async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    state: SharedState,
    shutdown: Arc<AtomicBool>,
    _notify: Arc<Notify>,
    registry: Arc<crate::tooling::ToolRegistry>,
    tool_context: crate::tooling::ToolContext,
    terminal_telemetry: Arc<RwLock<crate::ops_workbench::TerminalOpsTelemetryStore>>,
    supervisor_session_override: Arc<RwLock<Option<impulse_ops::SupervisorPermissionPolicy>>>,
    conflict_resolver: Arc<RwLock<crate::agent::coordinator::ConflictResolver>>,
    cached_agent: Arc<tokio::sync::Mutex<Option<crate::agent::ImpulseAgent>>>,
) -> Result<()> {
    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);

    const MAX_REQUEST_SIZE: usize = 10 * 1024 * 1024; // 10MB limit per request
    let mut line = String::new();
    while reader.read_line(&mut line).await? > 0 {
        if line.len() > MAX_REQUEST_SIZE {
            let err_response = DaemonResponse::Error {
                message: format!(
                    "Request too large ({} bytes, max {})",
                    line.len(),
                    MAX_REQUEST_SIZE
                ),
            };
            let response_json = serde_json::to_string(&err_response)?;
            writer.write_all(response_json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
            line.clear();
            continue;
        }
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
                line.clear();
                continue;
            }
        };

        let req_type = protocol::request_type_name(&request);
        let start = std::time::Instant::now();
        let response = handlers::process_request(
            request,
            state.clone(),
            &registry,
            &tool_context,
            &terminal_telemetry,
            &supervisor_session_override,
            &conflict_resolver,
            &cached_agent,
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

        line.clear();

        if shutdown.load(Ordering::SeqCst) {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests;
