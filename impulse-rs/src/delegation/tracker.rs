//! Delegation state tracker.
//!
//! Manages the lifecycle of tracked delegations across panes.
//! Generates handoff prompts for coordinator-readable summaries.

use std::collections::HashMap;

use chrono::Utc;
use impulse_ops::{AgentRole, DelegationSummary, DiffSummary, ToolInvocationRecord};

use super::types::{DelegationSpec, DelegationState, TrackedDelegation, MAX_DELEGATION_DEPTH};

/// Tracks all active and recent delegations.
#[derive(Debug, Default)]
pub struct DelegationTracker {
    delegations: HashMap<String, TrackedDelegation>,
    next_id: u64,
}

impl DelegationTracker {
    pub fn new() -> Self {
        Self {
            delegations: HashMap::new(),
            next_id: 1,
        }
    }

    /// Register a new delegation. Returns the delegation ID.
    /// Returns None if max depth would be exceeded.
    pub fn register(
        &mut self,
        spec: DelegationSpec,
        coordinator_pane_id: usize,
        context_snapshot: String,
        current_depth: u8,
    ) -> Option<String> {
        if current_depth >= MAX_DELEGATION_DEPTH {
            return None;
        }

        let id = format!("del-{}", self.next_id);
        self.next_id += 1;

        let delegation = TrackedDelegation {
            id: id.clone(),
            coordinator_pane_id,
            worker_pane_id: None,
            coordinator_role: AgentRole::Coordinator,
            spec,
            state: DelegationState::Pending,
            created_at: Utc::now(),
            completed_at: None,
            context_snapshot,
            depth: current_depth,
        };

        self.delegations.insert(id.clone(), delegation);
        Some(id)
    }

    /// Assign a worker pane to a delegation.
    pub fn assign_worker(&mut self, id: &str, worker_pane_id: usize) -> bool {
        if let Some(d) = self.delegations.get_mut(id) {
            d.worker_pane_id = Some(worker_pane_id);
            d.state = DelegationState::InProgress;
            true
        } else {
            false
        }
    }

    /// Mark a delegation as completed with results.
    pub fn complete(
        &mut self,
        id: &str,
        summary: String,
        tool_trace: Vec<ToolInvocationRecord>,
        diff_summary: Option<DiffSummary>,
    ) -> bool {
        if let Some(d) = self.delegations.get_mut(id) {
            d.state = DelegationState::Completed {
                summary,
                tool_trace,
                diff_summary,
            };
            d.completed_at = Some(Utc::now());
            true
        } else {
            false
        }
    }

    /// Mark a delegation as failed.
    pub fn fail(&mut self, id: &str, error: String) -> bool {
        if let Some(d) = self.delegations.get_mut(id) {
            d.state = DelegationState::Failed { error };
            d.completed_at = Some(Utc::now());
            true
        } else {
            false
        }
    }

    /// Build a handoff prompt for a completed delegation.
    /// Format inspired by Hermes Agent: status + summary + tool_trace.
    pub fn build_handoff_prompt(&self, id: &str) -> Option<String> {
        let d = self.delegations.get(id)?;
        match &d.state {
            DelegationState::Completed {
                summary,
                tool_trace,
                diff_summary,
            } => {
                let mut prompt = String::new();
                prompt.push_str("## Delegation Complete\n\n");
                prompt.push_str(&format!("**Task**: {}\n", d.spec.task));
                prompt.push_str("**Status**: completed\n");
                if !d.spec.target_files.is_empty() {
                    prompt.push_str(&format!("**Files**: {}\n", d.spec.target_files.join(", ")));
                }
                prompt.push_str(&format!("\n### Summary\n{}\n", summary));

                if !tool_trace.is_empty() {
                    prompt.push_str("\n### Tool Trace\n");
                    for tool in tool_trace {
                        prompt.push_str(&format!("- {} → {}\n", tool.kind, tool.target));
                    }
                }

                if let Some(diff) = diff_summary {
                    prompt.push_str(&format!(
                        "\n### Diff Summary\n{} files changed, +{} -{}\n",
                        diff.files_changed, diff.lines_added, diff.lines_removed,
                    ));
                }

                Some(prompt)
            }
            DelegationState::Failed { error } => Some(format!(
                "## Delegation Failed\n\n**Task**: {}\n**Error**: {}\n",
                d.spec.task, error
            )),
            _ => None,
        }
    }

    /// Get all active delegations for a pane (as coordinator or worker).
    pub fn active_for_pane(&self, pane_id: usize) -> Vec<&TrackedDelegation> {
        self.delegations
            .values()
            .filter(|d| {
                d.is_active()
                    && (d.coordinator_pane_id == pane_id || d.worker_pane_id == Some(pane_id))
            })
            .collect()
    }

    /// Get all pending delegations (not yet assigned a worker).
    pub fn pending(&self) -> Vec<&TrackedDelegation> {
        self.delegations
            .values()
            .filter(|d| matches!(d.state, DelegationState::Pending))
            .collect()
    }

    /// Get all completed delegations.
    pub fn completed(&self) -> Vec<&TrackedDelegation> {
        self.delegations
            .values()
            .filter(|d| d.is_completed())
            .collect()
    }

    /// Export delegation summaries for impulse-ops consumption.
    pub fn to_summaries(&self) -> Vec<DelegationSummary> {
        self.delegations
            .values()
            .map(|d| {
                let (tool_invocations, diff_summary) = match &d.state {
                    DelegationState::Completed {
                        tool_trace,
                        diff_summary,
                        ..
                    } => (tool_trace.clone(), diff_summary.clone()),
                    _ => (vec![], None),
                };
                DelegationSummary {
                    id: d.id.clone(),
                    task: d.spec.task.clone(),
                    state: d.state.as_str().to_string(),
                    coordinator_pane_id: d.coordinator_pane_id,
                    worker_pane_id: d.worker_pane_id,
                    created_at: d.created_at.to_rfc3339(),
                    completed_at: d.completed_at.map(|t| t.to_rfc3339()),
                    tool_invocations,
                    diff_summary,
                }
            })
            .collect()
    }

    /// Remove completed delegations older than the given duration.
    pub fn prune_completed(&mut self, max_age_secs: i64) {
        let now = Utc::now();
        self.delegations.retain(|_, d| {
            if let Some(completed_at) = d.completed_at {
                (now - completed_at).num_seconds() < max_age_secs
            } else {
                true // keep active delegations
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_spec() -> DelegationSpec {
        DelegationSpec {
            task: "refactor auth".into(),
            target_files: vec!["src/auth.rs".into()],
            constraints: None,
            max_depth: 2,
            restricted_tools: vec![],
        }
    }

    #[test]
    fn test_register_and_complete() {
        let mut tracker = DelegationTracker::new();
        let id = tracker
            .register(sample_spec(), 0, "snapshot".into(), 0)
            .unwrap();
        assert_eq!(tracker.pending().len(), 1);

        tracker.assign_worker(&id, 1);
        assert!(tracker.pending().is_empty());
        assert_eq!(tracker.active_for_pane(0).len(), 1);
        assert_eq!(tracker.active_for_pane(1).len(), 1);

        tracker.complete(
            &id,
            "Done refactoring".into(),
            vec![ToolInvocationRecord {
                kind: "edit".into(),
                target: "src/auth.rs".into(),
                timestamp: None,
            }],
            Some(DiffSummary {
                files_changed: 1,
                lines_added: 20,
                lines_removed: 5,
            }),
        );
        assert_eq!(tracker.completed().len(), 1);
        assert!(tracker.active_for_pane(0).is_empty());
    }

    #[test]
    fn test_depth_limit() {
        let mut tracker = DelegationTracker::new();
        // Depth 0 → OK
        assert!(tracker.register(sample_spec(), 0, "".into(), 0).is_some());
        // Depth 1 → OK
        assert!(tracker.register(sample_spec(), 0, "".into(), 1).is_some());
        // Depth 2 → REJECTED (MAX_DELEGATION_DEPTH = 2)
        assert!(tracker.register(sample_spec(), 0, "".into(), 2).is_none());
    }

    #[test]
    fn test_build_handoff_prompt_completed() {
        let mut tracker = DelegationTracker::new();
        let id = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        tracker.complete(
            &id,
            "Auth module refactored with zero-trust".into(),
            vec![ToolInvocationRecord {
                kind: "edit".into(),
                target: "src/auth.rs".into(),
                timestamp: None,
            }],
            Some(DiffSummary {
                files_changed: 1,
                lines_added: 30,
                lines_removed: 10,
            }),
        );

        let prompt = tracker.build_handoff_prompt(&id).unwrap();
        assert!(prompt.contains("Delegation Complete"));
        assert!(prompt.contains("refactor auth"));
        assert!(prompt.contains("zero-trust"));
        assert!(prompt.contains("edit → src/auth.rs"));
        assert!(prompt.contains("+30 -10"));
    }

    #[test]
    fn test_build_handoff_prompt_failed() {
        let mut tracker = DelegationTracker::new();
        let id = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        tracker.fail(&id, "compilation failed".into());

        let prompt = tracker.build_handoff_prompt(&id).unwrap();
        assert!(prompt.contains("Delegation Failed"));
        assert!(prompt.contains("compilation failed"));
    }

    #[test]
    fn test_build_handoff_prompt_pending_returns_none() {
        let mut tracker = DelegationTracker::new();
        let id = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        assert!(tracker.build_handoff_prompt(&id).is_none());
    }

    #[test]
    fn test_to_summaries() {
        let mut tracker = DelegationTracker::new();
        tracker.register(sample_spec(), 0, "".into(), 0);
        tracker.register(sample_spec(), 1, "".into(), 0);

        let summaries = tracker.to_summaries();
        assert_eq!(summaries.len(), 2);
    }

    #[test]
    fn test_prune_completed() {
        let mut tracker = DelegationTracker::new();
        let id = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        tracker.complete(&id, "done".into(), vec![], None);

        // With a very short max age, should prune
        // But since it just completed, won't be pruned with a long max age
        tracker.prune_completed(3600);
        assert_eq!(tracker.delegations.len(), 1);
    }
}
