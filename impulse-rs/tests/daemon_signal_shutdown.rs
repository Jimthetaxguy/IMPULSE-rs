#![cfg(unix)]

//! Real-process proof that the daemon handles operator and desktop shutdown
//! signals without losing its final state or leaving stale runtime files.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

use impulse_rs::client::DaemonClient;
use impulse_rs::state::LiveState;

const IMPULSE_BIN: &str = env!("CARGO_BIN_EXE_impulse-rs");

struct DaemonGuard {
    child: Option<Child>,
}

impl DaemonGuard {
    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("daemon child must be present")
    }

    fn stderr(&mut self) -> String {
        let mut stderr = String::new();
        if let Some(pipe) = self.child_mut().stderr.as_mut() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        stderr
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct OwnerParentGuard {
    parent: Option<Child>,
    daemon_pid_path: PathBuf,
    daemon_pid: Option<u32>,
}

impl OwnerParentGuard {
    fn parent_mut(&mut self) -> &mut Child {
        self.parent
            .as_mut()
            .expect("owner parent process must be present")
    }

    fn daemon_pid(&mut self) -> u32 {
        if let Some(pid) = self.daemon_pid {
            return pid;
        }
        let pid = std::fs::read_to_string(&self.daemon_pid_path)
            .expect("owned daemon PID marker must be readable")
            .trim()
            .parse()
            .expect("owned daemon PID marker must be numeric");
        self.daemon_pid = Some(pid);
        pid
    }

    fn disarm(&mut self) {
        self.parent = None;
        self.daemon_pid = None;
    }
}

impl Drop for OwnerParentGuard {
    fn drop(&mut self) {
        if let Some(mut parent) = self.parent.take() {
            let _ = parent.kill();
            let _ = parent.wait();
        }
        let daemon_pid = self.daemon_pid.or_else(|| {
            std::fs::read_to_string(&self.daemon_pid_path)
                .ok()?
                .trim()
                .parse()
                .ok()
        });
        let Some(daemon_pid) = daemon_pid else {
            return;
        };
        if !process_is_running(daemon_pid) {
            return;
        }
        // SAFETY: this is the exact PID recorded by the test-owned helper for
        // its daemon child.
        let _ = unsafe { libc::kill(daemon_pid as libc::pid_t, libc::SIGTERM) };
        for _ in 0..40 {
            if !process_is_running(daemon_pid) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        // SAFETY: same exact test-owned PID as above; this is only a final
        // failure cleanup path.
        let _ = unsafe { libc::kill(daemon_pid as libc::pid_t, libc::SIGKILL) };
    }
}

fn daemon_command(impulse_dir: &Path, owner_pid: Option<u32>) -> Command {
    let mut command = Command::new(IMPULSE_BIN);
    command.arg("-c").arg(impulse_dir);
    if let Some(owner_pid) = owner_pid {
        command.arg("--owner-pid").arg(owner_pid.to_string());
    }
    command
        .arg("daemon")
        .env("IMPULSE_TEST_MODE", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

fn spawn_daemon(impulse_dir: &std::path::Path) -> DaemonGuard {
    let child = daemon_command(impulse_dir, None)
        .spawn()
        .expect("daemon process must launch");
    DaemonGuard { child: Some(child) }
}

fn spawn_owned_daemon_parent(temp_root: &Path, impulse_dir: &Path) -> OwnerParentGuard {
    let daemon_pid_path = temp_root.join("owned-daemon.pid");
    let release_path = temp_root.join("release-owner");
    let daemon_stdout_path = temp_root.join("owned-daemon.stdout");
    let daemon_stderr_path = temp_root.join("owned-daemon.stderr");
    let script = r#"
set -eu
trap '' HUP
"$IMPULSE_BIN" --impulse-dir "$IMPULSE_DIR" --owner-pid "$$" daemon \
    >"$DAEMON_STDOUT" 2>"$DAEMON_STDERR" &
daemon_pid=$!
printf '%s' "$daemon_pid" >"$DAEMON_PID_PATH"
while [ ! -e "$RELEASE_PATH" ]; do
    sleep 0.02
done
exit 0
"#;
    let parent = Command::new("sh")
        .arg("-c")
        .arg(script)
        .env("IMPULSE_BIN", IMPULSE_BIN)
        .env("IMPULSE_DIR", impulse_dir)
        .env("DAEMON_PID_PATH", &daemon_pid_path)
        .env("RELEASE_PATH", release_path)
        .env("DAEMON_STDOUT", daemon_stdout_path)
        .env("DAEMON_STDERR", daemon_stderr_path)
        .env("IMPULSE_TEST_MODE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("owner parent process must launch");
    OwnerParentGuard {
        parent: Some(parent),
        daemon_pid_path,
        daemon_pid: None,
    }
}

fn process_is_running(pid: u32) -> bool {
    // SAFETY: signal 0 performs existence/permission probing without
    // delivering a signal.
    let probe = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if probe != 0 {
        return std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH);
    }
    let output = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let state = String::from_utf8_lossy(&output.stdout);
            !state.trim().is_empty() && !state.trim_start().starts_with('Z')
        }
        _ => true,
    }
}

async fn wait_until_ready(child: &mut Child, socket_path: &std::path::Path) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if std::os::unix::net::UnixStream::connect(socket_path).is_ok() {
                break;
            }
            if let Some(status) = child
                .try_wait()
                .expect("daemon child status must be readable")
            {
                panic!("daemon exited before its socket was ready: {status}");
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("daemon socket must become ready");
}

async fn wait_for_exit(child: &mut Child) -> ExitStatus {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(status) = child
                .try_wait()
                .expect("daemon child status must be readable")
            {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("daemon must exit after a shutdown signal")
}

async fn wait_for_owner_parent_exit(parent: &mut Child) -> ExitStatus {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(status) = parent
                .try_wait()
                .expect("owner parent status must be readable")
            {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("owner parent must exit after release")
}

async fn wait_until_owner_daemon_ready(
    parent: &mut Child,
    socket_path: &Path,
    daemon_stderr_path: &Path,
) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if std::os::unix::net::UnixStream::connect(socket_path).is_ok() {
                break;
            }
            if let Some(status) = parent
                .try_wait()
                .expect("owner parent status must be readable")
            {
                let daemon_stderr =
                    std::fs::read_to_string(daemon_stderr_path).unwrap_or_default();
                panic!(
                    "owner parent exited before daemon readiness: {status}; daemon stderr: {daemon_stderr}"
                );
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("owned daemon socket must become ready");
}

async fn assert_graceful_signal_shutdown(signal: libc::c_int) {
    // Keep the socket path short enough for macOS's Unix-domain path limit.
    let temp = tempfile::Builder::new()
        .prefix("impulse-signal-")
        .tempdir_in("/tmp")
        .expect("temporary daemon directory must be created");
    let impulse_dir = temp.path();
    let socket_path = impulse_dir.join("sockets/impulse.sock");
    let pid_path = impulse_dir.join("sockets/impulse.pid");
    let live_state_path = impulse_dir.join("LIVE_STATE.json");

    let mut guard = spawn_daemon(impulse_dir);

    wait_until_ready(guard.child_mut(), &socket_path).await;
    assert!(pid_path.exists(), "daemon must publish its PID file");

    let client = DaemonClient::new(socket_path.clone());
    let (session_id, _) = client
        .create_session(
            "shutdown-persistence-proof".to_string(),
            Some("codex".to_string()),
        )
        .await
        .expect("daemon must create a session before shutdown");

    // Corrupt the disk snapshot after the request completes. The daemon still
    // owns the in-memory session, so a forced shutdown sync must restore it.
    tokio::fs::write(&live_state_path, b"{}")
        .await
        .expect("test must replace the persisted snapshot");

    let signal_result = unsafe { libc::kill(guard.child_mut().id() as libc::pid_t, signal) };
    assert_eq!(signal_result, 0, "shutdown signal must reach daemon");

    let status = wait_for_exit(guard.child_mut()).await;
    if !status.success() {
        let stderr = guard.stderr();
        panic!("daemon signal shutdown failed with {status}: {stderr}");
    }

    assert!(
        !socket_path.exists(),
        "graceful shutdown must remove the owned socket"
    );
    assert!(
        !pid_path.exists(),
        "graceful shutdown must remove the owned PID file"
    );

    let persisted: LiveState = serde_json::from_slice(
        &tokio::fs::read(&live_state_path)
            .await
            .expect("shutdown must leave a live-state snapshot"),
    )
    .expect("shutdown snapshot must be valid LiveState JSON");
    assert!(
        persisted.sessions.contains_key(&session_id),
        "shutdown must force-sync the daemon's authoritative in-memory state"
    );
}

#[tokio::test]
async fn test_daemon_sigterm_syncs_state_and_removes_runtime_files() {
    assert_graceful_signal_shutdown(libc::SIGTERM).await;
}

#[tokio::test]
async fn test_daemon_ctrl_c_syncs_state_and_removes_runtime_files() {
    assert_graceful_signal_shutdown(libc::SIGINT).await;
}

#[tokio::test]
async fn test_daemon_rejects_wrong_owner_before_runtime_files() {
    let temp = tempfile::Builder::new()
        .prefix("impulse-owner-reject-")
        .tempdir_in("/tmp")
        .expect("temporary daemon directory must be created");
    let impulse_dir = temp.path();
    let socket_path = impulse_dir.join("sockets/impulse.sock");
    let pid_path = impulse_dir.join("sockets/impulse.pid");
    let lock_path = impulse_dir.join("sockets/impulse.daemon.lock");
    let mut guard = DaemonGuard {
        child: Some(
            daemon_command(impulse_dir, Some(u32::MAX))
                .spawn()
                .expect("wrong-owner daemon process must launch"),
        ),
    };

    let status = wait_for_exit(guard.child_mut()).await;
    assert!(!status.success(), "wrong owner must fail closed");
    let stderr = guard.stderr();
    assert!(
        stderr.contains("is not its direct parent"),
        "wrong-owner error must name the direct-parent invariant, got: {stderr}"
    );
    assert!(
        !socket_path.exists(),
        "rejected owner must not bind a socket"
    );
    assert!(!pid_path.exists(), "rejected owner must not publish a PID");
    assert!(
        !lock_path.exists(),
        "rejected owner must fail before daemon runtime-file locking"
    );
}

#[tokio::test]
async fn test_owned_daemon_parent_death_syncs_state_and_removes_runtime_files() {
    let temp = tempfile::Builder::new()
        .prefix("impulse-owner-exit-")
        .tempdir_in("/tmp")
        .expect("temporary daemon directory must be created");
    let impulse_dir = temp.path().join(".impulse");
    let socket_path = impulse_dir.join("sockets/impulse.sock");
    let pid_path = impulse_dir.join("sockets/impulse.pid");
    let live_state_path = impulse_dir.join("LIVE_STATE.json");
    let release_path = temp.path().join("release-owner");
    let daemon_stdout_path = temp.path().join("owned-daemon.stdout");
    let daemon_stderr_path = temp.path().join("owned-daemon.stderr");
    let mut guard = spawn_owned_daemon_parent(temp.path(), &impulse_dir);

    wait_until_owner_daemon_ready(guard.parent_mut(), &socket_path, &daemon_stderr_path).await;
    let daemon_pid = guard.daemon_pid();
    assert_eq!(
        tokio::fs::read_to_string(&pid_path)
            .await
            .expect("owned daemon must publish its PID")
            .trim(),
        daemon_pid.to_string()
    );

    let client = DaemonClient::new(socket_path.clone());
    let (session_id, _) = client
        .create_session(
            "owner-parent-shutdown-proof".to_string(),
            Some("codex".to_string()),
        )
        .await
        .expect("owned daemon must create a session");
    tokio::fs::write(&live_state_path, b"{}")
        .await
        .expect("test must replace the persisted snapshot");

    tokio::fs::write(&release_path, b"release")
        .await
        .expect("test must release the owner parent");
    let owner_status = wait_for_owner_parent_exit(guard.parent_mut()).await;
    assert!(owner_status.success(), "owner parent must exit cleanly");

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if !socket_path.exists() && !pid_path.exists() && !process_is_running(daemon_pid) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("owned daemon must exit and clean runtime files after parent death");

    let persisted: LiveState = serde_json::from_slice(
        &tokio::fs::read(&live_state_path)
            .await
            .expect("parent-death shutdown must leave a live-state snapshot"),
    )
    .expect("parent-death snapshot must be valid LiveState JSON");
    assert!(
        persisted.sessions.contains_key(&session_id),
        "parent-death shutdown must force-sync authoritative in-memory state"
    );
    let daemon_stdout = tokio::fs::read_to_string(&daemon_stdout_path)
        .await
        .expect("owned daemon stdout must be readable");
    assert!(
        daemon_stdout.contains("desktop owner PID") && daemon_stdout.contains("current parent PID"),
        "owned daemon must report why it shut down, got: {daemon_stdout}"
    );
    let daemon_stderr = tokio::fs::read_to_string(&daemon_stderr_path)
        .await
        .expect("owned daemon stderr must be readable");
    assert!(
        daemon_stderr.trim().is_empty(),
        "parent-death shutdown must not report an error: {daemon_stderr}"
    );
    guard.disarm();
}

#[tokio::test]
async fn test_second_daemon_cannot_replace_live_socket_or_pid() {
    let temp = tempfile::Builder::new()
        .prefix("impulse-lock-")
        .tempdir_in("/tmp")
        .expect("temporary daemon directory must be created");
    let impulse_dir = temp.path();
    let socket_path = impulse_dir.join("sockets/impulse.sock");
    let pid_path = impulse_dir.join("sockets/impulse.pid");

    let mut first = spawn_daemon(impulse_dir);
    wait_until_ready(first.child_mut(), &socket_path).await;
    let first_pid = tokio::fs::read_to_string(&pid_path)
        .await
        .expect("first daemon must publish its PID");

    let mut second = spawn_daemon(impulse_dir);
    let second_status = wait_for_exit(second.child_mut()).await;
    assert!(
        !second_status.success(),
        "a competing daemon must fail instead of replacing live runtime files"
    );
    let second_stderr = second.stderr();
    assert!(
        second_stderr.contains("Another daemon owns lifecycle lock"),
        "competing daemon must report the lifecycle lock, got: {second_stderr}"
    );

    assert_eq!(
        tokio::fs::read_to_string(&pid_path)
            .await
            .expect("first daemon PID file must remain"),
        first_pid,
        "competing startup must not replace the live daemon PID file"
    );
    assert!(
        DaemonClient::new(socket_path.clone())
            .ping()
            .await
            .expect("first daemon must remain reachable"),
        "first daemon must still answer after the competing startup"
    );

    let signal_result = unsafe { libc::kill(first.child_mut().id() as libc::pid_t, libc::SIGTERM) };
    assert_eq!(signal_result, 0, "SIGTERM must reach the first daemon");
    let first_status = wait_for_exit(first.child_mut()).await;
    if !first_status.success() {
        let stderr = first.stderr();
        panic!("first daemon failed graceful shutdown with {first_status}: {stderr}");
    }
    assert!(!socket_path.exists());
    assert!(!pid_path.exists());
}
