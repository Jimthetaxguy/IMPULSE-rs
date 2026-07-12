//! Desktop-to-daemon ops telemetry bridge.
//!
//! The desktop runtime owns PTY mechanics. The daemon owns reconciled project
//! truth. This module is the narrow wire between those ownership domains:
//! runtime lifecycle events update a latest-state cache, one background worker
//! publishes that cache as [`impulse_ops::TerminalOpsReport`], and only the
//! daemon's subscribed snapshot returns to Dioxus as `ops_update`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use impulse_ops::{AgentRuntime, ContextHealthSummary, MachineTarget, TerminalOpsReport};

use crate::runtime::{AgentRuntimeSnapshot, DesktopEvent, DesktopEventSink};

/// Desktop heartbeats stay well below the daemon's ten-second stale boundary.
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct DesktopDaemonOpsConfig {
    pub socket_path: PathBuf,
    /// This adapter intentionally binds one daemon/project. Agents from other
    /// registered workspaces remain visible to the local PTY runtime but are
    /// not mixed into this daemon's project snapshot.
    pub project_root: Option<PathBuf>,
    /// Base used by the PTY backend when a spawn request supplies a relative
    /// cwd. This is the desktop process cwd, not necessarily `project_root`
    /// when `.impulse` was discovered in an ancestor.
    pub relative_cwd_base: PathBuf,
    pub source_id: String,
    pub heartbeat_interval: Duration,
}

impl DesktopDaemonOpsConfig {
    pub fn new(socket_path: PathBuf, project_root: Option<PathBuf>) -> Self {
        let relative_cwd_base = std::env::current_dir()
            .ok()
            .or_else(|| project_root.clone())
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            socket_path,
            project_root,
            relative_cwd_base,
            source_id: format!("desktop-{}", uuid::Uuid::new_v4()),
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
        }
    }

    /// Resolve the daemon socket without conflating the desktop's global
    /// memory fallback with the daemon's project-local default.
    ///
    /// Precedence:
    /// 1. explicit `IMPULSE_SOCKET_PATH`;
    /// 2. an existing socket discovered from cwd upward;
    /// 3. an existing project-local `.impulse` directory from cwd upward;
    /// 4. an existing socket below `memory_root`;
    /// 5. the cwd-local daemon default (so a daemon started later is found);
    /// 6. the provided memory root.
    pub fn discover(memory_root: &Path) -> Self {
        if let Ok(path) = std::env::var("IMPULSE_SOCKET_PATH") {
            if !path.trim().is_empty() {
                let socket_path = PathBuf::from(path);
                let project_root =
                    project_root_from_socket(&socket_path).or_else(|| std::env::current_dir().ok());
                return Self::new(socket_path, project_root);
            }
        }

        let cwd = std::env::current_dir().ok();
        if let Some(root) = cwd.as_deref() {
            if let Some((socket, project)) = find_project_socket(root, true) {
                return Self::new(socket, Some(project));
            }
            if let Some((socket, project)) = find_project_socket(root, false) {
                return Self::new(socket, Some(project));
            }
        }

        let memory_socket = memory_root.join("sockets").join("impulse.sock");
        if memory_socket.exists() {
            return Self::new(memory_socket, memory_root.parent().map(Path::to_path_buf));
        }

        if let Some(root) = cwd {
            return Self::new(
                root.join(".impulse").join("sockets").join("impulse.sock"),
                Some(root),
            );
        }

        Self::new(memory_socket, memory_root.parent().map(Path::to_path_buf))
    }

    pub fn with_source_id(mut self, source_id: impl Into<String>) -> Self {
        self.source_id = source_id.into();
        self
    }

    pub fn with_heartbeat_interval(mut self, heartbeat_interval: Duration) -> Self {
        if !heartbeat_interval.is_zero() {
            self.heartbeat_interval = heartbeat_interval;
        }
        self
    }

    pub fn with_relative_cwd_base(mut self, base: PathBuf) -> Self {
        self.relative_cwd_base = base;
        self
    }
}

fn find_project_socket(start: &Path, require_socket: bool) -> Option<(PathBuf, PathBuf)> {
    for project_root in start.ancestors() {
        let impulse_root = project_root.join(".impulse");
        let socket = impulse_root.join("sockets").join("impulse.sock");
        let found = if require_socket {
            socket.exists()
        } else {
            impulse_root.is_dir()
        };
        if found {
            return Some((socket, project_root.to_path_buf()));
        }
    }
    None
}

fn project_root_from_socket(socket_path: &Path) -> Option<PathBuf> {
    let sockets = socket_path.parent()?;
    let impulse_root = sockets.parent()?;
    if sockets.file_name().and_then(|name| name.to_str()) == Some("sockets")
        && impulse_root.file_name().and_then(|name| name.to_str()) == Some(".impulse")
    {
        impulse_root.parent().map(Path::to_path_buf)
    } else {
        None
    }
}

/// Complete conversion from desktop PTY facts to the daemon's telemetry DTO.
pub fn agent_runtime_from_snapshot(snapshot: &AgentRuntimeSnapshot) -> AgentRuntime {
    let target_workdir = snapshot.target.as_ref().map(|target| match target {
        MachineTarget::Local { workdir } | MachineTarget::Remote { workdir, .. } => workdir.clone(),
    });
    let working_directory = snapshot
        .cwd
        .clone()
        .or_else(|| {
            snapshot
                .workspace
                .as_ref()
                .map(|workspace| workspace.root.clone())
        })
        .or(target_workdir)
        .unwrap_or_default();
    let current_task = snapshot.current_task.clone().or_else(|| {
        if let impulse_ops::AgentStatus::Working { task } = &snapshot.status {
            (!task.is_empty()).then(|| task.clone())
        } else {
            None
        }
    });
    let group = snapshot.workspace.as_ref().map(|workspace| {
        workspace
            .label
            .clone()
            .unwrap_or_else(|| workspace.root.clone())
    });

    AgentRuntime {
        id: snapshot.agent_id.clone(),
        label: snapshot.label.clone(),
        backend_kind: snapshot.platform.as_str().to_string(),
        session_id: snapshot.session_id.clone(),
        ephemeral: snapshot.session_id.is_none(),
        working_directory,
        status: snapshot.status.to_legacy_string(),
        current_task,
        active: snapshot.alive,
        context: snapshot.context.clone(),
        recent_files: Vec::new(),
        recent_tools: snapshot
            .mcp_tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect(),
        warnings: Vec::new(),
        agent_status: snapshot.status.clone(),
        role: snapshot.role.clone(),
        group,
        tool_invocations: Vec::new(),
        diff_summary: None,
        target: snapshot.target.clone(),
    }
}

#[derive(Debug)]
struct RuntimeTelemetryState {
    source_id: String,
    project_root: Option<PathBuf>,
    relative_cwd_base: PathBuf,
    agents: BTreeMap<String, AgentRuntimeSnapshot>,
}

impl RuntimeTelemetryState {
    fn new(source_id: String, project_root: Option<PathBuf>, relative_cwd_base: PathBuf) -> Self {
        Self {
            source_id,
            project_root,
            relative_cwd_base,
            agents: BTreeMap::new(),
        }
    }

    /// Apply only lifecycle facts. Terminal bytes never enter this cache.
    fn apply_event(&mut self, event: &DesktopEvent) -> bool {
        match event {
            DesktopEvent::AgentRuntimeUpdate { snapshot } => {
                if snapshot.alive {
                    let focus_changed = if snapshot.focused {
                        let mut changed = false;
                        for (agent_id, cached) in &mut self.agents {
                            if agent_id != &snapshot.agent_id && cached.focused {
                                cached.focused = false;
                                changed = true;
                            }
                        }
                        changed
                    } else {
                        false
                    };
                    let snapshot_changed = self
                        .agents
                        .insert(snapshot.agent_id.clone(), snapshot.as_ref().clone())
                        .as_ref()
                        != Some(snapshot.as_ref());
                    focus_changed || snapshot_changed
                } else {
                    self.agents.remove(&snapshot.agent_id).is_some()
                }
            }
            DesktopEvent::TerminalExit { agent_id } => self.agents.remove(agent_id).is_some(),
            _ => false,
        }
    }

    fn report(&self) -> TerminalOpsReport {
        let included = self
            .agents
            .values()
            .filter(|snapshot| self.belongs_to_bound_project(snapshot))
            .collect::<Vec<_>>();
        let context = included
            .iter()
            .copied()
            .find(|snapshot| snapshot.focused)
            .or_else(|| {
                included.iter().copied().max_by(|left, right| {
                    left.context
                        .usage_fraction
                        .total_cmp(&right.context.usage_fraction)
                })
            })
            .map(|snapshot| snapshot.context.clone())
            .unwrap_or_default();

        TerminalOpsReport {
            source_id: self.source_id.clone(),
            published_at: impulse_ops::now_rfc3339(),
            agents: included
                .into_iter()
                .map(agent_runtime_from_snapshot)
                .collect(),
            context,
            interventions: Vec::new(),
        }
    }

    fn empty_report(&self) -> TerminalOpsReport {
        TerminalOpsReport {
            source_id: self.source_id.clone(),
            published_at: impulse_ops::now_rfc3339(),
            agents: Vec::new(),
            context: ContextHealthSummary::default(),
            interventions: Vec::new(),
        }
    }

    fn belongs_to_bound_project(&self, snapshot: &AgentRuntimeSnapshot) -> bool {
        let Some(project_root) = self.project_root.as_deref() else {
            return true;
        };
        let candidate = snapshot.cwd.as_deref().or_else(|| {
            snapshot
                .workspace
                .as_ref()
                .map(|workspace| workspace.root.as_str())
        });
        let Some(candidate) = candidate else {
            // A PTY without an explicit cwd inherits the desktop process cwd.
            // An explicit daemon socket may bind a different project, so the
            // inherited directory still has to pass the project boundary.
            return path_is_within(
                project_root,
                &self.relative_cwd_base,
                &self.relative_cwd_base,
            );
        };
        path_is_within(project_root, &self.relative_cwd_base, Path::new(candidate))
    }
}

fn path_is_within(project_root: &Path, relative_cwd_base: &Path, candidate: &Path) -> bool {
    let root = project_root
        .canonicalize()
        .unwrap_or_else(|_| normalize_lexically(project_root));
    let candidate = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        relative_cwd_base.join(candidate)
    };
    let candidate = candidate
        .canonicalize()
        .unwrap_or_else(|_| normalize_lexically(&candidate));
    candidate.starts_with(root)
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn lock_state(state: &Mutex<RuntimeTelemetryState>) -> MutexGuard<'_, RuntimeTelemetryState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, thiserror::Error)]
pub enum DesktopDaemonOpsStartError {
    #[error("desktop daemon ops requires Unix-domain sockets")]
    UnsupportedPlatform,
    #[error("failed to start desktop daemon-ops worker: {0}")]
    WorkerStart(#[source] std::io::Error),
}

#[cfg(unix)]
mod unix {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
    use std::thread::{self, JoinHandle};

    use impulse_ops::{OpsSubscription, WorkbenchDaemonRequest, WorkbenchDaemonResponse};

    use super::*;

    const IO_TIMEOUT: Duration = Duration::from_secs(2);

    trait DaemonOpsTransport: Send {
        fn publish(&mut self, report: &TerminalOpsReport) -> Result<(), String>;
        fn subscribe(&mut self, since_seq: Option<u64>) -> Result<OpsSubscription, String>;
    }

    #[derive(Debug)]
    struct UnixDaemonOpsClient {
        socket_path: PathBuf,
    }

    impl UnixDaemonOpsClient {
        fn new(socket_path: PathBuf) -> Self {
            Self { socket_path }
        }

        fn send(
            &self,
            request: &WorkbenchDaemonRequest,
        ) -> Result<WorkbenchDaemonResponse, String> {
            let mut stream = UnixStream::connect(&self.socket_path).map_err(|error| {
                format!(
                    "connect to daemon at {}: {error}",
                    self.socket_path.display()
                )
            })?;
            stream
                .set_read_timeout(Some(IO_TIMEOUT))
                .map_err(|error| format!("set daemon read timeout: {error}"))?;
            stream
                .set_write_timeout(Some(IO_TIMEOUT))
                .map_err(|error| format!("set daemon write timeout: {error}"))?;

            let encoded = serde_json::to_vec(request)
                .map_err(|error| format!("serialize daemon request: {error}"))?;
            stream
                .write_all(&encoded)
                .and_then(|_| stream.write_all(b"\n"))
                .and_then(|_| stream.flush())
                .map_err(|error| format!("write daemon request: {error}"))?;

            let mut line = String::new();
            let bytes = BufReader::new(stream)
                .read_line(&mut line)
                .map_err(|error| format!("read daemon response: {error}"))?;
            if bytes == 0 {
                return Err("daemon closed connection before responding".to_string());
            }
            serde_json::from_str(&line).map_err(|error| format!("parse daemon response: {error}"))
        }

        fn ok_result(response: WorkbenchDaemonResponse) -> Result<serde_json::Value, String> {
            match response {
                WorkbenchDaemonResponse::Ok { result } => Ok(result),
                WorkbenchDaemonResponse::Error { message } => Err(message),
                WorkbenchDaemonResponse::Busy {
                    resource,
                    retry_after_ms,
                } => {
                    let resource = match resource {
                        impulse_ops::DaemonBusyResource::AgentTurn => "agent_turn",
                    };
                    Err(format!(
                        "daemon busy: resource={resource}, retry_after_ms={retry_after_ms}"
                    ))
                }
                WorkbenchDaemonResponse::ConflictCheck { .. } => {
                    Err("unexpected conflict-check response from ops request".to_string())
                }
            }
        }
    }

    impl DaemonOpsTransport for UnixDaemonOpsClient {
        fn publish(&mut self, report: &TerminalOpsReport) -> Result<(), String> {
            let response = self.send(&WorkbenchDaemonRequest::PublishTerminalOps {
                report: report.clone(),
            })?;
            Self::ok_result(response).map(|_| ())
        }

        fn subscribe(&mut self, since_seq: Option<u64>) -> Result<OpsSubscription, String> {
            let response = self.send(&WorkbenchDaemonRequest::SubscribeOps { since_seq })?;
            let result = Self::ok_result(response)?;
            serde_json::from_value(result)
                .map_err(|error| format!("parse ops subscription: {error}"))
        }
    }

    struct SubscriptionCursor {
        next_seq: Option<u64>,
        last_emitted_seq: Option<u64>,
    }

    impl SubscriptionCursor {
        fn new() -> Self {
            Self {
                next_seq: None,
                last_emitted_seq: None,
            }
        }

        fn accept(
            &mut self,
            subscription: OpsSubscription,
            downstream: &Arc<dyn DesktopEventSink>,
        ) -> Result<(), String> {
            let next_seq = subscription.next_seq;
            self.next_seq = Some(next_seq);
            if self.last_emitted_seq == Some(next_seq) {
                return Ok(());
            }
            let payload = serde_json::to_value(subscription.snapshot)
                .map_err(|error| format!("serialize subscribed ops snapshot: {error}"))?;
            downstream.emit(DesktopEvent::OpsUpdate { payload });
            self.last_emitted_seq = Some(next_seq);
            Ok(())
        }
    }

    pub(super) struct DesktopDaemonOpsSink {
        downstream: Arc<dyn DesktopEventSink>,
        telemetry: Arc<Mutex<RuntimeTelemetryState>>,
        wake_tx: SyncSender<()>,
        stop: Arc<AtomicBool>,
        worker: Mutex<Option<JoinHandle<()>>>,
    }

    impl DesktopEventSink for DesktopDaemonOpsSink {
        fn emit(&self, event: DesktopEvent) {
            // Runtime/UI responsiveness never waits on daemon I/O.
            self.downstream.emit(event.clone());

            let is_lifecycle = matches!(
                event,
                DesktopEvent::AgentRuntimeUpdate { .. } | DesktopEvent::TerminalExit { .. }
            );
            if !is_lifecycle {
                return;
            }
            let changed = lock_state(&self.telemetry).apply_event(&event);
            if changed {
                match self.wake_tx.try_send(()) {
                    Ok(()) | Err(TrySendError::Full(())) => {}
                    Err(TrySendError::Disconnected(())) => {
                        eprintln!("desktop daemon-ops worker is unavailable");
                    }
                }
            }
        }
    }

    impl Drop for DesktopDaemonOpsSink {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            let _ = self.wake_tx.try_send(());
            let handle = self
                .worker
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            if let Some(handle) = handle {
                if handle.thread().id() != thread::current().id() {
                    let _ = handle.join();
                }
            }
        }
    }

    pub(super) fn attach(
        downstream: Arc<dyn DesktopEventSink>,
        config: DesktopDaemonOpsConfig,
    ) -> Result<Arc<dyn DesktopEventSink>, DesktopDaemonOpsStartError> {
        let telemetry = Arc::new(Mutex::new(RuntimeTelemetryState::new(
            config.source_id,
            config.project_root,
            config.relative_cwd_base,
        )));
        let stop = Arc::new(AtomicBool::new(false));
        let (wake_tx, wake_rx) = sync_channel(1);
        let worker_telemetry = Arc::clone(&telemetry);
        let worker_stop = Arc::clone(&stop);
        let worker_downstream = Arc::clone(&downstream);
        let heartbeat_interval = config.heartbeat_interval;
        let mut client = UnixDaemonOpsClient::new(config.socket_path);
        let worker = thread::Builder::new()
            .name("impulse-desktop-daemon-ops".to_string())
            .spawn(move || {
                run_worker(
                    &mut client,
                    worker_telemetry,
                    worker_downstream,
                    wake_rx,
                    worker_stop,
                    heartbeat_interval,
                )
            })
            .map_err(DesktopDaemonOpsStartError::WorkerStart)?;

        Ok(Arc::new(DesktopDaemonOpsSink {
            downstream,
            telemetry,
            wake_tx,
            stop,
            worker: Mutex::new(Some(worker)),
        }))
    }

    fn run_worker<T: DaemonOpsTransport>(
        client: &mut T,
        telemetry: Arc<Mutex<RuntimeTelemetryState>>,
        downstream: Arc<dyn DesktopEventSink>,
        wake_rx: Receiver<()>,
        stop: Arc<AtomicBool>,
        heartbeat_interval: Duration,
    ) {
        let mut cursor = SubscriptionCursor::new();
        let mut last_error = None;
        let mut last_connected = None;
        let initial_errors = subscribe_and_emit(client, &mut cursor, &downstream);
        let initial_connected = initial_errors.is_empty();
        log_connection_transition(
            &mut last_error,
            &mut last_connected,
            initial_connected,
            initial_errors,
            &downstream,
        );

        loop {
            let wake = wake_rx.recv_timeout(heartbeat_interval);
            if stop.load(Ordering::Acquire) || matches!(wake, Err(RecvTimeoutError::Disconnected)) {
                let empty = lock_state(&telemetry).empty_report();
                if let Err(error) = client.publish(&empty) {
                    eprintln!("desktop daemon-ops shutdown publish failed: {error}");
                }
                break;
            }

            let report = lock_state(&telemetry).report();
            let mut errors = Vec::new();
            if let Err(error) = client.publish(&report) {
                errors.push(format!("publish: {error}"));
            }
            if stop.load(Ordering::Acquire) {
                continue;
            }
            let subscribe_errors = subscribe_and_emit(client, &mut cursor, &downstream);
            let connected = subscribe_errors.is_empty();
            errors.extend(subscribe_errors);
            log_connection_transition(
                &mut last_error,
                &mut last_connected,
                connected,
                errors,
                &downstream,
            );
        }
    }

    fn subscribe_and_emit<T: DaemonOpsTransport>(
        client: &mut T,
        cursor: &mut SubscriptionCursor,
        downstream: &Arc<dyn DesktopEventSink>,
    ) -> Vec<String> {
        match client.subscribe(cursor.next_seq) {
            Ok(subscription) => cursor
                .accept(subscription, downstream)
                .err()
                .into_iter()
                .collect(),
            Err(error) => vec![format!("subscribe: {error}")],
        }
    }

    fn log_connection_transition(
        last_error: &mut Option<String>,
        last_connected: &mut Option<bool>,
        connected: bool,
        errors: Vec<String>,
        downstream: &Arc<dyn DesktopEventSink>,
    ) {
        let error = (!errors.is_empty()).then(|| errors.join("; "));
        let error_changed = *last_error != error;
        let connection_changed = *last_connected != Some(connected);
        if error_changed {
            if let Some(error) = error.as_deref() {
                eprintln!("desktop daemon-ops degraded: {error}");
            } else if last_error.is_some() {
                eprintln!("desktop daemon-ops connection recovered");
            }
        }
        if connection_changed || error_changed {
            downstream.emit(DesktopEvent::OpsConnectionUpdate {
                connected,
                error: error.clone(),
            });
        }
        *last_error = error;
        *last_connected = Some(connected);
    }

    #[cfg(test)]
    mod tests {
        use std::collections::VecDeque;
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;
        use std::sync::mpsc;
        use std::time::Instant;

        use impulse_ops::{OpsSubscription, ProjectOpsSnapshot};

        use super::*;
        use crate::runtime::{AgentPlatformId, BuiltInMcpTool, WorkspaceTarget};

        #[test]
        fn busy_response_preserves_resource_and_retry_guidance() {
            let error = UnixDaemonOpsClient::ok_result(WorkbenchDaemonResponse::Busy {
                resource: impulse_ops::DaemonBusyResource::AgentTurn,
                retry_after_ms: 250,
            })
            .expect_err("busy responses must remain retryable errors");

            assert_eq!(
                error,
                "daemon busy: resource=agent_turn, retry_after_ms=250"
            );
        }

        #[derive(Default)]
        struct RecordingSink {
            events: Mutex<Vec<DesktopEvent>>,
        }

        impl RecordingSink {
            fn events(&self) -> Vec<DesktopEvent> {
                self.events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
            }

            fn wait_for_ops_agent(&self, agent_id: &str) -> bool {
                let deadline = Instant::now() + Duration::from_secs(3);
                while Instant::now() < deadline {
                    if self.events().iter().any(|event| {
                        let DesktopEvent::OpsUpdate { payload } = event else {
                            return false;
                        };
                        payload
                            .get("agents")
                            .and_then(serde_json::Value::as_array)
                            .is_some_and(|agents| {
                                agents.iter().any(|agent| agent["id"] == agent_id)
                            })
                    }) {
                        return true;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                false
            }

            fn wait_for_connection(&self, connected: bool) -> bool {
                let deadline = Instant::now() + Duration::from_secs(3);
                while Instant::now() < deadline {
                    if self.events().iter().any(|event| {
                        matches!(
                            event,
                            DesktopEvent::OpsConnectionUpdate { connected: value, .. }
                                if *value == connected
                        )
                    }) {
                        return true;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                false
            }

            fn wait_for_connection_health_sequence(&self, expected: &[(bool, bool)]) -> bool {
                let deadline = Instant::now() + Duration::from_secs(3);
                while Instant::now() < deadline {
                    let states = self
                        .events()
                        .iter()
                        .filter_map(|event| {
                            if let DesktopEvent::OpsConnectionUpdate {
                                connected, error, ..
                            } = event
                            {
                                Some((*connected, error.is_some()))
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>();
                    if states
                        .windows(expected.len())
                        .any(|window| window == expected)
                    {
                        return true;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                false
            }
        }

        impl DesktopEventSink for RecordingSink {
            fn emit(&self, event: DesktopEvent) {
                self.events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(event);
            }
        }

        struct ScriptedTransport {
            publish_results: VecDeque<Result<(), String>>,
            subscribe_results: VecDeque<Result<OpsSubscription, String>>,
            report_tx: mpsc::Sender<TerminalOpsReport>,
            since_tx: mpsc::Sender<Option<u64>>,
        }

        impl DaemonOpsTransport for ScriptedTransport {
            fn publish(&mut self, report: &TerminalOpsReport) -> Result<(), String> {
                self.report_tx
                    .send(report.clone())
                    .expect("record published report");
                self.publish_results.pop_front().unwrap_or(Ok(()))
            }

            fn subscribe(&mut self, since_seq: Option<u64>) -> Result<OpsSubscription, String> {
                self.since_tx
                    .send(since_seq)
                    .expect("record subscription cursor");
                self.subscribe_results
                    .pop_front()
                    .unwrap_or_else(|| Err("unexpected subscription".to_string()))
            }
        }

        fn subscription(next_seq: u64, agents: Vec<AgentRuntime>) -> OpsSubscription {
            OpsSubscription {
                snapshot: ProjectOpsSnapshot {
                    generated_at: format!("subscription-{next_seq}"),
                    agents,
                    ..Default::default()
                },
                events: Vec::new(),
                next_seq,
            }
        }

        fn snapshot(agent_id: &str) -> AgentRuntimeSnapshot {
            AgentRuntimeSnapshot {
                agent_id: agent_id.to_string(),
                label: "Codex".to_string(),
                platform: AgentPlatformId::try_new("codex").unwrap(),
                command: "codex".to_string(),
                args: vec!["exec".to_string()],
                cwd: Some("/tmp/project".to_string()),
                workspace: Some(WorkspaceTarget {
                    root: "/tmp/project".to_string(),
                    label: Some("project".to_string()),
                    purpose: None,
                    project_notes: None,
                }),
                session_id: Some(format!("{agent_id}-session")),
                rows: 24,
                cols: 80,
                alive: true,
                focused: true,
                status: impulse_ops::AgentStatus::Working {
                    task: "wire daemon truth".to_string(),
                },
                current_task: None,
                role: Some(impulse_ops::AgentRole::Coordinator),
                target: Some(MachineTarget::Local {
                    workdir: "/tmp/project".to_string(),
                }),
                mcp_tools: vec![BuiltInMcpTool::new(
                    "impulse.search_memory",
                    "search",
                    vec!["read_only".to_string()],
                    false,
                )],
                output_bytes: 10,
                output_lines: 1,
                context: ContextHealthSummary {
                    tier: "essential".to_string(),
                    usage_fraction: 0.4,
                    estimated_tokens: 40,
                    window_tokens: 100,
                    ..Default::default()
                },
            }
        }

        fn write_response(
            stream: &mut std::os::unix::net::UnixStream,
            value: &impl serde::Serialize,
        ) {
            let mut encoded = serde_json::to_vec(value).expect("serialize response");
            encoded.push(b'\n');
            stream.write_all(&encoded).expect("write response");
        }

        #[test]
        fn unix_transport_publish_subscribe_and_emit_snapshot() {
            let dir = tempfile::tempdir().expect("tempdir");
            let socket = dir.path().join("daemon.sock");
            let listener = UnixListener::bind(&socket).expect("bind fake daemon");
            let (requests_tx, requests_rx) = mpsc::channel();
            let server = thread::spawn(move || {
                let mut latest_agents = Vec::new();
                let mut subscription_count = 0_u64;
                for _ in 0..3 {
                    let (mut stream, _) = listener.accept().expect("accept request");
                    let mut line = String::new();
                    BufReader::new(stream.try_clone().expect("clone stream"))
                        .read_line(&mut line)
                        .expect("read request");
                    assert!(line.ends_with('\n'), "request must be JSONL framed");
                    let request: WorkbenchDaemonRequest =
                        serde_json::from_str(&line).expect("parse request");
                    requests_tx.send(request.clone()).expect("record request");
                    match request {
                        WorkbenchDaemonRequest::PublishTerminalOps { report } => {
                            latest_agents = report.agents;
                            write_response(
                                &mut stream,
                                &WorkbenchDaemonResponse::Ok {
                                    result: serde_json::json!({"accepted": true}),
                                },
                            );
                        }
                        WorkbenchDaemonRequest::SubscribeOps { .. } => {
                            subscription_count += 1;
                            let subscription = OpsSubscription {
                                snapshot: ProjectOpsSnapshot {
                                    generated_at: format!("subscription-{subscription_count}"),
                                    agents: latest_agents.clone(),
                                    ..Default::default()
                                },
                                events: Vec::new(),
                                next_seq: subscription_count * 11,
                            };
                            write_response(
                                &mut stream,
                                &WorkbenchDaemonResponse::Ok {
                                    result: serde_json::to_value(subscription)
                                        .expect("serialize subscription"),
                                },
                            );
                        }
                        other => panic!("unexpected request: {other:?}"),
                    }
                }
            });

            let downstream = Arc::new(RecordingSink::default());
            let downstream_trait: Arc<dyn DesktopEventSink> = downstream.clone();
            let sink = attach(
                downstream_trait,
                DesktopDaemonOpsConfig::new(socket, Some(PathBuf::from("/tmp/project")))
                    .with_source_id("desktop-contract")
                    .with_heartbeat_interval(Duration::from_secs(60)),
            )
            .expect("attach daemon ops");
            sink.emit(DesktopEvent::AgentRuntimeUpdate {
                snapshot: Box::new(snapshot("codex-live")),
            });

            assert!(downstream.wait_for_ops_agent("codex-live"));
            assert!(downstream.wait_for_connection(true));
            let requests = (0..3)
                .map(|_| {
                    requests_rx
                        .recv_timeout(Duration::from_secs(3))
                        .expect("request")
                })
                .collect::<Vec<_>>();
            assert!(matches!(
                requests[0],
                WorkbenchDaemonRequest::SubscribeOps { since_seq: None }
            ));
            assert!(matches!(
                requests[1],
                WorkbenchDaemonRequest::PublishTerminalOps { .. }
            ));
            assert!(matches!(
                requests[2],
                WorkbenchDaemonRequest::SubscribeOps {
                    since_seq: Some(11)
                }
            ));
            server.join().expect("server thread");
            drop(sink);
        }

        #[test]
        fn unix_transport_recovers_on_the_next_one_shot_request() {
            let dir = tempfile::tempdir().expect("tempdir");
            let socket = dir.path().join("daemon.sock");
            let mut client = UnixDaemonOpsClient::new(socket.clone());
            assert!(client.subscribe(None).is_err());

            let listener = UnixListener::bind(&socket).expect("bind fake daemon");
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("accept request");
                let mut line = String::new();
                BufReader::new(stream.try_clone().expect("clone stream"))
                    .read_line(&mut line)
                    .expect("read request");
                assert!(matches!(
                    serde_json::from_str::<WorkbenchDaemonRequest>(&line).expect("parse request"),
                    WorkbenchDaemonRequest::SubscribeOps { since_seq: None }
                ));
                write_response(
                    &mut stream,
                    &WorkbenchDaemonResponse::Ok {
                        result: serde_json::to_value(OpsSubscription {
                            snapshot: ProjectOpsSnapshot::default(),
                            events: Vec::new(),
                            next_seq: 7,
                        })
                        .expect("subscription value"),
                    },
                );
            });

            assert_eq!(client.subscribe(None).expect("recovered").next_seq, 7);
            server.join().expect("server thread");
        }

        #[test]
        fn shutdown_publishes_empty_same_source_report() {
            let dir = tempfile::tempdir().expect("tempdir");
            let socket = dir.path().join("daemon.sock");
            let listener = UnixListener::bind(&socket).expect("bind fake daemon");
            let (report_tx, report_rx) = mpsc::channel();
            let server = thread::spawn(move || {
                for index in 0..2 {
                    let (mut stream, _) = listener.accept().expect("accept request");
                    let mut line = String::new();
                    BufReader::new(stream.try_clone().expect("clone stream"))
                        .read_line(&mut line)
                        .expect("read request");
                    let request: WorkbenchDaemonRequest =
                        serde_json::from_str(&line).expect("parse request");
                    if index == 0 {
                        assert!(matches!(
                            request,
                            WorkbenchDaemonRequest::SubscribeOps { since_seq: None }
                        ));
                        write_response(
                            &mut stream,
                            &WorkbenchDaemonResponse::Ok {
                                result: serde_json::to_value(OpsSubscription {
                                    snapshot: ProjectOpsSnapshot::default(),
                                    events: Vec::new(),
                                    next_seq: 1,
                                })
                                .expect("subscription"),
                            },
                        );
                    } else if let WorkbenchDaemonRequest::PublishTerminalOps { report } = request {
                        report_tx.send(report).expect("record shutdown report");
                        write_response(
                            &mut stream,
                            &WorkbenchDaemonResponse::Ok {
                                result: serde_json::json!({"accepted": true}),
                            },
                        );
                    } else {
                        panic!("expected shutdown publish");
                    }
                }
            });

            let downstream: Arc<dyn DesktopEventSink> = Arc::new(RecordingSink::default());
            let sink = attach(
                downstream,
                DesktopDaemonOpsConfig::new(socket, None)
                    .with_source_id("desktop-shutdown")
                    .with_heartbeat_interval(Duration::from_secs(60)),
            )
            .expect("attach daemon ops");
            // Give the startup subscription a deterministic completion point.
            thread::sleep(Duration::from_millis(20));
            drop(sink);

            let report = report_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("shutdown report");
            assert_eq!(report.source_id, "desktop-shutdown");
            assert!(report.agents.is_empty());
            server.join().expect("server thread");
        }

        #[test]
        fn missing_daemon_never_blocks_downstream_terminal_events() {
            let dir = tempfile::tempdir().expect("tempdir");
            let downstream = Arc::new(RecordingSink::default());
            let downstream_trait: Arc<dyn DesktopEventSink> = downstream.clone();
            let sink = attach(
                downstream_trait,
                DesktopDaemonOpsConfig::new(dir.path().join("missing.sock"), None)
                    .with_heartbeat_interval(Duration::from_secs(60)),
            )
            .expect("worker still starts without daemon");
            sink.emit(DesktopEvent::TerminalOutput {
                agent_id: "agent".to_string(),
                data: b"live".to_vec(),
            });
            assert!(downstream.events().iter().any(|event| matches!(
                event,
                DesktopEvent::TerminalOutput { data, .. } if data == b"live"
            )));
            assert!(downstream.wait_for_connection(false));
            drop(sink);
        }

        #[test]
        fn worker_coalesces_full_wake_channel_around_latest_shared_state() {
            let telemetry = Arc::new(Mutex::new(RuntimeTelemetryState::new(
                "desktop-coalesce".to_string(),
                None,
                PathBuf::from("/tmp/project"),
            )));
            let mut first = snapshot("agent");
            first.current_task = Some("first".to_string());
            let mut latest = first.clone();
            latest.current_task = Some("latest".to_string());
            lock_state(&telemetry).apply_event(&DesktopEvent::AgentRuntimeUpdate {
                snapshot: Box::new(first),
            });

            let (wake_tx, wake_rx) = sync_channel(1);
            wake_tx.try_send(()).expect("first wake");
            lock_state(&telemetry).apply_event(&DesktopEvent::AgentRuntimeUpdate {
                snapshot: Box::new(latest),
            });
            assert!(matches!(wake_tx.try_send(()), Err(TrySendError::Full(()))));

            let (report_tx, report_rx) = mpsc::channel();
            let (since_tx, since_rx) = mpsc::channel();
            let mut transport = ScriptedTransport {
                publish_results: VecDeque::new(),
                subscribe_results: VecDeque::from([
                    Ok(subscription(1, Vec::new())),
                    Ok(subscription(2, Vec::new())),
                ]),
                report_tx,
                since_tx,
            };
            let downstream: Arc<dyn DesktopEventSink> = Arc::new(RecordingSink::default());
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = Arc::clone(&stop);
            let worker = thread::spawn(move || {
                run_worker(
                    &mut transport,
                    telemetry,
                    downstream,
                    wake_rx,
                    worker_stop,
                    Duration::from_secs(60),
                )
            });

            let report = report_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("coalesced report");
            assert_eq!(report.agents.len(), 1);
            assert_eq!(report.agents[0].current_task.as_deref(), Some("latest"));
            assert_eq!(since_rx.recv().expect("initial cursor"), None);
            assert_eq!(since_rx.recv().expect("cycle cursor"), Some(1));

            stop.store(true, Ordering::Release);
            let _ = wake_tx.try_send(());
            worker.join().expect("worker shutdown");
        }

        #[test]
        fn worker_retries_failed_subscribe_with_same_opaque_cursor() {
            let telemetry = Arc::new(Mutex::new(RuntimeTelemetryState::new(
                "desktop-retry".to_string(),
                None,
                PathBuf::from("/tmp/project"),
            )));
            let live_snapshot = snapshot("recovered-agent");
            lock_state(&telemetry).apply_event(&DesktopEvent::AgentRuntimeUpdate {
                snapshot: Box::new(live_snapshot.clone()),
            });
            let recovered_agent = agent_runtime_from_snapshot(&live_snapshot);

            let (wake_tx, wake_rx) = sync_channel(1);
            let (report_tx, _report_rx) = mpsc::channel();
            let (since_tx, since_rx) = mpsc::channel();
            let mut transport = ScriptedTransport {
                publish_results: VecDeque::new(),
                subscribe_results: VecDeque::from([
                    Ok(subscription(10, Vec::new())),
                    Err("temporary subscribe failure".to_string()),
                    Ok(subscription(20, vec![recovered_agent])),
                ]),
                report_tx,
                since_tx,
            };
            let downstream = Arc::new(RecordingSink::default());
            let downstream_trait: Arc<dyn DesktopEventSink> = downstream.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = Arc::clone(&stop);
            let worker = thread::spawn(move || {
                run_worker(
                    &mut transport,
                    telemetry,
                    downstream_trait,
                    wake_rx,
                    worker_stop,
                    Duration::from_secs(60),
                )
            });

            wake_tx.try_send(()).expect("first cycle");
            assert_eq!(since_rx.recv().expect("initial cursor"), None);
            assert_eq!(since_rx.recv().expect("failed cursor"), Some(10));
            wake_tx.try_send(()).expect("retry cycle");
            assert_eq!(since_rx.recv().expect("retried cursor"), Some(10));
            assert!(downstream.wait_for_ops_agent("recovered-agent"));
            assert!(downstream.wait_for_connection_health_sequence(&[
                (true, false),
                (false, true),
                (true, false),
            ]));

            stop.store(true, Ordering::Release);
            let _ = wake_tx.try_send(());
            worker.join().expect("worker shutdown");
        }

        #[test]
        fn worker_subscribes_after_publish_failure_and_reports_recovery() {
            let telemetry = Arc::new(Mutex::new(RuntimeTelemetryState::new(
                "desktop-publish-retry".to_string(),
                None,
                PathBuf::from("/tmp/project"),
            )));
            let live_snapshot = snapshot("publish-recovered-agent");
            lock_state(&telemetry).apply_event(&DesktopEvent::AgentRuntimeUpdate {
                snapshot: Box::new(live_snapshot.clone()),
            });
            let recovered_agent = agent_runtime_from_snapshot(&live_snapshot);

            let (wake_tx, wake_rx) = sync_channel(1);
            let (report_tx, _report_rx) = mpsc::channel();
            let (since_tx, since_rx) = mpsc::channel();
            let mut transport = ScriptedTransport {
                publish_results: VecDeque::from([
                    Err("temporary publish failure".to_string()),
                    Ok(()),
                ]),
                subscribe_results: VecDeque::from([
                    Ok(subscription(10, Vec::new())),
                    Ok(subscription(20, vec![recovered_agent.clone()])),
                    Ok(subscription(30, vec![recovered_agent])),
                ]),
                report_tx,
                since_tx,
            };
            let downstream = Arc::new(RecordingSink::default());
            let downstream_trait: Arc<dyn DesktopEventSink> = downstream.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = Arc::clone(&stop);
            let worker = thread::spawn(move || {
                run_worker(
                    &mut transport,
                    telemetry,
                    downstream_trait,
                    wake_rx,
                    worker_stop,
                    Duration::from_secs(60),
                )
            });

            wake_tx.try_send(()).expect("failed publish cycle");
            assert_eq!(since_rx.recv().expect("initial cursor"), None);
            assert_eq!(since_rx.recv().expect("post-failure cursor"), Some(10));
            assert!(downstream.wait_for_ops_agent("publish-recovered-agent"));
            assert!(downstream.wait_for_connection_health_sequence(&[(true, false), (true, true),]));

            wake_tx.try_send(()).expect("recovery cycle");
            assert_eq!(since_rx.recv().expect("recovery cursor"), Some(20));
            assert!(downstream.wait_for_connection_health_sequence(&[
                (true, false),
                (true, true),
                (true, false),
            ]));

            stop.store(true, Ordering::Release);
            let _ = wake_tx.try_send(());
            worker.join().expect("worker shutdown");
        }
    }
}

/// Wrap the UI event sink with the daemon-truth publisher/subscriber.
///
/// On non-Unix targets the caller receives an explicit unsupported result and
/// can keep using the downstream sink without pretending daemon IPC is live.
pub fn attach_desktop_daemon_ops(
    downstream: Arc<dyn DesktopEventSink>,
    config: DesktopDaemonOpsConfig,
) -> Result<Arc<dyn DesktopEventSink>, DesktopDaemonOpsStartError> {
    #[cfg(unix)]
    {
        unix::attach(downstream, config)
    }
    #[cfg(not(unix))]
    {
        let _ = (downstream, config);
        Err(DesktopDaemonOpsStartError::UnsupportedPlatform)
    }
}

#[cfg(test)]
mod tests {
    use impulse_ops::{AgentRole, AgentStatus};

    use super::*;
    use crate::runtime::{AgentPlatformId, BuiltInMcpTool, WorkspaceTarget};

    fn rich_snapshot(agent_id: &str, cwd: &str) -> AgentRuntimeSnapshot {
        AgentRuntimeSnapshot {
            agent_id: agent_id.to_string(),
            label: "Claude Code".to_string(),
            platform: AgentPlatformId::try_new("claude-code").unwrap(),
            command: "claude".to_string(),
            args: vec!["--print".to_string()],
            cwd: Some(cwd.to_string()),
            workspace: Some(WorkspaceTarget {
                root: cwd.to_string(),
                label: Some("manager-wave".to_string()),
                purpose: Some("agent management".to_string()),
                project_notes: None,
            }),
            session_id: Some("session-1".to_string()),
            rows: 32,
            cols: 100,
            alive: true,
            focused: true,
            status: AgentStatus::Working {
                task: "publish truth".to_string(),
            },
            current_task: None,
            role: Some(AgentRole::Worker { parent_pane_id: 7 }),
            target: Some(MachineTarget::Remote {
                user: "agent".to_string(),
                host: "builder".to_string(),
                workdir: "/srv/project".to_string(),
                session_name: Some("wave".to_string()),
            }),
            mcp_tools: vec![BuiltInMcpTool::new(
                "impulse.agent_write",
                "write",
                vec!["terminal".to_string()],
                true,
            )],
            output_bytes: 128,
            output_lines: 4,
            context: ContextHealthSummary {
                tier: "critical".to_string(),
                usage_fraction: 0.7,
                estimated_tokens: 70,
                window_tokens: 100,
                pending_review_count: 1,
                ..Default::default()
            },
        }
    }

    #[test]
    fn rich_snapshot_conversion_preserves_manager_facts() {
        let runtime = agent_runtime_from_snapshot(&rich_snapshot("claude-live", "/repo"));
        assert_eq!(runtime.id, "claude-live");
        assert_eq!(runtime.label, "Claude Code");
        assert_eq!(runtime.backend_kind, "claude-code");
        assert_eq!(runtime.session_id.as_deref(), Some("session-1"));
        assert_eq!(runtime.working_directory, "/repo");
        assert_eq!(runtime.status, "working: publish truth");
        assert_eq!(runtime.current_task.as_deref(), Some("publish truth"));
        assert!(runtime.active);
        assert_eq!(runtime.context.tier, "critical");
        assert_eq!(runtime.recent_tools, vec!["impulse.agent_write"]);
        assert_eq!(runtime.group.as_deref(), Some("manager-wave"));
        assert_eq!(runtime.role, Some(AgentRole::Worker { parent_pane_id: 7 }));
        assert_eq!(
            runtime.target,
            Some(MachineTarget::Remote {
                user: "agent".to_string(),
                host: "builder".to_string(),
                workdir: "/srv/project".to_string(),
                session_name: Some("wave".to_string()),
            })
        );
    }

    #[test]
    fn report_filters_other_workspaces_and_removes_exited_agents() {
        let mut state = RuntimeTelemetryState::new(
            "desktop-source".to_string(),
            Some(PathBuf::from("/repo")),
            PathBuf::from("/repo"),
        );
        assert!(state.apply_event(&DesktopEvent::AgentRuntimeUpdate {
            snapshot: Box::new(rich_snapshot("inside", "/repo/crate")),
        }));
        assert!(state.apply_event(&DesktopEvent::AgentRuntimeUpdate {
            snapshot: Box::new(rich_snapshot("outside", "/other")),
        }));
        assert!(state.apply_event(&DesktopEvent::AgentRuntimeUpdate {
            snapshot: Box::new(rich_snapshot("relative-escape", "../other")),
        }));
        let report = state.report();
        assert_eq!(report.source_id, "desktop-source");
        assert_eq!(report.agents.len(), 1);
        assert_eq!(report.agents[0].id, "inside");
        assert_eq!(report.context.tier, "critical");

        assert!(state.apply_event(&DesktopEvent::TerminalExit {
            agent_id: "inside".to_string(),
        }));
        assert!(!state.apply_event(&DesktopEvent::TerminalExit {
            agent_id: "inside".to_string(),
        }));
        assert!(state.report().agents.is_empty());
    }

    #[test]
    fn focus_update_clears_cached_peer_focus_and_selects_new_context() {
        let mut state =
            RuntimeTelemetryState::new("desktop-source".to_string(), None, PathBuf::from("/repo"));
        let mut first = rich_snapshot("a-agent", "/repo");
        first.context.tier = "essential".to_string();
        first.context.usage_fraction = 0.3;
        let mut second = rich_snapshot("b-agent", "/repo");
        second.focused = false;
        second.context.tier = "critical".to_string();
        second.context.usage_fraction = 0.8;
        assert!(state.apply_event(&DesktopEvent::AgentRuntimeUpdate {
            snapshot: Box::new(first),
        }));
        assert!(state.apply_event(&DesktopEvent::AgentRuntimeUpdate {
            snapshot: Box::new(second.clone()),
        }));

        second.focused = true;
        assert!(state.apply_event(&DesktopEvent::AgentRuntimeUpdate {
            snapshot: Box::new(second),
        }));
        assert!(!state.agents["a-agent"].focused);
        assert!(state.agents["b-agent"].focused);
        assert_eq!(state.report().context.tier, "critical");
    }

    #[test]
    fn newest_shared_state_survives_coalesced_wakeup_semantics() {
        let mut state =
            RuntimeTelemetryState::new("desktop-source".to_string(), None, PathBuf::from("/repo"));
        let mut first = rich_snapshot("agent", "/repo");
        first.rows = 24;
        let mut latest = first.clone();
        latest.rows = 48;
        latest.current_task = Some("latest task".to_string());
        assert!(state.apply_event(&DesktopEvent::AgentRuntimeUpdate {
            snapshot: Box::new(first),
        }));
        assert!(state.apply_event(&DesktopEvent::AgentRuntimeUpdate {
            snapshot: Box::new(latest),
        }));
        assert_eq!(
            state.report().agents[0].current_task.as_deref(),
            Some("latest task")
        );
    }

    #[test]
    fn project_root_is_derived_only_from_standard_socket_shape() {
        assert_eq!(
            project_root_from_socket(Path::new("/repo/.impulse/sockets/impulse.sock")),
            Some(PathBuf::from("/repo"))
        );
        assert_eq!(
            project_root_from_socket(Path::new("/tmp/custom.sock")),
            None
        );
    }

    #[test]
    fn relative_cwd_is_resolved_from_desktop_process_base() {
        assert!(path_is_within(
            Path::new("/repo"),
            Path::new("/repo/subdir"),
            Path::new(".."),
        ));
        assert!(!path_is_within(
            Path::new("/repo"),
            Path::new("/repo/subdir"),
            Path::new("../../../other"),
        ));
    }

    #[test]
    fn cwdless_agent_must_inherit_a_directory_inside_bound_project() {
        let mut outside_state = RuntimeTelemetryState::new(
            "desktop-source".to_string(),
            Some(PathBuf::from("/repo")),
            PathBuf::from("/other"),
        );
        let mut cwdless = rich_snapshot("cwdless", "/repo");
        cwdless.cwd = None;
        cwdless.workspace = None;
        assert!(
            outside_state.apply_event(&DesktopEvent::AgentRuntimeUpdate {
                snapshot: Box::new(cwdless.clone()),
            })
        );
        assert!(outside_state.report().agents.is_empty());

        let mut inside_state = RuntimeTelemetryState::new(
            "desktop-source".to_string(),
            Some(PathBuf::from("/repo")),
            PathBuf::from("/repo/subdir"),
        );
        assert!(inside_state.apply_event(&DesktopEvent::AgentRuntimeUpdate {
            snapshot: Box::new(cwdless),
        }));
        assert_eq!(inside_state.report().agents[0].id, "cwdless");
    }
}
