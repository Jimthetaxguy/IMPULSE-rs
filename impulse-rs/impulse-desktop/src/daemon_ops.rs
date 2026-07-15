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

use crate::runtime::{AgentRuntimeSnapshot, DesktopEvent, DesktopEventSink, GovernedTaskGateway};

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
        governed_task_id: snapshot.governed_task_id.clone(),
        governed_task_revision: snapshot.governed_task_revision,
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
        role_assignment: snapshot.role_assignment.clone(),
        role_compatibility: snapshot.role_compatibility.clone(),
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

/// Both halves of the desktop/daemon boundary: asynchronous lifecycle
/// telemetry and acknowledged governed-task commands.
pub struct DesktopDaemonOpsAttachment {
    pub event_sink: Arc<dyn DesktopEventSink>,
    pub governed_task_gateway: Arc<dyn GovernedTaskGateway>,
}

#[cfg(unix)]
mod unix {
    use std::collections::BTreeSet;
    use std::fs::{File, OpenOptions};
    use std::io::{BufRead, BufReader, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
    use std::thread::{self, JoinHandle};

    use impulse_ops::{OpsSubscription, WorkbenchDaemonRequest, WorkbenchDaemonResponse};

    use super::*;

    const IO_TIMEOUT: Duration = Duration::from_secs(2);
    const ACKNOWLEDGED_REQUEST_ATTEMPTS: usize = 3;
    const GOVERNED_LIFECYCLE_OUTBOX_SCHEMA: u32 = 1;
    const MAX_GOVERNED_LIFECYCLE_OUTBOX_ENTRIES: usize = 1_024;

    struct LifecycleOutboxFileLock(File);

    fn flock_retry(file: &File, operation: libc::c_int) -> std::io::Result<()> {
        loop {
            // SAFETY: callers retain ownership of `file` for the duration of
            // this call, so its descriptor remains valid across `flock`.
            if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
                return Ok(());
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    impl Drop for LifecycleOutboxFileLock {
        fn drop(&mut self) {
            if let Err(error) = flock_retry(&self.0, libc::LOCK_UN) {
                eprintln!("failed to unlock governed lifecycle outbox: {error}");
            }
        }
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
    struct PendingGovernedLifecycleMutation {
        project_id: String,
        task_id: impulse_ops::governed_task::GovernedTaskId,
        request_id: impulse_ops::governed_task::GovernedRequestId,
        mutation: impulse_ops::governed_task::GovernedTaskMutation,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    struct GovernedLifecycleOutbox {
        schema_version: u32,
        #[serde(default)]
        entries: Vec<PendingGovernedLifecycleMutation>,
    }

    impl Default for GovernedLifecycleOutbox {
        fn default() -> Self {
            Self {
                schema_version: GOVERNED_LIFECYCLE_OUTBOX_SCHEMA,
                entries: Vec::new(),
            }
        }
    }

    trait DaemonOpsTransport: Send {
        fn publish(&mut self, report: &TerminalOpsReport) -> Result<(), String>;
        fn subscribe(&mut self, since_seq: Option<u64>) -> Result<OpsSubscription, String>;
        fn drain_governed_lifecycle_outbox(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct UnixDaemonOpsClient {
        socket_path: PathBuf,
        lifecycle_outbox_path: Option<PathBuf>,
        lifecycle_outbox_lock: Arc<Mutex<()>>,
    }

    impl UnixDaemonOpsClient {
        fn new(socket_path: PathBuf) -> Self {
            Self {
                socket_path,
                lifecycle_outbox_path: None,
                lifecycle_outbox_lock: Arc::new(Mutex::new(())),
            }
        }

        fn with_lifecycle_outbox(mut self, path: Option<PathBuf>) -> Self {
            self.lifecycle_outbox_path = path;
            self
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

        /// Retry only ambiguous transport failures. The exact serialized
        /// request (including its idempotency key) is reused, so a daemon
        /// commit followed by a lost response is observed, never duplicated.
        fn send_acknowledged(
            &self,
            request: &WorkbenchDaemonRequest,
        ) -> Result<WorkbenchDaemonResponse, String> {
            let mut last_error = None;
            for _ in 0..ACKNOWLEDGED_REQUEST_ATTEMPTS {
                match self.send(request) {
                    Ok(response) => return Ok(response),
                    Err(error) => last_error = Some(error),
                }
            }
            Err(last_error.unwrap_or_else(|| {
                "acknowledged daemon request exhausted transport retries".to_string()
            }))
        }

        fn get_governed_task(
            &self,
            project_id: &str,
            task_id: &impulse_ops::governed_task::GovernedTaskId,
        ) -> Result<Option<impulse_ops::governed_task::GovernedTaskRun>, String> {
            let response = self.send_acknowledged(&WorkbenchDaemonRequest::GetGovernedTask {
                project_id: project_id.to_string(),
                task_id: task_id.clone(),
            })?;
            let value = Self::ok_result(response)?;
            serde_json::from_value(value)
                .map_err(|error| format!("parse current governed task: {error}"))
        }

        fn mutate_governed_task(
            &self,
            request: impulse_ops::governed_task::GovernedTaskMutationRequest,
        ) -> Result<impulse_ops::governed_task::GovernedTaskRun, String> {
            let response =
                self.send_acknowledged(&WorkbenchDaemonRequest::MutateGovernedTask { request })?;
            let value = Self::ok_result(response)?;
            serde_json::from_value(value)
                .map_err(|error| format!("parse mutated governed task: {error}"))
        }

        fn mutate_current_once(
            &self,
            project_id: &str,
            task_id: &impulse_ops::governed_task::GovernedTaskId,
            request_id: &impulse_ops::governed_task::GovernedRequestId,
            mutation: &impulse_ops::governed_task::GovernedTaskMutation,
        ) -> Result<impulse_ops::governed_task::GovernedTaskRun, String> {
            let mut last_conflict = None;
            for _ in 0..3 {
                let current = self
                    .get_governed_task(project_id, task_id)?
                    .ok_or_else(|| {
                        format!("governed task `{task_id}` disappeared before mutation")
                    })?;
                let request = impulse_ops::governed_task::GovernedTaskMutationRequest {
                    request_id: request_id.clone(),
                    project_id: project_id.to_string(),
                    task_id: task_id.clone(),
                    expected_revision: current.revision,
                    mutation: mutation.clone(),
                };
                match self.mutate_governed_task(request) {
                    Ok(task) => return Ok(task),
                    Err(error) if error.contains("revision conflict") => {
                        last_conflict = Some(error);
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(last_conflict
                .unwrap_or_else(|| "governed task mutation exhausted revision retries".to_string()))
        }

        fn queue_lifecycle_mutation(
            &self,
            pending: PendingGovernedLifecycleMutation,
        ) -> Result<(), String> {
            let Some(path) = self.lifecycle_outbox_path.as_deref() else {
                return Err("governed lifecycle outbox is not configured".to_string());
            };
            let _guard = self
                .lifecycle_outbox_lock
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _file_guard = Self::lock_lifecycle_outbox(path)?;
            let mut outbox = Self::read_lifecycle_outbox(path)?;
            if let Some(existing) = outbox
                .entries
                .iter()
                .find(|entry| entry.request_id == pending.request_id)
            {
                return if existing == &pending {
                    Ok(())
                } else {
                    Err(format!(
                        "governed lifecycle request id `{}` already queues a different operation",
                        pending.request_id
                    ))
                };
            }
            if outbox.entries.len() >= MAX_GOVERNED_LIFECYCLE_OUTBOX_ENTRIES {
                return Err(format!(
                    "governed lifecycle outbox reached {MAX_GOVERNED_LIFECYCLE_OUTBOX_ENTRIES} entries"
                ));
            }
            outbox.entries.push(pending);
            Self::write_lifecycle_outbox(path, &outbox)
        }

        fn drain_lifecycle_outbox(&self) -> Result<(), String> {
            let Some(path) = self.lifecycle_outbox_path.as_deref() else {
                return Ok(());
            };
            let entries = {
                let _guard = self
                    .lifecycle_outbox_lock
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let _file_guard = Self::lock_lifecycle_outbox(path)?;
                Self::read_lifecycle_outbox(path)?.entries
            };
            if entries.is_empty() {
                return Ok(());
            }

            // Never hold the outbox lock across daemon I/O. New exit intent
            // must remain appendable while an older retry is slow. Idempotent
            // request IDs make concurrent drainers safe; the merge below only
            // removes snapshot IDs that this pass actually resolved.
            let mut resolved = BTreeSet::new();
            let mut failure = None;
            for pending in entries {
                let current = match self.get_governed_task(&pending.project_id, &pending.task_id) {
                    Ok(current) => current,
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                };
                let Some(current) = current else {
                    // A timed-out registration may still commit after this
                    // read. Retain the intent until a durable tombstone or
                    // expiry policy can prove the task will never exist.
                    failure.get_or_insert_with(|| {
                        format!(
                            "governed task `{}` is not visible yet; lifecycle intent retained",
                            pending.task_id
                        )
                    });
                    continue;
                };
                if !lifecycle_mutation_is_applicable(&current, &pending.mutation) {
                    // Already applied or superseded by a later terminal state.
                    resolved.insert(pending.request_id);
                    continue;
                }
                if let Err(error) = self.mutate_current_once(
                    &pending.project_id,
                    &pending.task_id,
                    &pending.request_id,
                    &pending.mutation,
                ) {
                    failure = Some(error);
                    break;
                }
                resolved.insert(pending.request_id);
            }

            let retained = {
                let _guard = self
                    .lifecycle_outbox_lock
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let _file_guard = Self::lock_lifecycle_outbox(path)?;
                let mut latest = Self::read_lifecycle_outbox(path)?;
                if !resolved.is_empty() {
                    latest
                        .entries
                        .retain(|entry| !resolved.contains(&entry.request_id));
                    Self::write_lifecycle_outbox(path, &latest)?;
                }
                latest.entries.len()
            };
            match failure {
                Some(error) => Err(format!(
                    "governed lifecycle outbox retains {} operation(s): {error}",
                    retained
                )),
                None => Ok(()),
            }
        }

        fn remove_lifecycle_mutation(
            &self,
            request_id: &impulse_ops::governed_task::GovernedRequestId,
        ) -> Result<(), String> {
            let Some(path) = self.lifecycle_outbox_path.as_deref() else {
                return Err("governed lifecycle outbox is not configured".to_string());
            };
            let _guard = self
                .lifecycle_outbox_lock
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _file_guard = Self::lock_lifecycle_outbox(path)?;
            let mut outbox = Self::read_lifecycle_outbox(path)?;
            let original_len = outbox.entries.len();
            outbox
                .entries
                .retain(|entry| &entry.request_id != request_id);
            if outbox.entries.len() != original_len {
                Self::write_lifecycle_outbox(path, &outbox)?;
            }
            Ok(())
        }

        fn lock_lifecycle_outbox(path: &Path) -> Result<LifecycleOutboxFileLock, String> {
            let parent = path
                .parent()
                .ok_or_else(|| "governed lifecycle outbox path has no parent".to_string())?;
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create governed lifecycle outbox directory: {error}"))?;
            let lock_path = path.with_extension("lock");
            let file = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(false)
                .mode(0o600)
                .open(&lock_path)
                .map_err(|error| format!("open governed lifecycle outbox lock: {error}"))?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|error| format!("restrict governed lifecycle outbox lock: {error}"))?;
            flock_retry(&file, libc::LOCK_EX)
                .map_err(|error| format!("lock governed lifecycle outbox: {error}"))?;
            Ok(LifecycleOutboxFileLock(file))
        }

        fn read_lifecycle_outbox(path: &Path) -> Result<GovernedLifecycleOutbox, String> {
            if !path.exists() {
                return Ok(GovernedLifecycleOutbox::default());
            }
            let bytes = std::fs::read(path)
                .map_err(|error| format!("read governed lifecycle outbox: {error}"))?;
            let outbox: GovernedLifecycleOutbox = serde_json::from_slice(&bytes)
                .map_err(|error| format!("parse governed lifecycle outbox: {error}"))?;
            if outbox.schema_version != GOVERNED_LIFECYCLE_OUTBOX_SCHEMA {
                return Err(format!(
                    "unsupported governed lifecycle outbox schema {}",
                    outbox.schema_version
                ));
            }
            Ok(outbox)
        }

        fn write_lifecycle_outbox(
            path: &Path,
            outbox: &GovernedLifecycleOutbox,
        ) -> Result<(), String> {
            let parent = path
                .parent()
                .ok_or_else(|| "governed lifecycle outbox path has no parent".to_string())?;
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create governed lifecycle outbox directory: {error}"))?;
            let temporary = path.with_extension(format!(
                "tmp-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            let bytes = serde_json::to_vec_pretty(outbox)
                .map_err(|error| format!("serialize governed lifecycle outbox: {error}"))?;
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temporary)
                .map_err(|error| format!("create governed lifecycle outbox temp file: {error}"))?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|error| {
                    format!("restrict governed lifecycle outbox temp file: {error}")
                })?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|error| format!("write governed lifecycle outbox: {error}"))?;
            std::fs::rename(&temporary, path)
                .map_err(|error| format!("replace governed lifecycle outbox: {error}"))?;
            Ok(())
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

        fn drain_governed_lifecycle_outbox(&mut self) -> Result<(), String> {
            self.drain_lifecycle_outbox()
        }
    }

    impl GovernedTaskGateway for UnixDaemonOpsClient {
        fn register(
            &self,
            registration: impulse_ops::governed_task::GovernedTaskRegistration,
        ) -> Result<impulse_ops::governed_task::GovernedTaskRun, String> {
            let response =
                self.send_acknowledged(&WorkbenchDaemonRequest::RegisterGovernedTask {
                    registration,
                })?;
            let value = Self::ok_result(response)?;
            serde_json::from_value(value)
                .map_err(|error| format!("parse registered governed task: {error}"))
        }

        fn mutate(
            &self,
            request: impulse_ops::governed_task::GovernedTaskMutationRequest,
        ) -> Result<impulse_ops::governed_task::GovernedTaskRun, String> {
            self.mutate_governed_task(request)
        }

        fn mutate_current(
            &self,
            project_id: &str,
            task_id: &impulse_ops::governed_task::GovernedTaskId,
            request_id: impulse_ops::governed_task::GovernedRequestId,
            mutation: impulse_ops::governed_task::GovernedTaskMutation,
        ) -> Result<impulse_ops::governed_task::GovernedTaskRun, String> {
            if !lifecycle_mutation_is_queueable(&mutation) {
                return self.mutate_current_once(project_id, task_id, &request_id, &mutation);
            }

            // Persist lifecycle intent before the first daemon round trip. A
            // lost response or desktop failure during ambiguous I/O therefore
            // leaves an idempotent operation for the next attachment to drain.
            let pending = PendingGovernedLifecycleMutation {
                project_id: project_id.to_string(),
                task_id: task_id.clone(),
                request_id: request_id.clone(),
                mutation: mutation.clone(),
            };
            self.queue_lifecycle_mutation(pending)?;
            match self.mutate_current_once(project_id, task_id, &request_id, &mutation) {
                Ok(task) => {
                    self.remove_lifecycle_mutation(&request_id).map_err(|error| {
                        format!(
                            "lifecycle mutation committed but its durable outbox entry could not be cleared: {error}"
                        )
                    })?;
                    Ok(task)
                }
                Err(error) => Err(format!(
                    "{error}; lifecycle mutation retained for durable daemon retry"
                )),
            }
        }
    }

    fn lifecycle_mutation_is_queueable(
        mutation: &impulse_ops::governed_task::GovernedTaskMutation,
    ) -> bool {
        matches!(
            mutation,
            impulse_ops::governed_task::GovernedTaskMutation::MarkLaunchFailed { .. }
                | impulse_ops::governed_task::GovernedTaskMutation::MarkRuntimeExited { .. }
        )
    }

    fn lifecycle_mutation_is_applicable(
        task: &impulse_ops::governed_task::GovernedTaskRun,
        mutation: &impulse_ops::governed_task::GovernedTaskMutation,
    ) -> bool {
        use impulse_ops::governed_task::{GovernedExecutionState, GovernedTaskMutation};

        matches!(
            (task.execution_state, mutation),
            (
                GovernedExecutionState::Registered,
                GovernedTaskMutation::MarkLaunchFailed { .. }
            ) | (
                GovernedExecutionState::Running,
                GovernedTaskMutation::MarkRuntimeExited { .. }
            )
        )
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
    ) -> Result<DesktopDaemonOpsAttachment, DesktopDaemonOpsStartError> {
        let lifecycle_outbox_path = config.project_root.as_ref().map(|root| {
            root.join(".impulse")
                .join("DESKTOP_GOVERNED_LIFECYCLE_OUTBOX.json")
        });
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
        let client = UnixDaemonOpsClient::new(config.socket_path)
            .with_lifecycle_outbox(lifecycle_outbox_path);
        let command_client = Arc::new(client.clone());
        let mut client = client;
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

        let event_sink: Arc<dyn DesktopEventSink> = Arc::new(DesktopDaemonOpsSink {
            downstream,
            telemetry,
            wake_tx,
            stop,
            worker: Mutex::new(Some(worker)),
        });
        let governed_task_gateway: Arc<dyn GovernedTaskGateway> = command_client;
        Ok(DesktopDaemonOpsAttachment {
            event_sink,
            governed_task_gateway,
        })
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
        let mut initial_errors = client
            .drain_governed_lifecycle_outbox()
            .err()
            .map(|error| format!("governed lifecycle outbox: {error}"))
            .into_iter()
            .collect::<Vec<_>>();
        initial_errors.extend(subscribe_and_emit(client, &mut cursor, &downstream));
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
                if let Err(error) = client.drain_governed_lifecycle_outbox() {
                    eprintln!("desktop governed lifecycle outbox shutdown drain failed: {error}");
                }
                let empty = lock_state(&telemetry).empty_report();
                if let Err(error) = client.publish(&empty) {
                    eprintln!("desktop daemon-ops shutdown publish failed: {error}");
                }
                break;
            }

            let report = lock_state(&telemetry).report();
            let mut errors = Vec::new();
            if let Err(error) = client.drain_governed_lifecycle_outbox() {
                errors.push(format!("governed lifecycle outbox: {error}"));
            }
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

        #[test]
        fn governed_registration_retries_the_same_id_after_a_lost_response() {
            use impulse_ops::governed_task::{
                ApprovalPolicy, GovernedExecutionState, GovernedReviewState,
                GovernedTaskRegistration, GovernedTaskRun,
            };

            let dir = tempfile::tempdir().expect("tempdir");
            let socket = dir.path().join("governed-register.sock");
            let listener = UnixListener::bind(&socket).expect("bind fake daemon");
            let registration = GovernedTaskRegistration::builder(
                "transport-register-1",
                "transport-task-1",
                "project",
                "/tmp/project",
                "prove ambiguous acknowledgment retry",
                "worker-1",
                "codex",
            )
            .build()
            .unwrap();
            let expected = GovernedTaskRun {
                id: registration.task_id.clone(),
                revision: 0,
                project_id: registration.project_id.clone(),
                workspace_root: registration.workspace_root.clone(),
                task: registration.task.clone(),
                acceptance_criteria: registration.acceptance_criteria.clone(),
                approval_policy: ApprovalPolicy::OperatorRequired,
                role_assignment: None,
                role_compatibility: None,
                runtime_id: registration.runtime_id.clone(),
                agent_id: registration.agent_id.clone(),
                session_id: None,
                initial_subject_revision: None,
                execution_state: GovernedExecutionState::Registered,
                review_state: GovernedReviewState::AwaitingClaim,
                claims: vec![],
                verifications: vec![],
                supervisor_verdicts: vec![],
                operator_decisions: vec![],
                events: vec![],
                created_at: "2026-07-13T00:00:00Z".to_string(),
                updated_at: "2026-07-13T00:00:00Z".to_string(),
            };
            let expected_for_server = expected.clone();
            let (request_tx, request_rx) = mpsc::channel();
            let server = thread::spawn(move || {
                for attempt in 0..2 {
                    let (mut stream, _) = listener.accept().expect("accept registration");
                    let mut line = String::new();
                    BufReader::new(stream.try_clone().unwrap())
                        .read_line(&mut line)
                        .unwrap();
                    let request: WorkbenchDaemonRequest = serde_json::from_str(&line).unwrap();
                    request_tx.send(request).unwrap();
                    if attempt == 1 {
                        write_response(
                            &mut stream,
                            &WorkbenchDaemonResponse::Ok {
                                result: serde_json::to_value(&expected_for_server).unwrap(),
                            },
                        );
                    }
                }
            });

            let client = UnixDaemonOpsClient::new(socket);
            let acknowledged = client.register(registration).unwrap();
            assert_eq!(acknowledged, expected);
            let first = request_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let second = request_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert_eq!(
                first, second,
                "ambiguous retry must reuse the exact request"
            );
            server.join().unwrap();
        }

        #[test]
        fn governed_exit_outbox_persists_and_drains_after_daemon_recovery() {
            use impulse_ops::governed_task::{
                ApprovalPolicy, GovernedActor, GovernedActorKind, GovernedExecutionState,
                GovernedRequestId, GovernedReviewState, GovernedTaskId, GovernedTaskMutation,
                GovernedTaskRun,
            };

            let dir = tempfile::tempdir().expect("tempdir");
            let socket = dir.path().join("recovering-daemon.sock");
            let outbox_path = dir.path().join("lifecycle-outbox.json");
            let task_id = GovernedTaskId::try_new("recover-task-1").unwrap();
            let request_id = GovernedRequestId::try_new("recover-exit-1").unwrap();
            let running = GovernedTaskRun {
                id: task_id.clone(),
                revision: 1,
                project_id: "project".to_string(),
                workspace_root: "/tmp/project".to_string(),
                task: "persist exit while daemon is down".to_string(),
                acceptance_criteria: vec![],
                approval_policy: ApprovalPolicy::OperatorRequired,
                role_assignment: None,
                role_compatibility: None,
                runtime_id: "codex".to_string(),
                agent_id: "worker-1".to_string(),
                session_id: None,
                initial_subject_revision: None,
                execution_state: GovernedExecutionState::Running,
                review_state: GovernedReviewState::AwaitingClaim,
                claims: vec![],
                verifications: vec![],
                supervisor_verdicts: vec![],
                operator_decisions: vec![],
                events: vec![],
                created_at: "2026-07-13T00:00:00Z".to_string(),
                updated_at: "2026-07-13T00:01:00Z".to_string(),
            };
            let mutation = GovernedTaskMutation::MarkRuntimeExited {
                actor: GovernedActor {
                    kind: GovernedActorKind::System,
                    id: "desktop-runtime".to_string(),
                },
                reason: Some("worker exited while daemon was unavailable".to_string()),
            };
            let mut client = UnixDaemonOpsClient::new(socket.clone())
                .with_lifecycle_outbox(Some(outbox_path.clone()));

            let error = client
                .mutate_current("project", &task_id, request_id.clone(), mutation.clone())
                .unwrap_err();
            assert!(error.contains("retained for durable daemon retry"));
            let queued = UnixDaemonOpsClient::read_lifecycle_outbox(&outbox_path).unwrap();
            assert_eq!(queued.entries.len(), 1);
            assert_eq!(queued.entries[0].request_id, request_id);
            assert_eq!(
                std::fs::metadata(&outbox_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );

            let listener = UnixListener::bind(&socket).expect("bind recovered daemon");
            let running_for_server = running.clone();
            let request_id_for_server = request_id.clone();
            let server = thread::spawn(move || {
                for step in 0..3 {
                    let (mut stream, _) = listener.accept().expect("accept recovery request");
                    let mut line = String::new();
                    BufReader::new(stream.try_clone().unwrap())
                        .read_line(&mut line)
                        .unwrap();
                    let request: WorkbenchDaemonRequest = serde_json::from_str(&line).unwrap();
                    match (step, request) {
                        (0 | 1, WorkbenchDaemonRequest::GetGovernedTask { task_id, .. }) => {
                            assert_eq!(task_id, running_for_server.id);
                            write_response(
                                &mut stream,
                                &WorkbenchDaemonResponse::Ok {
                                    result: serde_json::to_value(Some(&running_for_server))
                                        .unwrap(),
                                },
                            );
                        }
                        (2, WorkbenchDaemonRequest::MutateGovernedTask { request }) => {
                            assert_eq!(request.request_id, request_id_for_server);
                            assert_eq!(request.expected_revision, running_for_server.revision);
                            let mut exited = running_for_server.clone();
                            exited.revision += 1;
                            exited.execution_state = GovernedExecutionState::RuntimeExited;
                            write_response(
                                &mut stream,
                                &WorkbenchDaemonResponse::Ok {
                                    result: serde_json::to_value(exited).unwrap(),
                                },
                            );
                        }
                        other => panic!("unexpected recovery request: {other:?}"),
                    }
                }
            });

            client.drain_governed_lifecycle_outbox().unwrap();
            server.join().unwrap();
            let drained = UnixDaemonOpsClient::read_lifecycle_outbox(&outbox_path).unwrap();
            assert!(drained.entries.is_empty());
        }

        #[test]
        fn slow_outbox_drain_allows_new_intent_and_merges_by_request_id() {
            use impulse_ops::governed_task::{
                ApprovalPolicy, GovernedActor, GovernedActorKind, GovernedExecutionState,
                GovernedRequestId, GovernedReviewState, GovernedTaskId, GovernedTaskMutation,
                GovernedTaskRun,
            };

            let dir = tempfile::tempdir().expect("tempdir");
            let socket = dir.path().join("slow-drain-daemon.sock");
            let outbox_path = dir.path().join("lifecycle-outbox.json");
            let first_task_id = GovernedTaskId::try_new("slow-drain-task").unwrap();
            let first_request_id = GovernedRequestId::try_new("slow-drain-exit").unwrap();
            let actor = GovernedActor {
                kind: GovernedActorKind::System,
                id: "desktop-runtime".to_string(),
            };
            let first_mutation = GovernedTaskMutation::MarkRuntimeExited {
                actor: actor.clone(),
                reason: Some("first runtime exited".to_string()),
            };
            let client = UnixDaemonOpsClient::new(socket.clone())
                .with_lifecycle_outbox(Some(outbox_path.clone()));
            client
                .queue_lifecycle_mutation(PendingGovernedLifecycleMutation {
                    project_id: "project".to_string(),
                    task_id: first_task_id.clone(),
                    request_id: first_request_id.clone(),
                    mutation: first_mutation,
                })
                .unwrap();

            let running = GovernedTaskRun {
                id: first_task_id,
                revision: 1,
                project_id: "project".to_string(),
                workspace_root: "/tmp/project".to_string(),
                task: "prove drain does not block new exit intent".to_string(),
                acceptance_criteria: vec![],
                approval_policy: ApprovalPolicy::OperatorRequired,
                role_assignment: None,
                role_compatibility: None,
                runtime_id: "codex".to_string(),
                agent_id: "worker-1".to_string(),
                session_id: None,
                initial_subject_revision: None,
                execution_state: GovernedExecutionState::Running,
                review_state: GovernedReviewState::AwaitingClaim,
                claims: vec![],
                verifications: vec![],
                supervisor_verdicts: vec![],
                operator_decisions: vec![],
                events: vec![],
                created_at: "2026-07-13T00:00:00Z".to_string(),
                updated_at: "2026-07-13T00:01:00Z".to_string(),
            };
            let listener = UnixListener::bind(&socket).expect("bind slow daemon");
            let (started_tx, started_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let running_for_server = running.clone();
            let first_request_for_server = first_request_id.clone();
            let server = thread::spawn(move || {
                for step in 0..3 {
                    let (mut stream, _) = listener.accept().expect("accept drain request");
                    let mut line = String::new();
                    BufReader::new(stream.try_clone().unwrap())
                        .read_line(&mut line)
                        .unwrap();
                    let request: WorkbenchDaemonRequest = serde_json::from_str(&line).unwrap();
                    if step == 0 {
                        started_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                    }
                    match (step, request) {
                        (0 | 1, WorkbenchDaemonRequest::GetGovernedTask { task_id, .. }) => {
                            assert_eq!(task_id, running_for_server.id);
                            write_response(
                                &mut stream,
                                &WorkbenchDaemonResponse::Ok {
                                    result: serde_json::to_value(Some(&running_for_server))
                                        .unwrap(),
                                },
                            );
                        }
                        (2, WorkbenchDaemonRequest::MutateGovernedTask { request }) => {
                            assert_eq!(request.request_id, first_request_for_server);
                            let mut exited = running_for_server.clone();
                            exited.revision += 1;
                            exited.execution_state = GovernedExecutionState::RuntimeExited;
                            write_response(
                                &mut stream,
                                &WorkbenchDaemonResponse::Ok {
                                    result: serde_json::to_value(exited).unwrap(),
                                },
                            );
                        }
                        other => panic!("unexpected slow-drain request: {other:?}"),
                    }
                }
            });

            let drain_client = client.clone();
            let drain = thread::spawn(move || drain_client.drain_lifecycle_outbox());
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();

            let second =
                UnixDaemonOpsClient::new(socket).with_lifecycle_outbox(Some(outbox_path.clone()));
            let (append_tx, append_rx) = mpsc::channel();
            let append = thread::spawn(move || {
                let result = second.queue_lifecycle_mutation(PendingGovernedLifecycleMutation {
                    project_id: "project".to_string(),
                    task_id: GovernedTaskId::try_new("concurrent-exit-task").unwrap(),
                    request_id: GovernedRequestId::try_new("concurrent-exit-intent").unwrap(),
                    mutation: GovernedTaskMutation::MarkRuntimeExited {
                        actor,
                        reason: Some("second runtime exited".to_string()),
                    },
                });
                append_tx.send(result).unwrap();
            });
            let append_result = append_rx.recv_timeout(Duration::from_secs(1));
            if append_result.is_err() {
                release_tx.send(()).unwrap();
                append.join().unwrap();
                drain.join().unwrap().unwrap();
                server.join().unwrap();
                panic!("slow daemon I/O blocked write-ahead lifecycle intent");
            }
            append_result.unwrap().unwrap();
            release_tx.send(()).unwrap();
            append.join().unwrap();
            drain.join().unwrap().unwrap();
            server.join().unwrap();

            let persisted = UnixDaemonOpsClient::read_lifecycle_outbox(&outbox_path).unwrap();
            assert_eq!(persisted.entries.len(), 1);
            assert_eq!(
                persisted.entries[0].request_id.as_str(),
                "concurrent-exit-intent"
            );
        }

        #[test]
        fn missing_target_is_retained_without_blocking_later_valid_intent() {
            use impulse_ops::governed_task::{
                ApprovalPolicy, GovernedActor, GovernedActorKind, GovernedExecutionState,
                GovernedRequestId, GovernedReviewState, GovernedTaskId, GovernedTaskMutation,
                GovernedTaskRun,
            };

            let dir = tempfile::tempdir().expect("tempdir");
            let socket = dir.path().join("missing-target-daemon.sock");
            let outbox_path = dir.path().join("lifecycle-outbox.json");
            let missing_request_id =
                GovernedRequestId::try_new("late-registration-failure").unwrap();
            let valid_request_id = GovernedRequestId::try_new("later-valid-exit").unwrap();
            let valid_task_id = GovernedTaskId::try_new("later-valid-task").unwrap();
            let actor = GovernedActor {
                kind: GovernedActorKind::System,
                id: "desktop-runtime".to_string(),
            };
            let client = UnixDaemonOpsClient::new(socket.clone())
                .with_lifecycle_outbox(Some(outbox_path.clone()));
            client
                .queue_lifecycle_mutation(PendingGovernedLifecycleMutation {
                    project_id: "project".to_string(),
                    task_id: GovernedTaskId::try_new("late-registration-task").unwrap(),
                    request_id: missing_request_id.clone(),
                    mutation: GovernedTaskMutation::MarkLaunchFailed {
                        actor: actor.clone(),
                        reason: "PTY launch failed after ambiguous registration".to_string(),
                    },
                })
                .unwrap();
            client
                .queue_lifecycle_mutation(PendingGovernedLifecycleMutation {
                    project_id: "project".to_string(),
                    task_id: valid_task_id.clone(),
                    request_id: valid_request_id.clone(),
                    mutation: GovernedTaskMutation::MarkRuntimeExited {
                        actor,
                        reason: Some("later runtime exited".to_string()),
                    },
                })
                .unwrap();

            let running = GovernedTaskRun {
                id: valid_task_id,
                revision: 1,
                project_id: "project".to_string(),
                workspace_root: "/tmp/project".to_string(),
                task: "later valid lifecycle intent".to_string(),
                acceptance_criteria: vec![],
                approval_policy: ApprovalPolicy::OperatorRequired,
                role_assignment: None,
                role_compatibility: None,
                runtime_id: "codex".to_string(),
                agent_id: "worker-valid".to_string(),
                session_id: None,
                initial_subject_revision: None,
                execution_state: GovernedExecutionState::Running,
                review_state: GovernedReviewState::AwaitingClaim,
                claims: vec![],
                verifications: vec![],
                supervisor_verdicts: vec![],
                operator_decisions: vec![],
                events: vec![],
                created_at: "2026-07-13T00:00:00Z".to_string(),
                updated_at: "2026-07-13T00:01:00Z".to_string(),
            };

            let listener = UnixListener::bind(&socket).expect("bind missing-target daemon");
            let running_for_server = running.clone();
            let valid_request_for_server = valid_request_id.clone();
            let server = thread::spawn(move || {
                for step in 0..4 {
                    let (mut stream, _) = listener.accept().expect("accept lifecycle query");
                    let mut line = String::new();
                    BufReader::new(stream.try_clone().unwrap())
                        .read_line(&mut line)
                        .unwrap();
                    let request: WorkbenchDaemonRequest = serde_json::from_str(&line).unwrap();
                    match (step, request) {
                        (0, WorkbenchDaemonRequest::GetGovernedTask { .. }) => write_response(
                            &mut stream,
                            &WorkbenchDaemonResponse::Ok {
                                result: serde_json::to_value(
                                    None::<impulse_ops::governed_task::GovernedTaskRun>,
                                )
                                .unwrap(),
                            },
                        ),
                        (1 | 2, WorkbenchDaemonRequest::GetGovernedTask { task_id, .. }) => {
                            assert_eq!(task_id, running_for_server.id);
                            write_response(
                                &mut stream,
                                &WorkbenchDaemonResponse::Ok {
                                    result: serde_json::to_value(Some(&running_for_server))
                                        .unwrap(),
                                },
                            );
                        }
                        (3, WorkbenchDaemonRequest::MutateGovernedTask { request }) => {
                            assert_eq!(request.request_id, valid_request_for_server);
                            let mut exited = running_for_server.clone();
                            exited.revision += 1;
                            exited.execution_state = GovernedExecutionState::RuntimeExited;
                            write_response(
                                &mut stream,
                                &WorkbenchDaemonResponse::Ok {
                                    result: serde_json::to_value(exited).unwrap(),
                                },
                            );
                        }
                        other => panic!("unexpected retained-target request: {other:?}"),
                    }
                }
            });

            let error = client.drain_lifecycle_outbox().unwrap_err();
            server.join().unwrap();
            assert!(error.contains("not visible yet"));
            let persisted = UnixDaemonOpsClient::read_lifecycle_outbox(&outbox_path).unwrap();
            assert_eq!(persisted.entries.len(), 1);
            assert_eq!(persisted.entries[0].request_id, missing_request_id);
            assert!(!persisted
                .entries
                .iter()
                .any(|entry| entry.request_id == valid_request_id));
        }

        #[test]
        fn lifecycle_outbox_serializes_distinct_client_writers() {
            use impulse_ops::governed_task::{
                GovernedActor, GovernedActorKind, GovernedRequestId, GovernedTaskId,
                GovernedTaskMutation,
            };

            let dir = tempfile::tempdir().expect("tempdir");
            let outbox_path = dir.path().join("governed-lifecycle-outbox.json");
            let socket = dir.path().join("unused.sock");
            let first = UnixDaemonOpsClient::new(socket.clone())
                .with_lifecycle_outbox(Some(outbox_path.clone()));
            let second =
                UnixDaemonOpsClient::new(socket).with_lifecycle_outbox(Some(outbox_path.clone()));
            let barrier = Arc::new(std::sync::Barrier::new(3));

            let queue = |request: &str, task: &str| PendingGovernedLifecycleMutation {
                project_id: "project".to_string(),
                task_id: GovernedTaskId::try_new(task).unwrap(),
                request_id: GovernedRequestId::try_new(request).unwrap(),
                mutation: GovernedTaskMutation::MarkRuntimeExited {
                    actor: GovernedActor {
                        kind: GovernedActorKind::System,
                        id: "desktop-runtime".to_string(),
                    },
                    reason: Some("confirmed runtime termination".to_string()),
                },
            };
            let first_pending = queue("outbox-writer-a", "task-writer-a");
            let second_pending = queue("outbox-writer-b", "task-writer-b");

            let first_barrier = Arc::clone(&barrier);
            let first_writer = thread::spawn(move || {
                first_barrier.wait();
                first.queue_lifecycle_mutation(first_pending)
            });
            let second_barrier = Arc::clone(&barrier);
            let second_writer = thread::spawn(move || {
                second_barrier.wait();
                second.queue_lifecycle_mutation(second_pending)
            });
            barrier.wait();

            first_writer.join().unwrap().unwrap();
            second_writer.join().unwrap().unwrap();
            let persisted = UnixDaemonOpsClient::read_lifecycle_outbox(&outbox_path).unwrap();
            assert_eq!(persisted.entries.len(), 2);
            assert!(persisted
                .entries
                .iter()
                .any(|entry| entry.request_id.as_str() == "outbox-writer-a"));
            assert!(persisted
                .entries
                .iter()
                .any(|entry| entry.request_id.as_str() == "outbox-writer-b"));
        }

        const OUTBOX_HELPER_PATH_ENV: &str = "IMPULSE_TEST_OUTBOX_PATH";
        const OUTBOX_HELPER_READY_ENV: &str = "IMPULSE_TEST_OUTBOX_READY";

        #[test]
        fn lifecycle_outbox_subprocess_writer_helper() {
            use impulse_ops::governed_task::{
                GovernedActor, GovernedActorKind, GovernedRequestId, GovernedTaskId,
                GovernedTaskMutation,
            };

            let Ok(outbox_path) = std::env::var(OUTBOX_HELPER_PATH_ENV) else {
                return;
            };
            let ready_path = std::env::var(OUTBOX_HELPER_READY_ENV).unwrap();
            std::fs::write(&ready_path, b"ready").unwrap();
            let pending = PendingGovernedLifecycleMutation {
                project_id: "project".to_string(),
                task_id: GovernedTaskId::try_new("task-subprocess").unwrap(),
                request_id: GovernedRequestId::try_new("outbox-subprocess").unwrap(),
                mutation: GovernedTaskMutation::MarkRuntimeExited {
                    actor: GovernedActor {
                        kind: GovernedActorKind::System,
                        id: "desktop-runtime".to_string(),
                    },
                    reason: Some("confirmed runtime termination".to_string()),
                },
            };
            UnixDaemonOpsClient::new(PathBuf::from("unused.sock"))
                .with_lifecycle_outbox(Some(PathBuf::from(outbox_path)))
                .queue_lifecycle_mutation(pending)
                .unwrap();
        }

        #[test]
        fn lifecycle_outbox_lock_serializes_a_real_subprocess_rmw() {
            use impulse_ops::governed_task::{
                GovernedActor, GovernedActorKind, GovernedRequestId, GovernedTaskId,
                GovernedTaskMutation,
            };
            use std::process::{Command, Stdio};

            let dir = tempfile::tempdir().expect("tempdir");
            let outbox_path = dir.path().join("governed-lifecycle-outbox.json");
            let ready_path = dir.path().join("child-ready");
            let file_guard = UnixDaemonOpsClient::lock_lifecycle_outbox(&outbox_path).unwrap();
            let helper_module = module_path!()
                .strip_prefix("impulse_desktop::")
                .unwrap_or(module_path!());
            let helper_name = format!("{helper_module}::lifecycle_outbox_subprocess_writer_helper");
            let mut child = Command::new(std::env::current_exe().unwrap())
                .arg("--exact")
                .arg(helper_name)
                .env(OUTBOX_HELPER_PATH_ENV, &outbox_path)
                .env(OUTBOX_HELPER_READY_ENV, &ready_path)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn outbox helper process");

            let ready_deadline = Instant::now() + Duration::from_secs(5);
            while !ready_path.exists() && Instant::now() < ready_deadline {
                thread::sleep(Duration::from_millis(10));
            }
            assert!(
                ready_path.exists(),
                "subprocess did not reach the lock boundary"
            );
            thread::sleep(Duration::from_millis(50));
            assert!(
                child.try_wait().unwrap().is_none(),
                "subprocess must block while the parent owns the sibling lock"
            );

            let first = PendingGovernedLifecycleMutation {
                project_id: "project".to_string(),
                task_id: GovernedTaskId::try_new("task-parent").unwrap(),
                request_id: GovernedRequestId::try_new("outbox-parent").unwrap(),
                mutation: GovernedTaskMutation::MarkRuntimeExited {
                    actor: GovernedActor {
                        kind: GovernedActorKind::System,
                        id: "desktop-runtime".to_string(),
                    },
                    reason: Some("confirmed parent runtime termination".to_string()),
                },
            };
            UnixDaemonOpsClient::write_lifecycle_outbox(
                &outbox_path,
                &GovernedLifecycleOutbox {
                    schema_version: GOVERNED_LIFECYCLE_OUTBOX_SCHEMA,
                    entries: vec![first],
                },
            )
            .unwrap();
            drop(file_guard);

            let exit_deadline = Instant::now() + Duration::from_secs(5);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() >= exit_deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("outbox helper did not finish after lock release");
                }
                thread::sleep(Duration::from_millis(10));
            };
            assert!(status.success());

            let persisted = UnixDaemonOpsClient::read_lifecycle_outbox(&outbox_path).unwrap();
            assert_eq!(persisted.entries.len(), 2);
            assert!(persisted
                .entries
                .iter()
                .any(|entry| entry.request_id.as_str() == "outbox-parent"));
            assert!(persisted
                .entries
                .iter()
                .any(|entry| entry.request_id.as_str() == "outbox-subprocess"));
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
                governed_task_id: None,
                governed_task_revision: None,
                rows: 24,
                cols: 80,
                alive: true,
                focused: true,
                status: impulse_ops::AgentStatus::Working {
                    task: "wire daemon truth".to_string(),
                },
                current_task: None,
                role: Some(impulse_ops::AgentRole::Coordinator),
                role_assignment: None,
                role_compatibility: None,
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
            let attachment = attach(
                downstream_trait,
                DesktopDaemonOpsConfig::new(socket, Some(PathBuf::from("/tmp/project")))
                    .with_source_id("desktop-contract")
                    .with_heartbeat_interval(Duration::from_secs(60)),
            )
            .expect("attach daemon ops");
            let sink = attachment.event_sink;
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
            let attachment = attach(
                downstream,
                DesktopDaemonOpsConfig::new(socket, None)
                    .with_source_id("desktop-shutdown")
                    .with_heartbeat_interval(Duration::from_secs(60)),
            )
            .expect("attach daemon ops");
            let sink = attachment.event_sink;
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
            let attachment = attach(
                downstream_trait,
                DesktopDaemonOpsConfig::new(dir.path().join("missing.sock"), None)
                    .with_heartbeat_interval(Duration::from_secs(60)),
            )
            .expect("worker still starts without daemon");
            let sink = attachment.event_sink;
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
) -> Result<DesktopDaemonOpsAttachment, DesktopDaemonOpsStartError> {
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
    use impulse_ops::role_assignment::{
        AgentRoleAssignment, AgentRoleId, EnforcementStrength, RoleCapabilityRequirement,
        RoleCompatibility, RuntimeCapabilityId,
    };
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
            governed_task_id: None,
            governed_task_revision: None,
            rows: 32,
            cols: 100,
            alive: true,
            focused: true,
            status: AgentStatus::Working {
                task: "publish truth".to_string(),
            },
            current_task: None,
            role: Some(AgentRole::Worker { parent_pane_id: 7 }),
            role_assignment: None,
            role_compatibility: None,
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
    fn rich_snapshot_conversion_preserves_task_assignment_and_compatibility() {
        let assignment = AgentRoleAssignment {
            role: AgentRoleId::try_new("builder").unwrap(),
            requirements: vec![RoleCapabilityRequirement {
                capability: RuntimeCapabilityId::try_new("workspace.target").unwrap(),
                minimum_enforcement: EnforcementStrength::Mediated,
                mandatory: true,
            }],
        };
        let compatibility = RoleCompatibility {
            platform: AgentPlatformId::try_new("claude-code").unwrap(),
            role: assignment.role.clone(),
            checks: Vec::new(),
        };
        let mut snapshot = rich_snapshot("claude-live", "/repo");
        snapshot.current_task = Some("typed launch task".to_string());
        snapshot.role_assignment = Some(assignment.clone());
        snapshot.role_compatibility = Some(compatibility.clone());

        let runtime = agent_runtime_from_snapshot(&snapshot);

        assert_eq!(runtime.current_task.as_deref(), Some("typed launch task"));
        assert_eq!(runtime.role_assignment, Some(assignment));
        assert_eq!(runtime.role_compatibility, Some(compatibility));
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
