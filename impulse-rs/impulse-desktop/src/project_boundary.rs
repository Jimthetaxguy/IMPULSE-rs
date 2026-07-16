//! Dynamic project boundary for the Dioxus desktop host.
//!
//! A Finder-launched app has no trustworthy repository cwd. This controller
//! therefore starts disconnected and binds daemon telemetry, governed-task
//! commands, and project memory together only after an exact workspace root is
//! selected. One desktop process owns one project boundary; cross-project
//! routing remains an explicit later contract.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use impulse_ops::governed_task::{
    GovernedRequestId, GovernedTaskId, GovernedTaskMutation, GovernedTaskMutationRequest,
    GovernedTaskRegistration, GovernedTaskRun,
};

use crate::daemon_ops::{
    attach_desktop_daemon_ops, DesktopDaemonOpsAttachment, DesktopDaemonOpsConfig,
};
use crate::daemon_sidecar::{DesktopDaemonSidecar, DesktopDaemonSidecarMode, DesktopInstanceLease};
use crate::desktop_shutdown::DesktopShutdownCoordinator;
use crate::runtime::{
    DesktopEvent, DesktopEventSink, GovernedRoutingMetadata, GovernedTaskGateway,
};

#[derive(Clone)]
struct CommitGatedDesktopEventSink {
    downstream: Arc<dyn DesktopEventSink>,
    state: Arc<Mutex<CommitGateState>>,
}

enum CommitGateState {
    Pending(Vec<DesktopEvent>),
    Committed,
}

impl CommitGatedDesktopEventSink {
    fn new(downstream: Arc<dyn DesktopEventSink>) -> Self {
        Self {
            downstream,
            state: Arc::new(Mutex::new(CommitGateState::Pending(Vec::new()))),
        }
    }

    fn commit(&self) {
        loop {
            let pending = {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match &mut *state {
                    CommitGateState::Pending(events) if events.is_empty() => {
                        *state = CommitGateState::Committed;
                        return;
                    }
                    CommitGateState::Pending(events) => std::mem::take(events),
                    CommitGateState::Committed => return,
                }
            };
            for event in pending {
                self.downstream.emit(event);
            }
        }
    }
}

impl DesktopEventSink for CommitGatedDesktopEventSink {
    fn emit(&self, event: DesktopEvent) {
        let immediate = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match &mut *state {
                CommitGateState::Pending(events) => {
                    events.push(event);
                    None
                }
                CommitGateState::Committed => Some(event),
            }
        };
        if let Some(event) = immediate {
            self.downstream.emit(event);
        }
    }
}

#[derive(Clone)]
pub struct SwitchableDesktopEventSink {
    current: Arc<RwLock<Arc<dyn DesktopEventSink>>>,
}

impl SwitchableDesktopEventSink {
    pub fn new(initial: Arc<dyn DesktopEventSink>) -> Self {
        Self {
            current: Arc::new(RwLock::new(initial)),
        }
    }

    fn install(&self, sink: Arc<dyn DesktopEventSink>) {
        *self
            .current
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = sink;
    }
}

impl DesktopEventSink for SwitchableDesktopEventSink {
    fn emit(&self, event: DesktopEvent) {
        let sink = self
            .current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        sink.emit(event);
    }
}

#[derive(Clone, Default)]
pub struct SwitchableGovernedTaskGateway {
    current: Arc<RwLock<Option<BoundGovernedTaskGateway>>>,
}

#[derive(Clone)]
struct BoundGovernedTaskGateway {
    project_root: PathBuf,
    project_id: String,
    inner: Arc<dyn GovernedTaskGateway>,
}

impl BoundGovernedTaskGateway {
    fn validate_project_id(&self, project_id: &str) -> Result<(), String> {
        if project_id == self.project_id {
            return Ok(());
        }
        Err(format!(
            "governed task project `{project_id}` does not match desktop project `{}`",
            self.project_id
        ))
    }

    fn validate_workspace_root(&self, workspace_root: &str) -> Result<(), String> {
        let requested = canonical_project_root(Path::new(workspace_root))?;
        if requested == self.project_root {
            return Ok(());
        }
        Err(format!(
            "governed task workspace `{}` does not match desktop project `{}`",
            requested.display(),
            self.project_root.display()
        ))
    }
}

impl SwitchableGovernedTaskGateway {
    fn install(
        &self,
        project_root: PathBuf,
        project_id: String,
        gateway: Arc<dyn GovernedTaskGateway>,
    ) {
        *self
            .current
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(BoundGovernedTaskGateway {
            project_root,
            project_id,
            inner: gateway,
        });
    }

    fn current(&self) -> Result<BoundGovernedTaskGateway, String> {
        self.current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(|| {
                "governed project boundary is disconnected; select a project and retry".to_string()
            })
    }
}

impl GovernedTaskGateway for SwitchableGovernedTaskGateway {
    fn register(&self, registration: GovernedTaskRegistration) -> Result<GovernedTaskRun, String> {
        let gateway = self.current()?;
        gateway.validate_project_id(&registration.project_id)?;
        gateway.validate_workspace_root(&registration.workspace_root)?;
        gateway.inner.register(registration)
    }

    fn mutate(&self, request: GovernedTaskMutationRequest) -> Result<GovernedTaskRun, String> {
        let gateway = self.current()?;
        gateway.validate_project_id(&request.project_id)?;
        gateway.inner.mutate(request)
    }

    fn mutate_current(
        &self,
        project_id: &str,
        task_id: &GovernedTaskId,
        request_id: GovernedRequestId,
        mutation: GovernedTaskMutation,
    ) -> Result<GovernedTaskRun, String> {
        let gateway = self.current()?;
        gateway.validate_project_id(project_id)?;
        gateway
            .inner
            .mutate_current(project_id, task_id, request_id, mutation)
    }

    fn routing_metadata(&self) -> Option<GovernedRoutingMetadata> {
        self.current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .and_then(|gateway| gateway.inner.routing_metadata())
    }
}

#[derive(Clone, Default)]
pub struct ProjectMemoryScope {
    root: Arc<RwLock<Option<PathBuf>>>,
}

impl ProjectMemoryScope {
    pub fn connected(root: PathBuf) -> Self {
        Self {
            root: Arc::new(RwLock::new(Some(root))),
        }
    }

    pub fn root(&self) -> Result<PathBuf, String> {
        self.root
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(|| {
                "project memory is unavailable until a project daemon boundary is connected"
                    .to_string()
            })
    }

    pub(crate) fn install(&self, root: PathBuf) {
        *self
            .root
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(root);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopProjectBoundary {
    pub project_root: PathBuf,
    pub memory_root: PathBuf,
    pub socket_path: PathBuf,
    pub daemon_mode: DesktopDaemonSidecarMode,
}

pub trait DesktopProjectBoundaryConnector: Send + Sync {
    fn connect_project(&self, project_root: &Path) -> Result<DesktopProjectBoundary, String>;
}

#[derive(Clone)]
pub struct DesktopProjectBoundaryController {
    current: Arc<Mutex<Option<DesktopProjectBoundary>>>,
    downstream: Arc<dyn DesktopEventSink>,
    runtime_sink: SwitchableDesktopEventSink,
    gateway: SwitchableGovernedTaskGateway,
    memory_scope: ProjectMemoryScope,
    shutdown: DesktopShutdownCoordinator,
}

impl DesktopProjectBoundaryController {
    pub fn new(
        downstream: Arc<dyn DesktopEventSink>,
        runtime_sink: SwitchableDesktopEventSink,
        gateway: SwitchableGovernedTaskGateway,
        memory_scope: ProjectMemoryScope,
        shutdown: DesktopShutdownCoordinator,
    ) -> Self {
        Self {
            current: Arc::new(Mutex::new(None)),
            downstream,
            runtime_sink,
            gateway,
            memory_scope,
            shutdown,
        }
    }

    pub fn connect_project(&self, project_root: &Path) -> Result<DesktopProjectBoundary, String> {
        let project_root = canonical_project_root(project_root)?;
        self.connect_config(DesktopDaemonOpsConfig::for_project(project_root))
    }

    pub fn connect_config(
        &self,
        mut config: DesktopDaemonOpsConfig,
    ) -> Result<DesktopProjectBoundary, String> {
        let requested_root = config.project_root.as_deref().ok_or_else(|| {
            "desktop daemon configuration is missing its project root".to_string()
        })?;
        let project_root = canonical_project_root(requested_root)?;
        validate_project_state_layout(&project_root)?;
        config = DesktopDaemonOpsConfig::for_project(project_root.clone())
            .with_source_id(config.source_id)
            .with_heartbeat_interval(config.heartbeat_interval);

        let mut current = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(boundary) = current.as_ref() {
            if boundary.project_root == project_root {
                return Ok(boundary.clone());
            }
            return Err(format!(
                "desktop is already bound to project `{}`; restart before switching to `{}`",
                boundary.project_root.display(),
                project_root.display()
            ));
        }

        let lease = DesktopInstanceLease::acquire(&config.socket_path)
            .map_err(|error| format!("acquire project daemon lease: {error}"))?;
        let sidecar = DesktopDaemonSidecar::ensure(&config)
            .map_err(|error| format!("connect project daemon: {error}"))?;
        let identity = sidecar
            .attest_project_root(&project_root)
            .map_err(|error| format!("attest project daemon identity: {error}"))?;
        let daemon_mode = sidecar.mode();
        let gated_downstream = Arc::new(CommitGatedDesktopEventSink::new(Arc::clone(
            &self.downstream,
        )));
        let DesktopDaemonOpsAttachment {
            event_sink,
            governed_task_gateway,
            shutdown_handle,
        } = attach_desktop_daemon_ops(gated_downstream.clone(), config.clone())
            .map_err(|error| format!("attach project daemon operations: {error}"))?;

        self.shutdown
            .install_daemon_boundary(shutdown_handle, sidecar, lease)?;
        self.runtime_sink.install(event_sink);

        let memory_root = project_root.join(".impulse");
        self.memory_scope.install(memory_root.clone());
        let boundary = DesktopProjectBoundary {
            project_root,
            memory_root,
            socket_path: config.socket_path,
            daemon_mode,
        };
        *current = Some(boundary.clone());
        self.gateway.install(
            identity.project_root,
            identity.project_id,
            governed_task_gateway,
        );
        gated_downstream.commit();
        Ok(boundary)
    }

    pub fn current(&self) -> Option<DesktopProjectBoundary> {
        self.current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl DesktopProjectBoundaryConnector for DesktopProjectBoundaryController {
    fn connect_project(&self, project_root: &Path) -> Result<DesktopProjectBoundary, String> {
        DesktopProjectBoundaryController::connect_project(self, project_root)
    }
}

fn canonical_project_root(project_root: &Path) -> Result<PathBuf, String> {
    if !project_root.is_absolute() {
        return Err(format!(
            "project root `{}` must be absolute",
            project_root.display()
        ));
    }
    let canonical = project_root.canonicalize().map_err(|error| {
        format!(
            "project root `{}` must resolve to an existing directory: {error}",
            project_root.display()
        )
    })?;
    if !canonical.is_dir() {
        return Err(format!(
            "project root `{}` is not a directory",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn validate_project_state_layout(project_root: &Path) -> Result<(), String> {
    let impulse_root = project_root.join(".impulse");
    validate_local_directory_if_present("project state", &impulse_root, project_root)?;

    let sockets = impulse_root.join("sockets");
    validate_local_directory_if_present("daemon socket directory", &sockets, &impulse_root)?;

    let socket_path = sockets.join("impulse.sock");
    let lifecycle_outbox = impulse_root.join("DESKTOP_GOVERNED_LIFECYCLE_OUTBOX.json");
    for (label, path) in [
        ("daemon socket", socket_path.clone()),
        ("daemon pid marker", socket_path.with_extension("pid")),
        (
            "daemon lifecycle lock",
            socket_path.with_extension("daemon.lock"),
        ),
        (
            "desktop instance lock",
            socket_path.with_extension("desktop.lock"),
        ),
        ("governed lifecycle outbox", lifecycle_outbox.clone()),
        (
            "governed lifecycle outbox lock",
            lifecycle_outbox.with_extension("lock"),
        ),
    ] {
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "{label} `{}` must not be a symlink; project activation made no changes",
                    path.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!("inspect {label} `{}`: {error}", path.display()));
            }
        }
    }
    Ok(())
}

fn validate_local_directory_if_present(
    label: &str,
    path: &Path,
    expected_parent: &Path,
) -> Result<(), String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("inspect {label} `{}`: {error}", path.display())),
    };
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "{label} `{}` must be a real directory inside `{}`; project activation made no changes",
            path.display(),
            expected_parent.display()
        ));
    }
    if !metadata.is_dir() {
        return Err(format!(
            "{label} `{}` exists but is not a directory; project activation made no changes",
            path.display()
        ));
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("canonicalize {label} `{}`: {error}", path.display()))?;
    if canonical.parent() != Some(expected_parent) {
        return Err(format!(
            "{label} `{}` resolves outside `{}`; project activation made no changes",
            path.display(),
            expected_parent.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct RecordingGateway {
        calls: AtomicUsize,
    }

    #[derive(Default)]
    struct RecordingSink {
        names: Mutex<Vec<&'static str>>,
    }

    impl DesktopEventSink for RecordingSink {
        fn emit(&self, event: DesktopEvent) {
            self.names
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(event.name());
        }
    }

    impl GovernedTaskGateway for RecordingGateway {
        fn register(
            &self,
            _registration: GovernedTaskRegistration,
        ) -> Result<GovernedTaskRun, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err("forwarded registration".to_string())
        }

        fn mutate(&self, _request: GovernedTaskMutationRequest) -> Result<GovernedTaskRun, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err("forwarded mutation".to_string())
        }

        fn mutate_current(
            &self,
            _project_id: &str,
            _task_id: &GovernedTaskId,
            _request_id: GovernedRequestId,
            _mutation: GovernedTaskMutation,
        ) -> Result<GovernedTaskRun, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err("forwarded current mutation".to_string())
        }
    }

    #[test]
    fn disconnected_memory_scope_fails_closed() {
        let scope = ProjectMemoryScope::default();
        assert!(scope.root().is_err());
    }

    #[test]
    fn commit_gate_buffers_then_preserves_fifo_delivery() {
        let downstream = Arc::new(RecordingSink::default());
        let gate = CommitGatedDesktopEventSink::new(downstream.clone());
        gate.emit(DesktopEvent::OpsUpdate {
            payload: serde_json::json!({}),
        });
        gate.emit(DesktopEvent::OpsConnectionUpdate {
            connected: true,
            error: None,
        });
        assert!(downstream.names.lock().unwrap().is_empty());

        gate.commit();
        gate.emit(DesktopEvent::TerminalExit {
            agent_id: "agent-1".to_string(),
        });

        assert_eq!(
            *downstream.names.lock().unwrap(),
            vec!["ops_update", "ops_connection_update", "terminal_exit"]
        );
    }

    #[test]
    fn connected_memory_scope_returns_exact_project_root() {
        let scope = ProjectMemoryScope::connected(PathBuf::from("/repo/.impulse"));
        assert_eq!(scope.root().unwrap(), PathBuf::from("/repo/.impulse"));
    }

    #[test]
    fn switchable_gateway_reports_disconnected_before_install() {
        let gateway = SwitchableGovernedTaskGateway::default();
        assert!(gateway.routing_metadata().is_none());
        assert!(matches!(
            gateway.current(),
            Err(message) if message.contains("disconnected")
        ));
    }

    #[test]
    fn bound_gateway_rejects_cross_project_registration_before_forwarding() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().join("project-a");
        let other_root = dir.path().join("project-b");
        std::fs::create_dir_all(&project_root).expect("create project A");
        std::fs::create_dir_all(&other_root).expect("create project B");
        let inner = Arc::new(RecordingGateway::default());
        let gateway = SwitchableGovernedTaskGateway::default();
        gateway.install(
            project_root.canonicalize().unwrap(),
            "project-a".to_string(),
            inner.clone(),
        );
        let registration = GovernedTaskRegistration::builder(
            "request-1",
            "task-1",
            "project-b",
            other_root.display().to_string(),
            "cross project task",
            "agent-1",
            "shell",
        )
        .build()
        .expect("valid registration");

        let error = gateway
            .register(registration)
            .expect_err("cross-project registration must fail closed");
        assert!(error.contains("does not match desktop project"));
        assert_eq!(inner.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn bound_gateway_rejects_wrong_project_id_for_exact_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().join("project-a");
        std::fs::create_dir_all(&project_root).expect("create project A");
        let inner = Arc::new(RecordingGateway::default());
        let gateway = SwitchableGovernedTaskGateway::default();
        gateway.install(
            project_root.canonicalize().unwrap(),
            "project-a".to_string(),
            inner.clone(),
        );
        let registration = GovernedTaskRegistration::builder(
            "request-1",
            "task-1",
            "wrong-project",
            project_root.display().to_string(),
            "wrong identity task",
            "agent-1",
            "shell",
        )
        .build()
        .expect("valid registration");

        assert!(gateway.register(registration).is_err());
        assert_eq!(inner.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn bound_gateway_forwards_exact_project_registration() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().join("project-a");
        std::fs::create_dir_all(&project_root).expect("create project A");
        let inner = Arc::new(RecordingGateway::default());
        let gateway = SwitchableGovernedTaskGateway::default();
        gateway.install(
            project_root.canonicalize().unwrap(),
            "project-a".to_string(),
            inner.clone(),
        );
        let registration = GovernedTaskRegistration::builder(
            "request-1",
            "task-1",
            "project-a",
            project_root.display().to_string(),
            "exact project task",
            "agent-1",
            "shell",
        )
        .build()
        .expect("valid registration");

        let error = gateway
            .register(registration)
            .expect_err("recording gateway returns a sentinel error");
        assert_eq!(error, "forwarded registration");
        assert_eq!(inner.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn bound_gateway_rejects_cross_project_mutation_before_forwarding() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().join("project-a");
        std::fs::create_dir_all(&project_root).expect("create project A");
        let inner = Arc::new(RecordingGateway::default());
        let gateway = SwitchableGovernedTaskGateway::default();
        gateway.install(
            project_root.canonicalize().unwrap(),
            "project-a".to_string(),
            inner.clone(),
        );
        let request = GovernedTaskMutationRequest {
            request_id: GovernedRequestId::try_new("request-1").expect("valid request id"),
            project_id: "project-b".to_string(),
            task_id: GovernedTaskId::try_new("task-1").expect("valid task id"),
            expected_revision: 0,
            mutation: GovernedTaskMutation::MarkRunning {
                actor: impulse_ops::governed_task::GovernedActor {
                    kind: impulse_ops::governed_task::GovernedActorKind::System,
                    id: "desktop".to_string(),
                },
            },
        };

        assert!(gateway.mutate(request).is_err());
        assert_eq!(inner.calls.load(Ordering::SeqCst), 0);
    }

    #[cfg(unix)]
    #[test]
    fn controller_rejects_external_state_symlink_before_any_mutation() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().join("project");
        let external_state = dir.path().join("external-state");
        std::fs::create_dir_all(&project_root).expect("create project root");
        std::fs::create_dir_all(&external_state).expect("create external state root");
        symlink(&external_state, project_root.join(".impulse"))
            .expect("link project state outside project");

        let downstream: Arc<dyn DesktopEventSink> = Arc::new(RecordingSink::default());
        let runtime_sink = SwitchableDesktopEventSink::new(Arc::clone(&downstream));
        let gateway = SwitchableGovernedTaskGateway::default();
        let memory_scope = ProjectMemoryScope::default();
        let runtime = Arc::new(crate::runtime::DesktopRuntime::default());
        let shutdown = DesktopShutdownCoordinator::new(
            runtime,
            None,
            crate::daemon_sidecar::DesktopDaemonSidecarHandle::default(),
            None,
        );
        let controller = DesktopProjectBoundaryController::new(
            downstream,
            runtime_sink,
            gateway.clone(),
            memory_scope.clone(),
            shutdown,
        );

        let error = controller
            .connect_project(&project_root)
            .expect_err("external state symlink must fail before activation");
        assert!(error.contains("must be a real directory inside"));
        assert!(controller.current().is_none());
        assert!(gateway.current().is_err());
        assert!(memory_scope.root().is_err());
        assert_eq!(
            std::fs::read_dir(&external_state)
                .expect("read untouched external state")
                .count(),
            0,
            "activation must not create locks, sockets, or daemon files outside the project"
        );
    }

    #[cfg(unix)]
    #[test]
    fn activation_rejects_lifecycle_outbox_symlinks_before_use() {
        use std::os::unix::fs::symlink;

        for relative_path in [
            "DESKTOP_GOVERNED_LIFECYCLE_OUTBOX.json",
            "DESKTOP_GOVERNED_LIFECYCLE_OUTBOX.lock",
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            let project_root = dir.path().join("project");
            let impulse_root = project_root.join(".impulse");
            let external = dir.path().join("external-target");
            std::fs::create_dir_all(&impulse_root).expect("create project state root");
            std::fs::write(&external, b"must remain untouched").expect("create external target");
            symlink(&external, impulse_root.join(relative_path)).expect("create hostile symlink");
            let canonical_project_root = project_root.canonicalize().expect("canonical project");

            let error = validate_project_state_layout(&canonical_project_root)
                .expect_err("project activation must reject lifecycle symlinks");
            assert!(error.contains("must not be a symlink"), "{error}");
            assert_eq!(
                std::fs::read(&external).expect("read external target"),
                b"must remain untouched"
            );
        }
    }
}
