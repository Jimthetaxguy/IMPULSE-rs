//! Orchestrator events — discrete facts emitted to subscribers.

use crate::harness::AgentPlatformKind;
use crate::id::{PaneId, SessionId, ToolCallId, WorkspaceId};
use crate::session::SessionPhase;
use crate::tool::{RiskClass, ToolSpec};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A chunk of bytes from a PTY session. Always safe to display as UTF-8 lossy.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct PtyChunk {
    /// Session the chunk belongs to.
    pub session_id: SessionId,
    /// Pane id (multi-pane sessions only).
    pub pane_id: PaneId,
    /// Bytes (raw). The runtime is responsible for vt100-parsing them.
    pub bytes: Vec<u8>,
    /// When the chunk was emitted by the kernel.
    pub emitted_at: DateTime<Utc>,
    /// Source stream.
    pub source: PtyStream,
}

/// Whether a chunk came from the child process's stdout or stderr.
#[derive(
    Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PtyStream {
    /// Standard output (typically the human-facing TUI).
    Stdout,
    /// Standard error (typically the structured log stream).
    Stderr,
}

impl PtyStream {
    /// Canonical lowercase label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

/// A tool call the orchestrator dispatched on behalf of a session.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct ToolEvent {
    /// Unique call id.
    pub id: ToolCallId,
    /// Session that initiated the call.
    pub session_id: SessionId,
    /// Workspace the call ran against.
    pub workspace_id: WorkspaceId,
    /// The tool spec used.
    pub tool: ToolSpec,
    /// Risk class actually applied (>= tool.risk).
    pub applied_risk: RiskClass,
    /// When the call started.
    pub started_at: DateTime<Utc>,
    /// When the call finished (if it has).
    #[serde(default)]
    pub finished_at: Option<DateTime<Utc>>,
    /// Outcome.
    pub outcome: ToolOutcome,
}

/// Outcome of a single tool call.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolOutcome {
    /// Tool completed successfully.
    Success {
        /// Truncated result (the runtime stores the full version).
        summary: String,
    },
    /// Tool failed but may be retried.
    Retryable {
        /// Failure reason.
        reason: String,
        /// Number of retries already attempted.
        attempts: u32,
    },
    /// Tool failed permanently.
    Failed {
        /// Failure reason.
        reason: String,
    },
    /// Tool was denied by the permission pipeline.
    Denied {
        /// Why.
        reason: String,
    },
    /// Tool was superseded (e.g. session ended before it ran).
    Cancelled,
}

/// The unified event stream the orchestrator publishes. Every event is
/// `Clone + Send + Sync` and serializes stably to JSON.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OrchestratorEvent {
    /// A new session was created.
    SessionCreated {
        /// Session id.
        session_id: SessionId,
        /// Backend.
        platform: AgentPlatformKind,
        /// When.
        at: DateTime<Utc>,
    },
    /// Session transitioned to a new phase.
    SessionPhaseChanged {
        /// Session id.
        session_id: SessionId,
        /// Old phase.
        from: SessionPhase,
        /// New phase.
        to: SessionPhase,
        /// When.
        at: DateTime<Utc>,
    },
    /// A chunk of PTY output arrived.
    PtyOutput(PtyChunk),
    /// The agent's subprocess exited.
    PtyExit {
        /// Session id.
        session_id: SessionId,
        /// Pane id.
        pane_id: PaneId,
        /// Exit code (None if killed by signal).
        exit_code: Option<i32>,
        /// When.
        at: DateTime<Utc>,
    },
    /// A tool was invoked.
    ToolInvoked(ToolEvent),
    /// A tool finished.
    ToolFinished(ToolEvent),
    /// The orchestrator needs human approval for a high-risk action.
    ApprovalRequested {
        /// Session id.
        session_id: SessionId,
        /// Tool that wants to run.
        tool_name: String,
        /// Risk class.
        risk: RiskClass,
        /// Human-readable description.
        description: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_chunk_round_trips_through_json() {
        let chunk = PtyChunk {
            session_id: SessionId::new(),
            pane_id: PaneId::new(),
            bytes: b"hello".to_vec(),
            emitted_at: Utc::now(),
            source: PtyStream::Stdout,
        };
        let j = serde_json::to_string(&chunk).unwrap();
        let back: PtyChunk = serde_json::from_str(&j).unwrap();
        assert_eq!(chunk, back);
    }

    #[test]
    fn orchestrator_event_serializes_with_tag() {
        let ev = OrchestratorEvent::PtyExit {
            session_id: SessionId::new(),
            pane_id: PaneId::new(),
            exit_code: Some(0),
            at: Utc::now(),
        };
        let j = serde_json::to_string(&ev).unwrap();
        assert!(j.contains("\"kind\":\"pty_exit\""));
    }

    #[test]
    fn tool_outcome_serializes_as_tagged() {
        let outcome = ToolOutcome::Success {
            summary: "ok".to_owned(),
        };
        let j = serde_json::to_string(&outcome).unwrap();
        assert!(j.contains("\"kind\":\"success\""));
    }
}
