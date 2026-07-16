//! Durable review queue projected from accepted governed-task evidence.
//!
//! Governed tasks remain the source of truth. This ledger is independently
//! replaceable and is repaired deterministically after an interrupted
//! acceptance response or daemon restart.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use impulse_ops::governed_task::{
    GovernedReviewState, GovernedTaskRun, GovernedVerificationOutcome, OperatorDecisionKind,
    SupervisorVerdictKind,
};
use impulse_ops::memory_candidate::{
    AcceptedRunCommandEvidence, AcceptedRunMemoryCandidate, AcceptedRunSourceAssurance,
    MemoryCandidateId, MemoryCandidateStatus, ACCEPTED_RUN_MEMORY_CANDIDATE_SCHEMA_VERSION,
    ACCEPTED_RUN_MEMORY_DERIVATION_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::State;
use crate::storage::Storage;

const MEMORY_CANDIDATES_FILE: &str = "MEMORY_CANDIDATES.json";
const MEMORY_CANDIDATES_LEDGER_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct MemoryCandidateLedger {
    schema_version: u32,
    #[serde(default)]
    candidates: BTreeMap<MemoryCandidateId, AcceptedRunMemoryCandidate>,
}

impl Default for MemoryCandidateLedger {
    fn default() -> Self {
        Self {
            schema_version: MEMORY_CANDIDATES_LEDGER_SCHEMA_VERSION,
            candidates: BTreeMap::new(),
        }
    }
}

impl MemoryCandidateLedger {
    fn load(storage: &Storage) -> Result<Self> {
        let ledger: Self = storage
            .read_json(MEMORY_CANDIDATES_FILE)
            .context("Failed to read accepted-run memory candidate ledger")?;
        ledger.validate_shape()?;
        Ok(ledger)
    }

    fn validate_shape(&self) -> Result<()> {
        if self.schema_version != MEMORY_CANDIDATES_LEDGER_SCHEMA_VERSION {
            anyhow::bail!(
                "Unsupported memory candidate ledger schema version {}",
                self.schema_version
            );
        }
        let mut task_ids = BTreeSet::new();
        for (candidate_id, candidate) in &self.candidates {
            if candidate_id != &candidate.id {
                anyhow::bail!(
                    "memory candidate ledger key `{candidate_id}` does not match record id `{}`",
                    candidate.id
                );
            }
            candidate
                .validate_shape()
                .with_context(|| format!("Invalid memory candidate `{candidate_id}`"))?;
            if !task_ids.insert(candidate.governed_task_id.clone()) {
                anyhow::bail!(
                    "governed task `{}` has more than one accepted-run memory candidate",
                    candidate.governed_task_id
                );
            }
        }
        Ok(())
    }
}

impl State {
    pub(super) fn load_memory_candidate_ledger(
        storage: &Storage,
    ) -> Result<std::sync::Mutex<MemoryCandidateLedger>> {
        Ok(std::sync::Mutex::new(MemoryCandidateLedger::load(storage)?))
    }

    /// Reconcile the independently persisted review queue from authoritative
    /// accepted tasks. Missing candidates are recoverable; orphaned or
    /// digest-mismatched candidates fail closed.
    pub(super) fn reconcile_accepted_run_memory_candidates(&self) -> Result<()> {
        let tasks = self.all_governed_tasks()?;
        let expected = tasks
            .iter()
            .filter(|task| task.is_accepted())
            .map(derive_accepted_run_memory_candidate)
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(|candidate| (candidate.id.clone(), candidate))
            .collect::<BTreeMap<_, _>>();

        let mut ledger = self
            .memory_candidates
            .lock()
            .map_err(|error| anyhow::anyhow!("memory candidate ledger lock poisoned: {error}"))?;
        for (candidate_id, stored) in &ledger.candidates {
            let expected_candidate = expected.get(candidate_id).ok_or_else(|| {
                anyhow::anyhow!(
                    "memory candidate `{candidate_id}` is orphaned from accepted governed-task truth"
                )
            })?;
            if stored != expected_candidate {
                anyhow::bail!(
                    "memory candidate `{candidate_id}` does not match its accepted governed-task source"
                );
            }
        }

        let mut repaired = ledger.clone();
        for (candidate_id, candidate) in expected {
            repaired.candidates.entry(candidate_id).or_insert(candidate);
        }
        if repaired.candidates != ledger.candidates {
            self.storage()
                .write_private_json(MEMORY_CANDIDATES_FILE, &repaired)
                .context("Failed to persist reconciled accepted-run memory candidates")?;
            *ledger = repaired;
        }
        Ok(())
    }

    /// Ensure the projection exists after an acceptance mutation. The caller
    /// may invoke this again for an idempotent request replay.
    pub(super) fn ensure_accepted_run_memory_candidate(
        &self,
        task: &GovernedTaskRun,
    ) -> Result<()> {
        if !task.is_accepted() {
            return Ok(());
        }
        let candidate = derive_accepted_run_memory_candidate(task)?;
        let mut ledger = self
            .memory_candidates
            .lock()
            .map_err(|error| anyhow::anyhow!("memory candidate ledger lock poisoned: {error}"))?;
        if let Some(stored) = ledger.candidates.get(&candidate.id) {
            if stored != &candidate {
                anyhow::bail!(
                    "memory candidate `{}` conflicts with its deterministic accepted-run projection",
                    candidate.id
                );
            }
            return Ok(());
        }
        if ledger
            .candidates
            .values()
            .any(|stored| stored.governed_task_id == task.id)
        {
            anyhow::bail!(
                "governed task `{}` already has a different memory candidate",
                task.id
            );
        }

        let mut updated = ledger.clone();
        updated.candidates.insert(candidate.id.clone(), candidate);
        self.storage()
            .write_private_json(MEMORY_CANDIDATES_FILE, &updated)
            .context("Failed to persist accepted-run memory candidate")?;
        *ledger = updated;
        Ok(())
    }

    pub fn list_accepted_run_memory_candidates(
        &self,
        project_id: &str,
    ) -> Result<Vec<AcceptedRunMemoryCandidate>> {
        // Reuse the governed-task project boundary instead of independently
        // reconstructing project identity in the projection layer.
        let _ = self.list_governed_tasks(project_id)?;
        let ledger = self
            .memory_candidates
            .lock()
            .map_err(|error| anyhow::anyhow!("memory candidate ledger lock poisoned: {error}"))?;
        let mut candidates = ledger
            .candidates
            .values()
            .filter(|candidate| candidate.project_id == project_id)
            .cloned()
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.staged_at.cmp(&left.staged_at));
        Ok(candidates)
    }
}

#[derive(Serialize)]
struct CandidateSourceV1<'a> {
    derivation_version: u32,
    project_id: &'a str,
    workspace_root: &'a str,
    governed_task_id: &'a impulse_ops::governed_task::GovernedTaskId,
    accepted_task_revision: u64,
    task: &'a str,
    acceptance_criteria: &'a [String],
    runtime_id: &'a str,
    agent_id: &'a str,
    session_id: &'a Option<String>,
    verification_profile: Option<impulse_ops::governed_task::GovernedVerificationProfile>,
    verification_policy: &'a str,
    subject_revision: &'a str,
    claim_id: &'a impulse_ops::governed_task::GovernedRecordId,
    verification_id: &'a impulse_ops::governed_task::GovernedRecordId,
    supervisor_verdict_id: &'a impulse_ops::governed_task::GovernedRecordId,
    operator_decision_id: &'a impulse_ops::governed_task::GovernedRecordId,
    claimed_artifact_ids: &'a [String],
    verification_artifact_ids: &'a [String],
    commands: &'a [AcceptedRunCommandEvidence],
    source_assurance: AcceptedRunSourceAssurance,
    staged_at: &'a str,
}

pub(super) fn derive_accepted_run_memory_candidate(
    task: &GovernedTaskRun,
) -> Result<AcceptedRunMemoryCandidate> {
    if task.review_state != GovernedReviewState::Accepted {
        anyhow::bail!(
            "governed task `{}` is not eligible for a memory candidate",
            task.id
        );
    }
    let claim = task
        .latest_claim()
        .context("accepted governed task has no current worker claim")?;
    let verification = task
        .latest_verification()
        .context("accepted governed task has no current verification")?;
    let supervisor = task
        .latest_supervisor_verdict()
        .context("accepted governed task has no current Supervisor verdict")?;
    let operator = task
        .operator_decisions
        .last()
        .context("accepted governed task has no current operator decision")?;
    if verification.outcome != GovernedVerificationOutcome::Passed
        || verification.claim_id != claim.id
        || verification.subject_revision != claim.subject_revision
        || supervisor.verdict != SupervisorVerdictKind::RecommendAccept
        || supervisor.verification_id != verification.id
        || operator.decision != OperatorDecisionKind::Approve
        || operator.supervisor_verdict_id != supervisor.id
        || operator.based_on_revision.checked_add(1) != Some(task.revision)
    {
        anyhow::bail!(
            "accepted governed task `{}` has an incoherent candidate evidence chain",
            task.id
        );
    }

    let commands = verification
        .commands
        .iter()
        .map(|command| AcceptedRunCommandEvidence {
            name: command.name.clone(),
            command_digest: command.command_digest.clone(),
            output_digest: command.output_digest.clone(),
            exit_code: command.exit_code,
            success: command.success,
            output_bytes: command.output_bytes,
            output_truncated: command.output_truncated,
        })
        .collect::<Vec<_>>();
    let source_assurance = if task.verification_profile.is_some() {
        AcceptedRunSourceAssurance::DaemonProfiledEvidenceDeclaredOperator
    } else {
        AcceptedRunSourceAssurance::CallerComposedEvidenceDeclaredOperator
    };
    let source = CandidateSourceV1 {
        derivation_version: ACCEPTED_RUN_MEMORY_DERIVATION_VERSION,
        project_id: &task.project_id,
        workspace_root: &task.workspace_root,
        governed_task_id: &task.id,
        accepted_task_revision: task.revision,
        task: &task.task,
        acceptance_criteria: &task.acceptance_criteria,
        runtime_id: &task.runtime_id,
        agent_id: &task.agent_id,
        session_id: &task.session_id,
        verification_profile: task.verification_profile,
        verification_policy: &verification.policy,
        subject_revision: &verification.subject_revision,
        claim_id: &claim.id,
        verification_id: &verification.id,
        supervisor_verdict_id: &supervisor.id,
        operator_decision_id: &operator.id,
        claimed_artifact_ids: &claim.artifact_ids,
        verification_artifact_ids: &verification.artifact_ids,
        commands: &commands,
        source_assurance,
        staged_at: &operator.decided_at,
    };
    let source_bytes = serde_json::to_vec(&source)
        .context("Failed to serialize accepted-run memory candidate source")?;
    let source_hex = format!("{:x}", Sha256::digest(source_bytes));
    let source_digest = format!("sha256-v1:{source_hex}");
    let candidate_id = MemoryCandidateId::try_new(format!("memory-candidate-{source_hex}"))?;
    let proposed_summary = match source_assurance {
        AcceptedRunSourceAssurance::DaemonProfiledEvidenceDeclaredOperator => format!(
            "Accepted governed outcome for task: {}. Daemon-profiled evidence passed; pending semantic-memory review.",
            task.task
        ),
        AcceptedRunSourceAssurance::CallerComposedEvidenceDeclaredOperator => format!(
            "Accepted governed outcome for task: {}. Caller-composed evidence passed; pending semantic-memory review.",
            task.task
        ),
    };
    let candidate = AcceptedRunMemoryCandidate {
        id: candidate_id,
        schema_version: ACCEPTED_RUN_MEMORY_CANDIDATE_SCHEMA_VERSION,
        derivation_version: ACCEPTED_RUN_MEMORY_DERIVATION_VERSION,
        status: MemoryCandidateStatus::PendingReview,
        project_id: task.project_id.clone(),
        workspace_root: task.workspace_root.clone(),
        governed_task_id: task.id.clone(),
        accepted_task_revision: task.revision,
        task: task.task.clone(),
        acceptance_criteria: task.acceptance_criteria.clone(),
        proposed_summary,
        runtime_id: task.runtime_id.clone(),
        agent_id: task.agent_id.clone(),
        session_id: task.session_id.clone(),
        verification_profile: task.verification_profile,
        verification_policy: verification.policy.clone(),
        subject_revision: verification.subject_revision.clone(),
        claim_id: claim.id.clone(),
        verification_id: verification.id.clone(),
        supervisor_verdict_id: supervisor.id.clone(),
        operator_decision_id: operator.id.clone(),
        claimed_artifact_ids: claim.artifact_ids.clone(),
        verification_artifact_ids: verification.artifact_ids.clone(),
        commands,
        source_assurance,
        source_digest,
        staged_at: operator.decided_at.clone(),
    };
    candidate.validate_shape()?;
    Ok(candidate)
}

#[cfg(test)]
pub(super) fn memory_candidates_file() -> &'static str {
    MEMORY_CANDIDATES_FILE
}
