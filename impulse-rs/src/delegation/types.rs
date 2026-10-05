//! Core types for the delegation tracking system.
//!
//! Tracks coordinator/worker delegation patterns detected in agent output.
//! Inspired by OpenSquirrel's JSON code-fence delegation and Hermes Agent's
//! depth-limited, restricted-toolset delegation model.

use chrono::{DateTime, Utc};
use impulse_ops::{AgentRole, DiffSummary, ToolInvocationRecord};
use serde::{Deserialize, Serialize};

/// Maximum depth of nested delegation (from Hermes Agent pattern).
pub const MAX_DELEGATION_DEPTH: u8 = 2;

/// Most delegations a tracker holds. A new one past this replaces the oldest
/// finished delegation; when every held delegation is still active, the new
/// one is refused rather than dropping active work.
pub const MAX_TRACKED_DELEGATIONS: usize = 256;

/// Most of a coordinator's context snapshot a delegation keeps. Nothing
/// reads the snapshot back yet, so the rest would only cost memory.
pub const MAX_CONTEXT_SNAPSHOT_BYTES: usize = 64 * 1024;

/// Most a delegation's spec may weigh, and separately its completion
/// (summary and tool trace) or failure message: their text, plus
/// [`DELEGATION_ITEM_BYTES`] for each list entry.
pub const MAX_DELEGATION_TEXT_BYTES: usize = 256 * 1024;

/// What one list entry (a target file, a restricted tool, a tool-trace
/// record) counts toward [`MAX_DELEGATION_TEXT_BYTES`] besides its text, so
/// millions of empty entries can't pass as a few bytes: each costs its
/// `String` headers and allocations whatever it holds.
pub const DELEGATION_ITEM_BYTES: usize = 64;

/// How long a delegation may stay pending or in progress before a full
/// tracker treats it as abandoned. Nothing in the protocol fails or cancels
/// a delegation, so one whose worker died would otherwise hold its slot
/// until the daemon restarts.
pub const STALE_DELEGATION_SECS: i64 = 60 * 60;

/// Why the tracker refused a delegation request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DelegationError {
    #[error("delegation rejected: max depth ({max}) would be exceeded")]
    DepthExceeded { max: u8 },
    #[error("delegation rejected: all {limit} tracked delegations are still active")]
    TrackerFull { limit: usize },
    #[error("delegation {part} refused: {bytes} bytes, over the {limit}-byte limit")]
    TooLarge {
        part: &'static str,
        bytes: usize,
        limit: usize,
    },
    #[error("delegation not found: {id}")]
    NotFound { id: String },
    #[error("delegation {id} is already {state}")]
    AlreadyFinished { id: String, state: &'static str },
}

/// Specification of a delegated task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationSpec {
    /// What the worker should accomplish.
    pub task: String,
    /// Files the worker should focus on.
    #[serde(default)]
    pub target_files: Vec<String>,
    /// Additional constraints or context.
    #[serde(default)]
    pub constraints: Option<String>,
    /// Maximum nesting depth (0 = no sub-delegation allowed).
    #[serde(default = "default_max_depth")]
    pub max_depth: u8,
    /// Tools the worker is NOT allowed to use (Hermes pattern).
    #[serde(default)]
    pub restricted_tools: Vec<String>,
}

fn default_max_depth() -> u8 {
    MAX_DELEGATION_DEPTH
}

/// Current state of a tracked delegation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DelegationState {
    Pending,
    InProgress,
    Completed {
        summary: String,
        #[serde(default)]
        tool_trace: Vec<ToolInvocationRecord>,
        #[serde(default)]
        diff_summary: Option<DiffSummary>,
    },
    Failed {
        error: String,
    },
}

impl DelegationState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed { .. } => "completed",
            Self::Failed { .. } => "failed",
        }
    }
}

/// A delegation tracked by IMPULSE across panes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedDelegation {
    /// Unique delegation ID.
    pub id: String,
    /// Pane where the coordinator issued the delegation.
    pub coordinator_pane_id: usize,
    /// Pane where the worker is executing (if assigned).
    pub worker_pane_id: Option<usize>,
    /// Role of the coordinator.
    pub coordinator_role: AgentRole,
    /// The delegation specification.
    pub spec: DelegationSpec,
    /// Current state.
    pub state: DelegationState,
    /// When the delegation was first detected.
    pub created_at: DateTime<Utc>,
    /// When the delegation completed (if finished).
    pub completed_at: Option<DateTime<Utc>>,
    /// Frozen context snapshot from the coordinator at delegation time (Hermes pattern).
    #[serde(default)]
    pub context_snapshot: String,
    /// Current nesting depth.
    #[serde(default)]
    pub depth: u8,
}

impl TrackedDelegation {
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            DelegationState::Pending | DelegationState::InProgress
        )
    }

    pub fn is_completed(&self) -> bool {
        matches!(
            self.state,
            DelegationState::Completed { .. } | DelegationState::Failed { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delegation_state_as_str() {
        assert_eq!(DelegationState::Pending.as_str(), "pending");
        assert_eq!(DelegationState::InProgress.as_str(), "in_progress");
        assert_eq!(
            DelegationState::Completed {
                summary: String::new(),
                tool_trace: vec![],
                diff_summary: None,
            }
            .as_str(),
            "completed"
        );
        assert_eq!(
            DelegationState::Failed {
                error: "oops".into()
            }
            .as_str(),
            "failed"
        );
    }

    #[test]
    fn test_tracked_delegation_active() {
        let d = TrackedDelegation {
            id: "d-1".into(),
            coordinator_pane_id: 0,
            worker_pane_id: None,
            coordinator_role: AgentRole::Coordinator,
            spec: DelegationSpec {
                task: "test".into(),
                target_files: vec![],
                constraints: None,
                max_depth: 2,
                restricted_tools: vec![],
            },
            state: DelegationState::Pending,
            created_at: Utc::now(),
            completed_at: None,
            context_snapshot: String::new(),
            depth: 0,
        };
        assert!(d.is_active());
        assert!(!d.is_completed());
    }

    #[test]
    fn test_delegation_error_display_names_the_cause() {
        assert_eq!(
            DelegationError::DepthExceeded { max: 2 }.to_string(),
            "delegation rejected: max depth (2) would be exceeded"
        );
        assert!(DelegationError::TrackerFull { limit: 256 }
            .to_string()
            .contains("all 256 tracked delegations are still active"));
        assert_eq!(
            DelegationError::TooLarge {
                part: "spec",
                bytes: 300,
                limit: 200
            }
            .to_string(),
            "delegation spec refused: 300 bytes, over the 200-byte limit"
        );
        assert_eq!(
            DelegationError::NotFound { id: "del-9".into() }.to_string(),
            "delegation not found: del-9"
        );
        assert_eq!(
            DelegationError::AlreadyFinished {
                id: "del-1".into(),
                state: "completed"
            }
            .to_string(),
            "delegation del-1 is already completed"
        );
    }

    #[test]
    fn test_delegation_spec_serde_defaults() {
        let json = r#"{"task": "refactor auth"}"#;
        let spec: DelegationSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.task, "refactor auth");
        assert!(spec.target_files.is_empty());
        assert!(spec.constraints.is_none());
        assert_eq!(spec.max_depth, MAX_DELEGATION_DEPTH);
        assert!(spec.restricted_tools.is_empty());
    }

    #[test]
    fn test_delegation_spec_full() {
        let json = r#"{
            "task": "refactor auth module",
            "target_files": ["src/auth.rs", "src/auth/mod.rs"],
            "constraints": "Use zero-trust principles",
            "max_depth": 1,
            "restricted_tools": ["delegate_task", "memory"]
        }"#;
        let spec: DelegationSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.target_files.len(), 2);
        assert_eq!(spec.max_depth, 1);
        assert_eq!(spec.restricted_tools.len(), 2);
    }
}
