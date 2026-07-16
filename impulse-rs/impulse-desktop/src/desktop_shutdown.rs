//! Ordered native-desktop shutdown.
//!
//! Tao ends the desktop process without unwinding the launcher stack. The
//! product therefore cannot leave PTY, governed-task, telemetry, or daemon
//! cleanup exclusively to `Drop`. This coordinator is shared by the Dioxus
//! host bridge and the native event-loop boundary and runs the close sequence
//! exactly once without holding its mutex across process waits or IPC.

use std::sync::{Arc, Condvar, Mutex};

use crate::daemon_ops::{DesktopDaemonOpsShutdownHandle, DesktopDaemonOpsShutdownOutcome};
use crate::daemon_sidecar::{
    DesktopDaemonSidecar, DesktopDaemonSidecarHandle, DesktopDaemonSidecarShutdownOutcome,
    DesktopInstanceLease,
};
use crate::runtime::{DesktopRuntime, DesktopRuntimeShutdownReport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopShutdownReport {
    pub runtime: DesktopRuntimeShutdownReport,
    pub daemon_ops: Option<DesktopDaemonOpsShutdownOutcome>,
    pub daemon_sidecar: Option<DesktopDaemonSidecarShutdownOutcome>,
}

struct DesktopShutdownParts {
    runtime: Arc<DesktopRuntime>,
    daemon_ops: Option<DesktopDaemonOpsShutdownHandle>,
    daemon_sidecar: DesktopDaemonSidecarHandle,
    _instance_lease: Option<DesktopInstanceLease>,
}

#[derive(Clone)]
pub struct DesktopShutdownCoordinator {
    shared: Arc<DesktopShutdownShared>,
}

struct DesktopShutdownShared {
    state: Mutex<DesktopShutdownState>,
    completed: Condvar,
}

enum DesktopShutdownState {
    Pending(DesktopShutdownParts),
    Running,
    Complete,
}

struct DesktopShutdownCompletion<'a> {
    shared: &'a DesktopShutdownShared,
}

impl Drop for DesktopShutdownCompletion<'_> {
    fn drop(&mut self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *state = DesktopShutdownState::Complete;
        self.shared.completed.notify_all();
    }
}

impl DesktopShutdownCoordinator {
    pub fn new(
        runtime: Arc<DesktopRuntime>,
        daemon_ops: Option<DesktopDaemonOpsShutdownHandle>,
        daemon_sidecar: DesktopDaemonSidecarHandle,
        instance_lease: Option<DesktopInstanceLease>,
    ) -> Self {
        Self {
            shared: Arc::new(DesktopShutdownShared {
                state: Mutex::new(DesktopShutdownState::Pending(DesktopShutdownParts {
                    runtime,
                    daemon_ops,
                    daemon_sidecar,
                    _instance_lease: instance_lease,
                })),
                completed: Condvar::new(),
            }),
        }
    }

    pub fn install_daemon_boundary(
        &self,
        daemon_ops: DesktopDaemonOpsShutdownHandle,
        sidecar: DesktopDaemonSidecar,
        instance_lease: DesktopInstanceLease,
    ) -> Result<(), String> {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let DesktopShutdownState::Pending(parts) = &mut *state else {
            return Err(
                "desktop shutdown has started; project daemon boundary cannot be installed"
                    .to_string(),
            );
        };
        if parts.daemon_ops.is_some() || parts._instance_lease.is_some() {
            return Err("desktop project daemon boundary is already installed".to_string());
        }
        parts.daemon_sidecar.install(sidecar)?;
        parts.daemon_ops = Some(daemon_ops);
        parts._instance_lease = Some(instance_lease);
        Ok(())
    }

    /// Execute the strict close order exactly once:
    ///
    /// 1. stop accepting launches and reap managed PTYs while daemon task
    ///    mutation is still available;
    /// 2. stop/join daemon telemetry so its lifecycle outbox drains and its
    ///    empty final report is published;
    /// 3. gracefully stop only the daemon companion this desktop spawned.
    pub fn shutdown(&self) -> Option<DesktopShutdownReport> {
        let parts = {
            let state = self
                .shared
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut state = self
                .shared
                .completed
                .wait_while(state, |state| {
                    matches!(state, DesktopShutdownState::Running)
                })
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match std::mem::replace(&mut *state, DesktopShutdownState::Running) {
                DesktopShutdownState::Pending(parts) => parts,
                DesktopShutdownState::Complete => {
                    *state = DesktopShutdownState::Complete;
                    return None;
                }
                DesktopShutdownState::Running => {
                    unreachable!("wait_while must not return a running shutdown state")
                }
            }
        };
        // Always release concurrent close callers, including during panic
        // unwinding from a lifecycle dependency. The extracted parts cannot
        // be replayed safely after a partial shutdown.
        let _completion = DesktopShutdownCompletion {
            shared: self.shared.as_ref(),
        };

        let runtime = parts.runtime.shutdown();
        let daemon_ops = parts.daemon_ops.map(|daemon_ops| daemon_ops.shutdown());
        let daemon_sidecar = parts.daemon_sidecar.shutdown();
        let report = DesktopShutdownReport {
            runtime,
            daemon_ops,
            daemon_sidecar,
        };
        eprintln!(
            "desktop shutdown outcomes: runtime={:?} daemon_ops={:?} daemon_sidecar={:?}",
            report.runtime, report.daemon_ops, report.daemon_sidecar
        );
        Some(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::io::{BufRead, BufReader, Write};
    #[cfg(unix)]
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    use crate::daemon_ops::{
        DesktopDaemonOpsConfig, DesktopDaemonOpsShutdownFailure, DesktopDaemonOpsShutdownHandle,
    };
    use crate::daemon_sidecar::{DesktopDaemonSidecarMode, DesktopDaemonTerminateReapOutcome};
    use crate::runtime::{AgentPlatformId, AgentSpawnRequest};

    #[cfg(unix)]
    fn answer_ping(mut stream: std::os::unix::net::UnixStream) {
        let mut request = String::new();
        BufReader::new(stream.try_clone().expect("clone ping stream"))
            .read_line(&mut request)
            .expect("read ping request");
        let parsed: impulse_ops::WorkbenchDaemonRequest =
            serde_json::from_str(&request).expect("parse ping request");
        assert_eq!(parsed, impulse_ops::WorkbenchDaemonRequest::Ping);
        let response = impulse_ops::WorkbenchDaemonResponse::Ok {
            result: serde_json::json!({
                "pong": true,
                "protocol_version": impulse_ops::DAEMON_PROTOCOL_VERSION,
            }),
        };
        serde_json::to_writer(&mut stream, &response).expect("write ping response");
        stream.write_all(b"\n").expect("terminate ping response");
        stream.flush().expect("flush ping response");
    }

    #[cfg(unix)]
    fn spawn_ping_server(
        socket_path: &std::path::Path,
    ) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let listener =
            std::os::unix::net::UnixListener::bind(socket_path).expect("bind daemon socket");
        listener
            .set_nonblocking(true)
            .expect("set listener nonblocking");
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => answer_ping(stream),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("accept daemon ping: {error}"),
                }
            }
        });
        (stop, worker)
    }

    #[cfg(unix)]
    fn daemon_answers_ping(socket_path: &std::path::Path) -> bool {
        let Ok(mut stream) = std::os::unix::net::UnixStream::connect(socket_path) else {
            return false;
        };
        let request = serde_json::to_vec(&impulse_ops::WorkbenchDaemonRequest::Ping)
            .expect("serialize ping request");
        if stream
            .write_all(&request)
            .and_then(|_| stream.write_all(b"\n"))
            .and_then(|_| stream.flush())
            .is_err()
        {
            return false;
        }
        let mut response = String::new();
        BufReader::new(stream).read_line(&mut response).is_ok()
            && serde_json::from_str::<impulse_ops::WorkbenchDaemonResponse>(&response).is_ok()
    }

    #[test]
    fn shutdown_is_idempotent_and_closes_active_runtime_before_returning() {
        let runtime = Arc::new(DesktopRuntime::default());
        let mut request = AgentSpawnRequest::terminal_harness(
            "coordinator-worker",
            AgentPlatformId::try_new("shell").expect("valid shell platform id"),
            std::env::current_dir()
                .expect("current directory")
                .display()
                .to_string(),
            24,
            80,
        );
        request.command = Some("sh".to_string());
        request.args = vec!["-lc".to_string(), "sleep 30".to_string()];
        runtime.spawn_agent(request).expect("spawn worker");

        let coordinator = DesktopShutdownCoordinator::new(
            Arc::clone(&runtime),
            None,
            DesktopDaemonSidecarHandle::default(),
            None,
        );
        let report = coordinator.shutdown().expect("first shutdown runs");
        assert_eq!(report.runtime.agents_seen, 1);
        assert_eq!(report.runtime.agents_closed, 1);
        assert!(report.runtime.errors.is_empty(), "{report:?}");
        assert_eq!(report.daemon_ops, None);
        assert_eq!(report.daemon_sidecar, None);
        assert!(runtime.snapshot_agents().is_empty());
        assert_eq!(coordinator.shutdown(), None);
    }

    #[cfg(unix)]
    #[test]
    fn late_installed_existing_daemon_boundary_is_ordered_and_not_adopted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().join("project");
        let sockets = project_root.join(".impulse").join("sockets");
        std::fs::create_dir_all(&sockets).expect("create sockets directory");
        let socket_path = sockets.join("impulse.sock");
        let (stop, worker) = spawn_ping_server(&socket_path);

        let runtime = Arc::new(DesktopRuntime::default());
        let coordinator = DesktopShutdownCoordinator::new(
            runtime,
            None,
            DesktopDaemonSidecarHandle::default(),
            None,
        );
        let config = DesktopDaemonOpsConfig::for_project(project_root);
        let lease = DesktopInstanceLease::acquire(&socket_path).expect("acquire desktop lease");
        let sidecar = DesktopDaemonSidecar::ensure(&config).expect("attach existing daemon");
        assert_eq!(sidecar.mode(), DesktopDaemonSidecarMode::Existing);
        coordinator
            .install_daemon_boundary(
                DesktopDaemonOpsShutdownHandle::new(DesktopDaemonOpsShutdownOutcome::successful),
                sidecar,
                lease,
            )
            .expect("install late daemon boundary");

        let report = coordinator.shutdown().expect("ordered shutdown report");
        assert_eq!(
            report.daemon_ops,
            Some(DesktopDaemonOpsShutdownOutcome::successful())
        );
        assert_eq!(
            report.daemon_sidecar,
            Some(DesktopDaemonSidecarShutdownOutcome {
                mode: DesktopDaemonSidecarMode::Existing,
                terminate_reap: DesktopDaemonTerminateReapOutcome::NotRequired,
            })
        );
        assert!(
            daemon_answers_ping(&socket_path),
            "existing daemon must remain operator-owned after desktop shutdown"
        );
        let reacquired = DesktopInstanceLease::acquire(&socket_path)
            .expect("ordered shutdown must release the desktop instance lease");
        drop(reacquired);
        assert_eq!(coordinator.shutdown(), None);

        stop.store(true, Ordering::Release);
        worker.join().expect("join ping server");
    }

    #[test]
    fn concurrent_shutdown_waits_for_the_in_flight_close_sequence() {
        let runtime = Arc::new(DesktopRuntime::default());
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let daemon_ops = DesktopDaemonOpsShutdownHandle::new(move || {
            entered_tx.send(()).expect("report shutdown entry");
            release_rx
                .lock()
                .expect("lock release receiver")
                .recv()
                .expect("release in-flight shutdown");
            DesktopDaemonOpsShutdownOutcome::successful()
        });
        let coordinator = DesktopShutdownCoordinator::new(
            runtime,
            Some(daemon_ops),
            DesktopDaemonSidecarHandle::default(),
            None,
        );

        let first = {
            let coordinator = coordinator.clone();
            std::thread::spawn(move || coordinator.shutdown())
        };
        entered_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first caller reached daemon-ops shutdown");

        let (second_done_tx, second_done_rx) = mpsc::channel();
        let second = {
            let coordinator = coordinator.clone();
            std::thread::spawn(move || {
                second_done_tx
                    .send(coordinator.shutdown())
                    .expect("return second shutdown result");
            })
        };
        assert!(
            second_done_rx
                .recv_timeout(Duration::from_millis(50))
                .is_err(),
            "a concurrent close caller must wait for cleanup instead of allowing process exit"
        );

        release_tx.send(()).expect("release first shutdown");
        assert!(first.join().expect("join first caller").is_some());
        assert_eq!(
            second_done_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("second caller returns after cleanup"),
            None
        );
        second.join().expect("join second caller");
    }

    #[test]
    fn shutdown_report_aggregates_daemon_ops_failures() {
        let runtime = Arc::new(DesktopRuntime::default());
        let expected = DesktopDaemonOpsShutdownOutcome {
            worker_joined: true,
            lifecycle_outbox_drained: false,
            final_report_published: false,
            failures: vec![
                DesktopDaemonOpsShutdownFailure::LifecycleOutboxDrain {
                    message: "outbox unavailable".to_string(),
                },
                DesktopDaemonOpsShutdownFailure::FinalReportPublish {
                    message: "publish unavailable".to_string(),
                },
            ],
        };
        let returned = expected.clone();
        let daemon_ops = DesktopDaemonOpsShutdownHandle::new(move || returned.clone());
        let coordinator = DesktopShutdownCoordinator::new(
            runtime,
            Some(daemon_ops),
            DesktopDaemonSidecarHandle::default(),
            None,
        );

        let report = coordinator.shutdown().expect("first shutdown report");

        assert_eq!(report.daemon_ops, Some(expected));
        assert_eq!(report.daemon_sidecar, None);
    }

    #[test]
    fn shutdown_panic_releases_waiters_and_cannot_replay_partial_cleanup() {
        let runtime = Arc::new(DesktopRuntime::default());
        let daemon_ops = DesktopDaemonOpsShutdownHandle::new(|| {
            panic!("test shutdown dependency panic");
        });
        let coordinator = DesktopShutdownCoordinator::new(
            runtime,
            Some(daemon_ops),
            DesktopDaemonSidecarHandle::default(),
            None,
        );

        let panic =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| coordinator.shutdown()));

        assert!(panic.is_err());
        assert_eq!(coordinator.shutdown(), None);
    }
}
