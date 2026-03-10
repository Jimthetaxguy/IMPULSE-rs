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

use crate::ipc::{
    DaemonClient, DaemonStatus, Genome, GuardRule, HistoryEntry, SearchResult, Session,
    EXPECTED_PROTOCOL_VERSION,
};

const DEFAULT_POLL_INTERVAL: u64 = 2;
const DEFAULT_HISTORY_POLL: u64 = 10;
const DEFAULT_GENOME_POLL: u64 = 30;
const DEFAULT_OPS_RECONCILE: u64 = 15;
const DEFAULT_RECONNECT_DELAY: u64 = 5;
const DEFAULT_CONTEXT_TICK_INTERVAL: u64 = 3;
const DEFAULT_SEARCH_LIMIT: usize = 20;
const DEFAULT_MAX_HISTORY_ENTRIES: usize = 100;
const DEFAULT_CACHE_TTL: u64 = 60;

// ---------------------------------------------------------------------------
// RuntimeSettings — typed values from GlobalConfig.settings HashMap
// ---------------------------------------------------------------------------

/// Parsed runtime settings derived from the key-value settings map.
/// Provides typed access with defaults for all 20 GUI settings.
#[derive(Debug, Clone)]
pub struct RuntimeSettings {
    // Performance
    pub poll_interval_secs: u64,
    pub cache_ttl_secs: u64,
    pub max_history_entries: usize,
    pub max_terminal_scrollback: usize,
    // Search
    pub search_limit: usize,
    pub search_threshold: u32,
    pub search_include_archived: bool,
    // Injection
    pub inject_mode: String,
    pub inject_explain: bool,
    pub inject_max_tokens: usize,
    pub inject_interval_secs: u64,
    // Agent
    pub agent_provider: String,
    pub agent_model: String,
    pub agent_harness: String,
    pub agent_max_tokens: usize,
    // Stewardship
    pub stewardship_mode: String,
    pub stewardship_threshold_surgical: u32,
    pub stewardship_threshold_thoughtful: u32,
    pub stewardship_threshold_emergency: u32,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            poll_interval_secs: DEFAULT_POLL_INTERVAL,
            cache_ttl_secs: DEFAULT_CACHE_TTL,
            max_history_entries: DEFAULT_MAX_HISTORY_ENTRIES,
            max_terminal_scrollback: 10000,
            search_limit: DEFAULT_SEARCH_LIMIT,
            search_threshold: 50,
            search_include_archived: false,
            inject_mode: "review".to_string(),
            inject_explain: false,
            inject_max_tokens: 2048,
            inject_interval_secs: DEFAULT_CONTEXT_TICK_INTERVAL,
            agent_provider: "anthropic".to_string(),
            agent_model: "claude-sonnet-4-20250514".to_string(),
            agent_harness: "claude-code".to_string(),
            agent_max_tokens: 4096,
            stewardship_mode: "review".to_string(),
            stewardship_threshold_surgical: 55,
            stewardship_threshold_thoughtful: 70,
            stewardship_threshold_emergency: 85,
        }
    }
}

impl RuntimeSettings {
    /// Parse settings from a key-value HashMap (as stored in GlobalConfig).
    pub fn from_map(map: &std::collections::HashMap<String, String>) -> Self {
        let mut s = Self::default();
        if let Some(v) = map.get("poll_interval_secs").and_then(|v| v.parse().ok()) {
            s.poll_interval_secs = v;
        }
        if let Some(v) = map.get("cache_ttl_secs").and_then(|v| v.parse().ok()) {
            s.cache_ttl_secs = v;
        }
        if let Some(v) = map.get("max_history_entries").and_then(|v| v.parse().ok()) {
            s.max_history_entries = v;
        }
        if let Some(v) = map
            .get("max_terminal_scrollback")
            .and_then(|v| v.parse().ok())
        {
            s.max_terminal_scrollback = v;
        }
        if let Some(v) = map.get("search_limit").and_then(|v| v.parse().ok()) {
            s.search_limit = v;
        }
        if let Some(v) = map.get("search_threshold").and_then(|v| v.parse().ok()) {
            s.search_threshold = v;
        }
        if let Some(v) = map.get("search_include_archived") {
            s.search_include_archived = v == "true";
        }
        if let Some(v) = map.get("inject_mode") {
            s.inject_mode = v.clone();
        }
        if let Some(v) = map.get("inject_explain") {
            s.inject_explain = v == "true";
        }
        if let Some(v) = map.get("inject_max_tokens").and_then(|v| v.parse().ok()) {
            s.inject_max_tokens = v;
        }
        if let Some(v) = map.get("inject_interval_secs").and_then(|v| v.parse().ok()) {
            s.inject_interval_secs = v;
        }
        if let Some(v) = map.get("agent_provider") {
            s.agent_provider = v.clone();
        }
        if let Some(v) = map.get("agent_model") {
            s.agent_model = v.clone();
        }
        if let Some(v) = map.get("agent_harness") {
            s.agent_harness = v.clone();
        }
        if let Some(v) = map.get("agent_max_tokens").and_then(|v| v.parse().ok()) {
            s.agent_max_tokens = v;
        }
        if let Some(v) = map.get("stewardship_mode") {
            s.stewardship_mode = v.clone();
        }
        if let Some(v) = map
            .get("stewardship_threshold_surgical")
            .and_then(|v| v.parse().ok())
        {
            s.stewardship_threshold_surgical = v;
        }
        if let Some(v) = map
            .get("stewardship_threshold_thoughtful")
            .and_then(|v| v.parse().ok())
        {
            s.stewardship_threshold_thoughtful = v;
        }
        if let Some(v) = map
            .get("stewardship_threshold_emergency")
            .and_then(|v| v.parse().ok())
        {
            s.stewardship_threshold_emergency = v;
        }
        s
    }

    /// Duration for the status polling interval.
    #[allow(dead_code)]
    pub fn status_poll(&self) -> Duration {
        Duration::from_secs(self.poll_interval_secs.max(1))
    }

    /// Duration for the context injection tick interval.
    pub fn context_tick_interval(&self) -> Duration {
        Duration::from_secs(self.inject_interval_secs.max(1))
    }
}

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
    /// Runtime settings parsed from GlobalConfig — affects polling, search, injection.
    pub runtime_settings: RuntimeSettings,
    /// Active guardrail rules (polled from daemon).
    pub guard_rules: Vec<GuardRule>,
    /// Last ping round-trip time (measures IPC latency).
    pub last_ping_rtt: Option<Duration>,
    /// Count of disconnects since GUI started.
    pub disconnect_count: u32,
    /// Recent signal log snapshot (synced from SignalBus each frame).
    pub signal_log: Vec<crate::widgets::signal_bus::SignalLogEntry>,
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
            runtime_settings: RuntimeSettings::default(),
            guard_rules: Vec::new(),
            last_ping_rtt: None,
            disconnect_count: 0,
            signal_log: Vec::new(),
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
    /// Settings changed — update runtime settings in SharedState.
    UpdateSettings(RuntimeSettings),
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
    initial_settings: RuntimeSettings,
) -> (
    StateHandle,
    std::sync::mpsc::Sender<PollerCommand>,
    std::sync::mpsc::Receiver<PollerEvent>,
    JoinHandle<()>,
) {
    let initial_state = SharedState {
        runtime_settings: initial_settings,
        ..Default::default()
    };
    let state: StateHandle = Arc::new(Mutex::new(initial_state));
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

    // Read initial settings from SharedState.
    let initial_settings = state
        .lock()
        .ok()
        .map(|s| s.runtime_settings.clone())
        .unwrap_or_default();
    let mut status_poll = Duration::from_secs(initial_settings.poll_interval_secs.max(1));
    let mut history_poll = Duration::from_secs(DEFAULT_HISTORY_POLL);
    let mut genome_poll = Duration::from_secs(DEFAULT_GENOME_POLL);
    let ops_reconcile = Duration::from_secs(DEFAULT_OPS_RECONCILE);
    let reconnect_delay = Duration::from_secs(DEFAULT_RECONNECT_DELAY);

    let mut last_status = Instant::now() - status_poll;
    let mut last_history = Instant::now() - history_poll;
    let mut last_genome = Instant::now() - genome_poll;
    let mut last_ops_reconcile = Instant::now() - ops_reconcile;
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
                last_status = Instant::now() - status_poll;
                last_history = Instant::now() - history_poll;
                last_genome = Instant::now() - genome_poll;
                last_ops_reconcile = Instant::now() - ops_reconcile;
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
                    last_history = Instant::now() - history_poll;
                    last_genome = Instant::now() - genome_poll;
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
            Ok(PollerCommand::UpdateSettings(new_settings)) => {
                status_poll = Duration::from_secs(new_settings.poll_interval_secs.max(1));
                history_poll = Duration::from_secs(DEFAULT_HISTORY_POLL);
                genome_poll = Duration::from_secs(DEFAULT_GENOME_POLL);
                if let Ok(mut shared) = state.lock() {
                    shared.runtime_settings = new_settings;
                }
                log::info!("Runtime settings updated (poll={}s)", status_poll.as_secs());
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
        if !connected || now.duration_since(last_status) >= status_poll {
            set_connection(&state, ConnectionStatus::Connecting);
            let ping_start = Instant::now();
            match client.ping() {
                Ok(true) => {
                    let rtt = ping_start.elapsed();
                    if let Ok(mut shared) = state.lock() {
                        shared.last_ping_rtt = Some(rtt);
                    }
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
                        // Check protocol version after first connect
                        check_protocol_version(&state);
                    }
                    set_connection(&state, ConnectionStatus::Connected);
                    refresh_status(&mut client, &state);
                    refresh_ops_delta(&mut client, &state);
                    if now.duration_since(last_ops_reconcile) >= ops_reconcile {
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
                        shared.last_ping_rtt = None;
                        shared.disconnect_count += 1;
                    }
                    ctx.request_repaint();
                    thread::sleep(reconnect_delay);
                    last_status = now;
                    continue;
                }
            }
        }

        if connected && memory_view_active && now.duration_since(last_history) >= history_poll {
            if let Ok(entries) = client.list_history() {
                if let Ok(mut shared) = state.lock() {
                    shared.history = entries;
                }
                ctx.request_repaint();
            }
            last_history = now;
        }

        if connected && memory_view_active && now.duration_since(last_genome) >= genome_poll {
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
    refresh_guard_rules(client, state);
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

fn check_protocol_version(state: &StateHandle) {
    if let Ok(shared) = state.lock() {
        if let Some(ref status) = shared.daemon_status {
            match status.protocol_version {
                Some(v) if v != EXPECTED_PROTOCOL_VERSION => {
                    drop(shared);
                    push_notice(
                        state,
                        TaskNoticeLevel::Warning,
                        format!(
                            "Protocol version mismatch: daemon v{}, GUI expects v{}. Consider updating.",
                            v, EXPECTED_PROTOCOL_VERSION
                        ),
                    );
                }
                None => {
                    drop(shared);
                    push_notice(
                        state,
                        TaskNoticeLevel::Info,
                        "Daemon does not report protocol version (older build).".to_string(),
                    );
                }
                _ => {}
            }
        }
    }
}

fn refresh_guard_rules(client: &mut DaemonClient, state: &StateHandle) {
    if let Ok(rules) = client.list_guard_rules() {
        if let Ok(mut shared) = state.lock() {
            shared.guard_rules = rules;
        }
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_settings_default() {
        let s = RuntimeSettings::default();
        assert_eq!(s.poll_interval_secs, DEFAULT_POLL_INTERVAL);
        assert_eq!(s.max_history_entries, DEFAULT_MAX_HISTORY_ENTRIES);
        assert_eq!(s.search_limit, DEFAULT_SEARCH_LIMIT);
        assert_eq!(s.inject_interval_secs, DEFAULT_CONTEXT_TICK_INTERVAL);
        assert!(!s.inject_explain);
        assert!(!s.search_include_archived);
    }

    #[test]
    fn test_runtime_settings_from_empty_map() {
        let map = std::collections::HashMap::new();
        let s = RuntimeSettings::from_map(&map);
        // All defaults should apply.
        assert_eq!(s.poll_interval_secs, DEFAULT_POLL_INTERVAL);
        assert_eq!(s.agent_provider, "anthropic");
    }

    #[test]
    fn test_runtime_settings_from_map_overrides() {
        let mut map = std::collections::HashMap::new();
        map.insert("poll_interval_secs".to_string(), "5".to_string());
        map.insert("search_limit".to_string(), "50".to_string());
        map.insert("inject_explain".to_string(), "true".to_string());
        map.insert("agent_provider".to_string(), "openai".to_string());
        map.insert(
            "stewardship_threshold_emergency".to_string(),
            "95".to_string(),
        );

        let s = RuntimeSettings::from_map(&map);
        assert_eq!(s.poll_interval_secs, 5);
        assert_eq!(s.search_limit, 50);
        assert!(s.inject_explain);
        assert_eq!(s.agent_provider, "openai");
        assert_eq!(s.stewardship_threshold_emergency, 95);
        // Unset values keep defaults.
        assert_eq!(s.max_history_entries, DEFAULT_MAX_HISTORY_ENTRIES);
    }

    #[test]
    fn test_runtime_settings_invalid_parse_uses_default() {
        let mut map = std::collections::HashMap::new();
        map.insert("poll_interval_secs".to_string(), "not_a_number".to_string());
        map.insert("search_limit".to_string(), "".to_string());

        let s = RuntimeSettings::from_map(&map);
        // Invalid parse falls back to default.
        assert_eq!(s.poll_interval_secs, DEFAULT_POLL_INTERVAL);
        assert_eq!(s.search_limit, DEFAULT_SEARCH_LIMIT);
    }

    #[test]
    fn test_status_poll_duration() {
        let mut s = RuntimeSettings::default();
        s.poll_interval_secs = 10;
        assert_eq!(s.status_poll(), Duration::from_secs(10));
    }

    #[test]
    fn test_status_poll_minimum_one_second() {
        let mut s = RuntimeSettings::default();
        s.poll_interval_secs = 0;
        assert_eq!(s.status_poll(), Duration::from_secs(1));
    }

    #[test]
    fn test_context_tick_interval_duration() {
        let mut s = RuntimeSettings::default();
        s.inject_interval_secs = 15;
        assert_eq!(s.context_tick_interval(), Duration::from_secs(15));
    }

    #[test]
    fn test_context_tick_minimum_one_second() {
        let mut s = RuntimeSettings::default();
        s.inject_interval_secs = 0;
        assert_eq!(s.context_tick_interval(), Duration::from_secs(1));
    }

    #[test]
    fn test_shared_state_includes_settings() {
        let shared = SharedState::default();
        assert_eq!(
            shared.runtime_settings.poll_interval_secs,
            DEFAULT_POLL_INTERVAL
        );
    }

    #[test]
    fn test_runtime_settings_toggle_fields() {
        let mut map = std::collections::HashMap::new();
        map.insert("search_include_archived".to_string(), "true".to_string());
        map.insert("inject_explain".to_string(), "false".to_string());

        let s = RuntimeSettings::from_map(&map);
        assert!(s.search_include_archived);
        assert!(!s.inject_explain);
    }

    #[test]
    fn test_runtime_settings_all_int_fields() {
        let mut map = std::collections::HashMap::new();
        map.insert("cache_ttl_secs".to_string(), "120".to_string());
        map.insert("max_terminal_scrollback".to_string(), "25000".to_string());
        map.insert("search_threshold".to_string(), "75".to_string());
        map.insert("inject_max_tokens".to_string(), "3000".to_string());
        map.insert("agent_max_tokens".to_string(), "6000".to_string());
        map.insert(
            "stewardship_threshold_surgical".to_string(),
            "40".to_string(),
        );
        map.insert(
            "stewardship_threshold_thoughtful".to_string(),
            "65".to_string(),
        );

        let s = RuntimeSettings::from_map(&map);
        assert_eq!(s.cache_ttl_secs, 120);
        assert_eq!(s.max_terminal_scrollback, 25000);
        assert_eq!(s.search_threshold, 75);
        assert_eq!(s.inject_max_tokens, 3000);
        assert_eq!(s.agent_max_tokens, 6000);
        assert_eq!(s.stewardship_threshold_surgical, 40);
        assert_eq!(s.stewardship_threshold_thoughtful, 65);
    }

    #[test]
    fn test_runtime_settings_all_string_fields() {
        let mut map = std::collections::HashMap::new();
        map.insert("inject_mode".to_string(), "auto".to_string());
        map.insert("agent_model".to_string(), "gpt-4o".to_string());
        map.insert("agent_harness".to_string(), "opencode".to_string());
        map.insert("stewardship_mode".to_string(), "off".to_string());

        let s = RuntimeSettings::from_map(&map);
        assert_eq!(s.inject_mode, "auto");
        assert_eq!(s.agent_model, "gpt-4o");
        assert_eq!(s.agent_harness, "opencode");
        assert_eq!(s.stewardship_mode, "off");
    }
}
