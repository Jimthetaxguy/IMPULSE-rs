//! Shared application state for the Dioxus supervisor shell.
//!
//! STATUS: SCAFFOLD (Loop 151). Domain sub-states that replace the
//! `impulse-gui::state.rs` 1,069-line god-object once `impulse-gui` retires.
//!
//! Key principle: separate sub-states by concern (sessions / terminals / ops)
//! so Dioxus signals can be fine-grained and reads dominate over writes.

use crate::panes::PaneRegistry;
use serde::{Deserialize, Serialize};

/// Top-level shell state. In the live runtime this is wrapped in
/// `std::sync::RwLock` and exposed through Dioxus signals so components
/// subscribe only to the sub-state they care about.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShellState {
    pub session: SessionState,
    pub terminals: TerminalState,
    pub ops: OpsState,
}

/// What Impulse session is active right now — project genome, registered
/// projects, current pane focus.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionState {
    pub active_project: Option<String>,
    pub registered_projects: Vec<String>,
    pub focused_pane: Option<uuid::Uuid>,
}

/// Everything about live terminal panes — registry + layout mode.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TerminalState {
    pub registry: PaneRegistry,
    pub layout: crate::layout::LayoutMode,
    pub worker_grid: Option<crate::layout::WorkerGrid>,
}

/// Operator/ops-workbench state — alerts, compaction proposals, ledger view.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpsState {
    pub active_compactions: Vec<CompactionProposal>,
    pub notification_count: u32,
}

/// A compaction proposal the supervisor has surfaced but the user hasn't
/// accepted yet. Matches the L122 CompactionFlow state machine's `Proposed`
/// step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionProposal {
    pub pane_id: uuid::Uuid,
    pub reason: CompactionReason,
    pub token_usage_pct: u8,
    pub message_count: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompactionReason {
    /// Token count crossed the auto threshold (default 82%).
    AutoThreshold,
    /// Token count crossed the warn threshold (default 75%) but not auto.
    WarnThreshold,
    /// Semantic drift detected by the rolling-window similarity + classifier.
    DriftDetected,
    /// User issued `@impulse compact` explicitly.
    UserRequested,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PaneRoleRef;
    use crate::chrono_ish::Timestamp;

    #[test]
    fn test_default_shell_state_is_empty() {
        let s = ShellState::default();
        assert!(s.session.active_project.is_none());
        assert_eq!(s.terminals.registry.worker_count(), 0);
        assert_eq!(s.ops.notification_count, 0);
    }

    #[test]
    fn test_session_state_projects() {
        let mut s = SessionState::default();
        s.registered_projects.push("impulse".into());
        s.registered_projects.push("tax-forms".into());
        s.active_project = Some("impulse".into());
        assert_eq!(s.registered_projects.len(), 2);
        assert_eq!(s.active_project.as_deref(), Some("impulse"));
    }

    #[test]
    fn test_compaction_proposal_roundtrip() {
        let proposal = CompactionProposal {
            pane_id: uuid::Uuid::nil(),
            reason: CompactionReason::AutoThreshold,
            token_usage_pct: 82,
            message_count: 47,
        };
        let json = serde_json::to_string(&proposal).unwrap();
        let recovered: CompactionProposal = serde_json::from_str(&json).unwrap();
        assert_eq!(proposal, recovered);
    }

    #[test]
    fn test_compaction_reason_variants_roundtrip() {
        for reason in [
            CompactionReason::AutoThreshold,
            CompactionReason::WarnThreshold,
            CompactionReason::DriftDetected,
            CompactionReason::UserRequested,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            let recovered: CompactionReason = serde_json::from_str(&json).unwrap();
            assert_eq!(reason, recovered);
        }
    }

    #[test]
    fn test_ops_state_tracks_proposals() {
        let mut ops = OpsState::default();
        ops.active_compactions.push(CompactionProposal {
            pane_id: uuid::Uuid::new_v4(),
            reason: CompactionReason::DriftDetected,
            token_usage_pct: 68,
            message_count: 23,
        });
        assert_eq!(ops.active_compactions.len(), 1);
    }

    #[test]
    fn test_terminal_state_with_supervisor() {
        let mut t = TerminalState::default();
        let sup = crate::PaneIdentity {
            id: uuid::Uuid::new_v4(),
            role: PaneRoleRef::Supervisor,
            project: Some("impulse".into()),
            cwd: std::path::PathBuf::from("/tmp"),
            spawned_at: Timestamp::now(),
        };
        t.registry.add(sup).unwrap();
        assert!(t.registry.supervisor().is_some());
    }

    #[test]
    fn test_full_state_serde_roundtrip() {
        let mut s = ShellState::default();
        s.session.active_project = Some("test".into());
        s.ops.notification_count = 3;
        let json = serde_json::to_string(&s).unwrap();
        let recovered: ShellState = serde_json::from_str(&json).unwrap();
        assert_eq!(s.session.active_project, recovered.session.active_project);
        assert_eq!(s.ops.notification_count, recovered.ops.notification_count);
    }
}
