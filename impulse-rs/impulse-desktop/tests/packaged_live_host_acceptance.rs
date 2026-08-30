#![cfg(unix)]
#![recursion_limit = "256"]

//! External acceptance harness for a packaged Dioxus desktop host.
//!
//! This test is ignored by default because it launches a GUI application from
//! a read-only mounted macOS bundle. `scripts/verify-packaged-host.sh` supplies
//! the explicit package/source inputs and invokes the one ignored test.

use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
#[cfg(target_os = "macos")]
use std::os::unix::process::ExitStatusExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStderr, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use impulse_desktop::packaged_acceptance::{
    PACKAGED_DAEMON_PID_ENV, PACKAGED_INVOKE_DRAIN_TIMEOUT_SECS, PACKAGED_OBSERVATION_TIMEOUT_SECS,
    PACKAGED_TERMINAL_CLEANUP_TIMEOUT_SECS,
};
use impulse_ops::{
    DaemonInstanceIdentity, DaemonPingResponse, WorkbenchDaemonRequest, WorkbenchDaemonResponse,
    DAEMON_INSTANCE_NONCE_ENV, DAEMON_PROTOCOL_VERSION,
};
#[cfg(target_os = "macos")]
use impulse_term::TerminalBackend;

const APP_PATH_ENV: &str = "IMPULSE_PACKAGED_APP_PATH";
const SOURCE_ROOT_ENV: &str = "IMPULSE_PACKAGED_SOURCE_ROOT";
const CANONICAL_ROOT_ENV: &str = "IMPULSE_PACKAGED_CANONICAL_ROOT";
const PROVENANCE_SHA_ENV: &str = "IMPULSE_PACKAGED_PROVENANCE_SHA256";
const ACCEPTANCE_NONCE_ENV: &str = "IMPULSE_PACKAGED_ACCEPTANCE_NONCE";
const ACCEPTANCE_ROOT_ENV: &str = "IMPULSE_PACKAGED_ACCEPTANCE_ROOT";
const RECEIPT_PREFIX: &str = "IMPULSE_PACKAGED_HOST_RECEIPT ";
const RECEIPT_SCHEMA: &str = "impulse.packaged-host/v1";
const MAX_RECEIPT_FAILURE_REASONS: usize = 24;
const MAX_RECEIPT_FAILURE_REASON_CHARS: usize = 240;
const DAEMON_READY_TIMEOUT: Duration = Duration::from_secs(15);
const RECEIPT_TIMEOUT_GRACE_SECS: u64 = 15;
const RECEIPT_TIMEOUT: Duration =
    Duration::from_secs(PACKAGED_OBSERVATION_TIMEOUT_SECS + RECEIPT_TIMEOUT_GRACE_SECS);
const PROCESS_TERM_TIMEOUT: Duration = Duration::from_secs(3);
const PROCESS_KILL_TIMEOUT: Duration = Duration::from_secs(3);
const SOCKET_CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);
const RECEIPT_READER_JOIN_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(target_os = "macos")]
const MACOS_UNIX_SOCKET_PATH_CAPACITY: usize = 104;
#[cfg(target_os = "macos")]
const PTY_PARENT_DEATH_HELPER_ENV: &str = "IMPULSE_PTY_PARENT_DEATH_HELPER";
#[cfg(target_os = "macos")]
const PTY_PARENT_DEATH_NONCE_ENV: &str = "IMPULSE_PTY_PARENT_DEATH_NONCE";
#[cfg(target_os = "macos")]
const PTY_PARENT_DEATH_READY_ENV: &str = "IMPULSE_PTY_PARENT_DEATH_READY";
#[cfg(target_os = "macos")]
const PTY_PARENT_DEATH_TIMEOUT: Duration = Duration::from_secs(5);

static CLEANUP_SIGNAL_RECEIVED: AtomicBool = AtomicBool::new(false);

extern "C" fn record_cleanup_signal(_signal: libc::c_int) {
    CLEANUP_SIGNAL_RECEIVED.store(true, Ordering::SeqCst);
}

fn check_cleanup_signal() -> Result<(), String> {
    if CLEANUP_SIGNAL_RECEIVED.load(Ordering::SeqCst) {
        Err("packaged-host acceptance was interrupted; cleaning up child processes".to_string())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
struct SignalCleanupGuard {
    previous_int: libc::sigaction,
    previous_term: libc::sigaction,
}

#[cfg(target_os = "macos")]
impl SignalCleanupGuard {
    fn install() -> Result<Self, String> {
        CLEANUP_SIGNAL_RECEIVED.store(false, Ordering::SeqCst);
        // SAFETY: sigaction structures are zero-initialized before libc fills
        // them, and the handler performs only an atomic store.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = record_cleanup_signal as *const () as usize;
            action.sa_flags = 0;
            libc::sigemptyset(&mut action.sa_mask);

            let mut previous_int: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(libc::SIGINT, &action, &mut previous_int) != 0 {
                return Err("install packaged-host SIGINT cleanup handler".to_string());
            }

            let mut previous_term: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(libc::SIGTERM, &action, &mut previous_term) != 0 {
                let _ = libc::sigaction(libc::SIGINT, &previous_int, std::ptr::null_mut());
                return Err("install packaged-host SIGTERM cleanup handler".to_string());
            }

            Ok(Self {
                previous_int,
                previous_term,
            })
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for SignalCleanupGuard {
    fn drop(&mut self) {
        // SAFETY: these are the exact actions returned by successful sigaction
        // calls in install(), restored before the test process exits.
        unsafe {
            let _ = libc::sigaction(libc::SIGINT, &self.previous_int, std::ptr::null_mut());
            let _ = libc::sigaction(libc::SIGTERM, &self.previous_term, std::ptr::null_mut());
        }
        CLEANUP_SIGNAL_RECEIVED.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug)]
struct AcceptanceConfig {
    app_path: PathBuf,
    source_root: PathBuf,
    canonical_root: PathBuf,
    provenance_sha256: String,
    live_impulse_home: PathBuf,
}

impl AcceptanceConfig {
    fn from_env() -> Result<Self, String> {
        let app_path = canonical_dir_from_env(APP_PATH_ENV)?;
        let source_root = canonical_dir_from_env(SOURCE_ROOT_ENV)?;
        let canonical_root = canonical_dir_from_env(CANONICAL_ROOT_ENV)?;
        let provenance_sha256 = required_env(PROVENANCE_SHA_ENV)?;
        if provenance_sha256.len() != 64
            || !provenance_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(format!(
                "{PROVENANCE_SHA_ENV} must be a lowercase 64-character SHA-256"
            ));
        }

        validate_git_root(&source_root, SOURCE_ROOT_ENV)?;
        validate_git_root(&canonical_root, CANONICAL_ROOT_ENV)?;
        validate_app_bundle(&app_path)?;
        #[cfg(target_os = "macos")]
        require_read_only_mount(&app_path)?;

        let live_home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "HOME is required to fingerprint the live Impulse home".to_string())?;

        Ok(Self {
            app_path,
            source_root,
            canonical_root,
            provenance_sha256,
            live_impulse_home: live_home.join(".impulse"),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OpaqueFingerprint {
    exists: bool,
    entries: u64,
    digest: [u8; 32],
}

#[derive(Debug)]
struct WardenSnapshot {
    source_git_visible: OpaqueFingerprint,
    canonical_git_visible: OpaqueFingerprint,
    source_root_impulse_state: OpaqueFingerprint,
    source_workspace_impulse_state: OpaqueFingerprint,
    canonical_root_impulse_state: OpaqueFingerprint,
    canonical_workspace_impulse_state: OpaqueFingerprint,
    live_impulse_home: OpaqueFingerprint,
}

impl WardenSnapshot {
    fn capture(config: &AcceptanceConfig) -> Result<Self, String> {
        Ok(Self {
            source_git_visible: git_visible_fingerprint(&config.source_root)?,
            canonical_git_visible: git_visible_fingerprint(&config.canonical_root)?,
            source_root_impulse_state: path_fingerprint(&config.source_root.join(".impulse"))?,
            source_workspace_impulse_state: path_fingerprint(
                &config.source_root.join("impulse-rs/.impulse"),
            )?,
            canonical_root_impulse_state: path_fingerprint(
                &config.canonical_root.join(".impulse"),
            )?,
            canonical_workspace_impulse_state: path_fingerprint(
                &config.canonical_root.join("impulse-rs/.impulse"),
            )?,
            live_impulse_home: path_fingerprint(&config.live_impulse_home)?,
        })
    }

    fn assert_unchanged(&self, after: &Self) -> Result<(), String> {
        let mut changed = Vec::new();
        if self.source_git_visible != after.source_git_visible {
            changed.push("source Git-visible worktree state");
        }
        if self.canonical_git_visible != after.canonical_git_visible {
            changed.push("canonical Git-visible checkout state");
        }
        if self.source_root_impulse_state != after.source_root_impulse_state {
            changed.push("source root .impulse state");
        }
        if self.source_workspace_impulse_state != after.source_workspace_impulse_state {
            changed.push("source impulse-rs/.impulse state");
        }
        if self.canonical_root_impulse_state != after.canonical_root_impulse_state {
            changed.push("canonical root .impulse state");
        }
        if self.canonical_workspace_impulse_state != after.canonical_workspace_impulse_state {
            changed.push("canonical impulse-rs/.impulse state");
        }
        if self.live_impulse_home != after.live_impulse_home {
            changed.push("live Impulse home");
        }
        if changed.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "packaged-host acceptance mutated protected state: {}",
                changed.join(", ")
            ))
        }
    }
}

#[derive(Debug)]
struct ProcessGroupGuard {
    child: Option<Child>,
    pgid: i32,
    label: &'static str,
}

impl ProcessGroupGuard {
    fn new(child: Child, label: &'static str) -> Result<Self, String> {
        let pgid = i32::try_from(child.id())
            .map_err(|_| format!("{label} pid does not fit in a process-group id"))?;
        Ok(Self {
            child: Some(child),
            pgid,
            label,
        })
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, String> {
        match self.child.as_mut() {
            Some(child) => child
                .try_wait()
                .map_err(|error| format!("poll {}: {error}", self.label)),
            None => Ok(None),
        }
    }

    fn stop(&mut self) -> Result<(), String> {
        let mut errors = Vec::new();

        if process_group_exists(self.pgid) {
            signal_process_group(self.pgid, "TERM");
        }
        if !wait_for_group_exit(self.pgid, PROCESS_TERM_TIMEOUT) {
            signal_process_group(self.pgid, "KILL");
        }

        if let Some(mut child) = self.child.take() {
            let deadline = Instant::now() + PROCESS_KILL_TIMEOUT;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Ok(None) => {
                        if let Err(error) = child.kill() {
                            errors
                                .push(format!("kill {} after reap deadline: {error}", self.label));
                        }
                        let label = self.label;
                        thread::spawn(move || {
                            if let Err(error) = child.wait() {
                                eprintln!("background reap {label}: {error}");
                            }
                        });
                        errors.push(format!(
                            "{} exceeded its bounded reap deadline; delegated to background reaper",
                            self.label
                        ));
                        break;
                    }
                    Err(error) => {
                        errors.push(format!("poll {} during cleanup: {error}", self.label));
                        break;
                    }
                }
            }
        }

        if process_group_exists(self.pgid) {
            errors.push(format!(
                "{} process group {} remained alive after cleanup",
                self.label, self.pgid
            ));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct ProcessIdentity {
    parent_pid: i32,
    process_group_id: i32,
    session_id: i32,
    command: String,
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct ParentDeathFixtureGuard {
    helper: Option<Child>,
    helper_pgid: i32,
    shell_identity: Option<(i32, String)>,
}

#[cfg(target_os = "macos")]
impl ParentDeathFixtureGuard {
    fn new(helper: Child) -> Result<Self, String> {
        let helper_pgid = i32::try_from(helper.id()).map_err(|_| {
            "parent-death helper pid does not fit in a process-group id".to_string()
        })?;
        Ok(Self {
            helper: Some(helper),
            helper_pgid,
            shell_identity: None,
        })
    }

    fn helper_pid(&self) -> i32 {
        self.helper_pgid
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, String> {
        self.helper
            .as_mut()
            .ok_or_else(|| "parent-death helper is no longer owned".to_string())?
            .try_wait()
            .map_err(|error| format!("poll parent-death helper: {error}"))
    }

    fn bind_shell(&mut self, pid: i32, nonce: String) {
        self.shell_identity = Some((pid, nonce));
    }

    fn kill_helper_abruptly(&mut self) -> Result<ExitStatus, String> {
        signal_process(self.helper_pgid, "KILL");
        let status = wait_for_child_exit(
            self.helper
                .as_mut()
                .ok_or_else(|| "parent-death helper is no longer owned".to_string())?,
            PTY_PARENT_DEATH_TIMEOUT,
        )?;
        self.helper = None;
        Ok(status)
    }

    fn disarm_shell(&mut self) {
        self.shell_identity = None;
    }
}

#[cfg(target_os = "macos")]
impl Drop for ParentDeathFixtureGuard {
    fn drop(&mut self) {
        if let Some(mut helper) = self.helper.take() {
            signal_process_group(self.helper_pgid, "KILL");
            let _ = wait_for_child_exit(&mut helper, PTY_PARENT_DEATH_TIMEOUT);
        }

        if let Some((pid, nonce)) = self.shell_identity.take() {
            if process_identity_matches(pid, &nonce) {
                signal_process_group(pid, "KILL");
                signal_process(pid, "KILL");
                let _ = wait_for_process_identity_exit(pid, &nonce, PTY_PARENT_DEATH_TIMEOUT);
            }
        }
    }
}

#[derive(Debug)]
enum ReceiptEvent {
    Receipt(String),
    ReadError,
}

fn spawn_receipt_reader(
    stderr: ChildStderr,
) -> (Receiver<ReceiptEvent>, thread::JoinHandle<usize>) {
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut count = 0_usize;
        for line in BufReader::new(stderr).lines() {
            match line {
                Ok(line) => {
                    if let Some(json) = line.strip_prefix(RECEIPT_PREFIX) {
                        count += 1;
                        let _ = sender.send(ReceiptEvent::Receipt(json.to_string()));
                    }
                }
                Err(_) => {
                    let _ = sender.send(ReceiptEvent::ReadError);
                    break;
                }
            }
        }
        count
    });
    (receiver, reader)
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires an explicit read-only mounted Impulse.app; use scripts/verify-packaged-host.sh"]
fn test_packaged_live_host_acceptance_real_mounted_app() {
    let config = AcceptanceConfig::from_env().unwrap_or_else(|error| panic!("{error}"));
    if let Err(error) = run_packaged_acceptance(&config) {
        panic!("{error}");
    }
}

#[cfg(target_os = "macos")]
fn run_packaged_acceptance(config: &AcceptanceConfig) -> Result<(), String> {
    let _signal_guard = SignalCleanupGuard::install()?;
    let before = WardenSnapshot::capture(config)?;
    let isolated = tempfile::Builder::new()
        .prefix("impulse-packaged-host-")
        .tempdir_in("/tmp")
        .map_err(|error| format!("create isolated acceptance root: {error}"))?;
    let root = isolated.path().to_path_buf();
    let child_home = root.join("home");
    let child_tmp = root.join("tmp");
    let child_cwd = root.join("cwd");
    let impulse_home = root.join("impulse-home");
    let workspace = root.join("workspace");
    for directory in [
        &child_home,
        &child_tmp,
        &child_cwd,
        &impulse_home,
        &workspace,
    ] {
        fs::create_dir_all(directory)
            .map_err(|error| format!("create {}: {error}", directory.display()))?;
    }
    if child_cwd.join("assets").exists() {
        return Err("isolated desktop cwd unexpectedly contains source assets".to_string());
    }

    let macos = config.app_path.join("Contents/MacOS");
    let daemon_binary = macos.join("impulse-rs");
    let desktop_binary = macos.join("impulse-desktop");
    let socket_path = impulse_home.join("sockets/impulse.sock");
    validate_macos_socket_path_length(&socket_path)?;
    let nonce = uuid::Uuid::new_v4().simple().to_string();

    let mut daemon_guard: Option<ProcessGroupGuard> = None;
    let mut desktop_guard: Option<ProcessGroupGuard> = None;
    let mut receipt_reader: Option<thread::JoinHandle<usize>> = None;

    let run_result = (|| -> Result<(), String> {
        let mut daemon_command = Command::new(&daemon_binary);
        daemon_command
            .arg("--impulse-dir")
            .arg(&impulse_home)
            .arg("--socket")
            .arg(&socket_path)
            .arg("daemon")
            .current_dir(&child_cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        set_isolated_child_env(
            &mut daemon_command,
            &child_home,
            &child_tmp,
            &impulse_home,
            &socket_path,
            &macos,
        );
        daemon_command.env(DAEMON_INSTANCE_NONCE_ENV, &nonce);
        let daemon = daemon_command
            .spawn()
            .map_err(|error| format!("launch packaged daemon: {error}"))?;
        let daemon_pid = daemon.id();
        let expected_daemon_identity = expected_daemon_identity(&nonce, daemon_pid, &impulse_home)?;
        daemon_guard = Some(ProcessGroupGuard::new(daemon, "packaged daemon")?);
        wait_for_socket(
            daemon_guard
                .as_mut()
                .expect("daemon guard was just installed"),
            &socket_path,
            DAEMON_READY_TIMEOUT,
        )?;

        let mut desktop_command = Command::new(&desktop_binary);
        desktop_command
            .current_dir(&child_cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .process_group(0);
        set_isolated_child_env(
            &mut desktop_command,
            &child_home,
            &child_tmp,
            &impulse_home,
            &socket_path,
            &macos,
        );
        desktop_command
            .env(ACCEPTANCE_NONCE_ENV, &nonce)
            .env(ACCEPTANCE_ROOT_ENV, &root)
            .env(PROVENANCE_SHA_ENV, &config.provenance_sha256)
            .env(PACKAGED_DAEMON_PID_ENV, daemon_pid.to_string());
        let mut desktop = desktop_command
            .spawn()
            .map_err(|error| format!("launch packaged desktop: {error}"))?;
        let desktop_pid = desktop.id();
        let stderr = desktop
            .stderr
            .take()
            .ok_or_else(|| "packaged desktop stderr pipe was unavailable".to_string())?;
        let (receipt_rx, reader) = spawn_receipt_reader(stderr);
        receipt_reader = Some(reader);
        desktop_guard = Some(ProcessGroupGuard::new(desktop, "packaged desktop")?);

        let receipt = wait_for_matching_receipt(
            desktop_guard
                .as_mut()
                .expect("desktop guard was just installed"),
            &receipt_rx,
            &nonce,
            RECEIPT_TIMEOUT,
        )?;
        check_cleanup_signal()?;
        validate_receipt(
            &receipt,
            &nonce,
            desktop_pid,
            &config.provenance_sha256,
            &expected_daemon_identity,
        )?;
        require_daemon_ready_at_receipt(
            daemon_guard
                .as_mut()
                .expect("daemon guard remains installed through receipt validation"),
            &socket_path,
            &expected_daemon_identity,
        )
    })();

    let mut cleanup_errors = Vec::new();
    if let Some(guard) = desktop_guard.as_mut() {
        if let Err(error) = guard.stop() {
            cleanup_errors.push(error);
        }
    }
    if let Some(reader) = receipt_reader.take() {
        match join_receipt_reader(reader, RECEIPT_READER_JOIN_TIMEOUT) {
            Ok(1) => {}
            Ok(count) => cleanup_errors.push(format!(
                "packaged desktop emitted {count} receipt lines; expected exactly one"
            )),
            Err(error) => cleanup_errors.push(error),
        }
    }
    if let Some(guard) = daemon_guard.as_mut() {
        if let Err(error) = guard.stop() {
            cleanup_errors.push(error);
        }
    }
    if !wait_for_socket_cleanup(&socket_path, SOCKET_CLEANUP_TIMEOUT) {
        cleanup_errors
            .push("isolated daemon socket remained connectable after cleanup".to_string());
    }

    match WardenSnapshot::capture(config) {
        Ok(after) => {
            if let Err(error) = before.assert_unchanged(&after) {
                cleanup_errors.push(error);
            }
        }
        Err(error) => cleanup_errors.push(format!("capture after-state wardens: {error}")),
    }

    let mut errors = Vec::new();
    if let Err(error) = run_result {
        errors.push(error);
    }
    errors.extend(cleanup_errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn set_isolated_child_env(
    command: &mut Command,
    home: &Path,
    tmp: &Path,
    impulse_home: &Path,
    socket_path: &Path,
    packaged_macos: &Path,
) {
    let path = format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", packaged_macos.display());
    command
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("CFFIXED_USER_HOME", home)
        .env("IMPULSE_HOME", impulse_home)
        .env("IMPULSE_SOCKET_PATH", socket_path)
        .env("TMPDIR", tmp)
        .env("LANG", "en_US.UTF-8")
        .env("LC_ALL", "en_US.UTF-8");
}

fn wait_for_socket(
    daemon: &mut ProcessGroupGuard,
    socket_path: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        check_cleanup_signal()?;
        if UnixStream::connect(socket_path).is_ok() {
            return Ok(());
        }
        if let Some(status) = daemon.try_wait()? {
            return Err(format!(
                "packaged daemon exited before socket readiness: {status}"
            ));
        }
        if Instant::now() >= deadline {
            return Err("packaged daemon socket readiness timed out".to_string());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_socket_cleanup(socket_path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if UnixStream::connect(socket_path).is_err() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn expected_daemon_identity(
    nonce: &str,
    daemon_pid: u32,
    impulse_root: &Path,
) -> Result<DaemonInstanceIdentity, String> {
    let impulse_root = impulse_root
        .canonicalize()
        .map_err(|error| format!("resolve isolated daemon Impulse root: {error}"))?;
    Ok(DaemonInstanceIdentity {
        protocol_version: DAEMON_PROTOCOL_VERSION,
        pid: daemon_pid,
        impulse_root: impulse_root.to_string_lossy().into_owned(),
        instance_nonce_sha256: Some(impulse_ops::daemon_instance_nonce_sha256(nonce)),
    })
}

#[cfg(target_os = "macos")]
fn kernel_peer_pid(stream: &UnixStream) -> Result<u32, String> {
    let mut peer_pid: libc::pid_t = 0;
    let mut length = libc::socklen_t::try_from(std::mem::size_of_val(&peer_pid))
        .map_err(|_| "receipt kernel peer PID size did not fit socklen_t".to_string())?;
    // SAFETY: the connected stream descriptor stays live and both output
    // pointers refer to correctly-sized writable storage for this call.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut peer_pid as *mut libc::pid_t).cast::<libc::c_void>(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(format!(
            "read receipt daemon kernel peer PID with LOCAL_PEERPID: {}",
            io::Error::last_os_error()
        ));
    }
    if usize::try_from(length).ok() != Some(std::mem::size_of_val(&peer_pid)) {
        return Err("receipt daemon kernel peer PID returned an unexpected size".to_string());
    }
    u32::try_from(peer_pid)
        .ok()
        .filter(|pid| *pid != 0)
        .ok_or_else(|| "receipt daemon kernel peer PID must be positive".to_string())
}

#[cfg(target_os = "linux")]
fn kernel_peer_pid(stream: &UnixStream) -> Result<u32, String> {
    // SAFETY: zeroed ucred is used only as a syscall output buffer and is
    // inspected only after getsockopt reports the exact expected size.
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = libc::socklen_t::try_from(std::mem::size_of_val(&credentials))
        .map_err(|_| "receipt kernel peer credentials size did not fit socklen_t".to_string())?;
    // SAFETY: the connected stream descriptor stays live and both output
    // pointers refer to correctly-sized writable storage for this call.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast::<libc::c_void>(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(format!(
            "read receipt daemon kernel peer PID with SO_PEERCRED: {}",
            io::Error::last_os_error()
        ));
    }
    if usize::try_from(length).ok() != Some(std::mem::size_of_val(&credentials)) {
        return Err(
            "receipt daemon kernel peer credentials returned an unexpected size".to_string(),
        );
    }
    u32::try_from(credentials.pid)
        .ok()
        .filter(|pid| *pid != 0)
        .ok_or_else(|| "receipt daemon kernel peer PID must be positive".to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn kernel_peer_pid(_stream: &UnixStream) -> Result<u32, String> {
    Err("receipt daemon kernel peer PID verification is unsupported on this platform".to_string())
}

fn ping_daemon_identity(
    socket_path: &Path,
    expected_peer_pid: u32,
) -> Result<DaemonInstanceIdentity, String> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|error| format!("connect to packaged daemon identity socket: {error}"))?;
    let kernel_peer_pid = kernel_peer_pid(&stream)?;
    let timeout = Some(Duration::from_secs(1));
    stream
        .set_read_timeout(timeout)
        .map_err(|error| format!("set daemon identity read timeout: {error}"))?;
    stream
        .set_write_timeout(timeout)
        .map_err(|error| format!("set daemon identity write timeout: {error}"))?;
    let mut encoded = serde_json::to_vec(&WorkbenchDaemonRequest::Ping)
        .map_err(|error| format!("encode daemon identity Ping: {error}"))?;
    encoded.push(b'\n');
    stream
        .write_all(&encoded)
        .and_then(|_| stream.flush())
        .map_err(|error| format!("write daemon identity Ping: {error}"))?;
    let mut line = String::new();
    let bytes = BufReader::new(&mut stream)
        .read_line(&mut line)
        .map_err(|error| format!("read daemon identity Ping: {error}"))?;
    if bytes == 0 {
        return Err("daemon closed before returning its typed identity".to_string());
    }
    let response: WorkbenchDaemonResponse = serde_json::from_str(&line)
        .map_err(|error| format!("decode daemon identity response: {error}"))?;
    let result = match response {
        WorkbenchDaemonResponse::Ok { result } => result,
        other => return Err(format!("daemon identity Ping failed: {other:?}")),
    };
    let ping: DaemonPingResponse = serde_json::from_value(result)
        .map_err(|error| format!("decode typed daemon identity: {error}"))?;
    if !ping.pong
        || ping.protocol_version != DAEMON_PROTOCOL_VERSION
        || ping.daemon_instance.protocol_version != ping.protocol_version
    {
        return Err("daemon identity Ping carried inconsistent protocol evidence".to_string());
    }
    if kernel_peer_pid != expected_peer_pid {
        return Err(format!(
            "packaged daemon kernel peer PID mismatch: expected {expected_peer_pid}, observed {kernel_peer_pid}"
        ));
    }
    if ping.daemon_instance.pid != kernel_peer_pid {
        return Err(
            "packaged daemon kernel peer PID did not match the typed Ping identity".to_string(),
        );
    }
    Ok(ping.daemon_instance)
}

fn require_daemon_ready_at_receipt(
    daemon: &mut ProcessGroupGuard,
    socket_path: &Path,
    expected_identity: &DaemonInstanceIdentity,
) -> Result<(), String> {
    if let Some(status) = daemon.try_wait()? {
        return Err(format!(
            "packaged daemon exited before acceptance receipt: {status}"
        ));
    }
    let observed = ping_daemon_identity(socket_path, expected_identity.pid)?;
    if &observed != expected_identity {
        return Err(
            "packaged daemon identity did not match the launched child at receipt".to_string(),
        );
    }
    Ok(())
}

fn join_receipt_reader(
    reader: thread::JoinHandle<usize>,
    timeout: Duration,
) -> Result<usize, String> {
    let deadline = Instant::now() + timeout;
    while !reader.is_finished() {
        if Instant::now() >= deadline {
            return Err(
                "packaged desktop receipt reader did not stop within its deadline".to_string(),
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
    reader
        .join()
        .map_err(|_| "packaged desktop receipt reader panicked".to_string())
}

fn wait_for_matching_receipt(
    desktop: &mut ProcessGroupGuard,
    receiver: &Receiver<ReceiptEvent>,
    expected_nonce: &str,
    timeout: Duration,
) -> Result<Value, String> {
    let deadline = Instant::now() + timeout;
    loop {
        check_cleanup_signal()?;
        if let Some(status) = desktop.try_wait()? {
            return Err(format!(
                "packaged desktop exited before a valid receipt: {status}"
            ));
        }
        let now = Instant::now();
        if now >= deadline {
            return Err("packaged desktop receipt timed out".to_string());
        }
        let wait = (deadline - now).min(Duration::from_millis(50));
        match receiver.recv_timeout(wait) {
            Ok(ReceiptEvent::Receipt(encoded)) => {
                let value: Value = serde_json::from_str(&encoded)
                    .map_err(|_| "packaged desktop emitted malformed receipt JSON".to_string())?;
                if value.get("nonce").and_then(Value::as_str) == Some(expected_nonce) {
                    return Ok(value);
                }
            }
            Ok(ReceiptEvent::ReadError) => {
                return Err("failed to read packaged desktop receipt stream".to_string());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("packaged desktop receipt stream closed before validation".to_string());
            }
        }
    }
}

fn validate_receipt(
    receipt: &Value,
    expected_nonce: &str,
    expected_pid: u32,
    expected_provenance_sha256: &str,
    expected_daemon_identity: &DaemonInstanceIdentity,
) -> Result<(), String> {
    let object = receipt
        .as_object()
        .ok_or_else(|| "packaged-host receipt must be a JSON object".to_string())?;
    require_string(object, "schema", RECEIPT_SCHEMA)?;
    require_string(object, "nonce", expected_nonce)?;
    require_u64(object, "pid", u64::from(expected_pid))?;
    require_string(object, "crate_version", env!("CARGO_PKG_VERSION"))?;
    require_string(object, "provenance_sha256", expected_provenance_sha256)?;
    let daemon_identity: DaemonInstanceIdentity = serde_json::from_value(
        object
            .get("daemon_identity")
            .cloned()
            .ok_or_else(|| "receipt daemon_identity must be present".to_string())?,
    )
    .map_err(|error| format!("receipt daemon_identity was invalid: {error}"))?;
    if &daemon_identity != expected_daemon_identity {
        return Err("receipt daemon_identity did not match the launched child".to_string());
    }
    let failure_reasons = receipt_failure_reasons(object)?;
    if object
        .get("rust_host_transcript_validated")
        .and_then(Value::as_bool)
        != Some(true)
    {
        let detail = if failure_reasons.is_empty() {
            "no bounded failure reason was supplied".to_string()
        } else {
            failure_reasons.join("; ")
        };
        return Err(format!(
            "packaged-host Rust transcript validation failed: {detail}"
        ));
    }
    require_string(object, "outcome", "passed")?;
    if !failure_reasons.is_empty() {
        return Err("passing receipt contains failure reasons".to_string());
    }

    let observation = object
        .get("observation")
        .and_then(Value::as_object)
        .ok_or_else(|| "receipt observation must be an object".to_string())?;
    require_string(observation, "host_kind", "dioxus")?;
    require_string(observation, "host_status", "dioxus-eval-bridge-ready")?;
    for field in [
        "xterm_loaded",
        "fit_addon_loaded",
        "stylesheet_loaded",
        "assets_local",
        "asset_paths_exact",
        "tauri_absent",
        "injected_test_api_absent",
        "ops_bridge_mounted",
        "terminal_interop_mounted",
        "xterm_session_mounted",
        "agent_snapshot_array",
        "agent_platforms_array",
        "workspaces_array",
        "mcp_descriptors_array",
        "review_queue_array",
        "unknown_command_rejected",
        "workspace_registered",
        "workspace_listed",
        "daemon_connected",
        "terminal_opened",
        "terminal_input",
        "terminal_output",
        "terminal_resized",
        "xterm_on_data_api_called",
        "xterm_on_resize_api_called",
        "xterm_output_buffer_rendered",
        "terminal_focused",
        "terminal_closed",
        "terminal_exited",
    ] {
        require_bool(observation, field, true)?;
    }
    require_bool(observation, "terminal_interop_degraded", false)?;
    require_string_contains(observation, "unknown_command_error", "unknown host command")?;
    Ok(())
}

fn receipt_failure_reasons(object: &Map<String, Value>) -> Result<Vec<String>, String> {
    let values = object
        .get("failure_reasons")
        .and_then(Value::as_array)
        .ok_or_else(|| "receipt failure_reasons must be an array".to_string())?;
    if values.len() > MAX_RECEIPT_FAILURE_REASONS {
        return Err("receipt carried too many failure reasons".to_string());
    }
    values
        .iter()
        .map(|value| {
            let reason = value
                .as_str()
                .ok_or_else(|| "receipt failure reason must be a string".to_string())?;
            if reason.chars().count() > MAX_RECEIPT_FAILURE_REASON_CHARS {
                return Err("receipt failure reason exceeded its character bound".to_string());
            }
            Ok(reason
                .chars()
                .map(|character| {
                    if character.is_control() {
                        ' '
                    } else {
                        character
                    }
                })
                .collect())
        })
        .collect()
}

fn require_string(object: &Map<String, Value>, field: &str, expected: &str) -> Result<(), String> {
    match object.get(field).and_then(Value::as_str) {
        Some(actual) if actual == expected => Ok(()),
        _ => Err(format!(
            "receipt field `{field}` did not match the acceptance contract"
        )),
    }
}

fn require_string_contains(
    object: &Map<String, Value>,
    field: &str,
    expected_fragment: &str,
) -> Result<(), String> {
    match object.get(field).and_then(Value::as_str) {
        Some(actual) if actual.contains(expected_fragment) => Ok(()),
        _ => Err(format!(
            "receipt field `{field}` did not match the acceptance contract"
        )),
    }
}

fn require_u64(object: &Map<String, Value>, field: &str, expected: u64) -> Result<(), String> {
    match object.get(field).and_then(Value::as_u64) {
        Some(actual) if actual == expected => Ok(()),
        _ => Err(format!(
            "receipt field `{field}` did not match the acceptance contract"
        )),
    }
}

fn require_bool(object: &Map<String, Value>, field: &str, expected: bool) -> Result<(), String> {
    match object.get(field).and_then(Value::as_bool) {
        Some(actual) if actual == expected => Ok(()),
        _ => Err(format!(
            "receipt field `{field}` did not match the acceptance contract"
        )),
    }
}

fn required_env(name: &str) -> Result<String, String> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} is required for the ignored packaged-host test"))
}

fn canonical_dir_from_env(name: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(required_env(name)?);
    let canonical = fs::canonicalize(&path)
        .map_err(|error| format!("canonicalize {name} directory: {error}"))?;
    if !canonical.is_dir() {
        return Err(format!("{name} must name a directory"));
    }
    Ok(canonical)
}

fn validate_app_bundle(app_path: &Path) -> Result<(), String> {
    if app_path.extension().and_then(|value| value.to_str()) != Some("app") {
        return Err(format!("{APP_PATH_ENV} must name an .app bundle"));
    }
    for relative in [
        "Contents/MacOS/impulse-desktop",
        "Contents/MacOS/impulse-rs",
        "Contents/Resources/ReleaseProvenance.v1.tsv",
    ] {
        let path = app_path.join(relative);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("inspect packaged payload {relative}: {error}"))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() == 0 {
            return Err(format!(
                "packaged payload is not a non-empty regular file: {relative}"
            ));
        }
        if relative.starts_with("Contents/MacOS/") && metadata.permissions().mode() & 0o111 == 0 {
            return Err(format!("packaged executable bit is missing: {relative}"));
        }
    }
    Ok(())
}

fn validate_git_root(root: &Path, env_name: &str) -> Result<(), String> {
    let output = git_output(root, &["rev-parse", "--show-toplevel"])?;
    let reported =
        String::from_utf8(output).map_err(|_| format!("Git root for {env_name} was not UTF-8"))?;
    let reported = fs::canonicalize(reported.trim())
        .map_err(|error| format!("canonicalize Git root for {env_name}: {error}"))?;
    if reported != root {
        return Err(format!("{env_name} must name an exact Git worktree root"));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn require_read_only_mount(app_path: &Path) -> Result<(), String> {
    let output = Command::new("/sbin/mount")
        .output()
        .map_err(|error| format!("inspect mounted package: {error}"))?;
    if !output.status.success() {
        return Err("mount inspection failed".to_string());
    }
    let mounts = String::from_utf8(output.stdout)
        .map_err(|_| "mount inspection output was not UTF-8".to_string())?;
    if path_is_on_read_only_mount(app_path, &mounts) {
        Ok(())
    } else {
        Err("packaged app must be launched from a read-only mounted filesystem".to_string())
    }
}

fn path_is_on_read_only_mount(path: &Path, mounts: &str) -> bool {
    let candidate = path.to_string_lossy();
    let mut best: Option<(&str, &str)> = None;
    for line in mounts.lines() {
        let Some((_, remainder)) = line.split_once(" on ") else {
            continue;
        };
        let Some((mount_point, options)) = remainder.split_once(" (") else {
            continue;
        };
        if path_has_prefix(&candidate, mount_point)
            && best
                .as_ref()
                .is_none_or(|(current, _)| mount_point.len() > current.len())
        {
            best = Some((mount_point, options));
        }
    }
    best.is_some_and(|(_, options)| {
        options
            .trim_end_matches(')')
            .split(',')
            .any(|option| option.trim() == "read-only")
    })
}

fn path_has_prefix(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn git_visible_fingerprint(root: &Path) -> Result<OpaqueFingerprint, String> {
    let mut hasher = Sha256::new();
    let commands: &[&[&str]] = &[
        &["rev-parse", "HEAD"],
        &["status", "--porcelain=v2", "--untracked-files=all"],
        &["diff", "--no-ext-diff", "--binary"],
        &["diff", "--cached", "--no-ext-diff", "--binary"],
    ];
    for args in commands {
        hash_part(&mut hasher, b"git-command");
        for arg in *args {
            hash_part(&mut hasher, arg.as_bytes());
        }
        hash_part(&mut hasher, &git_output(root, args)?);
    }
    let mut entries = u64::try_from(commands.len()).unwrap_or(u64::MAX);
    hash_part(&mut hasher, b"untracked-non-ignored-content");
    for relative_path in git_untracked_paths(root)? {
        fingerprint_entry(root, &root.join(relative_path), &mut hasher, &mut entries)?;
    }
    Ok(OpaqueFingerprint {
        exists: true,
        entries,
        digest: hasher.finalize().into(),
    })
}

fn git_untracked_paths(root: &Path) -> Result<Vec<PathBuf>, String> {
    let output = git_output(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    if !output.is_empty() && output.last() != Some(&0) {
        return Err("Git returned a malformed untracked-path inventory".to_string());
    }

    let mut paths = Vec::new();
    for encoded in output
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
    {
        let path = PathBuf::from(OsStr::from_bytes(encoded));
        if !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err("Git returned an unsafe untracked path".to_string());
        }
        paths.push(path);
    }
    paths.sort();
    Ok(paths)
}

fn git_output(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|error| format!("run Git warden command: {error}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err("Git warden command failed".to_string())
    }
}

fn path_fingerprint(path: &Path) -> Result<OpaqueFingerprint, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) => return absent_fingerprint_from_metadata_error(error),
    }
    let mut hasher = Sha256::new();
    let mut entries = 0_u64;
    fingerprint_entry(path, path, &mut hasher, &mut entries)?;
    Ok(OpaqueFingerprint {
        exists: true,
        entries,
        digest: hasher.finalize().into(),
    })
}

fn absent_fingerprint_from_metadata_error(error: io::Error) -> Result<OpaqueFingerprint, String> {
    if error.kind() == io::ErrorKind::NotFound {
        Ok(OpaqueFingerprint {
            exists: false,
            entries: 0,
            digest: [0; 32],
        })
    } else {
        Err(format!(
            "fingerprint protected state metadata failed closed: {error}"
        ))
    }
}

#[cfg(target_os = "macos")]
fn validate_macos_socket_path_length(socket_path: &Path) -> Result<(), String> {
    let encoded_len = socket_path.as_os_str().as_bytes().len();
    if encoded_len < MACOS_UNIX_SOCKET_PATH_CAPACITY {
        Ok(())
    } else {
        Err(format!(
            "isolated daemon socket path is {encoded_len} bytes; macOS requires fewer than {MACOS_UNIX_SOCKET_PATH_CAPACITY}"
        ))
    }
}

fn hash_part(hasher: &mut Sha256, bytes: &[u8]) {
    let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    hasher.update(length.to_le_bytes());
    hasher.update(bytes);
}

fn fingerprint_entry(
    root: &Path,
    path: &Path,
    hasher: &mut Sha256,
    entries: &mut u64,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("fingerprint protected state metadata: {error}"))?;
    *entries = entries.saturating_add(1);
    let relative_path = path
        .strip_prefix(root)
        .unwrap_or(path)
        .as_os_str()
        .as_encoded_bytes();
    hash_part(hasher, relative_path);
    hasher.update(metadata.mode().to_le_bytes());
    hasher.update(metadata.len().to_le_bytes());
    hasher.update(metadata.mtime().to_le_bytes());
    hasher.update(metadata.mtime_nsec().to_le_bytes());

    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(
            "protected state contains a symlink; acceptance requires a non-symlink tree"
                .to_string(),
        );
    } else if file_type.is_dir() {
        hasher.update(b"D");
        let mut children = fs::read_dir(path)
            .map_err(|error| format!("fingerprint protected directory: {error}"))?
            .map(|entry| entry.map(|value| value.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("enumerate protected directory: {error}"))?;
        children.sort();
        for child in children {
            fingerprint_entry(root, &child, hasher, entries)?;
        }
    } else if file_type.is_file() {
        hasher.update(b"F");
        let mut file =
            File::open(path).map_err(|error| format!("fingerprint protected file: {error}"))?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| format!("read protected file for fingerprint: {error}"))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
    } else if file_type.is_socket() {
        hasher.update(b"S");
    } else if file_type.is_fifo() {
        hasher.update(b"P");
    } else if file_type.is_block_device() {
        hasher.update(b"B");
    } else if file_type.is_char_device() {
        hasher.update(b"C");
    } else {
        hasher.update(b"?");
    }
    Ok(())
}

fn signal_process_group(pgid: i32, signal: &str) {
    let _ = Command::new("/bin/kill")
        .arg(format!("-{signal}"))
        .arg(format!("-{pgid}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn process_group_exists(pgid: i32) -> bool {
    Command::new("/bin/kill")
        .arg("-0")
        .arg(format!("-{pgid}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn wait_for_group_exit(pgid: i32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !process_group_exists(pgid) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "macos")]
fn signal_process(pid: i32, signal: &str) {
    let _ = Command::new("/bin/kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(target_os = "macos")]
fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                return Err("parent-death helper did not exit within its deadline".to_string())
            }
            Err(error) => return Err(format!("poll parent-death helper: {error}")),
        }
    }
}

#[cfg(target_os = "macos")]
fn inspect_process_identity(pid: i32) -> Result<Option<ProcessIdentity>, String> {
    let output = Command::new("/bin/ps")
        .args([
            "-ww",
            "-p",
            &pid.to_string(),
            "-o",
            "ppid=",
            "-o",
            "command=",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("inspect parent-death shell identity: {error}"))?;
    if !output.status.success() {
        return Ok(None);
    }

    let rendered = String::from_utf8(output.stdout)
        .map_err(|_| "parent-death shell identity was not UTF-8".to_string())?;
    let rendered = rendered.trim();
    if rendered.is_empty() {
        return Ok(None);
    }
    let split_at = rendered
        .find(|character: char| character.is_whitespace())
        .ok_or_else(|| "parent-death shell identity omitted its command".to_string())?;
    let parent_pid = rendered[..split_at]
        .parse::<i32>()
        .map_err(|_| "parent-death shell identity contained an invalid parent pid".to_string())?;
    let command = rendered[split_at..].trim_start().to_string();
    if command.is_empty() {
        return Ok(None);
    }
    // SAFETY: getpgid/getsid are read-only process-table queries for the
    // positive, nonce-bound pid parsed from the shell readiness record.
    let process_group_id = unsafe { libc::getpgid(pid) };
    if process_group_id == -1 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(None);
        }
        return Err(format!("inspect parent-death shell process group: {error}"));
    }
    // SAFETY: same read-only, validated-pid query as getpgid above.
    let session_id = unsafe { libc::getsid(pid) };
    if session_id == -1 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(None);
        }
        return Err(format!("inspect parent-death shell session: {error}"));
    }
    Ok(Some(ProcessIdentity {
        parent_pid,
        process_group_id,
        session_id,
        command,
    }))
}

#[cfg(target_os = "macos")]
fn process_identity_matches(pid: i32, nonce: &str) -> bool {
    matches!(
        inspect_process_identity(pid),
        Ok(Some(identity)) if identity.command.contains(nonce)
    )
}

#[cfg(target_os = "macos")]
fn wait_for_process_identity_exit(pid: i32, nonce: &str, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        match inspect_process_identity(pid)? {
            None => return Ok(()),
            Some(identity) if !identity.command.contains(nonce) => return Ok(()),
            Some(identity) if Instant::now() >= deadline => {
                return Err(format!(
                    "nonce-bound PTY shell remained after parent death with parent pid {}",
                    identity.parent_pid
                ));
            }
            Some(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

#[cfg(target_os = "macos")]
fn wait_for_parent_death_readiness(
    ready_path: &Path,
    fixture: &mut ParentDeathFixtureGuard,
    expected_nonce: &str,
    timeout: Duration,
) -> Result<i32, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match fs::read_to_string(ready_path) {
            Ok(encoded) if encoded.ends_with('\n') => {
                let fields = encoded.trim_end().split('\t').collect::<Vec<_>>();
                if fields.len() != 2 || fields[1] != expected_nonce {
                    return Err("parent-death readiness record failed nonce validation".to_string());
                }
                let pid = fields[0].parse::<i32>().map_err(|_| {
                    "parent-death readiness record contained an invalid pid".to_string()
                })?;
                if pid <= 1 {
                    return Err("parent-death readiness record contained an unsafe pid".to_string());
                }
                return Ok(pid);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("read parent-death readiness record: {error}")),
        }

        if let Some(status) = fixture.try_wait()? {
            return Err(format!(
                "parent-death helper exited before shell readiness: {status}"
            ));
        }
        if Instant::now() >= deadline {
            return Err("parent-death shell readiness timed out".to_string());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "macos")]
fn run_parent_death_helper() -> ! {
    let nonce =
        env::var(PTY_PARENT_DEATH_NONCE_ENV).expect("parent-death helper requires its nonce");
    let ready_path = PathBuf::from(
        env::var_os(PTY_PARENT_DEATH_READY_ENV)
            .expect("parent-death helper requires its readiness path"),
    );
    let working_dir = ready_path
        .parent()
        .expect("parent-death readiness path must have a parent");
    let script = r#"printf '%s\t%s\n' "$$" "$1" > "$2"; while :; do sleep 60; done"#;
    let args = vec![
        "-c".to_string(),
        script.to_string(),
        "impulse-parent-death-shell".to_string(),
        nonce,
        ready_path.to_string_lossy().into_owned(),
    ];
    let env_vars: [(&str, String); 0] = [];
    let _terminal = TerminalBackend::spawn(
        "/bin/sh",
        &args,
        Some(working_dir),
        &env_vars,
        24,
        80,
        Some(100),
    )
    .expect("spawn parent-death PTY shell");

    loop {
        thread::park_timeout(Duration::from_secs(60));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn test_terminal_backend_shell_exits_when_harness_parent_is_sigkilled() {
    if env::var_os(PTY_PARENT_DEATH_HELPER_ENV).is_some() {
        run_parent_death_helper();
    }

    let root = tempfile::Builder::new()
        .prefix("impulse-pty-parent-death-")
        .tempdir_in("/tmp")
        .expect("create short parent-death fixture root");
    let ready_path = root.path().join("shell-ready.tsv");
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let test_name = "test_terminal_backend_shell_exits_when_harness_parent_is_sigkilled";
    let mut command =
        Command::new(env::current_exe().expect("locate packaged harness test binary"));
    command
        .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
        .env(PTY_PARENT_DEATH_HELPER_ENV, "1")
        .env(PTY_PARENT_DEATH_NONCE_ENV, &nonce)
        .env(PTY_PARENT_DEATH_READY_ENV, &ready_path)
        .current_dir(root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let helper = command.spawn().expect("spawn parent-death helper");
    let mut fixture = ParentDeathFixtureGuard::new(helper).expect("guard parent-death helper");

    let shell_pid = wait_for_parent_death_readiness(
        &ready_path,
        &mut fixture,
        &nonce,
        PTY_PARENT_DEATH_TIMEOUT,
    )
    .expect("observe nonce-bound PTY shell readiness");
    fixture.bind_shell(shell_pid, nonce.clone());
    let identity = inspect_process_identity(shell_pid)
        .expect("inspect nonce-bound PTY shell")
        .expect("nonce-bound PTY shell must still exist before parent death");
    assert_eq!(identity.parent_pid, fixture.helper_pid());
    assert_eq!(identity.process_group_id, shell_pid);
    assert_eq!(identity.session_id, shell_pid);
    assert_ne!(identity.process_group_id, fixture.helper_pid());
    assert_ne!(identity.session_id, fixture.helper_pid());
    assert!(identity.command.contains("/bin/sh"));
    assert!(identity.command.contains(&nonce));

    let helper_status = fixture
        .kill_helper_abruptly()
        .expect("SIGKILL and reap parent-death helper");
    assert_eq!(helper_status.signal(), Some(libc::SIGKILL));
    wait_for_process_identity_exit(shell_pid, &nonce, PTY_PARENT_DEATH_TIMEOUT)
        .expect("the exact nonce-bound PTY shell must exit after its parent is SIGKILLed");
    fixture.disarm_shell();
}

#[test]
fn test_validate_receipt_accepts_complete_contract() {
    let identity = receipt_identity_fixture("expected");
    let receipt = valid_receipt_fixture("expected", 42, &"a".repeat(64), &identity);
    validate_receipt(&receipt, "expected", 42, &"a".repeat(64), &identity)
        .expect("complete receipt must satisfy the Rust-owned contract");
}

#[test]
fn test_harness_receipt_deadline_strictly_outlives_observer_deadline() {
    let observer_timeout = Duration::from_secs(PACKAGED_OBSERVATION_TIMEOUT_SECS);
    assert!(RECEIPT_TIMEOUT > observer_timeout);
    assert_eq!(
        RECEIPT_TIMEOUT - observer_timeout,
        Duration::from_secs(RECEIPT_TIMEOUT_GRACE_SECS)
    );
    const {
        assert!(
            PACKAGED_INVOKE_DRAIN_TIMEOUT_SECS + PACKAGED_TERMINAL_CLEANUP_TIMEOUT_SECS
                < RECEIPT_TIMEOUT_GRACE_SECS
        );
    }
}

#[test]
fn test_daemon_receipt_guard_rejects_exact_identity_forged_by_the_wrong_kernel_peer() {
    let root = tempfile::tempdir().expect("temporary daemon receipt guard root");
    let socket = root.path().join("daemon.sock");
    let listener = UnixListener::bind(&socket).expect("bind daemon receipt guard socket");
    let mut command = Command::new("/bin/sleep");
    command
        .arg("60")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let child = command.spawn().expect("spawn daemon receipt guard process");
    let child_pid = child.id();
    let mut guard = ProcessGroupGuard::new(child, "daemon receipt guard").expect("guard process");
    let nonce = "ab".repeat(16);
    let expected = expected_daemon_identity(&nonce, child_pid, root.path())
        .expect("build expected daemon identity");
    let forged = expected.clone();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept daemon identity Ping");
        let mut request = String::new();
        BufReader::new(stream.try_clone().expect("clone fake daemon stream"))
            .read_line(&mut request)
            .expect("read daemon identity Ping");
        let request: WorkbenchDaemonRequest =
            serde_json::from_str(&request).expect("decode daemon identity Ping");
        assert_eq!(request, WorkbenchDaemonRequest::Ping);
        let response = WorkbenchDaemonResponse::Ok {
            result: serde_json::to_value(DaemonPingResponse {
                pong: true,
                protocol_version: DAEMON_PROTOCOL_VERSION,
                daemon_instance: forged,
            })
            .expect("encode forged daemon identity"),
        };
        let mut encoded = serde_json::to_vec(&response).expect("encode fake daemon response");
        encoded.push(b'\n');
        stream
            .write_all(&encoded)
            .expect("write fake daemon identity");
    });

    let error = require_daemon_ready_at_receipt(&mut guard, &socket, &expected)
        .expect_err("an unrelated listener must not be composable with a live child");
    assert!(error.contains("kernel peer PID"), "{error}");
    server.join().expect("join fake daemon identity server");
    fs::remove_file(&socket).expect("remove daemon receipt guard socket");
    let error = require_daemon_ready_at_receipt(&mut guard, &socket, &expected)
        .expect_err("missing socket must fail even while the daemon process remains alive");
    assert!(error.contains("connect to packaged daemon identity socket"));
    guard.stop().expect("stop daemon receipt guard process");
}

#[test]
fn test_receipt_reader_join_has_a_bounded_failure_path() {
    let reader = thread::spawn(|| {
        thread::sleep(Duration::from_millis(100));
        0
    });
    let started = Instant::now();
    let error = join_receipt_reader(reader, Duration::from_millis(5))
        .expect_err("a blocked reader must fail its bounded join");
    assert!(error.contains("deadline"));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn test_validate_receipt_rejects_wrong_nonce_and_missing_observation() {
    let identity = receipt_identity_fixture("expected");
    let receipt = valid_receipt_fixture("wrong", 42, &"a".repeat(64), &identity);
    let error = validate_receipt(&receipt, "expected", 42, &"a".repeat(64), &identity)
        .expect_err("wrong nonce must fail closed");
    assert!(error.contains("nonce"));

    let mut missing_observation = valid_receipt_fixture("expected", 42, &"a".repeat(64), &identity);
    missing_observation
        .as_object_mut()
        .expect("fixture is an object")
        .remove("observation");
    let error = validate_receipt(
        &missing_observation,
        "expected",
        42,
        &"a".repeat(64),
        &identity,
    )
    .expect_err("missing observation must fail closed");
    assert!(error.contains("observation"));

    let mut wrong_unknown_command =
        valid_receipt_fixture("expected", 42, &"a".repeat(64), &identity);
    wrong_unknown_command["observation"]["unknown_command_error"] =
        Value::String("different failure".to_string());
    let error = validate_receipt(
        &wrong_unknown_command,
        "expected",
        42,
        &"a".repeat(64),
        &identity,
    )
    .expect_err("untyped unknown-command failure must fail closed");
    assert!(error.contains("unknown_command_error"));
}

#[test]
fn test_validate_receipt_surfaces_transcript_failure_reasons() {
    let identity = receipt_identity_fixture("expected");
    let mut receipt = valid_receipt_fixture("expected", 42, &"a".repeat(64), &identity);
    receipt["rust_host_transcript_validated"] = Value::Bool(false);
    receipt["outcome"] = Value::String("failed".to_string());
    receipt["failure_reasons"] = serde_json::json!([
        "Rust host transcript did not prove terminal output marker",
        "Rust host transcript did not prove terminal exit event"
    ]);

    let error = validate_receipt(&receipt, "expected", 42, &"a".repeat(64), &identity)
        .expect_err("a failed transcript must remain rejected");
    assert!(error.contains("terminal output marker"), "{error}");
    assert!(error.contains("terminal exit event"), "{error}");
}

fn receipt_identity_fixture(nonce: &str) -> DaemonInstanceIdentity {
    DaemonInstanceIdentity {
        protocol_version: DAEMON_PROTOCOL_VERSION,
        pid: 84,
        impulse_root: "/private/tmp/acceptance/impulse-home".to_string(),
        instance_nonce_sha256: Some(impulse_ops::daemon_instance_nonce_sha256(nonce)),
    }
}

fn valid_receipt_fixture(
    nonce: &str,
    pid: u32,
    provenance_sha256: &str,
    daemon_identity: &DaemonInstanceIdentity,
) -> Value {
    serde_json::json!({
        "schema": RECEIPT_SCHEMA,
        "nonce": nonce,
        "pid": pid,
        "crate_version": env!("CARGO_PKG_VERSION"),
        "provenance_sha256": provenance_sha256,
        "daemon_identity": daemon_identity,
        "rust_host_transcript_validated": true,
        "outcome": "passed",
        "failure_reasons": [],
        "observation": {
            "host_kind": "dioxus",
            "host_status": "dioxus-eval-bridge-ready",
            "xterm_loaded": true,
            "fit_addon_loaded": true,
            "stylesheet_loaded": true,
            "assets_local": true,
            "asset_paths_exact": true,
            "tauri_absent": true,
            "injected_test_api_absent": true,
            "ops_bridge_mounted": true,
            "terminal_interop_mounted": true,
            "terminal_interop_degraded": false,
            "xterm_session_mounted": true,
            "agent_snapshot_array": true,
            "agent_platforms_array": true,
            "workspaces_array": true,
            "mcp_descriptors_array": true,
            "review_queue_array": true,
            "unknown_command_rejected": true,
            "unknown_command_error": "unknown host command `sentinel`",
            "workspace_registered": true,
            "workspace_listed": true,
            "daemon_connected": true,
            "terminal_opened": true,
            "terminal_input": true,
            "terminal_output": true,
            "terminal_resized": true,
            "xterm_on_data_api_called": true,
            "xterm_on_resize_api_called": true,
            "xterm_output_buffer_rendered": true,
            "terminal_focused": true,
            "terminal_closed": true,
            "terminal_exited": true
        }
    })
}

#[test]
fn test_path_fingerprint_changes_without_exposing_file_contents() {
    let root = tempfile::tempdir().expect("temporary fingerprint root");
    let file = root.path().join("secret.txt");
    fs::write(&file, b"first private value").expect("write first fixture value");
    let before = path_fingerprint(root.path()).expect("fingerprint before");
    fs::write(&file, b"second private value").expect("write second fixture value");
    let after = path_fingerprint(root.path()).expect("fingerprint after");
    assert_ne!(before, after);
    assert!(!format!("{before:?}{after:?}").contains("private value"));
}

#[test]
fn test_git_visible_fingerprint_excludes_ignored_target_but_named_impulse_state_is_wardened() {
    let root = tempfile::tempdir().expect("temporary Git-visible fingerprint root");
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(root.path())
            .args(args)
            .env("GIT_AUTHOR_NAME", "Impulse Acceptance")
            .env("GIT_AUTHOR_EMAIL", "acceptance@example.invalid")
            .env("GIT_COMMITTER_NAME", "Impulse Acceptance")
            .env("GIT_COMMITTER_EMAIL", "acceptance@example.invalid")
            .output()
            .expect("run fixture Git command");
        assert!(
            output.status.success(),
            "fixture Git command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q"]);
    fs::write(root.path().join(".gitignore"), ".impulse/\ntarget/\n")
        .expect("write fixture ignore rules");
    fs::write(root.path().join("tracked.txt"), "stable\n").expect("write tracked fixture");
    git(&["add", ".gitignore", "tracked.txt"]);
    git(&["commit", "-q", "-m", "fixture"]);

    let before_git = git_visible_fingerprint(root.path()).expect("Git-visible baseline");
    let before_root_state =
        path_fingerprint(&root.path().join(".impulse")).expect("absent root state baseline");
    let before_nested_state = path_fingerprint(&root.path().join("impulse-rs/.impulse"))
        .expect("absent nested state baseline");

    fs::create_dir_all(root.path().join("target/debug")).expect("create ignored target");
    fs::write(root.path().join("target/debug/cache"), "churn").expect("write ignored target churn");
    assert_eq!(
        before_git,
        git_visible_fingerprint(root.path()).expect("Git-visible fingerprint after target churn")
    );

    fs::create_dir_all(root.path().join(".impulse")).expect("create root Impulse state");
    fs::write(root.path().join(".impulse/state"), "root-state").expect("write root Impulse state");
    fs::create_dir_all(root.path().join("impulse-rs/.impulse"))
        .expect("create nested Impulse state");
    fs::write(
        root.path().join("impulse-rs/.impulse/state"),
        "nested-state",
    )
    .expect("write nested Impulse state");

    assert_eq!(
        before_git,
        git_visible_fingerprint(root.path()).expect("Git-visible fingerprint ignores state")
    );
    assert_ne!(
        before_root_state,
        path_fingerprint(&root.path().join(".impulse")).expect("root state fingerprint")
    );
    assert_ne!(
        before_nested_state,
        path_fingerprint(&root.path().join("impulse-rs/.impulse"))
            .expect("nested state fingerprint")
    );

    let created_root_state =
        path_fingerprint(&root.path().join(".impulse")).expect("created root state fingerprint");
    fs::write(root.path().join(".impulse/state"), "root-state-changed")
        .expect("change root Impulse state");
    assert_ne!(
        created_root_state,
        path_fingerprint(&root.path().join(".impulse")).expect("changed root state fingerprint")
    );
}

#[test]
fn test_path_fingerprint_treats_only_not_found_as_absent() {
    let root = tempfile::tempdir().expect("temporary absent fingerprint root");
    let absent = path_fingerprint(&root.path().join("missing"))
        .expect("a genuinely missing protected path may be represented as absent");
    assert!(!absent.exists);

    let error =
        absent_fingerprint_from_metadata_error(io::Error::from(io::ErrorKind::PermissionDenied))
            .expect_err("permission errors must not collapse to absent state");
    assert!(error.contains("failed closed"));
}

#[cfg(target_os = "macos")]
#[test]
fn test_macos_socket_path_length_fails_before_daemon_launch() {
    let short = Path::new("/tmp/i/impulse.sock");
    validate_macos_socket_path_length(short).expect("short isolated socket path");

    let long = PathBuf::from(format!(
        "/tmp/{}/impulse.sock",
        "x".repeat(MACOS_UNIX_SOCKET_PATH_CAPACITY)
    ));
    let error = validate_macos_socket_path_length(&long)
        .expect_err("overlong macOS socket path must fail before bind");
    assert!(error.contains("macOS requires fewer"));
}

#[test]
fn test_path_fingerprint_rejects_symlink_backed_protected_state() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("temporary symlink fingerprint root");
    let outside = tempfile::tempdir().expect("temporary symlink target root");
    fs::write(outside.path().join("state"), b"private").expect("write symlink target fixture");
    symlink(outside.path(), root.path().join("linked-state")).expect("create protected symlink");

    let error = path_fingerprint(root.path())
        .expect_err("protected state must fail closed when a symlink is present");
    assert!(error.contains("contains a symlink"));
    assert!(!error.contains("linked-state"));
}

#[test]
fn test_read_only_mount_parser_selects_longest_matching_mount() {
    let mounts = concat!(
        "/dev/disk1 on / (apfs, local, journaled)\n",
        "/dev/disk9 on /private/tmp/impulse-mount (hfs, local, read-only, noowners)\n",
    );
    assert!(path_is_on_read_only_mount(
        Path::new("/private/tmp/impulse-mount/Impulse.app"),
        mounts
    ));
    assert!(!path_is_on_read_only_mount(
        Path::new("/private/tmp/outside/Impulse.app"),
        mounts
    ));
}

#[test]
fn test_process_group_guard_terminates_background_descendant() {
    let marker = tempfile::tempdir().expect("temporary process fixture");
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "trap '' TERM; sleep 60 & wait"])
        .current_dir(marker.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let child = command.spawn().expect("spawn process-group fixture");
    let mut guard = ProcessGroupGuard::new(child, "process fixture").expect("guard fixture");
    assert!(process_group_exists(guard.pgid));
    guard
        .stop()
        .expect("terminate entire fixture process group");
    assert!(!process_group_exists(guard.pgid));
}
