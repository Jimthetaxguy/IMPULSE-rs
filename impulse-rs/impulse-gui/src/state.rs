//! Shared egui operator state between the background IPC worker and the UI thread.
//!
//! The daemon is the source of truth for the operator workbench. The GUI keeps
//! only view state plus local task notices and publishes terminal telemetry back
//! to the daemon.

use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::ipc::{DaemonClient, DaemonStatus, Genome, HistoryEntry, SearchResult, Session};

const STATUS_POLL: Duration = Duration::from_secs(2);
const HISTORY_POLL: Duration = Duration::from_secs(10);
const GENOME_POLL: Duration = Duration::from_secs(30);
const OPS_RECONCILE: Duration = Duration::from_secs(15);
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonAutoStartState {
    NotAttempted,
    Starting,
    Running,
    Failed(String),
    BinaryNotFound,
}

#[derive(Debug, Clone)]
pub struct LiveSearchResult {
    pub title: String,
    pub agent: String,
    #[allow(dead_code)]
    pub timestamp: String,
}

#[derive(Debug, Clone)]
pub struct TaskNotice {
    pub level: TaskNoticeLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskNoticeLevel {
    Info,
    Success,
    Warning,
    Error,
}

pub struct SharedState {
    pub connection: ConnectionStatus,
    pub daemon_status: Option<DaemonStatus>,
    pub sessions: Vec<Session>,
    pub history: Vec<HistoryEntry>,
    pub genome: Option<Genome>,
    pub search_results: Vec<SearchResult>,
    pub search_query: String,
    pub search_in_progress: bool,
    pub last_poll: Option<Instant>,
    pub error: Option<String>,
    pub live_search_results: Vec<LiveSearchResult>,
    pub ops_snapshot: Option<impulse_ops::ProjectOpsSnapshot>,
    pub ops_events: Vec<impulse_ops::OpsEvent>,
    pub next_ops_seq: Option<u64>,
    pub supervisor_permissions: Option<impulse_ops::SupervisorPermissionState>,
    pub task_notices: Vec<TaskNotice>,
    pub daemon_auto_start: DaemonAutoStartState,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            connection: ConnectionStatus::Disconnected,
            daemon_status: None,
            sessions: Vec::new(),
            history: Vec::new(),
            genome: None,
            search_results: Vec::new(),
            search_query: String::new(),
            search_in_progress: false,
            last_poll: None,
            error: None,
            live_search_results: Vec::new(),
            ops_snapshot: None,
            ops_events: Vec::new(),
            next_ops_seq: None,
            supervisor_permissions: None,
            task_notices: Vec::new(),
            daemon_auto_start: DaemonAutoStartState::NotAttempted,
        }
    }
}

pub type StateHandle = Arc<Mutex<SharedState>>;

#[derive(Debug)]
pub enum PollerCommand {
    Refresh,
    Search(String),
    CreateSession {
        name: String,
        platform: String,
    },
    EndSession {
        session_id: String,
        summary: String,
    },
    RunArtifactAction {
        artifact_id: String,
        action_id: String,
        params: serde_json::Value,
    },
    RunSupervisorAction {
        action: impulse_ops::SupervisorAction,
    },
    PublishTerminalOps {
        report: impulse_ops::TerminalOpsReport,
    },
    SetMemoryView {
        active: bool,
    },
    CreateTabSession {
        tab_id: u64,
        name: String,
        platform: String,
    },
    EndTabSession {
        #[allow(dead_code)]
        tab_id: u64,
        session_id: String,
        summary: String,
    },
    TrackFile {
        session_id: String,
        file_path: String,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum PollerEvent {
    ArtifactActionResult(impulse_ops::ArtifactActionResult),
    SupervisorActionResult(impulse_ops::SupervisorActionResult),
    TabSessionCreated { tab_id: u64, session_id: String },
    TabSessionFailed { tab_id: u64, error: String },
}

pub fn start_poller(
    ctx: egui::Context,
) -> (
    StateHandle,
    std::sync::mpsc::Sender<PollerCommand>,
    std::sync::mpsc::Receiver<PollerEvent>,
    JoinHandle<()>,
) {
    let state: StateHandle = Arc::new(Mutex::new(SharedState::default()));
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<PollerCommand>();
    let (evt_tx, evt_rx) = std::sync::mpsc::channel::<PollerEvent>();

    let state_clone = Arc::clone(&state);
    let handle = thread::Builder::new()
        .name("impulse-poller".into())
        .spawn(move || {
            poller_loop(state_clone, cmd_rx, evt_tx, ctx);
        })
        .expect("failed to spawn poller thread");

    (state, cmd_tx, evt_rx, handle)
}

fn poller_loop(
    state: StateHandle,
    cmd_rx: std::sync::mpsc::Receiver<PollerCommand>,
    evt_tx: std::sync::mpsc::Sender<PollerEvent>,
    ctx: egui::Context,
) {
    let mut client = DaemonClient::discover();
    let mut last_status = Instant::now() - STATUS_POLL;
    let mut last_history = Instant::now() - HISTORY_POLL;
    let mut last_genome = Instant::now() - GENOME_POLL;
    let mut last_ops_reconcile = Instant::now() - OPS_RECONCILE;
    let mut connected = false;
    let mut memory_view_active = false;
    let mut daemon_process: Option<Child> = None;
    let mut daemon_start_attempted = false;

    loop {
        match cmd_rx.try_recv() {
            Ok(PollerCommand::Shutdown) => {
                log::info!("Poller thread shutting down");
                if let Some(mut child) = daemon_process.take() {
                    log::info!("Stopping auto-started daemon (pid {})", child.id());
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return;
            }
            Ok(PollerCommand::Refresh) => {
                last_status = Instant::now() - STATUS_POLL;
                last_history = Instant::now() - HISTORY_POLL;
                last_genome = Instant::now() - GENOME_POLL;
                last_ops_reconcile = Instant::now() - OPS_RECONCILE;
            }
            Ok(PollerCommand::Search(query)) => {
                run_search(&mut client, &state, &query);
                ctx.request_repaint();
            }
            Ok(PollerCommand::CreateSession { name, platform }) => {
                let outcome = client.create_session(&name, Some(&platform));
                match outcome {
                    Ok(session) => {
                        push_notice(
                            &state,
                            TaskNoticeLevel::Success,
                            format!("Created session {}", session.name),
                        );
                        refresh_connected(&mut client, &state, memory_view_active);
                        last_ops_reconcile = Instant::now();
                    }
                    Err(err) => {
                        push_notice(
                            &state,
                            TaskNoticeLevel::Error,
                            format!("Create session failed: {}", err),
                        );
                    }
                }
                ctx.request_repaint();
            }
            Ok(PollerCommand::EndSession {
                session_id,
                summary,
            }) => {
                let outcome = client.end_session(&session_id, &summary);
                match outcome {
                    Ok(()) => {
                        push_notice(
                            &state,
                            TaskNoticeLevel::Success,
                            format!("Ended session {}", session_id),
                        );
                        refresh_connected(&mut client, &state, memory_view_active);
                        last_ops_reconcile = Instant::now();
                    }
                    Err(err) => {
                        push_notice(
                            &state,
                            TaskNoticeLevel::Error,
                            format!("End session failed: {}", err),
                        );
                    }
                }
                ctx.request_repaint();
            }
            Ok(PollerCommand::RunArtifactAction {
                artifact_id,
                action_id,
                params,
            }) => {
                match client.run_artifact_action(&artifact_id, &action_id, params) {
                    Ok(result) => {
                        let _ = evt_tx.send(PollerEvent::ArtifactActionResult(result));
                        refresh_connected(&mut client, &state, false);
                        last_ops_reconcile = Instant::now();
                    }
                    Err(err) => {
                        push_notice(
                            &state,
                            TaskNoticeLevel::Error,
                            format!("Artifact action failed: {}", err),
                        );
                    }
                }
                ctx.request_repaint();
            }
            Ok(PollerCommand::RunSupervisorAction { action }) => {
                match client.run_supervisor_action(action) {
                    Ok(result) => {
                        if let Some(permission_state) = result.permission_state.clone() {
                            if let Ok(mut shared) = state.lock() {
                                shared.supervisor_permissions = Some(permission_state);
                            }
                        }
                        let _ = evt_tx.send(PollerEvent::SupervisorActionResult(result));
                        refresh_connected(&mut client, &state, memory_view_active);
                        last_ops_reconcile = Instant::now();
                    }
                    Err(err) => {
                        push_notice(
                            &state,
                            TaskNoticeLevel::Error,
                            format!("Supervisor action failed: {}", err),
                        );
                    }
                }
                ctx.request_repaint();
            }
            Ok(PollerCommand::PublishTerminalOps { report }) => {
                if connected {
                    if client.publish_terminal_ops(&report).is_ok() {
                        refresh_ops_delta(&mut client, &state);
                        ctx.request_repaint();
                    } else {
                        log::debug!(
                            "Terminal telemetry publish failed (daemon may have disconnected)"
                        );
                    }
                }
            }
            Ok(PollerCommand::SetMemoryView { active }) => {
                memory_view_active = active;
                if active {
                    last_history = Instant::now() - HISTORY_POLL;
                    last_genome = Instant::now() - GENOME_POLL;
                }
            }
            Ok(PollerCommand::CreateTabSession {
                tab_id,
                name,
                platform,
            }) => {
                if connected {
                    match client.create_session(&name, Some(&platform)) {
                        Ok(session) => {
                            let _ = evt_tx.send(PollerEvent::TabSessionCreated {
                                tab_id,
                                session_id: session.id.clone(),
                            });
                            log::info!("Created daemon session {} for tab {}", session.id, tab_id);
                        }
                        Err(err) => {
                            let _ = evt_tx.send(PollerEvent::TabSessionFailed {
                                tab_id,
                                error: err.clone(),
                            });
                            log::warn!("CreateTabSession failed for tab {}: {}", tab_id, err);
                        }
                    }
                    ctx.request_repaint();
                }
            }
            Ok(PollerCommand::EndTabSession {
                tab_id: _,
                session_id,
                summary,
            }) => {
                if connected {
                    if let Err(err) = client.end_session(&session_id, &summary) {
                        log::warn!("EndTabSession failed for {}: {}", session_id, err);
                    } else {
                        log::info!("Ended daemon session {}", session_id);
                    }
                    ctx.request_repaint();
                }
            }
            Ok(PollerCommand::TrackFile {
                session_id,
                file_path,
            }) => {
                if connected {
                    if let Err(err) = client.track_file(&session_id, &file_path) {
                        log::debug!("TrackFile failed for {}: {}", session_id, err);
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                log::info!("Poller command channel closed");
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }

        // Auto-start daemon if not connected and not yet attempted.
        if !connected && !daemon_start_attempted {
            daemon_start_attempted = true;
            match which::which("impulse-rs") {
                Err(_) => {
                    log::warn!("impulse-rs binary not found on PATH");
                    set_auto_start(&state, DaemonAutoStartState::BinaryNotFound);
                }
                Ok(binary_path) => {
                    log::info!("Auto-starting daemon: {}", binary_path.display());
                    set_auto_start(&state, DaemonAutoStartState::Starting);
                    ctx.request_repaint();
                    match Command::new(&binary_path)
                        .arg("daemon")
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        Ok(child) => {
                            log::info!("Daemon spawned (pid {})", child.id());
                            daemon_process = Some(child);
                            // Give the daemon time to create its socket.
                            thread::sleep(Duration::from_secs(2));
                            // Re-discover the socket path now that daemon may have created it.
                            client = DaemonClient::discover();
                        }
                        Err(e) => {
                            log::error!("Failed to start daemon: {}", e);
                            set_auto_start(&state, DaemonAutoStartState::Failed(e.to_string()));
                        }
                    }
                }
            }
        }

        // Check if auto-started daemon exited unexpectedly — allow retry.
        if let Some(ref mut child) = daemon_process {
            match child.try_wait() {
                Ok(Some(status)) => {
                    log::warn!("Auto-started daemon exited with status: {}", status);
                    daemon_process = None;
                    daemon_start_attempted = false;
                    if connected {
                        connected = false;
                        set_connection(&state, ConnectionStatus::Disconnected);
                        push_notice(
                            &state,
                            TaskNoticeLevel::Warning,
                            "Daemon exited unexpectedly — will auto-restart".to_string(),
                        );
                    }
                }
                Ok(None) => {} // still running
                Err(e) => {
                    log::warn!("Failed to check daemon status: {}", e);
                }
            }
        }

        let now = Instant::now();
        if !connected || now.duration_since(last_status) >= STATUS_POLL {
            set_connection(&state, ConnectionStatus::Connecting);
            match client.ping() {
                Ok(true) => {
                    if !connected {
                        connected = true;
                        set_auto_start(&state, DaemonAutoStartState::Running);
                        push_notice(
                            &state,
                            TaskNoticeLevel::Info,
                            "Connected to Impulse daemon".to_string(),
                        );
                        refresh_connected(&mut client, &state, memory_view_active);
                        last_ops_reconcile = now;
                    }
                    set_connection(&state, ConnectionStatus::Connected);
                    refresh_status(&mut client, &state);
                    refresh_ops_delta(&mut client, &state);
                    if now.duration_since(last_ops_reconcile) >= OPS_RECONCILE {
                        refresh_ops_full(&mut client, &state);
                        last_ops_reconcile = now;
                    }
                    last_status = now;
                    ctx.request_repaint();
                }
                _ => {
                    if connected {
                        log::warn!("Lost connection to daemon");
                        push_notice(
                            &state,
                            TaskNoticeLevel::Warning,
                            "Lost connection to Impulse daemon".to_string(),
                        );
                    }
                    connected = false;
                    set_connection(&state, ConnectionStatus::Disconnected);
                    if let Ok(mut shared) = state.lock() {
                        shared.next_ops_seq = None;
                        shared.supervisor_permissions = None;
                    }
                    ctx.request_repaint();
                    thread::sleep(RECONNECT_DELAY);
                    last_status = now;
                    continue;
                }
            }
        }

        if connected && memory_view_active && now.duration_since(last_history) >= HISTORY_POLL {
            if let Ok(entries) = client.list_history() {
                if let Ok(mut shared) = state.lock() {
                    shared.history = entries;
                }
                ctx.request_repaint();
            }
            last_history = now;
        }

        if connected && memory_view_active && now.duration_since(last_genome) >= GENOME_POLL {
            if let Ok(genome) = client.read_genome() {
                if let Ok(mut shared) = state.lock() {
                    shared.genome = Some(genome);
                }
                ctx.request_repaint();
            }
            last_genome = now;
        }

        thread::sleep(Duration::from_millis(500));
    }
}

fn refresh_connected(client: &mut DaemonClient, state: &StateHandle, include_memory: bool) {
    refresh_status(client, state);
    refresh_ops_full(client, state);
    refresh_supervisor_permissions(client, state);
    if include_memory {
        if let Ok(entries) = client.list_history() {
            if let Ok(mut shared) = state.lock() {
                shared.history = entries;
            }
        }
        if let Ok(genome) = client.read_genome() {
            if let Ok(mut shared) = state.lock() {
                shared.genome = Some(genome);
            }
        }
    }
}

fn refresh_status(client: &mut DaemonClient, state: &StateHandle) {
    if let Ok(status) = client.status() {
        if let Ok(sessions) = client.list_sessions() {
            if let Ok(mut shared) = state.lock() {
                shared.daemon_status = Some(status);
                shared.sessions = sessions;
                shared.last_poll = Some(Instant::now());
                shared.error = None;
            }
        }
    }
}

fn refresh_ops_full(client: &mut DaemonClient, state: &StateHandle) {
    if let Ok(snapshot) = client.get_ops_snapshot() {
        if let Ok(mut shared) = state.lock() {
            shared.ops_snapshot = Some(snapshot);
            shared.ops_events.clear();
            shared.next_ops_seq = None;
            shared.error = None;
        }
    }
}

fn refresh_ops_delta(client: &mut DaemonClient, state: &StateHandle) {
    let since_seq = state.lock().ok().and_then(|shared| shared.next_ops_seq);
    if let Ok(subscription) = client.subscribe_ops(since_seq) {
        if let Ok(mut shared) = state.lock() {
            shared.ops_snapshot = Some(subscription.snapshot);
            shared.ops_events = subscription.events;
            shared.next_ops_seq = Some(subscription.next_seq);
            shared.error = None;
        }
    }
}

fn refresh_supervisor_permissions(client: &mut DaemonClient, state: &StateHandle) {
    if let Ok(permission_state) = client.get_supervisor_permissions() {
        if let Ok(mut shared) = state.lock() {
            shared.supervisor_permissions = Some(permission_state);
            shared.error = None;
        }
    }
}

fn set_connection(state: &StateHandle, status: ConnectionStatus) {
    if let Ok(mut shared) = state.lock() {
        shared.connection = status;
    }
}

fn set_auto_start(state: &StateHandle, auto_start: DaemonAutoStartState) {
    if let Ok(mut shared) = state.lock() {
        shared.daemon_auto_start = auto_start;
    }
}

fn push_notice(state: &StateHandle, level: TaskNoticeLevel, message: String) {
    if let Ok(mut shared) = state.lock() {
        shared.task_notices.push(TaskNotice { level, message });
    }
}

fn run_search(client: &mut DaemonClient, state: &StateHandle, query: &str) {
    if let Ok(mut shared) = state.lock() {
        shared.search_in_progress = true;
        shared.search_query = query.to_string();
    }

    let results = client.search(query);
    if let Ok(mut shared) = state.lock() {
        shared.search_in_progress = false;
        match results {
            Ok(items) => {
                shared.search_results = items;
                shared.error = None;
            }
            Err(err) => {
                shared.search_results.clear();
                shared.error = Some(format!("Search failed: {}", err));
            }
        }
    }
}
