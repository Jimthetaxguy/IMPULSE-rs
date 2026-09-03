//! Project-bound persistent ledger for governed task runs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use impulse_ops::governed_task::{
    GovernedActor, GovernedActorKind, GovernedExecutionState, GovernedRecordId, GovernedRequestId,
    GovernedReviewState, GovernedTaskContractError, GovernedTaskEvent, GovernedTaskEventKind,
    GovernedTaskId, GovernedTaskMutation, GovernedTaskMutationRequest, GovernedTaskRegistration,
    GovernedTaskRun, GovernedVerification, GovernedVerificationInput, GovernedVerificationOutcome,
    OperatorAuthentication, OperatorDecision, OperatorDecisionInput, OperatorDecisionKind,
    SupervisorVerdict, SupervisorVerdictInput, SupervisorVerdictKind, WorkerCompletionClaim,
    WorkerCompletionClaimInput, MAX_GOVERNED_COMMANDS, MAX_GOVERNED_COMMAND_ARGS,
    MAX_GOVERNED_COMMAND_ARG_BYTES, MAX_GOVERNED_EVENTS, MAX_GOVERNED_RECORDS_PER_KIND,
    MAX_GOVERNED_REFERENCES, MAX_GOVERNED_REFERENCE_BYTES,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::State;
use crate::storage::Storage;

const GOVERNED_TASKS_FILE: &str = "GOVERNED_TASKS.json";
const GOVERNED_TASKS_SCHEMA_VERSION: u32 = 1;
const MAX_REASON_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GovernedTaskStateError {
    #[error(transparent)]
    Contract(#[from] GovernedTaskContractError),
    #[error("governed task `{0}` was not found")]
    NotFound(GovernedTaskId),
    #[error("governed task `{0}` is already registered under another request")]
    AlreadyExists(GovernedTaskId),
    #[error("governed task project mismatch: expected `{expected}`, received `{received}`")]
    ProjectMismatch { expected: String, received: String },
    #[error("governed task workspace mismatch: expected `{expected}`, received `{received}`")]
    WorkspaceMismatch { expected: String, received: String },
    #[error("governed task revision conflict: expected {expected}, current {current}")]
    RevisionConflict { expected: u64, current: u64 },
    #[error("governed task request id `{request_id}` was already used for task `{task_id}`")]
    IdempotencyConflict {
        request_id: GovernedRequestId,
        task_id: GovernedTaskId,
    },
    #[error("governed task request id `{request_id}` was replayed with a different payload")]
    IdempotencyPayloadConflict { request_id: GovernedRequestId },
    #[error("invalid governed task transition: {0}")]
    InvalidTransition(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GovernedMutationReceipt {
    task_id: GovernedTaskId,
    resulting_revision: u64,
    #[serde(default)]
    request_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct GovernedTaskLedger {
    schema_version: u32,
    #[serde(default)]
    tasks: BTreeMap<GovernedTaskId, GovernedTaskRun>,
    #[serde(default)]
    processed_requests: BTreeMap<GovernedRequestId, GovernedMutationReceipt>,
}

impl Default for GovernedTaskLedger {
    fn default() -> Self {
        Self {
            schema_version: GOVERNED_TASKS_SCHEMA_VERSION,
            tasks: BTreeMap::new(),
            processed_requests: BTreeMap::new(),
        }
    }
}

impl GovernedTaskLedger {
    fn load(storage: &Storage) -> Result<Self> {
        let ledger: Self = storage
            .read_json(GOVERNED_TASKS_FILE)
            .context("Failed to read governed task ledger")?;
        if ledger.schema_version != GOVERNED_TASKS_SCHEMA_VERSION {
            anyhow::bail!(
                "Unsupported governed task ledger schema version {}",
                ledger.schema_version
            );
        }
        ledger.validate(storage)?;
        Ok(ledger)
    }

    fn validate(&self, storage: &Storage) -> Result<()> {
        let expected_project = governed_project_id_for_storage(storage);

        for (task_id, task) in &self.tasks {
            if task_id != &task.id {
                anyhow::bail!(
                    "governed task ledger key `{task_id}` does not match record id `{}`",
                    task.id
                );
            }
        }

        let mut receipt_index = self
            .tasks
            .keys()
            .cloned()
            .map(|task_id| (task_id, BTreeMap::new()))
            .collect::<BTreeMap<_, _>>();
        for (request_id, receipt) in &self.processed_requests {
            let task = self.tasks.get(&receipt.task_id).ok_or_else(|| {
                anyhow::anyhow!(
                    "governed task receipt `{request_id}` references missing task `{}`",
                    receipt.task_id
                )
            })?;
            require_receipt_revision(receipt, task)?;
            require_request_fingerprint(&receipt.request_fingerprint).with_context(|| {
                format!("Invalid governed task receipt fingerprint for `{request_id}`")
            })?;
            let receipts = receipt_index.get_mut(&receipt.task_id).ok_or_else(|| {
                anyhow::anyhow!(
                    "governed task receipt `{request_id}` lost its validated task target"
                )
            })?;
            if receipts
                .insert(
                    receipt.resulting_revision,
                    (request_id.clone(), receipt.request_fingerprint.clone()),
                )
                .is_some()
            {
                anyhow::bail!(
                    "governed task `{}` has multiple receipts for revision {}",
                    receipt.task_id,
                    receipt.resulting_revision
                );
            }
        }

        // An empty ledger has no persisted workspace claim to validate. Keeping
        // this filesystem-independent preserves `impulse init -c <new-path>`
        // and ordinary commands that use the relative default `.impulse` path.
        if self.tasks.is_empty() {
            return Ok(());
        }

        let expected_workspace = canonical_governed_project_root(storage)?;

        for (task_id, task) in &self.tasks {
            require_project(&expected_project, &task.project_id)?;
            let stored_workspace = Path::new(&task.workspace_root);
            if !stored_workspace.is_absolute() {
                anyhow::bail!(
                    "governed task `{task_id}` workspace is not an absolute canonical path"
                );
            }
            let canonical_workspace = stored_workspace.canonicalize().with_context(|| {
                format!(
                    "Failed to canonicalize governed task `{task_id}` workspace {}",
                    stored_workspace.display()
                )
            })?;
            if canonical_workspace != expected_workspace
                || task.workspace_root != canonical_workspace.display().to_string()
            {
                return Err(GovernedTaskStateError::WorkspaceMismatch {
                    expected: expected_workspace.display().to_string(),
                    received: task.workspace_root.clone(),
                }
                .into());
            }

            let receipts = receipt_index
                .get(task_id)
                .ok_or_else(|| anyhow::anyhow!("governed task `{task_id}` has no receipt index"))?;
            let expected_count = task
                .revision
                .checked_add(1)
                .and_then(|count| usize::try_from(count).ok())
                .ok_or_else(|| {
                    anyhow::anyhow!("governed task `{task_id}` revision is too large")
                })?;
            if receipts.len() != expected_count
                || !(0..=task.revision).all(|revision| receipts.contains_key(&revision))
            {
                anyhow::bail!(
                    "governed task `{task_id}` receipt chain does not cover every revision from 0 through {}",
                    task.revision
                );
            }

            let (registration_request_id, stored_registration_fingerprint) =
                receipts.get(&0).ok_or_else(|| {
                    anyhow::anyhow!("governed task `{task_id}` has no registration receipt")
                })?;
            let registration = stored_registration(task, registration_request_id.clone())?;
            registration.validate()?;
            let expected_registration_fingerprint = fingerprint_request(&registration)
                .context("Failed to recompute governed task registration fingerprint")?;
            if stored_registration_fingerprint != &expected_registration_fingerprint {
                anyhow::bail!(
                    "governed task `{task_id}` registration receipt fingerprint does not match its immutable task record"
                );
            }

            let mutation_fingerprints = validate_task_history(task)
                .with_context(|| format!("Invalid governed task history for `{task_id}`"))?;
            for (revision, expected_fingerprint) in mutation_fingerprints {
                let (_, stored_fingerprint) = receipts.get(&revision).ok_or_else(|| {
                    anyhow::anyhow!(
                        "governed task `{task_id}` has no receipt for mutation revision {revision}"
                    )
                })?;
                if stored_fingerprint != &expected_fingerprint {
                    anyhow::bail!(
                        "governed task `{task_id}` receipt fingerprint at revision {revision} does not match its lifecycle history"
                    );
                }
            }
        }
        Ok(())
    }
}

fn stored_registration(
    task: &GovernedTaskRun,
    request_id: GovernedRequestId,
) -> Result<GovernedTaskRegistration> {
    Ok(GovernedTaskRegistration {
        request_id,
        task_id: task.id.clone(),
        project_id: task.project_id.clone(),
        workspace_root: task.workspace_root.clone(),
        task: task.task.clone(),
        acceptance_criteria: task.acceptance_criteria.clone(),
        approval_policy: task.approval_policy,
        verification_profile: task.verification_profile,
        role_assignment: task.role_assignment.clone(),
        role_compatibility: task.role_compatibility.clone(),
        runtime_id: task.runtime_id.clone(),
        agent_id: task.agent_id.clone(),
        session_id: task.session_id.clone(),
        initial_subject_revision: task.initial_subject_revision.clone(),
    })
}

impl State {
    pub(super) fn load_governed_task_ledger(
        storage: &Storage,
    ) -> Result<std::sync::Mutex<GovernedTaskLedger>> {
        Ok(std::sync::Mutex::new(GovernedTaskLedger::load(storage)?))
    }

    pub fn register_governed_task(
        &self,
        mut registration: GovernedTaskRegistration,
    ) -> Result<GovernedTaskRun> {
        registration.validate()?;
        let expected_project = self.governed_project_id();
        require_project(&expected_project, &registration.project_id)?;
        registration.workspace_root =
            self.canonical_governed_workspace(&registration.workspace_root)?;
        let request_fingerprint = fingerprint_request(&registration)
            .context("Failed to fingerprint governed task registration")?;

        let mut ledger = self
            .governed_tasks
            .lock()
            .map_err(|error| anyhow::anyhow!("governed task ledger lock poisoned: {error}"))?;
        if let Some(receipt) = ledger.processed_requests.get(&registration.request_id) {
            require_idempotent_fingerprint(
                &registration.request_id,
                receipt,
                request_fingerprint.as_str(),
            )?;
            let task = ledger.tasks.get(&receipt.task_id).cloned().ok_or_else(|| {
                anyhow::anyhow!(
                    "governed task receipt references missing task `{}`",
                    receipt.task_id
                )
            })?;
            require_receipt_revision(receipt, &task)?;
            return Ok(task);
        }

        if ledger.tasks.contains_key(&registration.task_id) {
            return Err(GovernedTaskStateError::AlreadyExists(registration.task_id).into());
        }

        let now = impulse_ops::now_rfc3339();
        let task_id = registration.task_id;
        let system_actor = GovernedActor {
            kind: GovernedActorKind::System,
            id: "impulse-daemon".to_string(),
        };
        let event = new_event(
            0,
            GovernedTaskEventKind::Registered,
            system_actor,
            "governed task registered before runtime launch".to_string(),
            &now,
        );
        let task = GovernedTaskRun {
            id: task_id.clone(),
            revision: 0,
            project_id: registration.project_id,
            workspace_root: registration.workspace_root,
            task: registration.task,
            acceptance_criteria: registration.acceptance_criteria,
            approval_policy: registration.approval_policy,
            verification_profile: registration.verification_profile,
            role_assignment: registration.role_assignment,
            role_compatibility: registration.role_compatibility,
            runtime_id: registration.runtime_id,
            agent_id: registration.agent_id,
            session_id: registration.session_id,
            initial_subject_revision: registration.initial_subject_revision,
            execution_state: GovernedExecutionState::Registered,
            review_state: GovernedReviewState::AwaitingClaim,
            claims: Vec::new(),
            verifications: Vec::new(),
            supervisor_verdicts: Vec::new(),
            operator_decisions: Vec::new(),
            events: vec![event],
            created_at: now.clone(),
            updated_at: now,
        };

        let mut candidate = ledger.clone();
        candidate.tasks.insert(task_id.clone(), task.clone());
        candidate.processed_requests.insert(
            registration.request_id,
            GovernedMutationReceipt {
                task_id,
                resulting_revision: 0,
                request_fingerprint,
            },
        );
        self.storage()
            .write_private_json(GOVERNED_TASKS_FILE, &candidate)
            .context("Failed to persist governed task registration")?;
        *ledger = candidate;
        Ok(task)
    }

    /// Apply a mutation whose operator provenance is only *declared*.
    ///
    /// Every caller that is not the daemon's socket boundary uses this: direct
    /// CLI mode, in-process callers, and tests. The socket boundary uses
    /// [`State::mutate_governed_task_authenticated`] so an approval that
    /// arrived on an operator-class connection is recorded as such (ADR-0018).
    pub fn mutate_governed_task(
        &self,
        request: GovernedTaskMutationRequest,
    ) -> Result<GovernedTaskRun> {
        self.mutate_governed_task_authenticated(request, OperatorAuthentication::Declared)
    }

    /// Apply a mutation, stamping `operator_authentication` onto any operator
    /// decision it records.
    ///
    /// `operator_authentication` is derived by the daemon from the connection
    /// the request arrived on. It is never read from the request payload:
    /// [`OperatorDecisionInput`] has no such field precisely so a client cannot
    /// assert its own provenance.
    pub fn mutate_governed_task_authenticated(
        &self,
        request: GovernedTaskMutationRequest,
        operator_authentication: OperatorAuthentication,
    ) -> Result<GovernedTaskRun> {
        let request_fingerprint = fingerprint_mutation_operation(&request)
            .context("Failed to fingerprint governed task mutation")?;
        let expected_project = self.governed_project_id();
        require_project(&expected_project, &request.project_id)?;
        let mut ledger = self
            .governed_tasks
            .lock()
            .map_err(|error| anyhow::anyhow!("governed task ledger lock poisoned: {error}"))?;

        if let Some(receipt) = ledger.processed_requests.get(&request.request_id) {
            if receipt.task_id != request.task_id {
                return Err(GovernedTaskStateError::IdempotencyConflict {
                    request_id: request.request_id,
                    task_id: receipt.task_id.clone(),
                }
                .into());
            }
            require_idempotent_fingerprint(
                &request.request_id,
                receipt,
                request_fingerprint.as_str(),
            )?;
            let task = ledger
                .tasks
                .get(&request.task_id)
                .cloned()
                .ok_or(GovernedTaskStateError::NotFound(request.task_id))?;
            require_receipt_revision(receipt, &task)?;
            drop(ledger);
            self.ensure_accepted_run_memory_candidate(&task)?;
            return Ok(task);
        }

        let current = ledger
            .tasks
            .get(&request.task_id)
            .cloned()
            .ok_or_else(|| GovernedTaskStateError::NotFound(request.task_id.clone()))?;
        require_project(&expected_project, &current.project_id)?;
        if current.revision != request.expected_revision {
            return Err(GovernedTaskStateError::RevisionConflict {
                expected: request.expected_revision,
                current: current.revision,
            }
            .into());
        }

        let next_revision = current.revision.checked_add(1).ok_or_else(|| {
            GovernedTaskStateError::InvalidTransition(
                "governed task revision exhausted u64".to_string(),
            )
        })?;
        let mut updated = current;
        apply_mutation(
            &mut updated,
            request.mutation,
            next_revision,
            operator_authentication,
        )?;
        updated.revision = next_revision;
        updated.updated_at = impulse_ops::now_rfc3339();
        if let Some(event) = updated.events.last_mut() {
            event.revision = updated.revision;
        }

        let mut candidate = ledger.clone();
        candidate.tasks.insert(updated.id.clone(), updated.clone());
        candidate.processed_requests.insert(
            request.request_id,
            GovernedMutationReceipt {
                task_id: updated.id.clone(),
                resulting_revision: updated.revision,
                request_fingerprint,
            },
        );
        self.storage()
            .write_private_json(GOVERNED_TASKS_FILE, &candidate)
            .context("Failed to persist governed task mutation")?;
        *ledger = candidate;
        drop(ledger);
        self.ensure_accepted_run_memory_candidate(&updated)?;
        Ok(updated)
    }

    /// Check whether a producer request is a replay before any external work.
    /// A request id already owned by another task is rejected immediately.
    pub(crate) fn governed_producer_request_is_replay(
        &self,
        request_id: &GovernedRequestId,
        task_id: &GovernedTaskId,
    ) -> Result<bool> {
        let ledger = self
            .governed_tasks
            .lock()
            .map_err(|error| anyhow::anyhow!("governed task ledger lock poisoned: {error}"))?;
        let Some(receipt) = ledger.processed_requests.get(request_id) else {
            return Ok(false);
        };
        if &receipt.task_id != task_id {
            return Err(GovernedTaskStateError::IdempotencyConflict {
                request_id: request_id.clone(),
                task_id: receipt.task_id.clone(),
            }
            .into());
        }
        Ok(true)
    }

    pub fn get_governed_task(
        &self,
        project_id: &str,
        task_id: &GovernedTaskId,
    ) -> Result<Option<GovernedTaskRun>> {
        require_project(&self.governed_project_id(), project_id)?;
        let ledger = self
            .governed_tasks
            .lock()
            .map_err(|error| anyhow::anyhow!("governed task ledger lock poisoned: {error}"))?;
        Ok(ledger.tasks.get(task_id).cloned())
    }

    pub fn list_governed_tasks(&self, project_id: &str) -> Result<Vec<GovernedTaskRun>> {
        require_project(&self.governed_project_id(), project_id)?;
        let ledger = self
            .governed_tasks
            .lock()
            .map_err(|error| anyhow::anyhow!("governed task ledger lock poisoned: {error}"))?;
        let mut tasks = ledger.tasks.values().cloned().collect::<Vec<_>>();
        tasks.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(tasks)
    }

    pub(super) fn all_governed_tasks(&self) -> Result<Vec<GovernedTaskRun>> {
        let ledger = self
            .governed_tasks
            .lock()
            .map_err(|error| anyhow::anyhow!("governed task ledger lock poisoned: {error}"))?;
        Ok(ledger.tasks.values().cloned().collect())
    }

    pub(crate) fn governed_project_id(&self) -> String {
        governed_project_id_for_storage(self.storage())
    }

    fn canonical_governed_workspace(&self, received: &str) -> Result<String> {
        let expected = canonical_governed_project_root(self.storage())?;
        let received_path = Path::new(received)
            .canonicalize()
            .with_context(|| format!("Failed to canonicalize governed workspace {received}"))?;
        if expected != received_path {
            return Err(GovernedTaskStateError::WorkspaceMismatch {
                expected: expected.display().to_string(),
                received: received_path.display().to_string(),
            }
            .into());
        }
        Ok(received_path.display().to_string())
    }
}

fn governed_project_root(storage: &Storage) -> &Path {
    match storage.base_path().parent() {
        Some(parent) if parent.as_os_str().is_empty() => Path::new("."),
        Some(parent) => parent,
        None if storage.base_path().is_absolute() => storage.base_path(),
        None => Path::new("."),
    }
}

fn canonical_governed_project_root(storage: &Storage) -> Result<PathBuf> {
    let root = governed_project_root(storage);
    root.canonicalize().with_context(|| {
        format!(
            "Failed to canonicalize governed task ledger project root {}",
            root.display()
        )
    })
}

fn governed_project_id_for_storage(storage: &Storage) -> String {
    let canonical_root = canonical_governed_project_root(storage).ok();
    let root = canonical_root
        .as_deref()
        .unwrap_or_else(|| governed_project_root(storage));
    let name = root
        .file_name()
        .map(|segment| segment.to_string_lossy().to_string())
        .unwrap_or_else(|| "impulse-project".to_string());
    impulse_ops::sanitize_id(&name)
}

fn validate_task_history(task: &GovernedTaskRun) -> Result<BTreeMap<u64, String>> {
    require_rfc3339("governed task created_at", &task.created_at)?;
    require_rfc3339("governed task updated_at", &task.updated_at)?;
    if task.events.is_empty() || task.events.len() > MAX_GOVERNED_EVENTS {
        anyhow::bail!(
            "governed task event history must contain between 1 and {MAX_GOVERNED_EVENTS} records"
        );
    }
    for (label, len) in [
        ("worker claims", task.claims.len()),
        ("verification records", task.verifications.len()),
        ("supervisor verdicts", task.supervisor_verdicts.len()),
        ("operator decisions", task.operator_decisions.len()),
    ] {
        if len > MAX_GOVERNED_RECORDS_PER_KIND {
            anyhow::bail!("{label} exceeds its limit of {MAX_GOVERNED_RECORDS_PER_KIND}");
        }
    }
    let expected_events = task
        .revision
        .checked_add(1)
        .and_then(|count| usize::try_from(count).ok())
        .ok_or_else(|| anyhow::anyhow!("governed task revision cannot fit in memory"))?;
    if task.events.len() != expected_events {
        anyhow::bail!(
            "governed task revision {} requires {} contiguous events, found {}",
            task.revision,
            expected_events,
            task.events.len()
        );
    }

    let first = &task.events[0];
    if first.revision != 0 || first.kind != GovernedTaskEventKind::Registered {
        anyhow::bail!("governed task event zero must be the registration event");
    }
    require_actor(&first.actor, GovernedActorKind::System)?;
    require_text("governed task event detail", &first.detail)?;
    require_rfc3339("governed task event timestamp", &first.created_at)?;
    if task.created_at != first.created_at {
        anyhow::bail!("governed task creation timestamp must match its registration event");
    }

    let mut seen_ids = BTreeSet::new();
    if !seen_ids.insert(first.id.clone()) {
        anyhow::bail!("duplicate governed record id `{}`", first.id);
    }
    let mut claim_index = 0usize;
    let mut verification_index = 0usize;
    let mut supervisor_index = 0usize;
    let mut operator_index = 0usize;
    let mut mutation_fingerprints = BTreeMap::new();
    let mut projection = task.clone();
    projection.revision = 0;
    projection.execution_state = GovernedExecutionState::Registered;
    projection.review_state = GovernedReviewState::AwaitingClaim;
    projection.claims.clear();
    projection.verifications.clear();
    projection.supervisor_verdicts.clear();
    projection.operator_decisions.clear();
    projection.events.clear();

    for (index, event) in task.events.iter().enumerate().skip(1) {
        let expected_revision = u64::try_from(index)
            .map_err(|_| anyhow::anyhow!("governed task event index cannot fit in u64"))?;
        if event.revision != expected_revision {
            anyhow::bail!(
                "governed task event revision {} is not contiguous at index {index}",
                event.revision
            );
        }
        require_text("governed task event detail", &event.detail)?;
        require_rfc3339("governed task event timestamp", &event.created_at)?;
        if !seen_ids.insert(event.id.clone()) {
            anyhow::bail!("duplicate governed record id `{}`", event.id);
        }
        let based_on_revision = event
            .revision
            .checked_sub(1)
            .ok_or_else(|| anyhow::anyhow!("non-registration event has revision zero"))?;
        projection.revision = based_on_revision;

        // Replaying an operator decision must reproduce the provenance the
        // daemon originally stamped, not re-derive it: the replayed projection
        // is compared against stored truth.
        let mut replay_operator_authentication = OperatorAuthentication::Declared;
        let (mutation, fingerprint_input_mutation) = match event.kind {
            GovernedTaskEventKind::Registered => {
                anyhow::bail!("registration event may appear only at revision zero")
            }
            GovernedTaskEventKind::Running => {
                let mutation = GovernedTaskMutation::MarkRunning {
                    actor: event.actor.clone(),
                };
                (mutation.clone(), mutation)
            }
            GovernedTaskEventKind::LaunchFailed => {
                let mutation = GovernedTaskMutation::MarkLaunchFailed {
                    actor: event.actor.clone(),
                    reason: event.detail.clone(),
                };
                (mutation.clone(), mutation)
            }
            GovernedTaskEventKind::RuntimeExited => {
                let mutation = GovernedTaskMutation::MarkRuntimeExited {
                    actor: event.actor.clone(),
                    reason: Some(event.detail.clone()),
                };
                (mutation.clone(), mutation)
            }
            GovernedTaskEventKind::ClaimSubmitted => {
                let claim = task.claims.get(claim_index).ok_or_else(|| {
                    anyhow::anyhow!("claim event has no corresponding worker claim")
                })?;
                validate_record_link(
                    &claim.id,
                    &claim.actor,
                    claim.based_on_revision,
                    &claim.submitted_at,
                    event,
                    &mut seen_ids,
                )?;
                claim_index += 1;
                let mutation = GovernedTaskMutation::SubmitClaim {
                    claim: WorkerCompletionClaimInput {
                        actor: claim.actor.clone(),
                        summary: claim.summary.clone(),
                        subject_revision: claim.subject_revision.clone(),
                        artifact_ids: claim.artifact_ids.clone(),
                        diff_ref: claim.diff_ref.clone(),
                    },
                };
                (mutation.clone(), mutation)
            }
            GovernedTaskEventKind::VerificationRecorded => {
                let verification = task.verifications.get(verification_index).ok_or_else(|| {
                    anyhow::anyhow!("verification event has no corresponding verification record")
                })?;
                validate_record_link(
                    &verification.id,
                    &verification.actor,
                    verification.based_on_revision,
                    &verification.recorded_at,
                    event,
                    &mut seen_ids,
                )?;
                let stored_claim =
                    task.claims
                        .get(claim_index.saturating_sub(1))
                        .ok_or_else(|| {
                            anyhow::anyhow!("verification record has no preceding worker claim")
                        })?;
                if verification.claim_id != stored_claim.id {
                    anyhow::bail!(
                        "verification record does not reference the current worker claim"
                    );
                }
                let replay_claim_id = projection
                    .latest_claim()
                    .ok_or_else(|| anyhow::anyhow!("replayed verification has no worker claim"))?
                    .id
                    .clone();
                verification_index += 1;
                let fingerprint_input = GovernedVerificationInput {
                    actor: verification.actor.clone(),
                    claim_id: stored_claim.id.clone(),
                    subject_revision: verification.subject_revision.clone(),
                    policy: verification.policy.clone(),
                    outcome: verification.outcome,
                    commands: verification.commands.clone(),
                    artifact_ids: verification.artifact_ids.clone(),
                    notes: verification.notes.clone(),
                };
                let mut replay_input = fingerprint_input.clone();
                replay_input.claim_id = replay_claim_id;
                (
                    GovernedTaskMutation::RecordVerification {
                        verification: replay_input,
                    },
                    GovernedTaskMutation::RecordVerification {
                        verification: fingerprint_input,
                    },
                )
            }
            GovernedTaskEventKind::SupervisorVerdictRecorded => {
                let verdict = task
                    .supervisor_verdicts
                    .get(supervisor_index)
                    .ok_or_else(|| {
                        anyhow::anyhow!("supervisor event has no corresponding supervisor verdict")
                    })?;
                validate_record_link(
                    &verdict.id,
                    &verdict.actor,
                    verdict.based_on_revision,
                    &verdict.decided_at,
                    event,
                    &mut seen_ids,
                )?;
                let stored_verification = task
                    .verifications
                    .get(verification_index.saturating_sub(1))
                    .ok_or_else(|| {
                        anyhow::anyhow!("supervisor verdict has no preceding verification")
                    })?;
                if verdict.verification_id != stored_verification.id {
                    anyhow::bail!("supervisor verdict does not reference current verification");
                }
                let replay_verification_id = projection
                    .latest_verification()
                    .ok_or_else(|| {
                        anyhow::anyhow!("replayed supervisor verdict has no verification")
                    })?
                    .id
                    .clone();
                supervisor_index += 1;
                let fingerprint_input = SupervisorVerdictInput {
                    actor: verdict.actor.clone(),
                    verification_id: stored_verification.id.clone(),
                    verdict: verdict.verdict,
                    rationale: verdict.rationale.clone(),
                };
                let mut replay_input = fingerprint_input.clone();
                replay_input.verification_id = replay_verification_id;
                (
                    GovernedTaskMutation::RecordSupervisorVerdict {
                        verdict: replay_input,
                    },
                    GovernedTaskMutation::RecordSupervisorVerdict {
                        verdict: fingerprint_input,
                    },
                )
            }
            GovernedTaskEventKind::OperatorDecisionRecorded => {
                let decision = task.operator_decisions.get(operator_index).ok_or_else(|| {
                    anyhow::anyhow!("operator event has no corresponding operator decision")
                })?;
                validate_record_link(
                    &decision.id,
                    &decision.actor,
                    decision.based_on_revision,
                    &decision.decided_at,
                    event,
                    &mut seen_ids,
                )?;
                let stored_verdict = task
                    .supervisor_verdicts
                    .get(supervisor_index.saturating_sub(1))
                    .ok_or_else(|| {
                        anyhow::anyhow!("operator decision has no preceding supervisor verdict")
                    })?;
                if decision.supervisor_verdict_id != stored_verdict.id {
                    anyhow::bail!(
                        "operator decision does not reference current supervisor verdict"
                    );
                }
                let replay_verdict_id = projection
                    .latest_supervisor_verdict()
                    .ok_or_else(|| {
                        anyhow::anyhow!("replayed operator decision has no supervisor verdict")
                    })?
                    .id
                    .clone();
                operator_index += 1;
                replay_operator_authentication = decision.authentication;
                let fingerprint_input = OperatorDecisionInput {
                    actor: decision.actor.clone(),
                    supervisor_verdict_id: stored_verdict.id.clone(),
                    decision: decision.decision,
                    rationale: decision.rationale.clone(),
                };
                let mut replay_input = fingerprint_input.clone();
                replay_input.supervisor_verdict_id = replay_verdict_id;
                (
                    GovernedTaskMutation::RecordOperatorDecision {
                        decision: replay_input,
                    },
                    GovernedTaskMutation::RecordOperatorDecision {
                        decision: fingerprint_input,
                    },
                )
            }
            GovernedTaskEventKind::ProducerReservationInterrupted => {
                let mutation = GovernedTaskMutation::NoteProducerReservationInterrupted {
                    actor: event.actor.clone(),
                    reason: event.detail.clone(),
                };
                (mutation.clone(), mutation)
            }
        };
        let fingerprint =
            fingerprint_mutation(&task.project_id, &task.id, &fingerprint_input_mutation)
                .with_context(|| {
                    format!(
                        "Failed to recompute governed task mutation fingerprint at revision {}",
                        event.revision
                    )
                })?;
        if mutation_fingerprints
            .insert(event.revision, fingerprint)
            .is_some()
        {
            anyhow::bail!(
                "governed task contains duplicate mutation revision {}",
                event.revision
            );
        }
        apply_mutation(
            &mut projection,
            mutation,
            event.revision,
            replay_operator_authentication,
        )
        .with_context(|| {
            format!(
                "governed task event {:?} at revision {} cannot be replayed",
                event.kind, event.revision
            )
        })?;
        projection.revision = event.revision;
    }

    if claim_index != task.claims.len()
        || verification_index != task.verifications.len()
        || supervisor_index != task.supervisor_verdicts.len()
        || operator_index != task.operator_decisions.len()
    {
        anyhow::bail!("governed task contains records without matching lifecycle events");
    }
    if projection.execution_state != task.execution_state
        || projection.review_state != task.review_state
    {
        anyhow::bail!(
            "governed task materialized state does not match its replayed lifecycle history"
        );
    }
    Ok(mutation_fingerprints)
}

fn validate_record_link(
    id: &GovernedRecordId,
    actor: &GovernedActor,
    based_on_revision: u64,
    timestamp: &str,
    event: &GovernedTaskEvent,
    seen_ids: &mut BTreeSet<GovernedRecordId>,
) -> Result<()> {
    if based_on_revision.checked_add(1) != Some(event.revision) {
        anyhow::bail!(
            "governed record `{id}` is not based on the revision immediately before its event"
        );
    }
    if actor != &event.actor {
        anyhow::bail!("governed record `{id}` actor does not match its lifecycle event");
    }
    require_rfc3339("governed record timestamp", timestamp)?;
    if !seen_ids.insert(id.clone()) {
        anyhow::bail!("duplicate governed record id `{id}`");
    }
    Ok(())
}

fn require_rfc3339(label: &str, value: &str) -> Result<()> {
    chrono::DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("{label} must be a valid RFC 3339 timestamp"))?;
    Ok(())
}

fn require_request_fingerprint(value: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256-v1:") else {
        anyhow::bail!("request fingerprint must use sha256-v1:<hex> format");
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        anyhow::bail!("request fingerprint must contain 64 lowercase hexadecimal characters");
    }
    Ok(())
}

fn require_project(expected: &str, received: &str) -> Result<()> {
    if expected != received {
        return Err(GovernedTaskStateError::ProjectMismatch {
            expected: expected.to_string(),
            received: received.to_string(),
        }
        .into());
    }
    Ok(())
}

fn fingerprint_request<T: Serialize>(request: &T) -> Result<String> {
    let payload = serde_json::to_vec(request)?;
    Ok(format!("sha256-v1:{:x}", Sha256::digest(payload)))
}

fn fingerprint_mutation_operation(request: &GovernedTaskMutationRequest) -> Result<String> {
    fingerprint_mutation(&request.project_id, &request.task_id, &request.mutation)
}

fn fingerprint_mutation(
    project_id: &str,
    task_id: &GovernedTaskId,
    mutation: &GovernedTaskMutation,
) -> Result<String> {
    #[derive(Serialize)]
    struct SemanticMutation<'a> {
        project_id: &'a str,
        task_id: &'a GovernedTaskId,
        mutation: &'a GovernedTaskMutation,
    }

    // `None` and the materialized default detail have identical transition
    // semantics. Canonicalize them so reload can derive the exact idempotency
    // fingerprint from append-only history without storing raw requests.
    let canonical_mutation = match mutation {
        GovernedTaskMutation::MarkRuntimeExited {
            actor,
            reason: None,
        } => GovernedTaskMutation::MarkRuntimeExited {
            actor: actor.clone(),
            reason: Some("runtime exited".to_string()),
        },
        mutation => mutation.clone(),
    };
    fingerprint_request(&SemanticMutation {
        project_id,
        task_id,
        mutation: &canonical_mutation,
    })
}

fn require_idempotent_fingerprint(
    request_id: &GovernedRequestId,
    receipt: &GovernedMutationReceipt,
    request_fingerprint: &str,
) -> Result<()> {
    if receipt.request_fingerprint != request_fingerprint {
        return Err(GovernedTaskStateError::IdempotencyPayloadConflict {
            request_id: request_id.clone(),
        }
        .into());
    }
    Ok(())
}

fn require_receipt_revision(
    receipt: &GovernedMutationReceipt,
    task: &GovernedTaskRun,
) -> Result<()> {
    if task.revision < receipt.resulting_revision {
        anyhow::bail!(
            "governed task receipt revision {} exceeds current task revision {}",
            receipt.resulting_revision,
            task.revision
        );
    }
    Ok(())
}

fn apply_mutation(
    task: &mut GovernedTaskRun,
    mutation: GovernedTaskMutation,
    event_revision: u64,
    operator_authentication: OperatorAuthentication,
) -> Result<()> {
    let now = impulse_ops::now_rfc3339();
    require_capacity(
        "governed task events",
        task.events.len(),
        MAX_GOVERNED_EVENTS,
    )?;
    match mutation {
        GovernedTaskMutation::MarkRunning { actor } => {
            require_actor(&actor, GovernedActorKind::System)?;
            if task.execution_state != GovernedExecutionState::Registered {
                return invalid_transition("only a registered task can start running");
            }
            task.execution_state = GovernedExecutionState::Running;
            task.events.push(new_event(
                event_revision,
                GovernedTaskEventKind::Running,
                actor,
                "runtime launch acknowledged".to_string(),
                &now,
            ));
        }
        GovernedTaskMutation::MarkLaunchFailed { actor, reason } => {
            require_actor(&actor, GovernedActorKind::System)?;
            require_text("launch failure reason", &reason)?;
            if task.execution_state != GovernedExecutionState::Registered {
                return invalid_transition("launch failure requires a registered task");
            }
            task.execution_state = GovernedExecutionState::LaunchFailed;
            task.events.push(new_event(
                event_revision,
                GovernedTaskEventKind::LaunchFailed,
                actor,
                reason,
                &now,
            ));
        }
        GovernedTaskMutation::MarkRuntimeExited { actor, reason } => {
            require_actor(&actor, GovernedActorKind::System)?;
            if task.execution_state != GovernedExecutionState::Running {
                return invalid_transition("runtime exit requires a running task");
            }
            if let Some(reason) = reason.as_deref() {
                require_text("runtime exit reason", reason)?;
            }
            task.execution_state = GovernedExecutionState::RuntimeExited;
            task.events.push(new_event(
                event_revision,
                GovernedTaskEventKind::RuntimeExited,
                actor,
                reason.unwrap_or_else(|| "runtime exited".to_string()),
                &now,
            ));
        }
        GovernedTaskMutation::SubmitClaim { claim } => {
            require_capacity(
                "worker claims",
                task.claims.len(),
                MAX_GOVERNED_RECORDS_PER_KIND,
            )?;
            require_actor(&claim.actor, GovernedActorKind::Worker)?;
            if claim.actor.id != task.agent_id {
                return invalid_transition("worker claim actor does not match assigned agent");
            }
            if !matches!(
                task.execution_state,
                GovernedExecutionState::Running | GovernedExecutionState::RuntimeExited
            ) {
                return invalid_transition("claim requires a launched or exited runtime");
            }
            if !matches!(
                task.review_state,
                GovernedReviewState::AwaitingClaim
                    | GovernedReviewState::ChangesRequested
                    | GovernedReviewState::VerificationFailed
            ) {
                return invalid_transition("review state does not accept a worker claim");
            }
            require_text("claim summary", &claim.summary)?;
            require_bounded_text(
                "claim subject revision",
                &claim.subject_revision,
                MAX_GOVERNED_REFERENCE_BYTES,
            )?;
            validate_reference_ids("claim artifact ids", &claim.artifact_ids)?;
            if let Some(diff_ref) = claim.diff_ref.as_deref() {
                require_project_relative_ref("claim diff ref", diff_ref)?;
            }
            let id = new_record_id("claim");
            task.claims.push(WorkerCompletionClaim {
                id: id.clone(),
                actor: claim.actor.clone(),
                summary: claim.summary,
                subject_revision: claim.subject_revision,
                artifact_ids: claim.artifact_ids,
                diff_ref: claim.diff_ref,
                submitted_at: now.clone(),
                based_on_revision: task.revision,
            });
            task.review_state = GovernedReviewState::AwaitingVerification;
            task.events.push(new_event(
                event_revision,
                GovernedTaskEventKind::ClaimSubmitted,
                claim.actor,
                format!("worker completion claim {id} submitted"),
                &now,
            ));
        }
        GovernedTaskMutation::RecordVerification { verification } => {
            require_capacity(
                "verification records",
                task.verifications.len(),
                MAX_GOVERNED_RECORDS_PER_KIND,
            )?;
            require_actor(&verification.actor, GovernedActorKind::Verifier)?;
            if task.review_state != GovernedReviewState::AwaitingVerification {
                return invalid_transition("verification requires an awaiting-verification task");
            }
            let claim = task.latest_claim().ok_or_else(|| {
                GovernedTaskStateError::InvalidTransition(
                    "verification requires a worker claim".to_string(),
                )
            })?;
            if verification.claim_id != claim.id {
                return invalid_transition("verification must reference the latest claim");
            }
            if verification.subject_revision != claim.subject_revision {
                return invalid_transition(
                    "verification subject revision must match the latest claim",
                );
            }
            validate_verification(&verification)?;
            let id = new_record_id("verification");
            let outcome = verification.outcome;
            task.verifications.push(GovernedVerification {
                id: id.clone(),
                actor: verification.actor.clone(),
                claim_id: verification.claim_id,
                subject_revision: verification.subject_revision,
                policy: verification.policy,
                outcome,
                commands: verification.commands,
                artifact_ids: verification.artifact_ids,
                notes: verification.notes,
                recorded_at: now.clone(),
                based_on_revision: task.revision,
            });
            task.review_state = if outcome == GovernedVerificationOutcome::Passed {
                GovernedReviewState::AwaitingSupervisor
            } else {
                GovernedReviewState::VerificationFailed
            };
            task.events.push(new_event(
                event_revision,
                GovernedTaskEventKind::VerificationRecorded,
                verification.actor,
                format!("verification {id} recorded as {outcome:?}"),
                &now,
            ));
        }
        GovernedTaskMutation::RecordSupervisorVerdict { verdict } => {
            require_capacity(
                "supervisor verdicts",
                task.supervisor_verdicts.len(),
                MAX_GOVERNED_RECORDS_PER_KIND,
            )?;
            require_actor(&verdict.actor, GovernedActorKind::Supervisor)?;
            if !matches!(
                task.review_state,
                GovernedReviewState::AwaitingSupervisor | GovernedReviewState::VerificationFailed
            ) {
                return invalid_transition("supervisor verdict requires current verification");
            }
            require_text("supervisor rationale", &verdict.rationale)?;
            let verification = task.latest_verification().ok_or_else(|| {
                GovernedTaskStateError::InvalidTransition(
                    "supervisor verdict requires verification".to_string(),
                )
            })?;
            if verdict.verification_id != verification.id {
                return invalid_transition("supervisor verdict must reference latest verification");
            }
            if verdict.verdict == SupervisorVerdictKind::RecommendAccept
                && verification.outcome != GovernedVerificationOutcome::Passed
            {
                return invalid_transition("accept recommendation requires passing verification");
            }
            let id = new_record_id("supervisor-verdict");
            let verdict_kind = verdict.verdict;
            task.supervisor_verdicts.push(SupervisorVerdict {
                id: id.clone(),
                actor: verdict.actor.clone(),
                verification_id: verdict.verification_id,
                verdict: verdict_kind,
                rationale: verdict.rationale,
                decided_at: now.clone(),
                based_on_revision: task.revision,
            });
            task.review_state = match verdict_kind {
                SupervisorVerdictKind::RecommendAccept => GovernedReviewState::AwaitingOperator,
                SupervisorVerdictKind::ChangesRequested => GovernedReviewState::ChangesRequested,
                SupervisorVerdictKind::Escalate => GovernedReviewState::Escalated,
            };
            task.events.push(new_event(
                event_revision,
                GovernedTaskEventKind::SupervisorVerdictRecorded,
                verdict.actor,
                format!("supervisor verdict {id} recorded as {verdict_kind:?}"),
                &now,
            ));
        }
        GovernedTaskMutation::RecordOperatorDecision { decision } => {
            require_capacity(
                "operator decisions",
                task.operator_decisions.len(),
                MAX_GOVERNED_RECORDS_PER_KIND,
            )?;
            require_actor(&decision.actor, GovernedActorKind::Operator)?;
            if task.review_state != GovernedReviewState::AwaitingOperator {
                return invalid_transition("operator decision requires awaiting-operator state");
            }
            require_text("operator rationale", &decision.rationale)?;
            let supervisor = task.latest_supervisor_verdict().ok_or_else(|| {
                GovernedTaskStateError::InvalidTransition(
                    "operator decision requires supervisor verdict".to_string(),
                )
            })?;
            if decision.supervisor_verdict_id != supervisor.id
                || supervisor.verdict != SupervisorVerdictKind::RecommendAccept
            {
                return invalid_transition(
                    "operator decision must reference latest accept recommendation",
                );
            }
            let id = new_record_id("operator-decision");
            let decision_kind = decision.decision;
            task.operator_decisions.push(OperatorDecision {
                id: id.clone(),
                actor: decision.actor.clone(),
                supervisor_verdict_id: decision.supervisor_verdict_id,
                decision: decision_kind,
                rationale: decision.rationale,
                decided_at: now.clone(),
                based_on_revision: task.revision,
                authentication: operator_authentication,
            });
            task.review_state = match decision_kind {
                OperatorDecisionKind::Approve => GovernedReviewState::Accepted,
                OperatorDecisionKind::Reject => GovernedReviewState::Rejected,
            };
            task.events.push(new_event(
                event_revision,
                GovernedTaskEventKind::OperatorDecisionRecorded,
                decision.actor,
                format!("operator decision {id} recorded as {decision_kind:?}"),
                &now,
            ));
        }
        GovernedTaskMutation::NoteProducerReservationInterrupted { actor, reason } => {
            // A pure observability note: it does not transition
            // execution_state or review_state, and is valid at any point in
            // the lifecycle, since the crash it records can interrupt any
            // daemon-owned producer regardless of current review state.
            require_actor(&actor, GovernedActorKind::System)?;
            require_text("producer reservation interruption reason", &reason)?;
            task.events.push(new_event(
                event_revision,
                GovernedTaskEventKind::ProducerReservationInterrupted,
                actor,
                reason,
                &now,
            ));
        }
    }
    Ok(())
}

fn validate_verification(
    verification: &impulse_ops::governed_task::GovernedVerificationInput,
) -> Result<()> {
    require_bounded_text(
        "verification subject revision",
        &verification.subject_revision,
        MAX_GOVERNED_REFERENCE_BYTES,
    )?;
    require_bounded_text(
        "verification policy",
        &verification.policy,
        MAX_GOVERNED_REFERENCE_BYTES,
    )?;
    if verification.commands.len() > MAX_GOVERNED_COMMANDS {
        return invalid_transition(&format!(
            "verification commands must contain at most {MAX_GOVERNED_COMMANDS} entries"
        ));
    }
    validate_reference_ids("verification artifact ids", &verification.artifact_ids)?;
    if let Some(notes) = verification.notes.as_deref() {
        require_text("verification notes", notes)?;
    }
    if matches!(
        verification.outcome,
        GovernedVerificationOutcome::Passed | GovernedVerificationOutcome::Failed
    ) && verification.commands.is_empty()
    {
        return invalid_transition("passed or failed verification requires command evidence");
    }
    if verification.outcome == GovernedVerificationOutcome::Passed
        && verification.commands.iter().any(|command| !command.success)
    {
        return invalid_transition("passing verification cannot contain failed commands");
    }
    if verification.outcome == GovernedVerificationOutcome::Failed
        && verification.commands.iter().all(|command| command.success)
    {
        return invalid_transition("failed verification requires at least one failed command");
    }
    for command in &verification.commands {
        require_bounded_text(
            "verification command name",
            &command.name,
            MAX_GOVERNED_REFERENCE_BYTES,
        )?;
        require_bounded_text(
            "verification command executable",
            &command.executable,
            MAX_GOVERNED_REFERENCE_BYTES,
        )?;
        if command.redacted_args.len() > MAX_GOVERNED_COMMAND_ARGS {
            return invalid_transition(&format!(
                "verification command evidence must contain at most {MAX_GOVERNED_COMMAND_ARGS} redacted arguments"
            ));
        }
        for argument in &command.redacted_args {
            if argument.contains('\0') || argument.len() > MAX_GOVERNED_COMMAND_ARG_BYTES {
                return invalid_transition(&format!(
                    "verification command arguments must be NUL-free and at most {MAX_GOVERNED_COMMAND_ARG_BYTES} bytes"
                ));
            }
        }
        require_sha256_digest("verification command digest", &command.command_digest)?;
        match (command.success, command.exit_code) {
            (true, Some(0)) | (false, None) => {}
            (false, Some(code)) if code != 0 => {}
            _ => {
                return invalid_transition(
                    "verification command success must agree with its process exit code",
                );
            }
        }
        require_sha256_digest("verification output digest", &command.output_digest)?;
        if let Some(output_ref) = command.output_ref.as_deref() {
            require_project_relative_ref("verification output ref", output_ref)?;
        }
    }
    Ok(())
}

fn require_capacity(label: &str, current: usize, maximum: usize) -> Result<()> {
    if current >= maximum {
        return invalid_transition(&format!("{label} reached its limit of {maximum}"));
    }
    Ok(())
}

fn validate_reference_ids(label: &str, references: &[String]) -> Result<()> {
    if references.len() > MAX_GOVERNED_REFERENCES {
        return invalid_transition(&format!(
            "{label} must contain at most {MAX_GOVERNED_REFERENCES} entries"
        ));
    }
    for reference in references {
        require_bounded_text(label, reference, MAX_GOVERNED_REFERENCE_BYTES)?;
    }
    Ok(())
}

fn require_project_relative_ref(label: &str, value: &str) -> Result<()> {
    require_bounded_text(label, value, MAX_GOVERNED_REFERENCE_BYTES)?;
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return invalid_transition(&format!(
            "{label} must be a project-local relative reference"
        ));
    }
    Ok(())
}

fn require_sha256_digest(label: &str, value: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return invalid_transition(&format!("{label} must use the sha256:<hex> format"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return invalid_transition(&format!(
            "{label} must contain exactly 64 hexadecimal SHA-256 characters"
        ));
    }
    Ok(())
}

fn require_actor(actor: &GovernedActor, expected: GovernedActorKind) -> Result<()> {
    if actor.kind != expected {
        return invalid_transition(&format!(
            "actor kind {:?} cannot perform {:?} transition",
            actor.kind, expected
        ));
    }
    require_text("actor id", &actor.id)
}

fn require_text(label: &str, value: &str) -> Result<()> {
    require_bounded_text(label, value, MAX_REASON_BYTES)
}

fn require_bounded_text(label: &str, value: &str, max_bytes: usize) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') || value.len() > max_bytes {
        return invalid_transition(&format!(
            "{label} must be nonblank, NUL-free, and at most {max_bytes} bytes"
        ));
    }
    Ok(())
}

fn invalid_transition<T>(message: &str) -> Result<T> {
    Err(GovernedTaskStateError::InvalidTransition(message.to_string()).into())
}

fn new_record_id(prefix: &str) -> GovernedRecordId {
    GovernedRecordId::try_new(format!("{prefix}-{}", Uuid::new_v4()))
        .expect("generated governed record UUID must be valid")
}

fn new_event(
    revision: u64,
    kind: GovernedTaskEventKind,
    actor: GovernedActor,
    detail: String,
    created_at: &str,
) -> GovernedTaskEvent {
    GovernedTaskEvent {
        id: new_record_id("event"),
        revision,
        kind,
        actor,
        detail,
        created_at: created_at.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use impulse_ops::governed_task::{
        ApprovalPolicy, GovernedCommandEvidence, GovernedTaskMutation, GovernedTaskMutationRequest,
        GovernedTaskRegistration, GovernedVerificationInput, OperatorDecisionInput,
        SupervisorVerdictInput, WorkerCompletionClaimInput,
    };
    use tempfile::TempDir;

    use super::*;

    fn actor(kind: GovernedActorKind, id: &str) -> GovernedActor {
        GovernedActor {
            kind,
            id: id.to_string(),
        }
    }

    fn digest(character: char) -> String {
        format!("sha256:{}", character.to_string().repeat(64))
    }

    fn state() -> (TempDir, Arc<State>) {
        let root = TempDir::new().unwrap();
        let base = root.path().join("impulse-test");
        std::fs::create_dir_all(base.join(".impulse")).unwrap();
        let state = Arc::new(State::new(base.join(".impulse")).unwrap());
        (root, state)
    }

    #[test]
    fn empty_ledger_validation_does_not_require_storage_parent() {
        let root = TempDir::new().unwrap();
        let storage = Storage::new(root.path().join("not-created").join(".impulse"));

        GovernedTaskLedger::default().validate(&storage).unwrap();
    }

    #[test]
    fn bare_relative_storage_uses_current_directory_as_project_root() {
        let storage = Storage::new(PathBuf::from(".impulse"));
        let expected = std::env::current_dir().unwrap().canonicalize().unwrap();

        assert_eq!(canonical_governed_project_root(&storage).unwrap(), expected);
        assert_eq!(
            governed_project_id_for_storage(&storage),
            impulse_ops::sanitize_id(
                &expected
                    .file_name()
                    .expect("current directory has a final component")
                    .to_string_lossy()
            )
        );
    }

    #[test]
    fn absolute_parentless_storage_preserves_its_root() {
        let root = std::env::current_dir()
            .unwrap()
            .canonicalize()
            .unwrap()
            .ancestors()
            .last()
            .expect("absolute current directory has a filesystem root")
            .to_path_buf();
        let storage = Storage::new(root.clone());

        assert_eq!(governed_project_root(&storage), root);
    }

    fn registration(state: &State, request_id: &str) -> GovernedTaskRegistration {
        let root = state.storage().base_path().parent().unwrap();
        GovernedTaskRegistration::builder(
            request_id,
            format!("task-{request_id}"),
            "impulse-test",
            root.display().to_string(),
            "Ship governed task truth",
            "worker-1",
            "codex",
        )
        .approval_policy(ApprovalPolicy::OperatorRequired)
        .build()
        .unwrap()
    }

    fn mutation(
        task: &GovernedTaskRun,
        request_id: &str,
        mutation: GovernedTaskMutation,
    ) -> GovernedTaskMutationRequest {
        GovernedTaskMutationRequest {
            request_id: GovernedRequestId::try_new(request_id).unwrap(),
            project_id: task.project_id.clone(),
            task_id: task.id.clone(),
            expected_revision: task.revision,
            mutation,
        }
    }

    fn rewrite_persisted_ledger(state: &State, mutate: impl FnOnce(&mut serde_json::Value)) {
        let path = state.storage().path(GOVERNED_TASKS_FILE);
        let mut ledger: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        mutate(&mut ledger);
        std::fs::write(path, serde_json::to_vec_pretty(&ledger).unwrap()).unwrap();
    }

    fn rewrite_persisted_candidate_ledger(
        state: &State,
        mutate: impl FnOnce(&mut serde_json::Value),
    ) {
        let path = state
            .storage()
            .path(crate::state::memory_candidate::memory_candidates_file());
        let mut ledger: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        mutate(&mut ledger);
        std::fs::write(path, serde_json::to_vec_pretty(&ledger).unwrap()).unwrap();
    }

    fn reload_error(base: std::path::PathBuf, expectation: &str) -> anyhow::Error {
        match State::new(base) {
            Ok(_) => panic!("{expectation}"),
            Err(error) => error,
        }
    }

    fn claim(state: &State, task: &GovernedTaskRun, request_id: &str) -> GovernedTaskRun {
        let request = mutation(
            task,
            request_id,
            GovernedTaskMutation::SubmitClaim {
                claim: WorkerCompletionClaimInput {
                    actor: actor(GovernedActorKind::Worker, "worker-1"),
                    summary: "implementation complete".into(),
                    subject_revision: "abc123".into(),
                    artifact_ids: vec!["artifact-1".into()],
                    diff_ref: Some("diffs/abc123.patch".into()),
                },
            },
        );
        state.mutate_governed_task(request).unwrap()
    }

    fn verify(
        state: &State,
        task: &GovernedTaskRun,
        request_id: &str,
        outcome: GovernedVerificationOutcome,
    ) -> GovernedTaskRun {
        let success = outcome == GovernedVerificationOutcome::Passed;
        state
            .mutate_governed_task(mutation(
                task,
                request_id,
                GovernedTaskMutation::RecordVerification {
                    verification: GovernedVerificationInput {
                        actor: actor(GovernedActorKind::Verifier, "cargo-gate"),
                        claim_id: task.latest_claim().unwrap().id.clone(),
                        subject_revision: task.latest_claim().unwrap().subject_revision.clone(),
                        policy: "workspace-default".into(),
                        outcome,
                        commands: vec![GovernedCommandEvidence {
                            name: "tests".into(),
                            executable: "cargo".into(),
                            redacted_args: vec!["test".into()],
                            command_digest: digest('a'),
                            exit_code: Some(if success { 0 } else { 1 }),
                            success,
                            output_digest: digest('b'),
                            output_ref: Some("evidence/tests.log".into()),
                            output_bytes: 42,
                            output_truncated: false,
                        }],
                        artifact_ids: vec![],
                        notes: None,
                    },
                },
            ))
            .unwrap()
    }

    fn recommend_accept(
        state: &State,
        task: &GovernedTaskRun,
        request_id: &str,
    ) -> GovernedTaskRun {
        state
            .mutate_governed_task(mutation(
                task,
                request_id,
                GovernedTaskMutation::RecordSupervisorVerdict {
                    verdict: SupervisorVerdictInput {
                        actor: actor(GovernedActorKind::Supervisor, "supervisor-1"),
                        verification_id: task.latest_verification().unwrap().id.clone(),
                        verdict: SupervisorVerdictKind::RecommendAccept,
                        rationale: "evidence satisfies acceptance criteria".into(),
                    },
                },
            ))
            .unwrap()
    }

    fn operator_decide(
        state: &State,
        task: &GovernedTaskRun,
        request_id: &str,
        decision: OperatorDecisionKind,
    ) -> GovernedTaskRun {
        state
            .mutate_governed_task(mutation(
                task,
                request_id,
                GovernedTaskMutation::RecordOperatorDecision {
                    decision: OperatorDecisionInput {
                        actor: actor(GovernedActorKind::Operator, "james"),
                        supervisor_verdict_id: task.latest_supervisor_verdict().unwrap().id.clone(),
                        decision,
                        rationale: "decided from operator surface".into(),
                    },
                },
            ))
            .unwrap()
    }

    fn accept_run(state: &State, suffix: &str) -> GovernedTaskRun {
        let registered = state
            .register_governed_task(registration(state, &format!("register-{suffix}")))
            .unwrap();
        let running = state
            .mutate_governed_task(mutation(
                &registered,
                &format!("running-{suffix}"),
                GovernedTaskMutation::MarkRunning {
                    actor: actor(GovernedActorKind::System, "desktop"),
                },
            ))
            .unwrap();
        let claimed = claim(state, &running, &format!("claim-{suffix}"));
        let verified = verify(
            state,
            &claimed,
            &format!("verify-{suffix}"),
            GovernedVerificationOutcome::Passed,
        );
        let judged = recommend_accept(state, &verified, &format!("review-{suffix}"));
        operator_decide(
            state,
            &judged,
            &format!("approve-{suffix}"),
            OperatorDecisionKind::Approve,
        )
    }

    #[test]
    fn full_flow_requires_distinct_claim_verification_judgment_and_operator_decision() {
        let (_root, state) = state();
        let registered = state
            .register_governed_task(registration(&state, "register-1"))
            .unwrap();
        let running = state
            .mutate_governed_task(mutation(
                &registered,
                "running-1",
                GovernedTaskMutation::MarkRunning {
                    actor: actor(GovernedActorKind::System, "desktop"),
                },
            ))
            .unwrap();
        let claimed = claim(&state, &running, "claim-1");
        assert_eq!(
            claimed.review_state,
            GovernedReviewState::AwaitingVerification
        );
        let claim_id = claimed.latest_claim().unwrap().id.clone();
        let verified = state
            .mutate_governed_task(mutation(
                &claimed,
                "verify-1",
                GovernedTaskMutation::RecordVerification {
                    verification: GovernedVerificationInput {
                        actor: actor(GovernedActorKind::Verifier, "cargo-gate"),
                        claim_id,
                        subject_revision: "abc123".into(),
                        policy: "workspace-default".into(),
                        outcome: GovernedVerificationOutcome::Passed,
                        commands: vec![GovernedCommandEvidence {
                            name: "tests".into(),
                            executable: "cargo".into(),
                            redacted_args: vec!["test".into()],
                            command_digest: digest('a'),
                            exit_code: Some(0),
                            success: true,
                            output_digest: digest('b'),
                            output_ref: Some("evidence/tests.log".into()),
                            output_bytes: 42,
                            output_truncated: false,
                        }],
                        artifact_ids: vec![],
                        notes: None,
                    },
                },
            ))
            .unwrap();
        let verification_id = verified.latest_verification().unwrap().id.clone();
        let judged = state
            .mutate_governed_task(mutation(
                &verified,
                "supervisor-1",
                GovernedTaskMutation::RecordSupervisorVerdict {
                    verdict: SupervisorVerdictInput {
                        actor: actor(GovernedActorKind::Supervisor, "supervisor-1"),
                        verification_id,
                        verdict: SupervisorVerdictKind::RecommendAccept,
                        rationale: "evidence satisfies acceptance criteria".into(),
                    },
                },
            ))
            .unwrap();
        assert_eq!(judged.review_state, GovernedReviewState::AwaitingOperator);
        let supervisor_id = judged.latest_supervisor_verdict().unwrap().id.clone();
        let accepted = state
            .mutate_governed_task(mutation(
                &judged,
                "operator-1",
                GovernedTaskMutation::RecordOperatorDecision {
                    decision: OperatorDecisionInput {
                        actor: actor(GovernedActorKind::Operator, "james"),
                        supervisor_verdict_id: supervisor_id,
                        decision: OperatorDecisionKind::Approve,
                        rationale: "approved from operator surface".into(),
                    },
                },
            ))
            .unwrap();

        assert!(accepted.is_accepted());
        assert_eq!(accepted.revision, 5);
        assert_eq!(accepted.events.len(), 6);
        let candidates = state
            .list_accepted_run_memory_candidates("impulse-test")
            .unwrap();
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert_eq!(candidate.governed_task_id, accepted.id);
        assert_eq!(candidate.accepted_task_revision, accepted.revision);
        assert_eq!(
            candidate.operator_decision_id,
            accepted.operator_decisions.last().unwrap().id
        );
        assert!(!candidate
            .proposed_summary
            .contains("implementation complete"));
        assert!(!candidate
            .proposed_summary
            .contains("evidence satisfies acceptance criteria"));
    }

    #[test]
    fn accepted_candidate_is_replay_safe_restart_repaired_and_never_mutates_memory() {
        let (_root, state) = state();
        let genome_path = state.storage().path("GENOME.md");
        let history_path = state.storage().path("HISTORY.jsonl");
        let genome_before = b"# Genome\n\nOperator curated only.\n";
        let history_before = b"{\"session_id\":\"existing\"}\n";
        std::fs::write(&genome_path, genome_before).unwrap();
        std::fs::write(&history_path, history_before).unwrap();

        let registered = state
            .register_governed_task(registration(&state, "register-candidate-replay"))
            .unwrap();
        let running = state
            .mutate_governed_task(mutation(
                &registered,
                "running-candidate-replay",
                GovernedTaskMutation::MarkRunning {
                    actor: actor(GovernedActorKind::System, "desktop"),
                },
            ))
            .unwrap();
        let claimed = claim(&state, &running, "claim-candidate-replay");
        let verified = verify(
            &state,
            &claimed,
            "verify-candidate-replay",
            GovernedVerificationOutcome::Passed,
        );
        let judged = recommend_accept(&state, &verified, "review-candidate-replay");
        let approval = mutation(
            &judged,
            "approve-candidate-replay",
            GovernedTaskMutation::RecordOperatorDecision {
                decision: OperatorDecisionInput {
                    actor: actor(GovernedActorKind::Operator, "james"),
                    supervisor_verdict_id: judged.latest_supervisor_verdict().unwrap().id.clone(),
                    decision: OperatorDecisionKind::Approve,
                    rationale: "candidate may be reviewed, not promoted".into(),
                },
            },
        );
        let accepted = state.mutate_governed_task(approval.clone()).unwrap();
        let first = state
            .list_accepted_run_memory_candidates("impulse-test")
            .unwrap();
        assert_eq!(first.len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = std::fs::metadata(
                state
                    .storage()
                    .path(crate::state::memory_candidate::memory_candidates_file()),
            )
            .unwrap()
            .permissions()
            .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "candidate ledger must remain owner-only");
        }

        let replay = state.mutate_governed_task(approval).unwrap();
        assert_eq!(replay, accepted);
        let after_replay = state
            .list_accepted_run_memory_candidates("impulse-test")
            .unwrap();
        assert_eq!(after_replay, first);
        assert_eq!(std::fs::read(&genome_path).unwrap(), genome_before);
        assert_eq!(std::fs::read(&history_path).unwrap(), history_before);

        let base = state.storage().base_path().to_path_buf();
        state
            .storage()
            .write_private_json(
                crate::state::memory_candidate::memory_candidates_file(),
                &serde_json::json!({"schema_version": 1, "candidates": {}}),
            )
            .unwrap();
        drop(state);

        let reloaded = State::new(base).unwrap();
        let repaired = reloaded
            .list_accepted_run_memory_candidates("impulse-test")
            .unwrap();
        assert_eq!(repaired, first);
        assert_eq!(std::fs::read(genome_path).unwrap(), genome_before);
        assert_eq!(std::fs::read(history_path).unwrap(), history_before);
    }

    #[test]
    fn operator_rejection_never_creates_a_memory_candidate() {
        let (_root, state) = state();
        let registered = state
            .register_governed_task(registration(&state, "register-candidate-reject"))
            .unwrap();
        let running = state
            .mutate_governed_task(mutation(
                &registered,
                "running-candidate-reject",
                GovernedTaskMutation::MarkRunning {
                    actor: actor(GovernedActorKind::System, "desktop"),
                },
            ))
            .unwrap();
        let claimed = claim(&state, &running, "claim-candidate-reject");
        let verified = verify(
            &state,
            &claimed,
            "verify-candidate-reject",
            GovernedVerificationOutcome::Passed,
        );
        let judged = recommend_accept(&state, &verified, "review-candidate-reject");
        let rejected = operator_decide(
            &state,
            &judged,
            "reject-candidate",
            OperatorDecisionKind::Reject,
        );

        assert_eq!(rejected.review_state, GovernedReviewState::Rejected);
        assert!(state
            .list_accepted_run_memory_candidates("impulse-test")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn memory_candidate_reload_rejects_tampered_projection() {
        let (_root, state) = state();
        let accepted = accept_run(&state, "candidate-tamper");
        let candidate = state
            .list_accepted_run_memory_candidates("impulse-test")
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(candidate.governed_task_id, accepted.id);
        let candidate_id = candidate.id.to_string();
        rewrite_persisted_candidate_ledger(&state, |ledger| {
            ledger["candidates"][candidate_id.as_str()]["proposed_summary"] =
                serde_json::Value::String("tampered but structurally valid summary".to_string());
        });

        let error = reload_error(
            state.storage().base_path().to_path_buf(),
            "tampered candidate projection must fail closed",
        );
        assert!(format!("{error:#}").contains("does not match its accepted governed-task source"));
    }

    #[test]
    fn memory_candidate_reload_rejects_orphan_projection() {
        let (_root, state) = state();
        accept_run(&state, "candidate-orphan");
        rewrite_persisted_ledger(&state, |ledger| {
            ledger["tasks"] = serde_json::json!({});
            ledger["processed_requests"] = serde_json::json!({});
        });

        let error = reload_error(
            state.storage().base_path().to_path_buf(),
            "orphan candidate projection must fail closed",
        );
        assert!(format!("{error:#}").contains("is orphaned from accepted governed-task truth"));
    }

    #[test]
    fn accepted_task_is_terminal_and_cannot_orphan_its_candidate_by_later_rejection() {
        let (_root, state) = state();
        let accepted = accept_run(&state, "candidate-terminal");
        let before = state
            .list_accepted_run_memory_candidates("impulse-test")
            .unwrap();
        assert_eq!(before.len(), 1);

        let error = state
            .mutate_governed_task(mutation(
                &accepted,
                "reject-after-accept",
                GovernedTaskMutation::RecordOperatorDecision {
                    decision: OperatorDecisionInput {
                        actor: actor(GovernedActorKind::Operator, "james"),
                        supervisor_verdict_id: accepted
                            .latest_supervisor_verdict()
                            .unwrap()
                            .id
                            .clone(),
                        decision: OperatorDecisionKind::Reject,
                        rationale: "attempt to reverse terminal acceptance".into(),
                    },
                },
            ))
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("operator decision requires awaiting-operator state"));
        assert_eq!(
            state
                .list_accepted_run_memory_candidates("impulse-test")
                .unwrap(),
            before
        );

        let reloaded = State::new(state.storage().base_path().to_path_buf()).unwrap();
        assert_eq!(
            reloaded
                .list_accepted_run_memory_candidates("impulse-test")
                .unwrap(),
            before
        );
    }

    #[test]
    fn runtime_exit_does_not_change_review_state_and_state_survives_reload() {
        let (_root, state) = state();
        let registered = state
            .register_governed_task(registration(&state, "register-restart"))
            .unwrap();
        let running = state
            .mutate_governed_task(mutation(
                &registered,
                "running-restart",
                GovernedTaskMutation::MarkRunning {
                    actor: actor(GovernedActorKind::System, "desktop"),
                },
            ))
            .unwrap();
        let exited = state
            .mutate_governed_task(mutation(
                &running,
                "exit-restart",
                GovernedTaskMutation::MarkRuntimeExited {
                    actor: actor(GovernedActorKind::System, "desktop"),
                    reason: None,
                },
            ))
            .unwrap();
        assert_eq!(exited.review_state, GovernedReviewState::AwaitingClaim);
        let base = state.storage().base_path().to_path_buf();
        drop(state);

        let reloaded = State::new(base).unwrap();
        let recovered = reloaded
            .get_governed_task("impulse-test", &exited.id)
            .unwrap()
            .unwrap();
        assert_eq!(recovered, exited);
    }

    #[cfg(unix)]
    #[test]
    fn governed_ledger_is_owner_only_after_registration_and_mutation() {
        let (_root, state) = state();
        let registered = state
            .register_governed_task(registration(&state, "register-private-ledger"))
            .unwrap();
        let ledger_path = state.storage().path(GOVERNED_TASKS_FILE);
        assert_eq!(
            std::fs::metadata(&ledger_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        std::fs::set_permissions(&ledger_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        state
            .mutate_governed_task(mutation(
                &registered,
                "running-private-ledger",
                GovernedTaskMutation::MarkRunning {
                    actor: actor(GovernedActorKind::System, "desktop"),
                },
            ))
            .unwrap();
        assert_eq!(
            std::fs::metadata(ledger_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn stale_revision_and_replayed_request_are_safe() {
        let (_root, state) = state();
        let registered = state
            .register_governed_task(registration(&state, "register-cas"))
            .unwrap();
        let request = mutation(
            &registered,
            "running-cas",
            GovernedTaskMutation::MarkRunning {
                actor: actor(GovernedActorKind::System, "desktop"),
            },
        );
        let running = state.mutate_governed_task(request.clone()).unwrap();
        let replay = state.mutate_governed_task(request.clone()).unwrap();
        assert_eq!(replay.revision, running.revision);

        let collision = mutation(
            &registered,
            "running-cas",
            GovernedTaskMutation::MarkLaunchFailed {
                actor: actor(GovernedActorKind::System, "desktop"),
                reason: "different operation with a reused request id".into(),
            },
        );
        let collision_error = state
            .mutate_governed_task(collision)
            .unwrap_err()
            .to_string();
        assert!(collision_error.contains("different payload"));

        let mut stale = mutation(
            &running,
            "exit-cas",
            GovernedTaskMutation::MarkRuntimeExited {
                actor: actor(GovernedActorKind::System, "desktop"),
                reason: None,
            },
        );
        stale.expected_revision = 0;
        let error = state.mutate_governed_task(stale).unwrap_err().to_string();
        assert!(error.contains("revision conflict"));
        assert_eq!(
            state
                .get_governed_task("impulse-test", &running.id)
                .unwrap()
                .unwrap()
                .revision,
            1
        );

        let exited = state
            .mutate_governed_task(mutation(
                &running,
                "exit-after-replay",
                GovernedTaskMutation::MarkRuntimeExited {
                    actor: actor(GovernedActorKind::System, "desktop"),
                    reason: None,
                },
            ))
            .unwrap();
        let mut late_request = request;
        late_request.expected_revision = exited.revision;
        let late_replay = state.mutate_governed_task(late_request).unwrap();
        assert_eq!(late_replay, exited);
        assert_eq!(late_replay.events.len(), 3);
    }

    #[test]
    fn failed_verification_cannot_be_recommended_for_acceptance() {
        let (_root, state) = state();
        let registered = state
            .register_governed_task(registration(&state, "register-failed-gate"))
            .unwrap();
        let running = state
            .mutate_governed_task(mutation(
                &registered,
                "running-failed-gate",
                GovernedTaskMutation::MarkRunning {
                    actor: actor(GovernedActorKind::System, "desktop"),
                },
            ))
            .unwrap();
        let claimed = claim(&state, &running, "claim-failed-gate");
        let failed = verify(
            &state,
            &claimed,
            "verify-failed-gate",
            GovernedVerificationOutcome::Failed,
        );

        let request = mutation(
            &failed,
            "supervisor-failed-gate",
            GovernedTaskMutation::RecordSupervisorVerdict {
                verdict: SupervisorVerdictInput {
                    actor: actor(GovernedActorKind::Supervisor, "supervisor-1"),
                    verification_id: failed.latest_verification().unwrap().id.clone(),
                    verdict: SupervisorVerdictKind::RecommendAccept,
                    rationale: "attempt to accept failed evidence".into(),
                },
            },
        );
        let error = state.mutate_governed_task(request).unwrap_err().to_string();

        assert!(error.contains("passing verification"));
        assert_eq!(
            state
                .get_governed_task("impulse-test", &failed.id)
                .unwrap()
                .unwrap()
                .review_state,
            GovernedReviewState::VerificationFailed
        );
    }

    #[test]
    fn wrong_project_requests_fail_without_creating_or_mutating_tasks() {
        let (_root, state) = state();
        let mut wrong_registration = registration(&state, "register-wrong-project");
        wrong_registration.project_id = "another-project".to_string();
        let error = state
            .register_governed_task(wrong_registration)
            .unwrap_err()
            .to_string();
        assert!(error.contains("project mismatch"));
        assert!(state
            .list_governed_tasks("impulse-test")
            .unwrap()
            .is_empty());

        let registered = state
            .register_governed_task(registration(&state, "register-project-scope"))
            .unwrap();
        assert!(state
            .get_governed_task("another-project", &registered.id)
            .unwrap_err()
            .to_string()
            .contains("project mismatch"));
        assert!(state
            .list_governed_tasks("another-project")
            .unwrap_err()
            .to_string()
            .contains("project mismatch"));

        let mut wrong_mutation = mutation(
            &registered,
            "running-wrong-project",
            GovernedTaskMutation::MarkRunning {
                actor: actor(GovernedActorKind::System, "desktop"),
            },
        );
        wrong_mutation.project_id = "another-project".to_string();
        assert!(state
            .mutate_governed_task(wrong_mutation)
            .unwrap_err()
            .to_string()
            .contains("project mismatch"));
        let unchanged = state
            .get_governed_task("impulse-test", &registered.id)
            .unwrap()
            .unwrap();
        assert_eq!(unchanged.revision, 0);
        assert_eq!(unchanged.events.len(), 1);
    }

    #[test]
    fn inconclusive_or_malformed_verification_cannot_advance_review() {
        let (_root, state) = state();
        let registered = state
            .register_governed_task(registration(&state, "register-evidence-boundary"))
            .unwrap();
        let running = state
            .mutate_governed_task(mutation(
                &registered,
                "running-evidence-boundary",
                GovernedTaskMutation::MarkRunning {
                    actor: actor(GovernedActorKind::System, "desktop"),
                },
            ))
            .unwrap();
        let claimed = claim(&state, &running, "claim-evidence-boundary");

        let malformed_cases = [
            GovernedCommandEvidence {
                name: "path traversal".into(),
                executable: "cargo".into(),
                redacted_args: vec!["test".into()],
                command_digest: digest('a'),
                exit_code: Some(0),
                success: true,
                output_digest: digest('b'),
                output_ref: Some("../secret.log".into()),
                output_bytes: 1,
                output_truncated: false,
            },
            GovernedCommandEvidence {
                name: "contradictory exit".into(),
                executable: "cargo".into(),
                redacted_args: vec!["test".into()],
                command_digest: digest('a'),
                exit_code: Some(1),
                success: true,
                output_digest: digest('b'),
                output_ref: None,
                output_bytes: 1,
                output_truncated: false,
            },
            GovernedCommandEvidence {
                name: "invalid digest".into(),
                executable: "cargo".into(),
                redacted_args: vec!["test".into()],
                command_digest: "not-a-digest".into(),
                exit_code: Some(0),
                success: true,
                output_digest: digest('b'),
                output_ref: None,
                output_bytes: 1,
                output_truncated: false,
            },
        ];
        for (index, command) in malformed_cases.into_iter().enumerate() {
            let request = mutation(
                &claimed,
                &format!("verify-malformed-{index}"),
                GovernedTaskMutation::RecordVerification {
                    verification: GovernedVerificationInput {
                        actor: actor(GovernedActorKind::Verifier, "cargo-gate"),
                        claim_id: claimed.latest_claim().unwrap().id.clone(),
                        subject_revision: claimed.latest_claim().unwrap().subject_revision.clone(),
                        policy: "workspace-default".into(),
                        outcome: GovernedVerificationOutcome::Passed,
                        commands: vec![command],
                        artifact_ids: Vec::new(),
                        notes: None,
                    },
                },
            );
            assert!(state.mutate_governed_task(request).is_err());
        }
        let after_rejections = state
            .get_governed_task("impulse-test", &claimed.id)
            .unwrap()
            .unwrap();
        assert_eq!(after_rejections.revision, claimed.revision);
        assert!(after_rejections.verifications.is_empty());

        let inconclusive = verify(
            &state,
            &after_rejections,
            "verify-inconclusive",
            GovernedVerificationOutcome::Inconclusive,
        );
        assert_eq!(
            inconclusive.review_state,
            GovernedReviewState::VerificationFailed
        );
        assert!(state
            .mutate_governed_task(mutation(
                &inconclusive,
                "supervisor-inconclusive",
                GovernedTaskMutation::RecordSupervisorVerdict {
                    verdict: SupervisorVerdictInput {
                        actor: actor(GovernedActorKind::Supervisor, "supervisor-1"),
                        verification_id: inconclusive.latest_verification().unwrap().id.clone(),
                        verdict: SupervisorVerdictKind::RecommendAccept,
                        rationale: "attempt to accept inconclusive evidence".into(),
                    },
                },
            ))
            .unwrap_err()
            .to_string()
            .contains("passing verification"));
    }

    #[test]
    fn ledger_reload_rejects_forged_accepted_state_and_mismatched_task_key() {
        let (_root, state) = state();
        let registered = state
            .register_governed_task(registration(&state, "register-corrupt-state"))
            .unwrap();
        let base = state.storage().base_path().to_path_buf();
        rewrite_persisted_ledger(&state, |ledger| {
            ledger["tasks"][registered.id.as_str()]["review_state"] =
                serde_json::Value::String("accepted".to_string());
        });
        let error = reload_error(base.clone(), "forged state must fail");
        assert!(format!("{error:#}").contains("Invalid governed task history"));

        rewrite_persisted_ledger(&state, |ledger| {
            ledger["tasks"][registered.id.as_str()]["review_state"] =
                serde_json::Value::String("awaiting_claim".to_string());
            let task = ledger["tasks"]
                .as_object_mut()
                .unwrap()
                .remove(registered.id.as_str())
                .unwrap();
            ledger["tasks"]
                .as_object_mut()
                .unwrap()
                .insert("task-forged-map-key".to_string(), task);
        });
        let error = reload_error(base, "mismatched task map key must fail");
        assert!(format!("{error:#}").contains("does not match record id"));
    }

    #[test]
    fn ledger_reload_rejects_receipt_fingerprint_or_immutable_task_corruption() {
        let (_root, state) = state();
        let registered = state
            .register_governed_task(registration(&state, "register-fingerprint-corruption"))
            .unwrap();
        let running = state
            .mutate_governed_task(mutation(
                &registered,
                "running-fingerprint-corruption",
                GovernedTaskMutation::MarkRunning {
                    actor: actor(GovernedActorKind::System, "desktop"),
                },
            ))
            .unwrap();
        let base = state.storage().base_path().to_path_buf();

        rewrite_persisted_ledger(&state, |ledger| {
            ledger["processed_requests"]["running-fingerprint-corruption"]["request_fingerprint"] =
                serde_json::Value::String(format!("sha256-v1:{}", "f".repeat(64)));
        });
        let error = reload_error(base.clone(), "changed receipt fingerprint must fail closed");
        assert!(format!("{error:#}").contains("does not match its lifecycle history"));

        state
            .storage()
            .write_private_json(
                GOVERNED_TASKS_FILE,
                &*state
                    .governed_tasks
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            )
            .unwrap();
        rewrite_persisted_ledger(&state, |ledger| {
            ledger["tasks"][running.id.as_str()]["task"] =
                serde_json::Value::String("tampered but otherwise valid task text".to_string());
        });
        let error = reload_error(base, "immutable task mutation must fail closed");
        assert!(format!("{error:#}").contains("immutable task record"));
    }

    #[test]
    fn ledger_reload_rejects_event_gaps_missing_receipts_and_malformed_evidence() {
        let (_root, state) = state();
        let registered = state
            .register_governed_task(registration(&state, "register-corrupt-history"))
            .unwrap();
        let running = state
            .mutate_governed_task(mutation(
                &registered,
                "running-corrupt-history",
                GovernedTaskMutation::MarkRunning {
                    actor: actor(GovernedActorKind::System, "desktop"),
                },
            ))
            .unwrap();
        let claimed = claim(&state, &running, "claim-corrupt-history");
        let verified = verify(
            &state,
            &claimed,
            "verify-corrupt-history",
            GovernedVerificationOutcome::Passed,
        );
        let base = state.storage().base_path().to_path_buf();

        rewrite_persisted_ledger(&state, |ledger| {
            ledger["tasks"][verified.id.as_str()]["events"]
                .as_array_mut()
                .unwrap()
                .pop();
        });
        let error = reload_error(base.clone(), "event gap must fail closed");
        assert!(format!("{error:#}").contains("contiguous events"));

        state
            .storage()
            .write_json(
                GOVERNED_TASKS_FILE,
                &*state
                    .governed_tasks
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            )
            .unwrap();
        rewrite_persisted_ledger(&state, |ledger| {
            ledger["processed_requests"] = serde_json::json!({});
        });
        let error = reload_error(base.clone(), "missing receipts must fail closed");
        assert!(format!("{error:#}").contains("receipt chain"));

        state
            .storage()
            .write_json(
                GOVERNED_TASKS_FILE,
                &*state
                    .governed_tasks
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            )
            .unwrap();
        rewrite_persisted_ledger(&state, |ledger| {
            ledger["tasks"][verified.id.as_str()]["verifications"][0]["commands"][0]
                ["command_digest"] = serde_json::Value::String("not-a-digest".to_string());
        });
        let error = reload_error(base, "malformed persisted evidence must fail closed");
        assert!(format!("{error:#}").contains("cannot be replayed"));
    }

    #[test]
    fn concurrent_operator_decisions_have_exactly_one_cas_winner() {
        let (_root, state) = state();
        let registered = state
            .register_governed_task(registration(&state, "register-race"))
            .unwrap();
        let running = state
            .mutate_governed_task(mutation(
                &registered,
                "running-race",
                GovernedTaskMutation::MarkRunning {
                    actor: actor(GovernedActorKind::System, "desktop"),
                },
            ))
            .unwrap();
        let claimed = claim(&state, &running, "claim-race");
        let verified = verify(
            &state,
            &claimed,
            "verify-race",
            GovernedVerificationOutcome::Passed,
        );
        let judged = recommend_accept(&state, &verified, "supervisor-race");
        let verdict_id = judged.latest_supervisor_verdict().unwrap().id.clone();
        let approve = mutation(
            &judged,
            "operator-race-approve",
            GovernedTaskMutation::RecordOperatorDecision {
                decision: OperatorDecisionInput {
                    actor: actor(GovernedActorKind::Operator, "operator-a"),
                    supervisor_verdict_id: verdict_id.clone(),
                    decision: OperatorDecisionKind::Approve,
                    rationale: "approve current evidence".into(),
                },
            },
        );
        let reject = mutation(
            &judged,
            "operator-race-reject",
            GovernedTaskMutation::RecordOperatorDecision {
                decision: OperatorDecisionInput {
                    actor: actor(GovernedActorKind::Operator, "operator-b"),
                    supervisor_verdict_id: verdict_id,
                    decision: OperatorDecisionKind::Reject,
                    rationale: "reject current evidence".into(),
                },
            },
        );
        let barrier = Arc::new(Barrier::new(3));
        let approve_state = Arc::clone(&state);
        let approve_barrier = Arc::clone(&barrier);
        let approve_thread = std::thread::spawn(move || {
            approve_barrier.wait();
            approve_state.mutate_governed_task(approve)
        });
        let reject_state = Arc::clone(&state);
        let reject_barrier = Arc::clone(&barrier);
        let reject_thread = std::thread::spawn(move || {
            reject_barrier.wait();
            reject_state.mutate_governed_task(reject)
        });
        barrier.wait();

        let results = [
            approve_thread.join().unwrap(),
            reject_thread.join().unwrap(),
        ];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| result
                    .as_ref()
                    .is_err_and(|error| error.to_string().contains("revision conflict")))
                .count(),
            1
        );
        let final_task = state
            .get_governed_task("impulse-test", &judged.id)
            .unwrap()
            .unwrap();
        assert_eq!(final_task.operator_decisions.len(), 1);
        assert!(matches!(
            final_task.review_state,
            GovernedReviewState::Accepted | GovernedReviewState::Rejected
        ));
        assert_eq!(final_task.revision, judged.revision + 1);
        assert_eq!(final_task.events.len(), judged.events.len() + 1);
        let candidates = state
            .list_accepted_run_memory_candidates("impulse-test")
            .unwrap();
        assert_eq!(candidates.len(), usize::from(final_task.is_accepted()));
        if final_task.is_accepted() {
            assert_eq!(candidates[0].governed_task_id, final_task.id);
        }

        let reloaded = State::new(state.storage().base_path().to_path_buf()).unwrap();
        assert_eq!(
            reloaded
                .get_governed_task("impulse-test", &judged.id)
                .unwrap()
                .unwrap(),
            final_task
        );
        assert_eq!(
            reloaded
                .list_accepted_run_memory_candidates("impulse-test")
                .unwrap(),
            candidates
        );
    }

    // ── ADR-0018: operator authentication provenance ────────────────────

    fn operator_decide_authenticated(
        state: &State,
        task: &GovernedTaskRun,
        request_id: &str,
        authentication: OperatorAuthentication,
    ) -> GovernedTaskRun {
        state
            .mutate_governed_task_authenticated(
                mutation(
                    task,
                    request_id,
                    GovernedTaskMutation::RecordOperatorDecision {
                        decision: OperatorDecisionInput {
                            actor: actor(GovernedActorKind::Operator, "james"),
                            supervisor_verdict_id: task
                                .latest_supervisor_verdict()
                                .unwrap()
                                .id
                                .clone(),
                            decision: OperatorDecisionKind::Approve,
                            rationale: "decided from operator surface".into(),
                        },
                    },
                ),
                authentication,
            )
            .unwrap()
    }

    fn awaiting_operator(state: &State, suffix: &str) -> GovernedTaskRun {
        let registered = state
            .register_governed_task(registration(state, &format!("register-{suffix}")))
            .unwrap();
        let running = state
            .mutate_governed_task(mutation(
                &registered,
                &format!("running-{suffix}"),
                GovernedTaskMutation::MarkRunning {
                    actor: actor(GovernedActorKind::System, "desktop"),
                },
            ))
            .unwrap();
        let claimed = claim(state, &running, &format!("claim-{suffix}"));
        let verified = verify(
            state,
            &claimed,
            &format!("verify-{suffix}"),
            GovernedVerificationOutcome::Passed,
        );
        recommend_accept(state, &verified, &format!("review-{suffix}"))
    }

    #[test]
    fn mutate_governed_task_defaults_operator_decisions_to_declared_provenance() {
        let (_root, state) = state();
        let judged = awaiting_operator(&state, "declared");
        let accepted = operator_decide(
            &state,
            &judged,
            "approve-declared",
            OperatorDecisionKind::Approve,
        );

        assert_eq!(
            accepted.operator_decisions.last().unwrap().authentication,
            OperatorAuthentication::Declared,
            "a caller that does not pass connection provenance never claims authentication"
        );
    }

    #[test]
    fn authenticated_mutation_stamps_capability_provenance_on_the_decision() {
        let (_root, state) = state();
        let judged = awaiting_operator(&state, "authenticated");
        let accepted = operator_decide_authenticated(
            &state,
            &judged,
            "approve-authenticated",
            OperatorAuthentication::CapabilityAuthenticated,
        );

        let decision = accepted.operator_decisions.last().unwrap();
        assert!(decision.authentication.is_capability_authenticated());
        assert_eq!(accepted.review_state, GovernedReviewState::Accepted);

        // The stamp survives a reload, and the replayed history still
        // validates against stored truth.
        let reloaded = State::new(state.storage().base_path().to_path_buf()).unwrap();
        let stored = reloaded
            .get_governed_task("impulse-test", &accepted.id)
            .unwrap()
            .unwrap();
        assert_eq!(stored, accepted);
        assert!(stored
            .operator_decisions
            .last()
            .unwrap()
            .authentication
            .is_capability_authenticated());
    }

    #[test]
    fn client_supplied_authentication_cannot_be_smuggled_through_the_mutation_payload() {
        let (_root, state) = state();
        let judged = awaiting_operator(&state, "smuggle");

        // A client crafting the wire payload cannot assert its own provenance:
        // the field lives on the persisted record, is written by the daemon,
        // and `OperatorDecisionInput` denies unknown keys, so the attempt fails
        // before it reaches the state layer at all.
        let mut wire = serde_json::to_value(mutation(
            &judged,
            "approve-smuggle",
            GovernedTaskMutation::RecordOperatorDecision {
                decision: OperatorDecisionInput {
                    actor: actor(GovernedActorKind::Operator, "james"),
                    supervisor_verdict_id: judged.latest_supervisor_verdict().unwrap().id.clone(),
                    decision: OperatorDecisionKind::Approve,
                    rationale: "decided from operator surface".into(),
                },
            },
        ))
        .unwrap();
        let legitimate = wire.clone();
        wire["mutation"]["data"]["decision"]["authentication"] =
            serde_json::json!("capability_authenticated");
        let error = serde_json::from_value::<GovernedTaskMutationRequest>(wire).unwrap_err();
        assert!(
            format!("{error}").contains("authentication"),
            "a payload asserting its own provenance must be rejected at the boundary"
        );

        // The same request without that key is accepted, and the decision it
        // records is `Declared` because this caller passed no connection class.
        let request: GovernedTaskMutationRequest = serde_json::from_value(legitimate).unwrap();
        let accepted = state.mutate_governed_task(request).unwrap();
        assert_eq!(
            accepted.operator_decisions.last().unwrap().authentication,
            OperatorAuthentication::Declared
        );
    }

    #[test]
    fn caller_composed_evidence_never_claims_an_authenticated_operator() {
        use impulse_ops::memory_candidate::AcceptedRunSourceAssurance;

        let (_root, state) = state();
        let judged = awaiting_operator(&state, "composed");
        operator_decide_authenticated(
            &state,
            &judged,
            "approve-composed",
            OperatorAuthentication::CapabilityAuthenticated,
        );

        let candidates = state
            .list_accepted_run_memory_candidates("impulse-test")
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].source_assurance,
            AcceptedRunSourceAssurance::CallerComposedEvidenceDeclaredOperator,
            "these fixtures carry no verification profile, so the evidence half is the weaker one"
        );
        assert!(!candidates[0].source_assurance.is_authenticated_operator());
    }

    /// A profiled registration, materialized directly in the ledger.
    ///
    /// The daemon's own profiled-registration path additionally proves a clean
    /// canonical Git worktree at a committed HEAD (`handle_governed_task_request`),
    /// which is out of scope here: this fixture exists to prove the *assurance
    /// mapping*, and the accepted-run chain is driven through the ordinary
    /// state API afterwards.
    fn profiled_awaiting_operator(state: &State, suffix: &str) -> GovernedTaskRun {
        let oid = "b".repeat(40);
        let root = state.storage().base_path().parent().unwrap();
        let assignment = impulse_ops::role_assignment::canonical_governed_builder_assignment();
        let registration = GovernedTaskRegistration::builder(
            format!("register-{suffix}"),
            format!("task-{suffix}"),
            "impulse-test",
            root.display().to_string(),
            "Ship governed task truth",
            "worker-1",
            "ion",
        )
        .acceptance_criteria(vec!["the committed Rust workspace passes".to_string()])
        .verification_profile(
            impulse_ops::governed_task::GovernedVerificationProfile::RustWorkspaceV1,
        )
        .initial_subject_revision(oid.clone())
        .role_assignment(assignment.clone())
        .role_compatibility(
            impulse_ops::agent_registry::AgentRegistry::builtin()
                .evaluate_role_compatibility(
                    &impulse_ops::agent_registry::AgentPlatformId::try_new("ion").unwrap(),
                    &assignment,
                )
                .unwrap(),
        )
        .build()
        .unwrap();
        let registered = state.register_governed_task(registration).unwrap();
        let running = state
            .mutate_governed_task(mutation(
                &registered,
                &format!("running-{suffix}"),
                GovernedTaskMutation::MarkRunning {
                    actor: actor(GovernedActorKind::System, "desktop"),
                },
            ))
            .unwrap();
        let claimed = state
            .mutate_governed_task(mutation(
                &running,
                &format!("claim-{suffix}"),
                GovernedTaskMutation::SubmitClaim {
                    claim: WorkerCompletionClaimInput {
                        actor: actor(GovernedActorKind::Worker, "worker-1"),
                        summary: "implementation complete".into(),
                        subject_revision: oid.clone(),
                        artifact_ids: vec!["artifact-1".into()],
                        diff_ref: Some(format!("git:{oid}")),
                    },
                },
            ))
            .unwrap();
        let verified = verify(
            state,
            &claimed,
            &format!("verify-{suffix}"),
            GovernedVerificationOutcome::Passed,
        );
        recommend_accept(state, &verified, &format!("review-{suffix}"))
    }

    #[test]
    fn profiled_evidence_plus_an_authenticated_operator_yields_the_authenticated_assurance() {
        use impulse_ops::memory_candidate::AcceptedRunSourceAssurance;

        let (_root, state) = state();
        let judged = profiled_awaiting_operator(&state, "profiled-auth");
        operator_decide_authenticated(
            &state,
            &judged,
            "approve-profiled-auth",
            OperatorAuthentication::CapabilityAuthenticated,
        );

        let candidates = state
            .list_accepted_run_memory_candidates("impulse-test")
            .unwrap();
        assert_eq!(
            candidates.len(),
            1,
            "exactly one candidate per accepted run"
        );
        assert_eq!(
            candidates[0].source_assurance,
            AcceptedRunSourceAssurance::DaemonProfiledEvidenceAuthenticatedOperator
        );
        assert!(candidates[0]
            .proposed_summary
            .contains("authenticated operator"));
        candidates[0].validate_shape().unwrap();
    }

    #[test]
    fn profiled_evidence_with_a_declared_operator_keeps_the_declared_assurance() {
        use impulse_ops::memory_candidate::AcceptedRunSourceAssurance;

        let (_root, state) = state();
        let judged = profiled_awaiting_operator(&state, "profiled-declared");
        operator_decide(
            &state,
            &judged,
            "approve-profiled-declared",
            OperatorDecisionKind::Approve,
        );

        let candidates = state
            .list_accepted_run_memory_candidates("impulse-test")
            .unwrap();
        assert_eq!(
            candidates[0].source_assurance,
            AcceptedRunSourceAssurance::DaemonProfiledEvidenceDeclaredOperator
        );
        assert!(!candidates[0].source_assurance.is_authenticated_operator());
    }

    #[test]
    fn a_ledger_at_a_superseded_derivation_version_reconciles_instead_of_failing_startup() {
        let (_root, state) = state();
        let accepted = accept_run(&state, "supersede");
        let before = state
            .list_accepted_run_memory_candidates("impulse-test")
            .unwrap();
        assert_eq!(before.len(), 1);

        // Simulate a ledger written before the ADR-0018 derivation bump: the
        // stored candidate carries the previous derivation version (and, with
        // it, a different id than today's derivation produces).
        rewrite_persisted_candidate_ledger(&state, |ledger| {
            let candidates = ledger["candidates"].as_object_mut().unwrap();
            let (id, mut candidate) = candidates
                .iter()
                .next()
                .map(|(k, v)| (k.clone(), v.clone()))
                .unwrap();
            candidates.remove(&id);
            candidate["derivation_version"] = serde_json::json!(
                impulse_ops::memory_candidate::ACCEPTED_RUN_MEMORY_DERIVATION_VERSION - 1
            );
            let legacy_id = format!("memory-candidate-{}", "f".repeat(64));
            candidate["id"] = serde_json::json!(legacy_id);
            candidates.insert(legacy_id, candidate);
        });

        let reloaded = State::new(state.storage().base_path().to_path_buf()).unwrap();
        let after = reloaded
            .list_accepted_run_memory_candidates("impulse-test")
            .unwrap();
        assert_eq!(
            after, before,
            "the superseded entry is dropped and re-derived from governed-task truth"
        );
        assert_eq!(
            reloaded
                .get_governed_task("impulse-test", &accepted.id)
                .unwrap()
                .unwrap(),
            accepted,
            "governed-task truth is untouched by the projection repair"
        );
    }
}
