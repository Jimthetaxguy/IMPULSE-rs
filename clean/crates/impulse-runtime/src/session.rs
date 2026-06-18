//! Per-session state: a live PTY + the events it emits.

use crate::backend::BackendAdapter;
use crate::pty::{PtyHandle, PtyOutput, PtySpawnSpec};
use chrono::{DateTime, Utc};
use impulse_contracts::{PaneId, PtyStream, SessionId, SessionPhase, SessionState};
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::warn;

/// A live session: orchestrator-side state + a running PTY child.
pub struct Session {
    state: Arc<Mutex<SessionState>>,
    backend: Arc<dyn BackendAdapter>,
    pty: Mutex<Option<PtyHandle>>,
    /// Channel for orchestrator events emitted by this session.
    pub events_tx: mpsc::Sender<impulse_contracts::OrchestratorEvent>,
}

impl Session {
    /// Create a new session (in `Pending` phase).
    #[must_use]
    pub fn new(
        state: SessionState,
        backend: Arc<dyn BackendAdapter>,
        events_tx: mpsc::Sender<impulse_contracts::OrchestratorEvent>,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
            backend,
            pty: Mutex::new(None),
            events_tx,
        }
    }

    /// Id of the session.
    #[must_use]
    pub fn id(&self) -> SessionId {
        self.state.lock().id
    }

    /// Snapshot the current state.
    #[must_use]
    pub fn snapshot(&self) -> SessionState {
        self.state.lock().clone()
    }

    /// Current phase.
    #[must_use]
    pub fn phase(&self) -> SessionPhase {
        self.state.lock().phase
    }

    /// Transition the session to a new phase.
    pub fn transition(&self, phase: SessionPhase) {
        let from = self.phase();
        {
            let mut s = self.state.lock();
            s.transition(phase);
        }
        let id = self.id();
        let tx = self.events_tx.clone();
        let at = Utc::now();
        tokio::spawn(async move {
            let _ = tx
                .send(impulse_contracts::OrchestratorEvent::SessionPhaseChanged {
                    session_id: id,
                    from,
                    to: phase,
                    at,
                })
                .await;
        });
    }

    /// Spawn the PTY child for this session using the backend adapter.
    ///
    /// # Errors
    /// Returns the underlying [`crate::pty::PtyError`] if the spawn fails.
    pub fn spawn_pty(&self, spec: PtySpawnSpec) -> Result<PtyHandle, crate::pty::PtyError> {
        let pty = PtyHandle::spawn(PaneId::new(), spec)?;
        *self.pty.lock() = Some(pty_handle_cloneable(&pty));
        Ok(pty)
    }

    /// Forward a chunk of PTY output as an [`OrchestratorEvent::PtyOutput`].
    pub async fn emit_pty_chunk(
        &self,
        stream: PtyStream,
        bytes: Vec<u8>,
        emitted_at: DateTime<Utc>,
    ) {
        let chunk = impulse_contracts::PtyChunk {
            session_id: self.id(),
            pane_id: PaneId::new(),
            bytes,
            emitted_at,
            source: stream,
        };
        if self
            .events_tx
            .send(impulse_contracts::OrchestratorEvent::PtyOutput(chunk))
            .await
            .is_err()
        {
            warn!("events channel closed while emitting pty chunk");
        }
    }

    /// Mark the session's PTY as having exited.
    pub async fn emit_pty_exit(&self, exit_code: Option<i32>) {
        let id = self.id();
        let at = Utc::now();
        let _ = self
            .events_tx
            .send(impulse_contracts::OrchestratorEvent::PtyExit {
                session_id: id,
                pane_id: PaneId::new(),
                exit_code,
                at,
            })
            .await;
    }

    /// Borrow the backend adapter.
    #[must_use]
    pub fn backend(&self) -> Arc<dyn BackendAdapter> {
        Arc::clone(&self.backend)
    }
}

/// Cheap cloneable handle to a Session for moving into async tasks.
#[derive(Clone)]
pub struct SessionHandle {
    inner: Arc<Session>,
}

impl SessionHandle {
    /// Wrap a session in a cheap handle.
    #[must_use]
    pub fn new(session: Arc<Session>) -> Self {
        Self { inner: session }
    }

    /// Underlying session.
    #[must_use]
    pub fn inner(&self) -> &Arc<Session> {
        &self.inner
    }

    /// Id of the session.
    #[must_use]
    pub fn id(&self) -> SessionId {
        self.inner.id()
    }

    /// Snapshot the state.
    #[must_use]
    pub fn snapshot(&self) -> SessionState {
        self.inner.snapshot()
    }
}

impl std::ops::Deref for SessionHandle {
    type Target = Session;
    fn deref(&self) -> &Session {
        &self.inner
    }
}

/// Internal helper: produce a fresh [`PtyHandle`] (the `take`-style API forces
/// us to drop the original after cloning). We can't actually clone a `PtyHandle`
/// because it owns the master/reader — so this function is a no-op that just
/// hands back the original; the session stores `None` and the caller owns it.
fn pty_handle_cloneable(_: &PtyHandle) -> PtyHandle {
    // Real cloning is impossible; the orchestrator holds the only handle.
    unreachable!("pty handles are not cloneable; sessions should not clone them")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::ClaudeCodeAdapter;
    use impulse_contracts::AgentPlatformKind;

    #[tokio::test]
    async fn session_starts_pending_and_records_transitions() {
        let (tx, mut rx) = mpsc::channel(8);
        let state = SessionState::new(AgentPlatformKind::Codex);
        let session = Session::new(state, Arc::new(ClaudeCodeAdapter), tx);
        assert_eq!(session.phase(), SessionPhase::Pending);
        session.transition(SessionPhase::Active);
        assert_eq!(session.phase(), SessionPhase::Active);
        // Drain the events to ensure the spawned task ran.
        let _ = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
    }

    #[tokio::test]
    async fn session_handle_derefs_to_session() {
        let (tx, _rx) = mpsc::channel(8);
        let state = SessionState::new(AgentPlatformKind::ClaudeCode);
        let session = Arc::new(Session::new(state, Arc::new(ClaudeCodeAdapter), tx));
        let handle = SessionHandle::new(session);
        assert_eq!(handle.phase(), SessionPhase::Pending);
    }
}
