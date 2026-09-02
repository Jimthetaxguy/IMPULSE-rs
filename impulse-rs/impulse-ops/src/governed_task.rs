//! Durable operating contract for daemon-owned governed task runs.
//!
//! A governed task is deliberately not a terminal session. Its execution can
//! stop while review continues, so execution and review state are independent.

use serde::de::Error as _;
use serde::{Deserialize, Serialize};

use crate::role_assignment::{
    canonical_governed_builder_assignment, AgentRoleAssignment, RoleCompatibility,
};

pub const MAX_GOVERNED_TASK_BYTES: usize = 8 * 1024;
pub const MAX_ACCEPTANCE_CRITERIA: usize = 64;
pub const MAX_GOVERNED_TEXT_BYTES: usize = 16 * 1024;
pub const MAX_GOVERNED_REFERENCES: usize = 64;
pub const MAX_GOVERNED_COMMANDS: usize = 64;
pub const MAX_GOVERNED_COMMAND_ARGS: usize = 256;
pub const MAX_GOVERNED_REFERENCE_BYTES: usize = 4 * 1024;
pub const MAX_GOVERNED_COMMAND_ARG_BYTES: usize = 16 * 1024;
pub const MAX_GOVERNED_RECORDS_PER_KIND: usize = 256;
pub const MAX_GOVERNED_EVENTS: usize = 1_024;
pub const GOVERNED_SUPERVISOR_REVIEW_CONTRACT_VERSION: &str = "1";
pub const MAX_PROFILED_TASK_BYTES: usize = 2 * 1024;
pub const MAX_PROFILED_ACCEPTANCE_CRITERIA: usize = 16;
pub const MAX_PROFILED_ACCEPTANCE_CRITERION_BYTES: usize = 512;
pub const MAX_PROFILED_CLAIM_SUMMARY_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GovernedTaskContractError {
    #[error("invalid {kind} `{value}`: ids must be nonempty and contain no whitespace or control characters")]
    InvalidId { kind: &'static str, value: String },
    #[error("invalid governed task field `{field}`: {message}")]
    InvalidField {
        field: &'static str,
        message: String,
    },
}

fn validate_open_id(
    kind: &'static str,
    value: String,
) -> Result<String, GovernedTaskContractError> {
    if value.is_empty()
        || value.len() > 256
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(GovernedTaskContractError::InvalidId { kind, value });
    }
    Ok(value)
}

macro_rules! governed_id {
    ($name:ident, $kind:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn try_new(value: impl Into<String>) -> Result<Self, GovernedTaskContractError> {
                validate_open_id($kind, value.into()).map(Self)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::try_new(value).map_err(D::Error::custom)
            }
        }
    };
}

governed_id!(GovernedTaskId, "governed_task_id");
governed_id!(GovernedRequestId, "governed_request_id");
governed_id!(GovernedRecordId, "governed_record_id");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPolicy {
    #[default]
    OperatorRequired,
}

/// Closed, versioned producer profiles that the daemon can expand without
/// accepting caller-authored shell commands or evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedVerificationProfile {
    RustWorkspaceV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GovernedExecutionState {
    #[default]
    Registered,
    Running,
    LaunchFailed,
    RuntimeExited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GovernedReviewState {
    #[default]
    AwaitingClaim,
    AwaitingVerification,
    VerificationFailed,
    AwaitingSupervisor,
    ChangesRequested,
    Escalated,
    AwaitingOperator,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedActorKind {
    System,
    Worker,
    Verifier,
    Supervisor,
    Operator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedActor {
    pub kind: GovernedActorKind,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedTaskRegistration {
    pub request_id: GovernedRequestId,
    /// Client-proposed durable identity. The daemon validates uniqueness and
    /// becomes authoritative for all subsequent revisions.
    pub task_id: GovernedTaskId,
    pub project_id: String,
    pub workspace_root: String,
    pub task: String,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub approval_policy: ApprovalPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_profile: Option<GovernedVerificationProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_assignment: Option<AgentRoleAssignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_compatibility: Option<RoleCompatibility>,
    pub runtime_id: String,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_subject_revision: Option<String>,
}

impl GovernedTaskRegistration {
    pub fn builder(
        request_id: impl Into<String>,
        task_id: impl Into<String>,
        project_id: impl Into<String>,
        workspace_root: impl Into<String>,
        task: impl Into<String>,
        agent_id: impl Into<String>,
        runtime_id: impl Into<String>,
    ) -> GovernedTaskRegistrationBuilder {
        GovernedTaskRegistrationBuilder {
            request_id: request_id.into(),
            task_id: task_id.into(),
            project_id: project_id.into(),
            workspace_root: workspace_root.into(),
            task: task.into(),
            acceptance_criteria: Vec::new(),
            approval_policy: ApprovalPolicy::OperatorRequired,
            verification_profile: None,
            role_assignment: None,
            role_compatibility: None,
            runtime_id: runtime_id.into(),
            agent_id: agent_id.into(),
            session_id: None,
            initial_subject_revision: None,
        }
    }

    pub fn validate(&self) -> Result<(), GovernedTaskContractError> {
        validate_nonblank("project_id", &self.project_id, 256)?;
        validate_nonblank("workspace_root", &self.workspace_root, 16 * 1024)?;
        validate_nonblank("task", &self.task, MAX_GOVERNED_TASK_BYTES)?;
        validate_nonblank("runtime_id", &self.runtime_id, 256)?;
        validate_nonblank("agent_id", &self.agent_id, 256)?;
        if self.acceptance_criteria.len() > MAX_ACCEPTANCE_CRITERIA {
            return Err(GovernedTaskContractError::InvalidField {
                field: "acceptance_criteria",
                message: format!("must contain at most {MAX_ACCEPTANCE_CRITERIA} entries"),
            });
        }
        for criterion in &self.acceptance_criteria {
            validate_nonblank("acceptance_criteria", criterion, MAX_GOVERNED_TEXT_BYTES)?;
        }
        if let Some(session_id) = &self.session_id {
            validate_nonblank("session_id", session_id, 256)?;
        }
        if self.task_id.as_str() == self.agent_id
            || self.session_id.as_deref() == Some(self.task_id.as_str())
        {
            return Err(GovernedTaskContractError::InvalidField {
                field: "task_id",
                message: "must be distinct from agent_id and session_id".to_string(),
            });
        }
        if let Some(subject) = &self.initial_subject_revision {
            validate_nonblank("initial_subject_revision", subject, 1024)?;
        }
        if self.verification_profile.is_some() {
            if self.acceptance_criteria.is_empty() {
                return Err(GovernedTaskContractError::InvalidField {
                    field: "acceptance_criteria",
                    message: "a closed-loop verification profile requires at least one criterion"
                        .to_string(),
                });
            }
            let subject = self.initial_subject_revision.as_deref().ok_or_else(|| {
                GovernedTaskContractError::InvalidField {
                    field: "initial_subject_revision",
                    message: "a closed-loop verification profile requires a clean Git commit OID"
                        .to_string(),
                }
            })?;
            validate_git_commit_oid("initial_subject_revision", subject)?;
            validate_nonblank("task", &self.task, MAX_PROFILED_TASK_BYTES)?;
            if self.acceptance_criteria.len() > MAX_PROFILED_ACCEPTANCE_CRITERIA {
                return Err(GovernedTaskContractError::InvalidField {
                    field: "acceptance_criteria",
                    message: format!(
                        "a closed-loop profile supports at most {MAX_PROFILED_ACCEPTANCE_CRITERIA} exact criteria"
                    ),
                });
            }
            for criterion in &self.acceptance_criteria {
                validate_nonblank(
                    "acceptance_criteria",
                    criterion,
                    MAX_PROFILED_ACCEPTANCE_CRITERION_BYTES,
                )?;
            }
            let assignment = self.role_assignment.as_ref().ok_or_else(|| {
                GovernedTaskContractError::InvalidField {
                    field: "role_assignment",
                    message:
                        "a closed-loop verification profile requires the canonical Builder role"
                            .to_string(),
                }
            })?;
            if assignment != &canonical_governed_builder_assignment() {
                return Err(GovernedTaskContractError::InvalidField {
                    field: "role_assignment",
                    message:
                        "a closed-loop verification profile requires the canonical Builder role requirements"
                            .to_string(),
                });
            }
            if self.role_compatibility.is_none() {
                return Err(GovernedTaskContractError::InvalidField {
                    field: "role_compatibility",
                    message:
                        "a closed-loop verification profile requires a matching runtime compatibility result"
                            .to_string(),
                });
            }
        }
        if self
            .role_compatibility
            .as_ref()
            .is_some_and(|compatibility| compatibility.is_blocked())
        {
            return Err(GovernedTaskContractError::InvalidField {
                field: "role_compatibility",
                message: "blocked compatibility cannot be registered".to_string(),
            });
        }
        match (&self.role_assignment, &self.role_compatibility) {
            (Some(assignment), Some(compatibility)) => {
                if assignment.role != compatibility.role {
                    return Err(GovernedTaskContractError::InvalidField {
                        field: "role_compatibility",
                        message: "compatibility role must match the assigned role".to_string(),
                    });
                }
                if compatibility.platform.as_str() != self.runtime_id {
                    return Err(GovernedTaskContractError::InvalidField {
                        field: "role_compatibility",
                        message: "compatibility platform must match runtime_id".to_string(),
                    });
                }
                if assignment.requirements.len() != compatibility.checks.len()
                    || assignment.requirements.iter().any(|requirement| {
                        compatibility
                            .checks
                            .iter()
                            .filter(|check| check.capability == requirement.capability)
                            .count()
                            != 1
                    })
                    || compatibility.checks.iter().any(|check| {
                        !assignment.requirements.iter().any(|requirement| {
                            requirement.capability == check.capability
                                && requirement.minimum_enforcement == check.required
                                && requirement.mandatory == check.mandatory
                        })
                    })
                {
                    return Err(GovernedTaskContractError::InvalidField {
                        field: "role_compatibility",
                        message:
                            "compatibility checks must exactly cover the assigned requirements"
                                .to_string(),
                    });
                }
            }
            (None, None) => {}
            _ => {
                return Err(GovernedTaskContractError::InvalidField {
                    field: "role_assignment",
                    message: "role assignment and compatibility must be supplied together"
                        .to_string(),
                });
            }
        }
        Ok(())
    }
}

pub struct GovernedTaskRegistrationBuilder {
    request_id: String,
    task_id: String,
    project_id: String,
    workspace_root: String,
    task: String,
    acceptance_criteria: Vec<String>,
    approval_policy: ApprovalPolicy,
    verification_profile: Option<GovernedVerificationProfile>,
    role_assignment: Option<AgentRoleAssignment>,
    role_compatibility: Option<RoleCompatibility>,
    runtime_id: String,
    agent_id: String,
    session_id: Option<String>,
    initial_subject_revision: Option<String>,
}

impl GovernedTaskRegistrationBuilder {
    pub fn acceptance_criteria(mut self, criteria: Vec<String>) -> Self {
        self.acceptance_criteria = criteria;
        self
    }

    pub fn approval_policy(mut self, policy: ApprovalPolicy) -> Self {
        self.approval_policy = policy;
        self
    }

    pub fn verification_profile(mut self, profile: GovernedVerificationProfile) -> Self {
        self.verification_profile = Some(profile);
        self
    }

    pub fn role_assignment(mut self, assignment: AgentRoleAssignment) -> Self {
        self.role_assignment = Some(assignment);
        self
    }

    pub fn role_compatibility(mut self, compatibility: RoleCompatibility) -> Self {
        self.role_compatibility = Some(compatibility);
        self
    }

    pub fn session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn initial_subject_revision(mut self, revision: impl Into<String>) -> Self {
        self.initial_subject_revision = Some(revision.into());
        self
    }

    pub fn build(self) -> Result<GovernedTaskRegistration, GovernedTaskContractError> {
        let registration = GovernedTaskRegistration {
            request_id: GovernedRequestId::try_new(self.request_id)?,
            task_id: GovernedTaskId::try_new(self.task_id)?,
            project_id: self.project_id,
            workspace_root: self.workspace_root,
            task: self.task,
            acceptance_criteria: self.acceptance_criteria,
            approval_policy: self.approval_policy,
            verification_profile: self.verification_profile,
            role_assignment: self.role_assignment,
            role_compatibility: self.role_compatibility,
            runtime_id: self.runtime_id,
            agent_id: self.agent_id,
            session_id: self.session_id,
            initial_subject_revision: self.initial_subject_revision,
        };
        registration.validate()?;
        Ok(registration)
    }
}

fn validate_nonblank(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), GovernedTaskContractError> {
    if value.trim().is_empty() || value.contains('\0') || value.len() > max_bytes {
        return Err(GovernedTaskContractError::InvalidField {
            field,
            message: format!("must be nonblank, NUL-free, and at most {max_bytes} UTF-8 bytes"),
        });
    }
    Ok(())
}

fn validate_git_commit_oid(
    field: &'static str,
    value: &str,
) -> Result<(), GovernedTaskContractError> {
    if !matches!(value.len(), 40 | 64)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(GovernedTaskContractError::InvalidField {
            field,
            message: "must be a 40- or 64-character lowercase hexadecimal Git commit OID"
                .to_string(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerCompletionClaimInput {
    pub actor: GovernedActor,
    pub summary: String,
    pub subject_revision: String,
    #[serde(default)]
    pub artifact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerCompletionClaim {
    pub id: GovernedRecordId,
    pub actor: GovernedActor,
    pub summary: String,
    pub subject_revision: String,
    #[serde(default)]
    pub artifact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_ref: Option<String>,
    pub submitted_at: String,
    pub based_on_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedCommandEvidence {
    pub name: String,
    /// Executable name or path with no embedded credentials.
    pub executable: String,
    /// Display-safe arguments. Producers must replace secret-bearing values
    /// with an explicit `<redacted>` token before persistence.
    #[serde(default)]
    pub redacted_args: Vec<String>,
    /// Digest of the exact unredacted argv, which must never be persisted.
    pub command_digest: String,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub output_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_ref: Option<String>,
    pub output_bytes: u64,
    #[serde(default)]
    pub output_truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedVerificationOutcome {
    Passed,
    Failed,
    Inconclusive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedVerificationInput {
    pub actor: GovernedActor,
    pub claim_id: GovernedRecordId,
    pub subject_revision: String,
    pub policy: String,
    pub outcome: GovernedVerificationOutcome,
    #[serde(default)]
    pub commands: Vec<GovernedCommandEvidence>,
    #[serde(default)]
    pub artifact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedVerification {
    pub id: GovernedRecordId,
    pub actor: GovernedActor,
    pub claim_id: GovernedRecordId,
    pub subject_revision: String,
    pub policy: String,
    pub outcome: GovernedVerificationOutcome,
    #[serde(default)]
    pub commands: Vec<GovernedCommandEvidence>,
    #[serde(default)]
    pub artifact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub recorded_at: String,
    pub based_on_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorVerdictKind {
    RecommendAccept,
    ChangesRequested,
    Escalate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorVerdictInput {
    pub actor: GovernedActor,
    pub verification_id: GovernedRecordId,
    pub verdict: SupervisorVerdictKind,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorVerdict {
    pub id: GovernedRecordId,
    pub actor: GovernedActor,
    pub verification_id: GovernedRecordId,
    pub verdict: SupervisorVerdictKind,
    pub rationale: String,
    pub decided_at: String,
    pub based_on_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorDecisionKind {
    Approve,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorDecisionInput {
    pub actor: GovernedActor,
    pub supervisor_verdict_id: GovernedRecordId,
    pub decision: OperatorDecisionKind,
    pub rationale: String,
}

/// How much the daemon can honestly attest about the operator behind a
/// decision (ADR-0018).
///
/// This is deliberately absent from [`OperatorDecisionInput`]: the value is
/// stamped by the daemon from the *connection class* the mutation arrived on
/// and is never read from client-supplied payload. A caller that reaches the
/// state layer directly (direct CLI mode, in-process tests) gets the
/// serde-default [`OperatorAuthentication::Declared`], which is also what an
/// operator decision recorded before ADR-0018 deserializes as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorAuthentication {
    /// Operator identity is asserted by the caller inside the same-user socket
    /// trust boundary. No connection-level proof was presented.
    #[default]
    Declared,
    /// The decision arrived on a connection that presented this daemon run's
    /// operator capability and whose peer uid matched the daemon's own uid.
    CapabilityAuthenticated,
}

impl OperatorAuthentication {
    pub fn is_capability_authenticated(self) -> bool {
        matches!(self, Self::CapabilityAuthenticated)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorDecision {
    pub id: GovernedRecordId,
    pub actor: GovernedActor,
    pub supervisor_verdict_id: GovernedRecordId,
    pub decision: OperatorDecisionKind,
    pub rationale: String,
    pub decided_at: String,
    pub based_on_revision: u64,
    /// Daemon-stamped connection provenance (ADR-0018). Serde-defaulted so
    /// records written before ADR-0018 load as `Declared`.
    #[serde(default)]
    pub authentication: OperatorAuthentication,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum GovernedTaskMutation {
    MarkRunning {
        actor: GovernedActor,
    },
    MarkLaunchFailed {
        actor: GovernedActor,
        reason: String,
    },
    MarkRuntimeExited {
        actor: GovernedActor,
        reason: Option<String>,
    },
    SubmitClaim {
        claim: WorkerCompletionClaimInput,
    },
    RecordVerification {
        verification: GovernedVerificationInput,
    },
    RecordSupervisorVerdict {
        verdict: SupervisorVerdictInput,
    },
    RecordOperatorDecision {
        decision: OperatorDecisionInput,
    },
    /// Records that a durable producer reservation for this task was found
    /// open by a fresh process and closed as needs-rerun. `reason` is a
    /// prepared, self-contained detail string (see
    /// `state/producer_reservation.rs`'s `note_producer_reservation_interrupted`)
    /// rather than structured fields, matching the existing
    /// `MarkLaunchFailed`/`MarkRuntimeExited` pattern of reconstructing the
    /// mutation directly from the recorded event's `actor`/`detail` on
    /// reload.
    NoteProducerReservationInterrupted {
        actor: GovernedActor,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedTaskMutationRequest {
    pub request_id: GovernedRequestId,
    pub project_id: String,
    pub task_id: GovernedTaskId,
    pub expected_revision: u64,
    pub mutation: GovernedTaskMutation,
}

/// Agent-facing claim request. Actor identity and subject revision are
/// intentionally absent: the project daemon derives both from task state and
/// the canonical Git workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedClaimRequest {
    pub request_id: GovernedRequestId,
    pub project_id: String,
    pub task_id: GovernedTaskId,
    pub expected_revision: u64,
    pub summary: String,
    #[serde(default)]
    pub artifact_ids: Vec<String>,
}

impl GovernedClaimRequest {
    pub fn validate(&self) -> Result<(), GovernedTaskContractError> {
        validate_nonblank("summary", &self.summary, MAX_PROFILED_CLAIM_SUMMARY_BYTES)?;
        if self.artifact_ids.len() > MAX_GOVERNED_REFERENCES {
            return Err(GovernedTaskContractError::InvalidField {
                field: "artifact_ids",
                message: format!("must contain at most {MAX_GOVERNED_REFERENCES} entries"),
            });
        }
        for artifact_id in &self.artifact_ids {
            validate_nonblank("artifact_ids", artifact_id, MAX_GOVERNED_REFERENCE_BYTES)?;
        }
        Ok(())
    }
}

/// Trigger for daemon-owned verification. The request contains no command or
/// evidence fields; the daemon expands the task's closed verification profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedVerificationRequest {
    pub request_id: GovernedRequestId,
    pub project_id: String,
    pub task_id: GovernedTaskId,
    pub expected_revision: u64,
}

/// Trigger for a daemon-launched Supervisor turn. The verdict payload is
/// produced and bound by the daemon after the model call returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedSupervisorReviewRequest {
    pub request_id: GovernedRequestId,
    pub project_id: String,
    pub task_id: GovernedTaskId,
    pub expected_revision: u64,
}

/// Strict response contract required from the launched Supervisor. Every
/// identity field is echoed so a response cannot be applied to another task,
/// revision, claim, verification, or subject by accident.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedSupervisorReviewEnvelope {
    pub contract_version: String,
    pub task_id: GovernedTaskId,
    pub task_revision: u64,
    pub claim_id: GovernedRecordId,
    pub verification_id: GovernedRecordId,
    pub subject_revision: String,
    pub acceptance_criteria_count: usize,
    pub acceptance_criteria_digest: String,
    pub verdict: SupervisorVerdictKind,
    pub rationale: String,
}

impl GovernedSupervisorReviewEnvelope {
    pub fn validate_shape(&self) -> Result<(), GovernedTaskContractError> {
        if self.contract_version != GOVERNED_SUPERVISOR_REVIEW_CONTRACT_VERSION {
            return Err(GovernedTaskContractError::InvalidField {
                field: "contract_version",
                message: format!("must equal {GOVERNED_SUPERVISOR_REVIEW_CONTRACT_VERSION}"),
            });
        }
        validate_git_commit_oid("subject_revision", &self.subject_revision)?;
        validate_sha256_digest(
            "acceptance_criteria_digest",
            &self.acceptance_criteria_digest,
        )?;
        validate_nonblank("rationale", &self.rationale, MAX_GOVERNED_TEXT_BYTES)
    }
}

fn validate_sha256_digest(
    field: &'static str,
    value: &str,
) -> Result<(), GovernedTaskContractError> {
    let valid = value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
    if !valid {
        return Err(GovernedTaskContractError::InvalidField {
            field,
            message: "must use the sha256:<64 hexadecimal characters> format".to_string(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedTaskEventKind {
    Registered,
    Running,
    LaunchFailed,
    RuntimeExited,
    ClaimSubmitted,
    VerificationRecorded,
    SupervisorVerdictRecorded,
    OperatorDecisionRecorded,
    /// A durable producer reservation (see `state/producer_reservation.rs`)
    /// was left open by a process that exited before persisting a receipt.
    /// Purely observational: it does not change `execution_state` or
    /// `review_state`.
    ProducerReservationInterrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedTaskEvent {
    pub id: GovernedRecordId,
    pub revision: u64,
    pub kind: GovernedTaskEventKind,
    pub actor: GovernedActor,
    pub detail: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedTaskRun {
    pub id: GovernedTaskId,
    pub revision: u64,
    pub project_id: String,
    pub workspace_root: String,
    pub task: String,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub approval_policy: ApprovalPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_profile: Option<GovernedVerificationProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_assignment: Option<AgentRoleAssignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_compatibility: Option<RoleCompatibility>,
    pub runtime_id: String,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_subject_revision: Option<String>,
    #[serde(default)]
    pub execution_state: GovernedExecutionState,
    #[serde(default)]
    pub review_state: GovernedReviewState,
    #[serde(default)]
    pub claims: Vec<WorkerCompletionClaim>,
    #[serde(default)]
    pub verifications: Vec<GovernedVerification>,
    #[serde(default)]
    pub supervisor_verdicts: Vec<SupervisorVerdict>,
    #[serde(default)]
    pub operator_decisions: Vec<OperatorDecision>,
    #[serde(default)]
    pub events: Vec<GovernedTaskEvent>,
    pub created_at: String,
    pub updated_at: String,
}

impl GovernedTaskRun {
    pub fn latest_claim(&self) -> Option<&WorkerCompletionClaim> {
        self.claims.last()
    }

    pub fn latest_verification(&self) -> Option<&GovernedVerification> {
        self.verifications.last()
    }

    pub fn latest_supervisor_verdict(&self) -> Option<&SupervisorVerdict> {
        self.supervisor_verdicts.last()
    }

    pub fn is_accepted(&self) -> bool {
        self.review_state == GovernedReviewState::Accepted
    }
}

/// The snapshot intentionally carries the full typed record in the first
/// protocol version. Raw command output remains out-of-line.
pub type GovernedTaskSnapshot = GovernedTaskRun;

#[cfg(test)]
mod operator_authentication_tests {
    use super::*;

    fn decision() -> OperatorDecision {
        OperatorDecision {
            id: GovernedRecordId::try_new("operator-decision-a").unwrap(),
            actor: GovernedActor {
                kind: GovernedActorKind::Operator,
                id: "operator-a".to_string(),
            },
            supervisor_verdict_id: GovernedRecordId::try_new("verdict-a").unwrap(),
            decision: OperatorDecisionKind::Approve,
            rationale: "accepted".to_string(),
            decided_at: "2026-09-02T00:00:00Z".to_string(),
            based_on_revision: 4,
            authentication: OperatorAuthentication::CapabilityAuthenticated,
        }
    }

    #[test]
    fn test_operator_authentication_round_trips_through_serde() {
        for value in [
            OperatorAuthentication::Declared,
            OperatorAuthentication::CapabilityAuthenticated,
        ] {
            let json = serde_json::to_string(&value).unwrap();
            let decoded: OperatorAuthentication = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, value);
        }
        assert_eq!(
            serde_json::to_string(&OperatorAuthentication::CapabilityAuthenticated).unwrap(),
            "\"capability_authenticated\""
        );
    }

    #[test]
    fn test_operator_decision_round_trips_with_authentication() {
        let original = decision();
        let json = serde_json::to_string(&original).unwrap();
        let decoded: OperatorDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, original);
        assert!(decoded.authentication.is_capability_authenticated());
    }

    #[test]
    fn test_operator_decision_without_authentication_field_loads_as_declared() {
        // A record persisted before ADR-0018 has no `authentication` key.
        let mut value = serde_json::to_value(decision()).unwrap();
        value.as_object_mut().unwrap().remove("authentication");
        let decoded: OperatorDecision = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.authentication, OperatorAuthentication::Declared);
        assert!(!decoded.authentication.is_capability_authenticated());
    }

    #[test]
    fn test_operator_decision_input_cannot_carry_authentication() {
        // The daemon stamps provenance; a client payload must not be able to
        // assert it. `OperatorDecisionInput` therefore has no such field, and
        // an unknown key is dropped rather than honored.
        let input: OperatorDecisionInput = serde_json::from_value(serde_json::json!({
            "actor": {"kind": "operator", "id": "operator-a"},
            "supervisor_verdict_id": "verdict-a",
            "decision": "approve",
            "rationale": "accepted",
            "authentication": "capability_authenticated",
        }))
        .unwrap();
        let encoded = serde_json::to_value(&input).unwrap();
        assert!(encoded.get("authentication").is_none());
    }
}
