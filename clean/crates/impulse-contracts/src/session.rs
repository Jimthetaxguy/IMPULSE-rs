//! Session lifecycle types.

use crate::harness::AgentPlatformKind;
use crate::id::SessionId;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Discrete lifecycle phase a session is in.
#[derive(
    Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    /// Session has been created but no harness has been spawned yet.
    Pending,
    /// The PTY/harness is starting up.
    Starting,
    /// The agent is actively running and accepting prompts.
    Active,
    /// The agent is paused (e.g. awaiting human approval).
    Paused,
    /// The agent has finished a turn and is waiting for the next prompt.
    Idle,
    /// The session is closing (verifying, flushing).
    Ending,
    /// The session has been closed; the entry is now historical.
    Ended,
    /// The session failed during startup or execution.
    Failed,
}

impl SessionPhase {
    /// Whether the session is in a terminal phase (no more state transitions expected).
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Ended | Self::Failed)
    }

    /// Whether the harness subprocess is alive in this phase.
    #[must_use]
    pub fn is_live(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Active | Self::Paused | Self::Idle
        )
    }
}

/// Snapshot of a session's state at a point in time.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct SessionState {
    /// Session id.
    pub id: SessionId,
    /// Backend driving the session.
    pub platform: AgentPlatformKind,
    /// Current phase.
    pub phase: SessionPhase,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
    /// When the session last transitioned to a new phase.
    pub updated_at: DateTime<Utc>,
    /// Optional human-readable label.
    #[serde(default)]
    pub label: Option<String>,
    /// Number of prompts the agent has processed.
    #[serde(default)]
    pub prompt_count: u32,
    /// Number of tool calls the orchestrator has dispatched.
    #[serde(default)]
    pub tool_call_count: u32,
}

impl SessionState {
    /// Create a fresh `Pending` session.
    #[must_use]
    pub fn new(platform: AgentPlatformKind) -> Self {
        let now = Utc::now();
        Self {
            id: SessionId::new(),
            platform,
            phase: SessionPhase::Pending,
            created_at: now,
            updated_at: now,
            label: None,
            prompt_count: 0,
            tool_call_count: 0,
        }
    }

    /// Transition to a new phase, updating `updated_at`.
    pub fn transition(&mut self, phase: SessionPhase) {
        self.phase = phase;
        self.updated_at = Utc::now();
    }

    /// Record a prompt processed.
    pub fn record_prompt(&mut self) {
        self.prompt_count = self.prompt_count.saturating_add(1);
        self.updated_at = Utc::now();
    }

    /// Record a tool call dispatched.
    pub fn record_tool_call(&mut self) {
        self.tool_call_count = self.tool_call_count.saturating_add(1);
        self.updated_at = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_phases_are_terminal() {
        assert!(SessionPhase::Ended.is_terminal());
        assert!(SessionPhase::Failed.is_terminal());
        assert!(!SessionPhase::Active.is_terminal());
    }

    #[test]
    fn live_phases_have_running_subprocess() {
        assert!(SessionPhase::Active.is_live());
        assert!(SessionPhase::Paused.is_live());
        assert!(!SessionPhase::Pending.is_live());
        assert!(!SessionPhase::Ended.is_live());
    }

    #[test]
    fn new_session_starts_pending() {
        let s = SessionState::new(AgentPlatformKind::ClaudeCode);
        assert_eq!(s.phase, SessionPhase::Pending);
        assert_eq!(s.platform, AgentPlatformKind::ClaudeCode);
        assert_eq!(s.prompt_count, 0);
    }

    #[test]
    fn transitions_bump_updated_at() {
        let mut s = SessionState::new(AgentPlatformKind::Codex);
        let before = s.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        s.transition(SessionPhase::Active);
        assert!(s.updated_at > before);
    }

    #[test]
    fn session_state_round_trips_through_json() {
        let s = SessionState::new(AgentPlatformKind::GeminiCli);
        let j = serde_json::to_string(&s).unwrap();
        let back: SessionState = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
    }
}
