//! Delegation state tracker.
//!
//! Manages the lifecycle of tracked delegations across panes.
//! Generates handoff prompts for coordinator-readable summaries.

use std::collections::HashMap;

use chrono::Utc;
use impulse_ops::{AgentRole, DelegationSummary, DiffSummary, ToolInvocationRecord};

use super::types::{
    DelegationError, DelegationSpec, DelegationState, TrackedDelegation, DELEGATION_ITEM_BYTES,
    MAX_CONTEXT_SNAPSHOT_BYTES, MAX_DELEGATION_DEPTH, MAX_DELEGATION_TEXT_BYTES,
    MAX_TRACKED_DELEGATIONS, STALE_DELEGATION_SECS,
};

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

    /// Register a new delegation and return its ID.
    ///
    /// Refused past the depth limit, for a spec over
    /// [`MAX_DELEGATION_TEXT_BYTES`], or when all [`MAX_TRACKED_DELEGATIONS`]
    /// held delegations are still active and none is older than
    /// [`STALE_DELEGATION_SECS`]. Otherwise a full tracker drops its oldest
    /// finished delegation to make room, or failing that its oldest stale one.
    /// The context snapshot is cut to [`MAX_CONTEXT_SNAPSHOT_BYTES`].
    pub fn register(
        &mut self,
        spec: DelegationSpec,
        coordinator_pane_id: usize,
        mut context_snapshot: String,
        current_depth: u8,
    ) -> Result<String, DelegationError> {
        if current_depth >= MAX_DELEGATION_DEPTH {
            return Err(DelegationError::DepthExceeded {
                max: MAX_DELEGATION_DEPTH,
            });
        }
        check_size("spec", spec_bytes(&spec))?;
        if self.delegations.len() >= MAX_TRACKED_DELEGATIONS
            && !self.drop_oldest_finished()
            && !self.drop_oldest_stale()
        {
            return Err(DelegationError::TrackerFull {
                limit: MAX_TRACKED_DELEGATIONS,
            });
        }
        if context_snapshot.len() > MAX_CONTEXT_SNAPSHOT_BYTES {
            let cut = context_snapshot.floor_char_boundary(MAX_CONTEXT_SNAPSHOT_BYTES);
            context_snapshot.truncate(cut);
            // `truncate` keeps the whole allocation, which is what the cut is
            // meant to release.
            context_snapshot.shrink_to_fit();
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
        Ok(id)
    }

    /// Removes the finished delegation that finished first. Returns whether
    /// there was one.
    fn drop_oldest_finished(&mut self) -> bool {
        let oldest = self
            .delegations
            .values()
            .filter(|d| d.is_completed())
            .min_by_key(|d| (d.completed_at, d.created_at))
            .map(|d| d.id.clone());
        match oldest {
            Some(id) => self.delegations.remove(&id).is_some(),
            None => false,
        }
    }

    /// Removes the oldest delegation that has been pending or in progress for
    /// [`STALE_DELEGATION_SECS`] or more. Returns whether there was one.
    fn drop_oldest_stale(&mut self) -> bool {
        let oldest = self
            .stale_active(STALE_DELEGATION_SECS)
            .first()
            .map(|d| d.id.clone());
        match oldest {
            Some(id) => self.delegations.remove(&id).is_some(),
            None => false,
        }
    }

    /// The delegation `id`, if it exists and has not finished. A finished
    /// delegation's result is final: a second completion, failure or worker
    /// assignment would overwrite what the coordinator was already handed.
    fn active_mut(&mut self, id: &str) -> Result<&mut TrackedDelegation, DelegationError> {
        let delegation = self
            .delegations
            .get_mut(id)
            .ok_or_else(|| DelegationError::NotFound { id: id.to_string() })?;
        if delegation.is_completed() {
            return Err(DelegationError::AlreadyFinished {
                id: id.to_string(),
                state: delegation.state.as_str(),
            });
        }
        Ok(delegation)
    }

    /// Assign a worker pane to an unfinished delegation.
    pub fn assign_worker(
        &mut self,
        id: &str,
        worker_pane_id: usize,
    ) -> Result<(), DelegationError> {
        let d = self.active_mut(id)?;
        d.worker_pane_id = Some(worker_pane_id);
        d.state = DelegationState::InProgress;
        Ok(())
    }

    /// Mark an unfinished delegation as completed with results.
    pub fn complete(
        &mut self,
        id: &str,
        summary: String,
        tool_trace: Vec<ToolInvocationRecord>,
        diff_summary: Option<DiffSummary>,
    ) -> Result<(), DelegationError> {
        let completion_bytes = tool_trace
            .iter()
            .map(|tool| {
                [
                    DELEGATION_ITEM_BYTES,
                    tool.kind.len(),
                    tool.target.len(),
                    tool.timestamp.as_ref().map_or(0, String::len),
                ]
                .into_iter()
                .fold(0, usize::saturating_add)
            })
            .fold(summary.len(), usize::saturating_add);
        let d = self.active_mut(id)?;
        check_size("completion", completion_bytes)?;
        d.state = DelegationState::Completed {
            summary,
            tool_trace,
            diff_summary,
        };
        d.completed_at = Some(Utc::now());
        Ok(())
    }

    /// Mark an unfinished delegation as failed.
    pub fn fail(&mut self, id: &str, error: String) -> Result<(), DelegationError> {
        let d = self.active_mut(id)?;
        check_size("failure message", error.len())?;
        d.state = DelegationState::Failed { error };
        d.completed_at = Some(Utc::now());
        Ok(())
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

    /// Active delegations (Pending or InProgress) created more than
    /// `max_age_secs` ago — handoffs that look stalled because no worker has
    /// completed or failed them. Lets the coordinator surface stuck work (e.g.
    /// a worker that crashed or never picked up the task). Returned oldest-first.
    pub fn stale_active(&self, max_age_secs: i64) -> Vec<&TrackedDelegation> {
        let now = Utc::now();
        let mut stale: Vec<&TrackedDelegation> = self
            .delegations
            .values()
            .filter(|d| d.is_active() && (now - d.created_at).num_seconds() >= max_age_secs)
            .collect();
        stale.sort_by_key(|d| d.created_at);
        stale
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

/// What a spec weighs: its text, plus [`DELEGATION_ITEM_BYTES`] for each
/// list entry.
fn spec_bytes(spec: &DelegationSpec) -> usize {
    let list_bytes = |list: &[String]| {
        list.iter()
            .map(|item| item.len().saturating_add(DELEGATION_ITEM_BYTES))
            .fold(0, usize::saturating_add)
    };
    [
        spec.task.len(),
        spec.constraints.as_ref().map_or(0, String::len),
        list_bytes(&spec.target_files),
        list_bytes(&spec.restricted_tools),
    ]
    .into_iter()
    .fold(0, usize::saturating_add)
}

fn check_size(part: &'static str, bytes: usize) -> Result<(), DelegationError> {
    if bytes > MAX_DELEGATION_TEXT_BYTES {
        return Err(DelegationError::TooLarge {
            part,
            bytes,
            limit: MAX_DELEGATION_TEXT_BYTES,
        });
    }
    Ok(())
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

        tracker.assign_worker(&id, 1).unwrap();
        assert!(tracker.pending().is_empty());
        assert_eq!(tracker.active_for_pane(0).len(), 1);
        assert_eq!(tracker.active_for_pane(1).len(), 1);

        tracker
            .complete(
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
            )
            .unwrap();
        assert_eq!(tracker.completed().len(), 1);
        assert!(tracker.active_for_pane(0).is_empty());
    }

    #[test]
    fn test_depth_limit() {
        let mut tracker = DelegationTracker::new();
        // Depth 0 → OK
        assert!(tracker.register(sample_spec(), 0, "".into(), 0).is_ok());
        // Depth 1 → OK
        assert!(tracker.register(sample_spec(), 0, "".into(), 1).is_ok());
        // Depth 2 → REJECTED (MAX_DELEGATION_DEPTH = 2)
        assert_eq!(
            tracker.register(sample_spec(), 0, "".into(), 2),
            Err(DelegationError::DepthExceeded { max: 2 })
        );
    }

    #[test]
    fn test_build_handoff_prompt_completed() {
        let mut tracker = DelegationTracker::new();
        let id = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        tracker
            .complete(
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
            )
            .unwrap();

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
        tracker.fail(&id, "compilation failed".into()).unwrap();

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
        tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        tracker.register(sample_spec(), 1, "".into(), 0).unwrap();

        let summaries = tracker.to_summaries();
        assert_eq!(summaries.len(), 2);
    }

    #[test]
    fn test_stale_active_detection() {
        let mut tracker = DelegationTracker::new();
        let pending_id = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        let inprogress_id = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        tracker.assign_worker(&inprogress_id, 2).unwrap();
        let done_id = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        tracker
            .complete(&done_id, "done".into(), vec![], None)
            .unwrap();

        // max_age 0: every active (pending + in-progress) delegation is stale;
        // the completed one is excluded by is_active().
        let stale = tracker.stale_active(0);
        assert_eq!(stale.len(), 2);
        let ids: Vec<&str> = stale.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&pending_id.as_str()));
        assert!(ids.contains(&inprogress_id.as_str()));

        // A long threshold: freshly created delegations are not yet stale.
        assert!(tracker.stale_active(3600).is_empty());
    }

    #[test]
    fn test_prune_completed() {
        let mut tracker = DelegationTracker::new();
        let id = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        tracker.complete(&id, "done".into(), vec![], None).unwrap();

        // With a very short max age, should prune
        // But since it just completed, won't be pruned with a long max age
        tracker.prune_completed(3600);
        assert_eq!(tracker.delegations.len(), 1);
    }

    /// Review finding: a second CompleteDelegation was accepted and replaced
    /// the first worker's result after the coordinator had been handed it.
    #[test]
    fn test_a_finished_delegation_cannot_be_completed_failed_or_reassigned() {
        let mut tracker = DelegationTracker::new();
        let id = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        tracker
            .complete(&id, "worker A: refactored src/auth.rs".into(), vec![], None)
            .unwrap();

        let already = DelegationError::AlreadyFinished {
            id: id.clone(),
            state: "completed",
        };
        assert_eq!(
            tracker.complete(&id, "worker B: nothing to do".into(), vec![], None),
            Err(already.clone())
        );
        assert_eq!(
            tracker.fail(&id, "late failure".into()),
            Err(already.clone())
        );
        assert_eq!(tracker.assign_worker(&id, 7), Err(already));

        let prompt = tracker.build_handoff_prompt(&id).unwrap();
        assert!(prompt.contains("worker A"), "{prompt}");
        assert!(!prompt.contains("worker B"), "{prompt}");
        assert!(tracker.delegations[&id].worker_pane_id.is_none());
    }

    #[test]
    fn test_unknown_delegation_ids_are_not_found() {
        let mut tracker = DelegationTracker::new();
        let missing = DelegationError::NotFound { id: "del-9".into() };
        assert_eq!(
            tracker.complete("del-9", "x".into(), vec![], None),
            Err(missing.clone())
        );
        assert_eq!(tracker.fail("del-9", "x".into()), Err(missing.clone()));
        assert_eq!(tracker.assign_worker("del-9", 1), Err(missing));
    }

    /// Review finding: nothing ever removed a delegation, so the daemon's
    /// tracker grew with every one registered.
    #[test]
    fn test_register_at_capacity_drops_the_oldest_finished_delegation() {
        let mut tracker = DelegationTracker::new();
        let first = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        let second = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        tracker
            .complete(&second, "done".into(), vec![], None)
            .unwrap();
        tracker.fail(&first, "broke".into()).unwrap();
        // `second` finished first.
        let earlier = Utc::now() - chrono::Duration::seconds(60);
        tracker.delegations.get_mut(&second).unwrap().completed_at = Some(earlier);
        while tracker.delegations.len() < MAX_TRACKED_DELEGATIONS {
            tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        }

        let newest = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        assert_eq!(tracker.delegations.len(), MAX_TRACKED_DELEGATIONS);
        assert!(!tracker.delegations.contains_key(&second));
        assert!(tracker.delegations.contains_key(&first));
        assert!(tracker.delegations.contains_key(&newest));

        tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        assert!(!tracker.delegations.contains_key(&first));
        assert_eq!(tracker.delegations.len(), MAX_TRACKED_DELEGATIONS);
    }

    #[test]
    fn test_register_refuses_when_every_tracked_delegation_is_active() {
        let mut tracker = DelegationTracker::new();
        for _ in 0..MAX_TRACKED_DELEGATIONS {
            tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        }
        assert_eq!(
            tracker.register(sample_spec(), 0, "".into(), 0),
            Err(DelegationError::TrackerFull {
                limit: MAX_TRACKED_DELEGATIONS
            })
        );
        assert_eq!(tracker.pending().len(), MAX_TRACKED_DELEGATIONS);
    }

    /// Review finding: each registration kept its whole context snapshot
    /// (up to the daemon's 10 MiB request size), though nothing reads it.
    #[test]
    fn test_register_cuts_the_context_snapshot_at_a_char_boundary() {
        let mut tracker = DelegationTracker::new();
        // One ASCII byte, then two-byte characters: the cap lands mid-character.
        let snapshot = format!("a{}", "é".repeat(MAX_CONTEXT_SNAPSHOT_BYTES / 2));
        let id = tracker.register(sample_spec(), 0, snapshot, 0).unwrap();
        let kept = &tracker.delegations[&id].context_snapshot;
        assert_eq!(kept.len(), MAX_CONTEXT_SNAPSHOT_BYTES - 1);
        assert!(kept.ends_with('é'));
        // Verification finding: `truncate` alone kept the whole allocation.
        assert!(
            kept.capacity() <= MAX_CONTEXT_SNAPSHOT_BYTES,
            "kept {} bytes of capacity",
            kept.capacity()
        );

        let small = tracker.register(sample_spec(), 0, "ctx".into(), 0).unwrap();
        assert_eq!(tracker.delegations[&small].context_snapshot, "ctx");
    }

    #[test]
    fn test_oversized_spec_and_completion_are_refused() {
        let mut tracker = DelegationTracker::new();
        let mut spec = sample_spec();
        spec.task = "x".repeat(MAX_DELEGATION_TEXT_BYTES);
        assert!(matches!(
            tracker.register(spec, 0, "".into(), 0),
            Err(DelegationError::TooLarge { part: "spec", .. })
        ));
        assert!(tracker.delegations.is_empty());

        let id = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        let summary = "x".repeat(MAX_DELEGATION_TEXT_BYTES - 3);
        let trace = vec![ToolInvocationRecord {
            kind: "edit".into(),
            target: "a.rs".into(),
            timestamp: None,
        }];
        assert!(matches!(
            tracker.complete(&id, summary.clone(), trace, None),
            Err(DelegationError::TooLarge {
                part: "completion",
                ..
            })
        ));
        assert!(tracker.delegations[&id].is_active());
        tracker.complete(&id, summary, vec![], None).unwrap();
    }

    /// Verification finding: the limits counted text only, so a request of
    /// millions of empty list entries passed as a few bytes and held
    /// hundreds of megabytes.
    #[test]
    fn test_floods_of_empty_entries_are_refused() {
        let mut tracker = DelegationTracker::new();
        let mut spec = sample_spec();
        spec.target_files = vec![String::new(); MAX_DELEGATION_TEXT_BYTES / DELEGATION_ITEM_BYTES];
        assert!(matches!(
            tracker.register(spec, 0, "".into(), 0),
            Err(DelegationError::TooLarge { part: "spec", .. })
        ));

        let id = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        let empty_record = ToolInvocationRecord {
            kind: String::new(),
            target: String::new(),
            timestamp: None,
        };
        let trace = vec![empty_record; MAX_DELEGATION_TEXT_BYTES / DELEGATION_ITEM_BYTES + 1];
        assert!(matches!(
            tracker.complete(&id, String::new(), trace, None),
            Err(DelegationError::TooLarge {
                part: "completion",
                ..
            })
        ));
        assert!(tracker.delegations[&id].is_active());
    }

    /// Verification finding: nothing in the protocol fails or cancels a
    /// delegation, so a tracker full of abandoned ones refused every new one
    /// until the daemon restarted.
    #[test]
    fn test_a_full_tracker_drops_its_oldest_stale_delegation() {
        let mut tracker = DelegationTracker::new();
        let ids: Vec<String> = (0..MAX_TRACKED_DELEGATIONS)
            .map(|_| tracker.register(sample_spec(), 0, "".into(), 0).unwrap())
            .collect();
        let long_ago = Utc::now() - chrono::Duration::seconds(STALE_DELEGATION_SECS + 60);
        let older = long_ago - chrono::Duration::seconds(60);
        tracker.delegations.get_mut(&ids[7]).unwrap().created_at = long_ago;
        tracker.delegations.get_mut(&ids[3]).unwrap().created_at = older;

        let newest = tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        assert_eq!(tracker.delegations.len(), MAX_TRACKED_DELEGATIONS);
        assert!(!tracker.delegations.contains_key(&ids[3]));
        assert!(tracker.delegations.contains_key(&ids[7]));
        assert!(tracker.delegations.contains_key(&newest));

        tracker.register(sample_spec(), 0, "".into(), 0).unwrap();
        assert!(!tracker.delegations.contains_key(&ids[7]));
        assert_eq!(
            tracker.register(sample_spec(), 0, "".into(), 0),
            Err(DelegationError::TrackerFull {
                limit: MAX_TRACKED_DELEGATIONS
            })
        );
    }
}
