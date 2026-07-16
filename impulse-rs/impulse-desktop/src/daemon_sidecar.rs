//! Lifecycle owner for the daemon companion used by the packaged desktop app.
//!
//! The desktop may attach to an operator-owned daemon or start its packaged
//! `impulse-rs` sibling. Only the process started here is reaped on desktop
//! shutdown; an existing daemon is never stopped or otherwise adopted.

#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(unix)]
use impulse_ops::{
    ProjectOpsSnapshot, WorkbenchDaemonRequest, WorkbenchDaemonResponse, DAEMON_PROTOCOL_VERSION,
};
use thiserror::Error;

use crate::daemon_ops::DesktopDaemonOpsConfig;

pub const DEFAULT_DAEMON_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const DAEMON_STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(25);
const DAEMON_HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_DAEMON_HANDSHAKE_RESPONSE_BYTES: usize = 8 * 1024;
const DAEMON_PROJECT_ATTESTATION_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_DAEMON_PROJECT_ATTESTATION_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const DAEMON_SHUTDOWN_GRACE_TIMEOUT: Duration = Duration::from_secs(5);
const DAEMON_SOCKET_NAME: &str = "impulse.sock";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopDaemonSidecarMode {
    Existing,
    Spawned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopDaemonProjectIdentity {
    pub project_id: String,
    pub project_root: PathBuf,
    pub impulse_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopDaemonSidecarShutdownOutcome {
    pub mode: DesktopDaemonSidecarMode,
    pub terminate_reap: DesktopDaemonTerminateReapOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopDaemonTerminateReapOutcome {
    NotRequired,
    AlreadyComplete,
    Reaped { pid: u32, status: String },
    Failed { pid: u32, message: String },
}

#[derive(Debug, Error)]
pub enum DesktopDaemonSidecarError {
    #[error("desktop daemon sidecar requires Unix-domain sockets")]
    UnsupportedPlatform,
    #[error(
        "daemon socket `{socket_path}` is not below a `sockets` directory; cannot derive the Impulse state root"
    )]
    InvalidSocketPath { socket_path: PathBuf },
    #[error(
        "unsupported daemon socket filename for `{socket_path}`; the packaged companion requires `{expected}`"
    )]
    UnsupportedSocketFilename {
        socket_path: PathBuf,
        expected: &'static str,
    },
    #[error("failed to prepare desktop instance lock `{lock_path}`: {source}")]
    InstanceLockIo {
        lock_path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "another Impulse Desktop instance already owns daemon socket `{socket_path}` (lock: `{lock_path}`)"
    )]
    InstanceAlreadyRunning {
        socket_path: PathBuf,
        lock_path: PathBuf,
    },
    #[error("daemon at `{socket_path}` failed the Impulse protocol handshake: {reason}")]
    Handshake {
        socket_path: PathBuf,
        reason: String,
    },
    #[error("daemon at `{socket_path}` could not attest its project identity: {reason}")]
    ProjectIdentitySnapshot {
        socket_path: PathBuf,
        reason: String,
    },
    #[error(
        "daemon at `{socket_path}` belongs to project `{observed_root}`, not requested project `{expected_root}`"
    )]
    ProjectIdentityMismatch {
        socket_path: PathBuf,
        expected_root: PathBuf,
        observed_root: PathBuf,
    },
    #[error(
        "daemon at `{socket_path}` uses Impulse state `{observed_impulse_root}`, not expected state `{expected_impulse_root}`"
    )]
    ProjectStatePathMismatch {
        socket_path: PathBuf,
        expected_impulse_root: PathBuf,
        observed_impulse_root: PathBuf,
    },
    #[error(
        "project `{project_root}` resolves its `.impulse` state outside the project at `{observed_impulse_root}`"
    )]
    ProjectStateOutsideProject {
        project_root: PathBuf,
        observed_impulse_root: PathBuf,
    },
    #[error(
        "daemon at `{socket_path}` reports project id `{observed_project_id}`, not expected id `{expected_project_id}`"
    )]
    ProjectIdMismatch {
        socket_path: PathBuf,
        expected_project_id: String,
        observed_project_id: String,
    },
    #[error(
        "Impulse control CLI is unavailable; set IMPULSE_CONTROL_CLI, package an executable `impulse-rs` sibling, or add it to PATH"
    )]
    ControlCliUnavailable,
    #[error("failed to spawn daemon companion `{executable}`: {source}")]
    Spawn {
        executable: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to inspect daemon companion process: {0}")]
    Inspect(#[source] std::io::Error),
    #[error("daemon companion exited before `{socket_path}` became ready: {status}")]
    Exited {
        socket_path: PathBuf,
        status: ExitStatus,
    },
    #[error("daemon companion did not make `{socket_path}` ready within {timeout_ms} ms")]
    StartupTimeout {
        socket_path: PathBuf,
        timeout_ms: u128,
    },
}

/// Process-lifetime ownership of the desktop instance assigned to one daemon
/// socket.
///
/// The lock file is intentionally retained after release. Removing a flocked
/// path would let another process create and lock a different inode while the
/// original descriptor is still held.
#[derive(Debug)]
pub struct DesktopInstanceLease {
    #[cfg(unix)]
    file: File,
    lock_path: PathBuf,
}

impl DesktopInstanceLease {
    /// Acquire the single-desktop lease for `socket_path` without blocking.
    pub fn acquire(socket_path: &Path) -> Result<Self, DesktopDaemonSidecarError> {
        validate_socket_path(socket_path)?;
        Self::acquire_platform(socket_path)
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    #[cfg(unix)]
    fn acquire_platform(socket_path: &Path) -> Result<Self, DesktopDaemonSidecarError> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let socket_dir =
            socket_path
                .parent()
                .ok_or_else(|| DesktopDaemonSidecarError::InvalidSocketPath {
                    socket_path: socket_path.to_path_buf(),
                })?;
        std::fs::create_dir_all(socket_dir).map_err(|source| {
            DesktopDaemonSidecarError::InstanceLockIo {
                lock_path: socket_dir.to_path_buf(),
                source,
            }
        })?;
        let lock_path = socket_path.with_extension("desktop.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&lock_path)
            .map_err(|source| DesktopDaemonSidecarError::InstanceLockIo {
                lock_path: lock_path.clone(),
                source,
            })?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|source| DesktopDaemonSidecarError::InstanceLockIo {
                lock_path: lock_path.clone(),
                source,
            })?;

        loop {
            // SAFETY: `file` remains owned by the returned lease, so the file
            // descriptor is valid for both this call and the lock lifetime.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                break;
            }
            let source = std::io::Error::last_os_error();
            if source.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if source.kind() == std::io::ErrorKind::WouldBlock {
                return Err(DesktopDaemonSidecarError::InstanceAlreadyRunning {
                    socket_path: socket_path.to_path_buf(),
                    lock_path,
                });
            }
            return Err(DesktopDaemonSidecarError::InstanceLockIo { lock_path, source });
        }

        Ok(Self { file, lock_path })
    }

    #[cfg(not(unix))]
    fn acquire_platform(_socket_path: &Path) -> Result<Self, DesktopDaemonSidecarError> {
        Err(DesktopDaemonSidecarError::UnsupportedPlatform)
    }
}

#[cfg(unix)]
impl Drop for DesktopInstanceLease {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;

        // SAFETY: the descriptor remains valid until this drop completes. A
        // failure is non-actionable because closing the file also releases the
        // advisory lock.
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

/// RAII handle that distinguishes an attached daemon from a desktop-owned
/// daemon. Dropping an `Existing` handle is intentionally a no-op.
pub struct DesktopDaemonSidecar {
    mode: DesktopDaemonSidecarMode,
    child: Option<Child>,
    socket_path: PathBuf,
}

/// Proof that an owned process has been observed in an exited state and
/// reaped. Runtime-file cleanup accepts this token instead of a bare PID so no
/// call site can unlink daemon files after an unconfirmed termination attempt.
struct ReapedChild {
    pid: u32,
    status: ExitStatus,
}

/// Cloneable lifecycle handle shared by the Dioxus event loop and host bridge.
///
/// Dioxus's native launcher never returns, so a sidecar left on `main`'s stack
/// cannot rely on ordinary RAII at window shutdown. This handle lets every
/// native close path take the same sidecar exactly once and reap it before the
/// event loop exits. An attached [`DesktopDaemonSidecarMode::Existing`] daemon
/// remains a no-op when taken.
#[derive(Clone, Default)]
pub struct DesktopDaemonSidecarHandle {
    sidecar: Arc<Mutex<Option<DesktopDaemonSidecar>>>,
}

impl DesktopDaemonSidecarHandle {
    pub fn new(sidecar: Option<DesktopDaemonSidecar>) -> Self {
        Self {
            sidecar: Arc::new(Mutex::new(sidecar)),
        }
    }

    pub fn install(&self, sidecar: DesktopDaemonSidecar) -> Result<(), String> {
        let mut current = self
            .sidecar
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if current.is_some() {
            return Err("desktop daemon sidecar is already installed".to_string());
        }
        *current = Some(sidecar);
        Ok(())
    }

    /// Reap the desktop-owned daemon at most once.
    ///
    /// Returns the typed termination/reap outcome from the first shutdown
    /// request and `None` for later requests. The sidecar is dropped outside
    /// the mutex so process termination and `wait()` never block another close
    /// observer while the lock is held.
    pub fn shutdown(&self) -> Option<DesktopDaemonSidecarShutdownOutcome> {
        let sidecar = match self.sidecar.lock() {
            Ok(mut guard) => guard.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        let mut sidecar = sidecar?;
        Some(sidecar.shutdown())
    }
}

impl DesktopDaemonSidecar {
    pub fn ensure(config: &DesktopDaemonOpsConfig) -> Result<Self, DesktopDaemonSidecarError> {
        Self::ensure_with_timeout(config, DEFAULT_DAEMON_STARTUP_TIMEOUT)
    }

    pub fn mode(&self) -> DesktopDaemonSidecarMode {
        self.mode
    }

    pub fn spawned(&self) -> bool {
        self.mode == DesktopDaemonSidecarMode::Spawned
    }

    /// Prove that the daemon behind this socket owns the exact selected
    /// repository before desktop telemetry, memory, or governed-task commands
    /// are attached to it.
    pub fn attest_project_root(
        &self,
        expected_root: &Path,
    ) -> Result<DesktopDaemonProjectIdentity, DesktopDaemonSidecarError> {
        attest_daemon_project(
            &self.socket_path,
            expected_root,
            DAEMON_PROJECT_ATTESTATION_TIMEOUT,
        )
    }

    /// Reap an owned child and remove its stale runtime files.
    ///
    /// This operation is idempotent. Attached daemons have no child handle and
    /// are deliberately left untouched.
    pub fn shutdown(&mut self) -> DesktopDaemonSidecarShutdownOutcome {
        let terminate_reap = match self.child.take() {
            Some(mut child) => {
                let pid = child.id();
                match terminate_and_reap(&mut child, DAEMON_SHUTDOWN_GRACE_TIMEOUT) {
                    Ok(reaped) => {
                        cleanup_reaped_runtime_files(&self.socket_path, &reaped);
                        DesktopDaemonTerminateReapOutcome::Reaped {
                            pid: reaped.pid,
                            status: reaped.status.to_string(),
                        }
                    }
                    Err(error) => DesktopDaemonTerminateReapOutcome::Failed {
                        pid,
                        message: error.to_string(),
                    },
                }
            }
            None if self.mode == DesktopDaemonSidecarMode::Existing => {
                DesktopDaemonTerminateReapOutcome::NotRequired
            }
            None => DesktopDaemonTerminateReapOutcome::AlreadyComplete,
        };
        DesktopDaemonSidecarShutdownOutcome {
            mode: self.mode,
            terminate_reap,
        }
    }

    #[cfg(unix)]
    fn ensure_with_timeout(
        config: &DesktopDaemonOpsConfig,
        timeout: Duration,
    ) -> Result<Self, DesktopDaemonSidecarError> {
        let impulse_root = validate_socket_path(&config.socket_path)?;
        if config.socket_path.exists() {
            match probe_daemon(&config.socket_path, DAEMON_HANDSHAKE_TIMEOUT) {
                DaemonProbe::Ready => {
                    return Ok(Self {
                        mode: DesktopDaemonSidecarMode::Existing,
                        child: None,
                        socket_path: config.socket_path.clone(),
                    });
                }
                DaemonProbe::Unavailable => {}
                DaemonProbe::Incompatible(reason) => {
                    return Err(DesktopDaemonSidecarError::Handshake {
                        socket_path: config.socket_path.clone(),
                        reason,
                    });
                }
            }
        }
        let executable = crate::daemon_ops::resolve_desktop_control_cli()
            .ok_or(DesktopDaemonSidecarError::ControlCliUnavailable)?;
        let command = owned_daemon_command(&executable, &impulse_root, std::process::id());
        Self::spawn_and_wait(config, timeout, executable, command)
    }

    #[cfg(unix)]
    fn spawn_and_wait(
        config: &DesktopDaemonOpsConfig,
        timeout: Duration,
        executable: PathBuf,
        mut command: Command,
    ) -> Result<Self, DesktopDaemonSidecarError> {
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| DesktopDaemonSidecarError::Spawn {
                executable: executable.clone(),
                source,
            })?;
        let child_pid = child.id();

        let deadline = Instant::now() + timeout;
        loop {
            let probe = probe_daemon(&config.socket_path, DAEMON_HANDSHAKE_TIMEOUT);
            match inspect_child(&mut child) {
                Ok(Some(status)) => {
                    let reaped = ReapedChild {
                        pid: child_pid,
                        status,
                    };
                    if probe == DaemonProbe::Ready {
                        // Another process won the startup race. Its verified
                        // daemon is operator-owned, and our child is reaped.
                        cleanup_reaped_runtime_files(&config.socket_path, &reaped);
                        return Ok(Self {
                            mode: DesktopDaemonSidecarMode::Existing,
                            child: None,
                            socket_path: config.socket_path.clone(),
                        });
                    }
                    cleanup_reaped_runtime_files(&config.socket_path, &reaped);
                    return Err(DesktopDaemonSidecarError::Exited {
                        socket_path: config.socket_path.clone(),
                        status: reaped.status,
                    });
                }
                Ok(None) => {}
                Err(source) => {
                    if let Ok(reaped) =
                        terminate_and_reap(&mut child, DAEMON_SHUTDOWN_GRACE_TIMEOUT)
                    {
                        cleanup_reaped_runtime_files(&config.socket_path, &reaped);
                    }
                    return Err(DesktopDaemonSidecarError::Inspect(source));
                }
            }

            if probe == DaemonProbe::Ready {
                match daemon_pid_marker(&config.socket_path) {
                    Some(pid) if pid == child_pid => {
                        return Ok(Self {
                            mode: DesktopDaemonSidecarMode::Spawned,
                            child: Some(child),
                            socket_path: config.socket_path.clone(),
                        });
                    }
                    Some(_) => {
                        // A different verified daemon won the bind/PID race.
                        // Reap our still-running contender before attaching.
                        let reaped = terminate_and_reap(&mut child, DAEMON_SHUTDOWN_GRACE_TIMEOUT)
                            .map_err(DesktopDaemonSidecarError::Inspect)?;
                        cleanup_reaped_runtime_files(&config.socket_path, &reaped);
                        return Ok(Self {
                            mode: DesktopDaemonSidecarMode::Existing,
                            child: None,
                            socket_path: config.socket_path.clone(),
                        });
                    }
                    None => {
                        // Binding precedes PID publication. Keep polling until
                        // readiness and ownership agree or startup times out.
                    }
                }
            }

            if Instant::now() >= deadline {
                if let Ok(reaped) = terminate_and_reap(&mut child, DAEMON_SHUTDOWN_GRACE_TIMEOUT) {
                    cleanup_reaped_runtime_files(&config.socket_path, &reaped);
                }
                return Err(DesktopDaemonSidecarError::StartupTimeout {
                    socket_path: config.socket_path.clone(),
                    timeout_ms: timeout.as_millis(),
                });
            }
            std::thread::sleep(DAEMON_STARTUP_POLL_INTERVAL);
        }
    }

    #[cfg(not(unix))]
    fn ensure_with_timeout(
        config: &DesktopDaemonOpsConfig,
        timeout: Duration,
    ) -> Result<Self, DesktopDaemonSidecarError> {
        let _ = (config, timeout);
        Err(DesktopDaemonSidecarError::UnsupportedPlatform)
    }
}

fn owned_daemon_command(executable: &Path, impulse_root: &Path, owner_pid: u32) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("--impulse-dir")
        .arg(impulse_root)
        .arg("--owner-pid")
        .arg(owner_pid.to_string())
        .arg("daemon");
    command
}

impl Drop for DesktopDaemonSidecar {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn daemon_root_from_socket(socket_path: &Path) -> Option<PathBuf> {
    let sockets = socket_path.parent()?;
    if sockets.file_name().and_then(|name| name.to_str()) != Some("sockets") {
        return None;
    }
    sockets.parent().map(Path::to_path_buf)
}

fn validate_socket_path(socket_path: &Path) -> Result<PathBuf, DesktopDaemonSidecarError> {
    if socket_path.file_name().and_then(|name| name.to_str()) != Some(DAEMON_SOCKET_NAME) {
        return Err(DesktopDaemonSidecarError::UnsupportedSocketFilename {
            socket_path: socket_path.to_path_buf(),
            expected: DAEMON_SOCKET_NAME,
        });
    }
    daemon_root_from_socket(socket_path).ok_or_else(|| {
        DesktopDaemonSidecarError::InvalidSocketPath {
            socket_path: socket_path.to_path_buf(),
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DaemonProbe {
    Ready,
    Unavailable,
    Incompatible(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DaemonRequestError {
    Unavailable,
    Incompatible(String),
}

impl DaemonRequestError {
    fn into_reason(self) -> String {
        match self {
            Self::Unavailable => "daemon socket is unavailable".to_string(),
            Self::Incompatible(reason) => reason,
        }
    }
}

#[cfg(unix)]
fn request_daemon(
    socket_path: &Path,
    request: &WorkbenchDaemonRequest,
    timeout: Duration,
    max_response_bytes: usize,
    response_name: &str,
) -> Result<WorkbenchDaemonResponse, DaemonRequestError> {
    use std::os::unix::net::UnixStream;

    let mut stream = match UnixStream::connect(socket_path) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Err(DaemonRequestError::Unavailable);
        }
        Err(error) => {
            return Err(DaemonRequestError::Incompatible(format!(
                "connect failed: {error}"
            )));
        }
    };
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| DaemonRequestError::Incompatible(format!("set read timeout: {error}")))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| DaemonRequestError::Incompatible(format!("set write timeout: {error}")))?;
    let encoded = serde_json::to_vec(request).map_err(|error| {
        DaemonRequestError::Incompatible(format!("serialize daemon request: {error}"))
    })?;
    stream
        .write_all(&encoded)
        .and_then(|_| stream.write_all(b"\n"))
        .and_then(|_| stream.flush())
        .map_err(|error| {
            DaemonRequestError::Incompatible(format!("write daemon request: {error}"))
        })?;

    let mut response_bytes = Vec::with_capacity(256);
    let mut reader = BufReader::new(stream);
    let mut bounded_reader = reader.by_ref().take((max_response_bytes + 1) as u64);
    match bounded_reader.read_until(b'\n', &mut response_bytes) {
        Ok(0) => {
            return Err(DaemonRequestError::Incompatible(format!(
                "connection closed before the {response_name} response"
            )));
        }
        Ok(_) => {}
        Err(error) => {
            return Err(DaemonRequestError::Incompatible(format!(
                "read {response_name} response: {error}"
            )));
        }
    }
    if response_bytes.len() > max_response_bytes {
        return Err(DaemonRequestError::Incompatible(format!(
            "{response_name} response exceeded {max_response_bytes} bytes"
        )));
    }
    if response_bytes.last() != Some(&b'\n') {
        return Err(DaemonRequestError::Incompatible(format!(
            "{response_name} response did not end with a newline within the byte limit"
        )));
    }
    serde_json::from_slice(&response_bytes).map_err(|error| {
        DaemonRequestError::Incompatible(format!("parse {response_name} response: {error}"))
    })
}

#[cfg(unix)]
fn attest_daemon_project(
    socket_path: &Path,
    expected_root: &Path,
    timeout: Duration,
) -> Result<DesktopDaemonProjectIdentity, DesktopDaemonSidecarError> {
    let expected_root = expected_root.canonicalize().map_err(|error| {
        DesktopDaemonSidecarError::ProjectIdentitySnapshot {
            socket_path: socket_path.to_path_buf(),
            reason: format!(
                "canonicalize expected project root `{}`: {error}",
                expected_root.display()
            ),
        }
    })?;
    let response = request_daemon(
        socket_path,
        &WorkbenchDaemonRequest::GetOpsSnapshot,
        timeout,
        MAX_DAEMON_PROJECT_ATTESTATION_RESPONSE_BYTES,
        "project snapshot",
    )
    .map_err(|error| DesktopDaemonSidecarError::ProjectIdentitySnapshot {
        socket_path: socket_path.to_path_buf(),
        reason: error.into_reason(),
    })?;
    let result = match response {
        WorkbenchDaemonResponse::Ok { result } => result,
        other => {
            return Err(DesktopDaemonSidecarError::ProjectIdentitySnapshot {
                socket_path: socket_path.to_path_buf(),
                reason: format!("expected a successful project snapshot, received {other:?}"),
            });
        }
    };
    let snapshot: ProjectOpsSnapshot = serde_json::from_value(result).map_err(|error| {
        DesktopDaemonSidecarError::ProjectIdentitySnapshot {
            socket_path: socket_path.to_path_buf(),
            reason: format!("parse project snapshot: {error}"),
        }
    })?;
    let observed_raw = PathBuf::from(snapshot.project.root_path);
    let observed_root = observed_raw.canonicalize().map_err(|error| {
        DesktopDaemonSidecarError::ProjectIdentitySnapshot {
            socket_path: socket_path.to_path_buf(),
            reason: format!(
                "canonicalize daemon project root `{}`: {error}",
                observed_raw.display()
            ),
        }
    })?;
    if observed_root != expected_root {
        return Err(DesktopDaemonSidecarError::ProjectIdentityMismatch {
            socket_path: socket_path.to_path_buf(),
            expected_root,
            observed_root,
        });
    }
    let expected_impulse_root = expected_root
        .join(".impulse")
        .canonicalize()
        .map_err(|error| DesktopDaemonSidecarError::ProjectIdentitySnapshot {
            socket_path: socket_path.to_path_buf(),
            reason: format!(
                "canonicalize expected Impulse state root `{}`: {error}",
                expected_root.join(".impulse").display()
            ),
        })?;
    let observed_impulse_raw = PathBuf::from(snapshot.project.impulse_path);
    let observed_impulse_root = observed_impulse_raw.canonicalize().map_err(|error| {
        DesktopDaemonSidecarError::ProjectIdentitySnapshot {
            socket_path: socket_path.to_path_buf(),
            reason: format!(
                "canonicalize daemon Impulse state root `{}`: {error}",
                observed_impulse_raw.display()
            ),
        }
    })?;
    if expected_impulse_root.parent() != Some(expected_root.as_path()) {
        return Err(DesktopDaemonSidecarError::ProjectStateOutsideProject {
            project_root: expected_root,
            observed_impulse_root: expected_impulse_root,
        });
    }
    if observed_impulse_root != expected_impulse_root {
        return Err(DesktopDaemonSidecarError::ProjectStatePathMismatch {
            socket_path: socket_path.to_path_buf(),
            expected_impulse_root,
            observed_impulse_root,
        });
    }
    let expected_project_id = expected_root
        .file_name()
        .and_then(|segment| segment.to_str())
        .map(impulse_ops::sanitize_id)
        .unwrap_or_else(|| "unknown".to_string());
    if snapshot.project.id != expected_project_id {
        return Err(DesktopDaemonSidecarError::ProjectIdMismatch {
            socket_path: socket_path.to_path_buf(),
            expected_project_id,
            observed_project_id: snapshot.project.id,
        });
    }
    Ok(DesktopDaemonProjectIdentity {
        project_id: expected_project_id,
        project_root: expected_root,
        impulse_root: expected_impulse_root,
    })
}

#[cfg(not(unix))]
fn attest_daemon_project(
    _socket_path: &Path,
    _expected_root: &Path,
    _timeout: Duration,
) -> Result<DesktopDaemonProjectIdentity, DesktopDaemonSidecarError> {
    Err(DesktopDaemonSidecarError::UnsupportedPlatform)
}

#[cfg(unix)]
fn probe_daemon(socket_path: &Path, timeout: Duration) -> DaemonProbe {
    let response = match request_daemon(
        socket_path,
        &WorkbenchDaemonRequest::Ping,
        timeout,
        MAX_DAEMON_HANDSHAKE_RESPONSE_BYTES,
        "ping",
    ) {
        Ok(response) => response,
        Err(DaemonRequestError::Unavailable) => return DaemonProbe::Unavailable,
        Err(DaemonRequestError::Incompatible(reason)) => {
            return DaemonProbe::Incompatible(reason);
        }
    };
    match response {
        WorkbenchDaemonResponse::Ok { result }
            if result.get("pong").and_then(serde_json::Value::as_bool) == Some(true)
                && result
                    .get("protocol_version")
                    .and_then(serde_json::Value::as_u64)
                    == Some(u64::from(DAEMON_PROTOCOL_VERSION)) =>
        {
            DaemonProbe::Ready
        }
        WorkbenchDaemonResponse::Ok { result } => DaemonProbe::Incompatible(format!(
            "expected pong=true and protocol_version={DAEMON_PROTOCOL_VERSION}, received {result}"
        )),
        response => DaemonProbe::Incompatible(format!(
            "expected a successful ping response, received {response:?}"
        )),
    }
}

#[cfg(not(unix))]
fn probe_daemon(_socket_path: &Path, _timeout: Duration) -> DaemonProbe {
    DaemonProbe::Unavailable
}

fn inspect_child(child: &mut Child) -> std::io::Result<Option<ExitStatus>> {
    child.try_wait()
}

fn daemon_pid_marker(socket_path: &Path) -> Option<u32> {
    std::fs::read_to_string(socket_path.with_extension("pid"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(unix)]
fn terminate_and_reap(child: &mut Child, grace_timeout: Duration) -> std::io::Result<ReapedChild> {
    let pid = child.id();
    match child.try_wait() {
        Ok(Some(status)) => return Ok(ReapedChild { pid, status }),
        Ok(None) => {}
        Err(error) => {
            let _ = child.kill();
            return child
                .wait()
                .map(|status| ReapedChild { pid, status })
                .map_err(|_| error);
        }
    }

    // SAFETY: `child.id()` is the PID returned by this process's successful
    // spawn call. SIGTERM gives the daemon time to drain IPC and persist state.
    if unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            let _ = child.kill();
            return child
                .wait()
                .map(|status| ReapedChild { pid, status })
                .map_err(|_| error);
        }
    }

    let deadline = Instant::now() + grace_timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(ReapedChild { pid, status }),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(DAEMON_STARTUP_POLL_INTERVAL);
            }
            Ok(None) => {
                if let Err(error) = child.kill() {
                    return child
                        .wait()
                        .map(|status| ReapedChild { pid, status })
                        .map_err(|_| error);
                }
                return child.wait().map(|status| ReapedChild { pid, status });
            }
            Err(error) => {
                let _ = child.kill();
                return child
                    .wait()
                    .map(|status| ReapedChild { pid, status })
                    .map_err(|_| error);
            }
        }
    }
}

#[cfg(not(unix))]
fn terminate_and_reap(child: &mut Child, _grace_timeout: Duration) -> std::io::Result<ReapedChild> {
    let pid = child.id();
    match child.try_wait() {
        Ok(Some(status)) => return Ok(ReapedChild { pid, status }),
        Ok(None) => {}
        Err(error) => {
            let _ = child.kill();
            return child
                .wait()
                .map(|status| ReapedChild { pid, status })
                .map_err(|_| error);
        }
    }
    child.kill()?;
    child.wait().map(|status| ReapedChild { pid, status })
}

#[cfg(unix)]
struct DaemonCleanupLock(File);

#[cfg(unix)]
impl DaemonCleanupLock {
    /// Try to acquire the daemon's own lifecycle lock without waiting. A
    /// replacement daemon holding this lock owns every runtime-file decision.
    fn try_acquire(socket_path: &Path) -> Option<Self> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let lock_path = socket_path.with_extension("daemon.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&lock_path)
            .ok()?;
        if !file.metadata().ok()?.is_file() {
            return None;
        }
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .ok()?;
        loop {
            // SAFETY: `file` stays alive in the returned guard for the full
            // cleanup critical section.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Some(Self(file));
            }
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return None;
        }
    }
}

#[cfg(unix)]
impl Drop for DaemonCleanupLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;

        // SAFETY: the descriptor remains valid through this call; descriptor
        // close is a second release path if explicit unlock fails.
        let _ = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

/// Remove crash residue only after a child has been confirmed exited/reaped,
/// while holding the same daemon-specific lock used across bind and shutdown.
/// PID ownership is still checked inside that critical section.
#[cfg(unix)]
fn cleanup_reaped_runtime_files(socket_path: &Path, reaped: &ReapedChild) {
    let Some(_daemon_lock) = DaemonCleanupLock::try_acquire(socket_path) else {
        return;
    };
    if probe_daemon(socket_path, DAEMON_HANDSHAKE_TIMEOUT) == DaemonProbe::Ready {
        return;
    }
    let pid_path = socket_path.with_extension("pid");
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut pid_file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&pid_path)
    {
        Ok(file) if file.metadata().is_ok_and(|metadata| metadata.is_file()) => file,
        _ => return,
    };
    let mut marker = String::new();
    if pid_file.read_to_string(&mut marker).is_err() {
        return;
    }
    if marker.trim().parse::<u32>().ok() != Some(reaped.pid) {
        return;
    }
    let _ = std::fs::remove_file(socket_path);
    let _ = std::fs::remove_file(pid_path);
}

#[cfg(not(unix))]
fn cleanup_reaped_runtime_files(_socket_path: &Path, _reaped: &ReapedChild) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::sync::atomic::{AtomicBool, Ordering};

    const SIDECAR_HELPER_SOCKET_ENV: &str = "IMPULSE_TEST_SIDECAR_SOCKET";

    #[cfg(unix)]
    fn answer_ping(mut stream: std::os::unix::net::UnixStream, protocol_version: u32) {
        let mut request = String::new();
        BufReader::new(stream.try_clone().expect("clone ping stream"))
            .read_line(&mut request)
            .expect("read ping request");
        let parsed: WorkbenchDaemonRequest =
            serde_json::from_str(&request).expect("parse ping request");
        assert_eq!(parsed, WorkbenchDaemonRequest::Ping);
        let response = WorkbenchDaemonResponse::Ok {
            result: serde_json::json!({
                "pong": true,
                "protocol_version": protocol_version,
            }),
        };
        serde_json::to_writer(&mut stream, &response).expect("write ping response");
        stream.write_all(b"\n").expect("terminate ping response");
        stream.flush().expect("flush ping response");
    }

    #[cfg(unix)]
    fn answer_project_snapshot(
        mut stream: std::os::unix::net::UnixStream,
        project_id: &str,
        project_root: &Path,
        impulse_root: &Path,
    ) {
        let mut request = String::new();
        BufReader::new(stream.try_clone().expect("clone snapshot stream"))
            .read_line(&mut request)
            .expect("read project snapshot request");
        let parsed: WorkbenchDaemonRequest =
            serde_json::from_str(&request).expect("parse project snapshot request");
        assert_eq!(parsed, WorkbenchDaemonRequest::GetOpsSnapshot);
        let mut snapshot = ProjectOpsSnapshot::default();
        snapshot.project.id = project_id.to_string();
        snapshot.project.name = project_id.to_string();
        snapshot.project.root_path = project_root.display().to_string();
        snapshot.project.impulse_path = impulse_root.display().to_string();
        let response = WorkbenchDaemonResponse::Ok {
            result: serde_json::to_value(snapshot).expect("serialize project snapshot"),
        };
        serde_json::to_writer(&mut stream, &response).expect("write project snapshot response");
        stream
            .write_all(b"\n")
            .expect("terminate project snapshot response");
        stream.flush().expect("flush project snapshot response");
    }

    #[cfg(unix)]
    fn spawn_project_snapshot_server(
        socket_path: &Path,
        project_id: String,
        project_root: PathBuf,
        impulse_root: PathBuf,
    ) -> std::thread::JoinHandle<()> {
        let listener = std::os::unix::net::UnixListener::bind(socket_path)
            .expect("bind project snapshot socket");
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept project snapshot request");
            answer_project_snapshot(stream, &project_id, &project_root, &impulse_root);
        })
    }

    #[cfg(unix)]
    fn spawn_ping_server(
        socket_path: &Path,
        protocol_version: u32,
    ) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let listener = std::os::unix::net::UnixListener::bind(socket_path)
            .expect("bind existing daemon socket");
        listener
            .set_nonblocking(true)
            .expect("set ping listener nonblocking");
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream
                            .set_nonblocking(false)
                            .expect("restore blocking ping stream");
                        answer_ping(stream, protocol_version);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("accept ping connection: {error}"),
                }
            }
        });
        (stop, worker)
    }

    #[test]
    fn derives_state_root_only_from_standard_socket_directory() {
        assert_eq!(
            daemon_root_from_socket(Path::new("/repo/.impulse/sockets/impulse.sock")),
            Some(PathBuf::from("/repo/.impulse"))
        );
        assert_eq!(daemon_root_from_socket(Path::new("/tmp/custom.sock")), None);
        assert_eq!(
            daemon_root_from_socket(Path::new("/repo/.impulse/not-sockets/impulse.sock")),
            None
        );
    }

    #[test]
    fn rejects_socket_filenames_the_packaged_cli_cannot_bind() {
        let socket_path = Path::new("/repo/.impulse/sockets/custom.sock");
        let error = validate_socket_path(socket_path).expect_err("custom name must fail closed");
        assert!(matches!(
            error,
            DesktopDaemonSidecarError::UnsupportedSocketFilename { .. }
        ));
    }

    #[test]
    fn desktop_spawn_command_binds_companion_to_exact_owner_pid() {
        use std::ffi::OsString;

        let executable = Path::new("/Applications/Impulse.app/Contents/MacOS/impulse-rs");
        let impulse_root = Path::new("/tmp/impulse-owned/.impulse");
        let command = owned_daemon_command(executable, impulse_root, 4242);
        let args = command.get_args().map(OsString::from).collect::<Vec<_>>();

        assert_eq!(command.get_program(), executable.as_os_str());
        assert_eq!(
            args,
            vec![
                OsString::from("--impulse-dir"),
                impulse_root.as_os_str().to_os_string(),
                OsString::from("--owner-pid"),
                OsString::from("4242"),
                OsString::from("daemon"),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn sidecar_public_api_rejects_unsupported_socket_filename_before_launch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir
            .path()
            .join(".impulse")
            .join("sockets")
            .join("custom.sock");
        let config = DesktopDaemonOpsConfig::new(socket_path, Some(dir.path().into()));
        let error = match DesktopDaemonSidecar::ensure(&config) {
            Ok(_) => panic!("custom socket filename must fail closed"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            DesktopDaemonSidecarError::UnsupportedSocketFilename { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn desktop_instance_lease_is_exclusive_and_reacquirable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("sockets").join(DAEMON_SOCKET_NAME);
        let first = DesktopInstanceLease::acquire(&socket_path).expect("first lease");
        assert_eq!(
            std::fs::metadata(first.lock_path())
                .expect("lock metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(matches!(
            DesktopInstanceLease::acquire(&socket_path),
            Err(DesktopDaemonSidecarError::InstanceAlreadyRunning { .. })
        ));

        let lock_path = first.lock_path().to_path_buf();
        drop(first);
        let reacquired = DesktopInstanceLease::acquire(&socket_path).expect("reacquire lease");
        assert_eq!(reacquired.lock_path(), lock_path);
    }

    #[cfg(unix)]
    #[test]
    fn socket_listener_without_valid_impulse_handshake_is_not_existing_daemon() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sockets = dir.path().join(".impulse").join("sockets");
        std::fs::create_dir_all(&sockets).expect("socket directory");
        let socket_path = sockets.join(DAEMON_SOCKET_NAME);
        let (stop, worker) = spawn_ping_server(&socket_path, DAEMON_PROTOCOL_VERSION + 1);

        let config = DesktopDaemonOpsConfig::new(socket_path.clone(), Some(dir.path().into()));
        let error = match DesktopDaemonSidecar::ensure(&config) {
            Ok(_) => panic!("wrong protocol version must fail closed"),
            Err(error) => error,
        };
        assert!(matches!(error, DesktopDaemonSidecarError::Handshake { .. }));

        stop.store(true, Ordering::Release);
        worker.join().expect("join ping server");
    }

    #[cfg(unix)]
    #[test]
    fn daemon_handshake_rejects_responses_above_the_byte_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join(DAEMON_SOCKET_NAME);
        let listener = std::os::unix::net::UnixListener::bind(&socket_path)
            .expect("bind oversized response server");
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept ping");
            let mut request = String::new();
            BufReader::new(stream.try_clone().expect("clone ping stream"))
                .read_line(&mut request)
                .expect("read ping");
            let oversized = vec![b'x'; MAX_DAEMON_HANDSHAKE_RESPONSE_BYTES + 1];
            stream.write_all(&oversized).expect("write oversized body");
            let _ = stream.write_all(b"\n");
        });

        let probe = probe_daemon(&socket_path, DAEMON_HANDSHAKE_TIMEOUT);
        assert!(matches!(
            probe,
            DaemonProbe::Incompatible(reason) if reason.contains("exceeded")
        ));
        worker.join().expect("join oversized response server");
    }

    #[cfg(unix)]
    #[test]
    fn project_attestation_accepts_exact_root_state_and_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().join("attested-project");
        let impulse_root = project_root.join(".impulse");
        std::fs::create_dir_all(&impulse_root).expect("create project state root");
        let socket_path = dir.path().join("attestation.sock");
        let worker = spawn_project_snapshot_server(
            &socket_path,
            "attested-project".to_string(),
            project_root.clone(),
            impulse_root.clone(),
        );

        let identity = attest_daemon_project(
            &socket_path,
            &project_root,
            DAEMON_PROJECT_ATTESTATION_TIMEOUT,
        )
        .expect("attest exact project identity");
        assert_eq!(identity.project_id, "attested-project");
        assert_eq!(identity.project_root, project_root.canonicalize().unwrap());
        assert_eq!(identity.impulse_root, impulse_root.canonicalize().unwrap());
        worker.join().expect("join project snapshot server");
    }

    #[cfg(unix)]
    #[test]
    fn project_attestation_rejects_wrong_root_before_attachment() {
        let dir = tempfile::tempdir().expect("tempdir");
        let expected_root = dir.path().join("expected-project");
        let observed_root = dir.path().join("observed-project");
        std::fs::create_dir_all(expected_root.join(".impulse"))
            .expect("create expected state root");
        std::fs::create_dir_all(observed_root.join(".impulse"))
            .expect("create observed state root");
        let socket_path = dir.path().join("wrong-root.sock");
        let worker = spawn_project_snapshot_server(
            &socket_path,
            "observed-project".to_string(),
            observed_root.clone(),
            observed_root.join(".impulse"),
        );

        let error = attest_daemon_project(
            &socket_path,
            &expected_root,
            DAEMON_PROJECT_ATTESTATION_TIMEOUT,
        )
        .expect_err("cross-project daemon must fail closed");
        assert!(matches!(
            error,
            DesktopDaemonSidecarError::ProjectIdentityMismatch { .. }
        ));
        worker.join().expect("join wrong-root snapshot server");
    }

    #[cfg(unix)]
    #[test]
    fn project_attestation_rejects_wrong_state_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().join("attested-project");
        let expected_impulse_root = project_root.join(".impulse");
        let observed_impulse_root = project_root.join(".other-impulse");
        std::fs::create_dir_all(&expected_impulse_root).expect("create expected state root");
        std::fs::create_dir_all(&observed_impulse_root).expect("create observed state root");
        let socket_path = dir.path().join("wrong-state.sock");
        let worker = spawn_project_snapshot_server(
            &socket_path,
            "attested-project".to_string(),
            project_root.clone(),
            observed_impulse_root,
        );

        let error = attest_daemon_project(
            &socket_path,
            &project_root,
            DAEMON_PROJECT_ATTESTATION_TIMEOUT,
        )
        .expect_err("alternate state root must fail closed");
        assert!(matches!(
            error,
            DesktopDaemonSidecarError::ProjectStatePathMismatch { .. }
        ));
        worker.join().expect("join wrong-state snapshot server");
    }

    #[cfg(unix)]
    #[test]
    fn project_attestation_rejects_state_symlink_outside_project() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().join("attested-project");
        let external_state = dir.path().join("external-state");
        std::fs::create_dir_all(&project_root).expect("create project root");
        std::fs::create_dir_all(&external_state).expect("create external state root");
        symlink(&external_state, project_root.join(".impulse"))
            .expect("link project state outside project");
        let socket_path = dir.path().join("external-state.sock");
        let worker = spawn_project_snapshot_server(
            &socket_path,
            "attested-project".to_string(),
            project_root.clone(),
            external_state,
        );

        let error = attest_daemon_project(
            &socket_path,
            &project_root,
            DAEMON_PROJECT_ATTESTATION_TIMEOUT,
        )
        .expect_err("external state symlink must fail closed");
        assert!(matches!(
            error,
            DesktopDaemonSidecarError::ProjectStateOutsideProject { .. }
        ));
        worker.join().expect("join external-state snapshot server");
    }

    #[cfg(unix)]
    #[test]
    fn existing_daemon_socket_is_attached_but_never_owned() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sockets = dir.path().join(".impulse").join("sockets");
        std::fs::create_dir_all(&sockets).expect("socket directory");
        let socket_path = sockets.join("impulse.sock");
        let (stop, worker) = spawn_ping_server(&socket_path, DAEMON_PROTOCOL_VERSION);
        let config = DesktopDaemonOpsConfig::new(socket_path.clone(), Some(dir.path().into()));

        let sidecar = DesktopDaemonSidecar::ensure(&config).expect("attach existing daemon");
        assert_eq!(sidecar.mode(), DesktopDaemonSidecarMode::Existing);
        assert!(!sidecar.spawned());
        let handle = DesktopDaemonSidecarHandle::new(Some(sidecar));
        assert_eq!(
            handle.shutdown(),
            Some(DesktopDaemonSidecarShutdownOutcome {
                mode: DesktopDaemonSidecarMode::Existing,
                terminate_reap: DesktopDaemonTerminateReapOutcome::NotRequired,
            })
        );
        assert_eq!(handle.shutdown(), None, "shutdown must be idempotent");

        assert_eq!(
            probe_daemon(&socket_path, DAEMON_HANDSHAKE_TIMEOUT),
            DaemonProbe::Ready,
            "dropping an attachment must not stop or delete the existing daemon"
        );
        stop.store(true, Ordering::Release);
        worker.join().expect("join ping server");
    }

    #[cfg(unix)]
    #[test]
    fn verified_daemon_with_different_pid_wins_spawn_race_and_contender_is_reaped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sockets = dir.path().join(".impulse").join("sockets");
        std::fs::create_dir_all(&sockets).expect("socket directory");
        let socket_path = sockets.join(DAEMON_SOCKET_NAME);
        let (stop, worker) = spawn_ping_server(&socket_path, DAEMON_PROTOCOL_VERSION);
        std::fs::write(
            socket_path.with_extension("pid"),
            std::process::id().to_string(),
        )
        .expect("write winning daemon pid");
        let _winning_daemon_lock =
            DaemonCleanupLock::try_acquire(&socket_path).expect("hold winning daemon lock");

        let contender_pid_path = dir.path().join("contender.pid");
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("printf '%s' \"$$\" > \"$CONTENDER_PID\"; exec sleep 30")
            .env("CONTENDER_PID", &contender_pid_path);
        let config = DesktopDaemonOpsConfig::new(socket_path.clone(), Some(dir.path().into()));
        let sidecar = DesktopDaemonSidecar::spawn_and_wait(
            &config,
            Duration::from_secs(2),
            PathBuf::from("sh"),
            command,
        )
        .expect("attach to race-winning daemon");

        assert_eq!(sidecar.mode(), DesktopDaemonSidecarMode::Existing);
        let contender_pid = std::fs::read_to_string(&contender_pid_path)
            .expect("read contender pid")
            .trim()
            .parse::<libc::pid_t>()
            .expect("parse contender pid");
        assert_eq!(unsafe { libc::kill(contender_pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        assert_eq!(
            probe_daemon(&socket_path, DAEMON_HANDSHAKE_TIMEOUT),
            DaemonProbe::Ready
        );
        stop.store(true, Ordering::Release);
        worker.join().expect("join winning daemon");
    }

    #[cfg(unix)]
    #[test]
    fn spawned_sidecar_helper_process() {
        let Ok(socket_path) = std::env::var(SIDECAR_HELPER_SOCKET_ENV) else {
            return;
        };
        let socket_path = PathBuf::from(socket_path);
        std::fs::create_dir_all(socket_path.parent().expect("socket parent"))
            .expect("create helper socket directory");
        let _listener = std::os::unix::net::UnixListener::bind(&socket_path)
            .expect("bind helper daemon socket");
        std::fs::write(
            socket_path.with_extension("pid"),
            std::process::id().to_string(),
        )
        .expect("write helper pid");
        loop {
            let (stream, _) = _listener.accept().expect("accept helper ping");
            answer_ping(stream, DAEMON_PROTOCOL_VERSION);
        }
    }

    #[cfg(unix)]
    #[test]
    fn spawned_daemon_is_reaped_and_runtime_files_are_removed_on_drop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sockets = dir.path().join(".impulse").join("sockets");
        let socket_path = sockets.join("impulse.sock");
        let config = DesktopDaemonOpsConfig::new(socket_path.clone(), Some(dir.path().into()));
        let current_exe = std::env::current_exe().expect("current test executable");
        let helper_module = module_path!()
            .strip_prefix("impulse_desktop::")
            .unwrap_or(module_path!());
        let helper_name = format!("{helper_module}::spawned_sidecar_helper_process");
        let mut command = Command::new(&current_exe);
        command
            .arg("--exact")
            .arg(helper_name)
            .arg("--nocapture")
            .env(SIDECAR_HELPER_SOCKET_ENV, &socket_path);

        let sidecar = DesktopDaemonSidecar::spawn_and_wait(
            &config,
            Duration::from_secs(5),
            current_exe,
            command,
        )
        .expect("spawn test daemon sidecar");
        assert_eq!(sidecar.mode(), DesktopDaemonSidecarMode::Spawned);
        let child_pid = sidecar.child.as_ref().expect("owned child").id();
        assert_eq!(
            probe_daemon(&socket_path, DAEMON_HANDSHAKE_TIMEOUT),
            DaemonProbe::Ready
        );
        assert!(socket_path.with_extension("pid").exists());

        let handle = DesktopDaemonSidecarHandle::new(Some(sidecar));
        let outcome = handle.shutdown().expect("first shutdown outcome");
        assert_eq!(outcome.mode, DesktopDaemonSidecarMode::Spawned);
        assert!(matches!(
            outcome.terminate_reap,
            DesktopDaemonTerminateReapOutcome::Reaped { pid, .. } if pid == child_pid
        ));
        assert_eq!(handle.shutdown(), None, "shutdown must be idempotent");

        assert!(!socket_path.exists(), "owned socket must be cleaned up");
        assert!(
            !socket_path.with_extension("pid").exists(),
            "owned pid file must be cleaned up"
        );
        let probe = unsafe { libc::kill(child_pid as libc::pid_t, 0) };
        assert_eq!(probe, -1, "owned daemon process must be reaped");
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH),
            "reaped daemon pid must no longer identify a process"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_never_unlinks_runtime_files_with_a_different_pid_owner() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sockets = dir.path().join(".impulse").join("sockets");
        std::fs::create_dir_all(&sockets).expect("socket directory");
        let socket_path = sockets.join(DAEMON_SOCKET_NAME);
        let mut child = Command::new("sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("spawn already-exiting child");
        let reaped =
            terminate_and_reap(&mut child, Duration::from_secs(1)).expect("reap exited child");
        std::fs::write(&socket_path, b"crash residue").expect("socket marker");
        std::fs::write(socket_path.with_extension("pid"), "424242").expect("pid marker");

        cleanup_reaped_runtime_files(&socket_path, &reaped);

        assert!(socket_path.exists());
        assert!(socket_path.with_extension("pid").exists());
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_refuses_to_race_a_replacement_holding_the_daemon_lock() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sockets = dir.path().join(".impulse").join("sockets");
        std::fs::create_dir_all(&sockets).expect("socket directory");
        let socket_path = sockets.join(DAEMON_SOCKET_NAME);
        let mut child = Command::new("sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("spawn already-exiting child");
        let reaped =
            terminate_and_reap(&mut child, Duration::from_secs(1)).expect("reap exited child");
        std::fs::write(&socket_path, b"owned crash residue").expect("socket marker");
        std::fs::write(socket_path.with_extension("pid"), reaped.pid.to_string())
            .expect("pid marker");
        let replacement_lock =
            DaemonCleanupLock::try_acquire(&socket_path).expect("replacement lock");

        cleanup_reaped_runtime_files(&socket_path, &reaped);
        assert!(socket_path.exists());
        assert!(socket_path.with_extension("pid").exists());

        drop(replacement_lock);
        cleanup_reaped_runtime_files(&socket_path, &reaped);
        assert!(!socket_path.exists());
        assert!(!socket_path.with_extension("pid").exists());
        assert!(
            socket_path.with_extension("daemon.lock").exists(),
            "the shared lock inode must be retained"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_lock_rejects_symlink_without_mutating_target() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("impulse.sock");
        let lock_path = socket_path.with_extension("daemon.lock");
        let external = dir.path().join("external-lock-target");
        std::fs::write(&external, b"must remain untouched").expect("external target");
        symlink(&external, &lock_path).expect("hostile cleanup lock symlink");

        assert!(DaemonCleanupLock::try_acquire(&socket_path).is_none());
        assert_eq!(std::fs::read(&external).unwrap(), b"must remain untouched");
    }

    #[cfg(unix)]
    #[test]
    fn owned_child_receives_sigterm_before_force_kill_fallback() {
        let dir = tempfile::tempdir().expect("tempdir");
        let marker = dir.path().join("received-sigterm");
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("trap 'printf term > \"$MARKER\"; exit 0' TERM; while :; do sleep 1; done")
            .env("MARKER", &marker)
            .spawn()
            .expect("spawn signal-aware child");
        std::thread::sleep(Duration::from_millis(100));

        let _ = terminate_and_reap(&mut child, Duration::from_secs(2))
            .expect("gracefully terminate child");

        assert_eq!(
            std::fs::read_to_string(marker).expect("SIGTERM marker"),
            "term"
        );
    }
}
