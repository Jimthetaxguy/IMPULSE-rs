//! Durable operating contract for daemon-owned governed task runs.
//!
//! A governed task is deliberately not a terminal session. Its execution can
//! stop while review continues, so execution and review state are independent.

use std::path::PathBuf;

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

/// What an agent instance may see and touch in the project filesystem while it
/// works (ROSA's world-scope vocabulary, adapted to Git-level mediation).
///
/// The variants are ordered from least to most authority. Only
/// [`WorldScope::Authoritative`] (the serde default, so every pre-ADR-0019
/// record loads unchanged) and [`WorldScope::StagedAuthoritative`] are
/// materializable today; the two weaker scopes are declared so the contract is
/// stable, and registration refuses them rather than pretending.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum WorldScope {
    /// Observe a frozen copy; never write. What daemon-owned verification
    /// already does in its detached worktree.
    ReadOnlySnapshot,
    /// Write freely into a copy that is thrown away; nothing is ever promoted.
    DisposableScratch,
    /// Write into a disposable staged worktree; an operator acceptance plus a
    /// separate promotion step is what makes the result canonical.
    StagedAuthoritative,
    /// Write directly into the canonical project tree.
    #[default]
    Authoritative,
}

impl WorldScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReadOnlySnapshot => "read_only_snapshot",
            Self::DisposableScratch => "disposable_scratch",
            Self::StagedAuthoritative => "staged_authoritative",
            Self::Authoritative => "authoritative",
        }
    }

    /// Whether the scope requires a staged worktree before the runtime launches.
    pub fn requires_staged_worktree(&self) -> bool {
        matches!(self, Self::StagedAuthoritative)
    }

    /// Whether the daemon can materialize the scope today. A declared but
    /// unmaterializable scope must fail registration closed.
    pub fn is_materializable(&self) -> bool {
        matches!(self, Self::StagedAuthoritative | Self::Authoritative)
    }
}

impl std::fmt::Display for WorldScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Lifecycle of the disposable worktree a staged Builder works in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StagedWorktreeStatus {
    Active,
    Discarded,
}

/// Daemon-observed record of the staged worktree materialized for one task.
/// `root` is an absolute canonical path inside the project's `.impulse`
/// namespace; it is never caller-authored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedWorktree {
    pub id: GovernedRecordId,
    pub actor: GovernedActor,
    pub root: String,
    pub initial_subject_revision: String,
    /// Digests of the worktree-shared repository configuration observed when
    /// this worktree was materialized. Promotion refuses to check anything out
    /// unless the same digests still hold. Defaults to
    /// [`SharedRepositoryConfigPin::Unknown`] so a record written before the pin
    /// existed still loads instead of failing the whole ledger closed.
    #[serde(default)]
    pub shared_config_digest: SharedRepositoryConfigPin,
    pub status: StagedWorktreeStatus,
    pub materialized_at: String,
    pub based_on_revision: u64,
}

/// Caller-free input for the staged-worktree materialization mutation. The
/// daemon producer observes both fields; no client supplies them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedWorktreeInput {
    pub actor: GovernedActor,
    pub root: String,
    pub initial_subject_revision: String,
    #[serde(default)]
    pub shared_config_digest: SharedRepositoryConfigPin,
}

/// Why a promotion could not move the canonical branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionBlockedReason {
    /// The canonical head no longer equals the OID the task was registered at.
    CanonicalHeadMoved,
    /// The canonical checkout is on a detached HEAD, so there is no branch to
    /// advance. Promoting here would move HEAD only, and the next `git switch`
    /// would orphan the accepted commit.
    DetachedHead,
    /// The compare-and-swap on the canonical branch lost to a concurrent
    /// writer between observation and the ref update.
    ConcurrentBranchUpdate,
    /// Shared repository configuration changed while the Builder was working.
    /// A `filter` or `diff` driver defined there executes during the checkout
    /// promotion performs, so a run that rewrote it is not promotable without a
    /// human looking at what changed. The component names *what* changed:
    /// benign churn (a new remote, a credential helper) hard-blocks promotion
    /// too, and the operator must not have to guess which file to inspect.
    RepositoryConfigChanged { component: SharedConfigComponent },
    /// The staged worktree was materialized before shared configuration was
    /// pinned, so there is nothing to compare against. Not a failure of the
    /// run: the operator discards the worktree and re-materializes.
    RepositoryConfigUnpinned,
}

/// Which piece of worktree-shared Git state changed under a staged run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedConfigComponent {
    /// `.git/config`, shared by the main worktree and every linked worktree.
    RepositoryConfig,
    /// `.git/config.worktree`, read only when `extensions.worktreeConfig` is on.
    WorktreeConfig,
    /// `.git/info/attributes`, shared and invisible to a diff of the work tree.
    InfoAttributes,
}

impl SharedConfigComponent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RepositoryConfig => ".git/config",
            Self::WorktreeConfig => ".git/config.worktree",
            Self::InfoAttributes => ".git/info/attributes",
        }
    }
}

impl std::fmt::Display for SharedConfigComponent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Whether a staged worktree carries a shared-configuration pin at all.
///
/// A worktree materialized before ADR-0019 round 2 has no pin, and there is no
/// honest default for one: an empty digest would either compare equal to
/// nothing (silently unsafe) or to everything (blocked forever with no
/// explanation). `Unknown` says exactly what is true — the comparison cannot be
/// made — and promotion refuses on it with its own reason so the operator is
/// told to discard and re-materialize rather than left guessing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SharedRepositoryConfigPin {
    Recorded(SharedRepositoryConfigDigest),
    #[default]
    Unknown,
}

impl SharedRepositoryConfigPin {
    pub fn recorded(&self) -> Option<&SharedRepositoryConfigDigest> {
        match self {
            Self::Recorded(digest) => Some(digest),
            Self::Unknown => None,
        }
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

/// Digests of every piece of worktree-shared Git state that can turn a later
/// checkout into command execution. `None` means the component was absent,
/// which is itself pinned: creating it is a change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedRepositoryConfigDigest {
    pub repository_config: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_config: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_attributes: Option<String>,
}

impl SharedRepositoryConfigDigest {
    /// The first component that differs, which is what the operator is told.
    pub fn first_difference(&self, other: &Self) -> Option<SharedConfigComponent> {
        if self.repository_config != other.repository_config {
            return Some(SharedConfigComponent::RepositoryConfig);
        }
        if self.worktree_config != other.worktree_config {
            return Some(SharedConfigComponent::WorktreeConfig);
        }
        if self.info_attributes != other.info_attributes {
            return Some(SharedConfigComponent::InfoAttributes);
        }
        None
    }
}

impl PromotionBlockedReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CanonicalHeadMoved => "canonical_head_moved",
            Self::DetachedHead => "detached_head",
            Self::ConcurrentBranchUpdate => "concurrent_branch_update",
            Self::RepositoryConfigChanged { .. } => "repository_config_changed",
            Self::RepositoryConfigUnpinned => "repository_config_unpinned",
        }
    }
}

impl std::fmt::Display for PromotionBlockedReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Name the file, so an operator facing a hard block is not guessing.
            Self::RepositoryConfigChanged { component } => {
                write!(formatter, "{} changed ({component})", self.as_str())
            }
            other => formatter.write_str(other.as_str()),
        }
    }
}

/// What a promotion attempt actually did to the canonical branch.
///
/// A blocked promotion is an execution fact, not an error: the run stays
/// `accepted` and the operator decides what to do with a canonical branch that
/// moved, is detached, or lost a compare-and-swap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GovernedPromotionOutcome {
    Promoted {
        promoted_revision: String,
    },
    PromotionBlocked {
        canonical_head: String,
        reason: PromotionBlockedReason,
    },
}

impl GovernedPromotionOutcome {
    pub fn is_promoted(&self) -> bool {
        matches!(self, Self::Promoted { .. })
    }

    pub fn blocked_reason(&self) -> Option<PromotionBlockedReason> {
        match self {
            Self::Promoted { .. } => None,
            Self::PromotionBlocked { reason, .. } => Some(*reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedPromotionInput {
    pub actor: GovernedActor,
    pub accepted_revision: String,
    pub initial_subject_revision: String,
    pub outcome: GovernedPromotionOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedPromotion {
    pub id: GovernedRecordId,
    pub actor: GovernedActor,
    pub accepted_revision: String,
    pub initial_subject_revision: String,
    pub outcome: GovernedPromotionOutcome,
    pub recorded_at: String,
    pub based_on_revision: u64,
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
    /// Filesystem authority the launched runtime works under. Defaults to
    /// `authoritative` so every record written before ADR-0019 loads unchanged.
    #[serde(default)]
    pub world_scope: WorldScope,
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
            world_scope: WorldScope::default(),
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
        if !self.world_scope.is_materializable() {
            return Err(GovernedTaskContractError::InvalidField {
                field: "world_scope",
                message: format!(
                    "world scope `{}` is declared but not materializable by this daemon",
                    self.world_scope
                ),
            });
        }
        if self.world_scope.requires_staged_worktree() {
            if self.verification_profile.is_none() {
                return Err(GovernedTaskContractError::InvalidField {
                    field: "world_scope",
                    message:
                        "a staged world scope requires a closed-loop verification profile so the initial Git OID is daemon-attested"
                            .to_string(),
                });
            }
            validate_path_segment("task_id", self.task_id.as_str())?;
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
    world_scope: WorldScope,
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

    pub fn world_scope(mut self, scope: WorldScope) -> Self {
        self.world_scope = scope;
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
            world_scope: self.world_scope,
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

/// A staged worktree is materialized at `.impulse/worktrees/<task_id>`, so the
/// task id must be usable as exactly one filesystem path segment.
pub fn validate_path_segment(
    field: &'static str,
    value: &str,
) -> Result<(), GovernedTaskContractError> {
    let rejected = value.is_empty()
        || value.len() > 128
        || value == "."
        || value == ".."
        || value.starts_with('.')
        || value
            .chars()
            .any(|character| !matches!(character, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_'));
    if rejected {
        return Err(GovernedTaskContractError::InvalidField {
            field,
            message:
                "must be one filesystem path segment of at most 128 ASCII letters, digits, `-`, or `_`"
                    .to_string(),
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
    /// Digest of the deterministic loop report evaluated when this claim was
    /// accepted. Daemon-derived, never caller-supplied, and present only for a
    /// staged world scope (ADR-0019 rule 6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_report_digest: Option<String>,
    /// Governed-loop evidence version the digest was computed under. Stored
    /// beside the digest so revising the budget constants, or adding a field to
    /// `LoopReport`, cannot make an existing ledger unloadable: replay only
    /// recomputes digests written under the version it is running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_report_version: Option<u32>,
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

/// The client-supplied half of an operator decision.
///
/// `deny_unknown_fields` matches [`GovernedClaimRequest`] and does real work
/// here: the daemon-owned provenance field lives on [`OperatorDecision`], not on
/// this type, so a payload that tries to assert its own `authentication` is
/// rejected at the boundary rather than silently ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    MaterializeStagedWorktree {
        staged: StagedWorktreeInput,
    },
    DiscardStagedWorktree {
        actor: GovernedActor,
        reason: String,
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
    RecordPromotion {
        promotion: GovernedPromotionInput,
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
    StagedWorktreeMaterialized,
    StagedWorktreeDiscarded,
    Running,
    LaunchFailed,
    RuntimeExited,
    ClaimSubmitted,
    /// A claim that the governed Builder loop contract refused. The claim is
    /// still recorded as evidence; review state escalates instead of advancing.
    LoopTripped,
    VerificationRecorded,
    SupervisorVerdictRecorded,
    OperatorDecisionRecorded,
    /// A durable producer reservation (see `state/producer_reservation.rs`)
    /// was left open by a process that exited before persisting a receipt.
    /// Purely observational: it does not change `execution_state` or
    /// `review_state`.
    ProducerReservationInterrupted,
    PromotionRecorded,
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
    #[serde(default)]
    pub world_scope: WorldScope,
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
    /// Present only for a staged world scope, once the daemon has materialized
    /// the worktree. `None` on every pre-ADR-0019 record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staged_worktree: Option<StagedWorktree>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub promotions: Vec<GovernedPromotion>,
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

    /// The one path a staged worktree may occupy for this task:
    /// `<workspace root>/.impulse/worktrees/<task id>`. Defined once here so
    /// the daemon producer that creates it and the ledger that validates it
    /// can never disagree.
    pub fn expected_staged_worktree_root(&self) -> Result<PathBuf, GovernedTaskContractError> {
        validate_path_segment("task_id", self.id.as_str())?;
        Ok(PathBuf::from(&self.workspace_root)
            .join(".impulse")
            .join("worktrees")
            .join(self.id.as_str()))
    }

    pub fn latest_promotion(&self) -> Option<&GovernedPromotion> {
        self.promotions.last()
    }

    /// The staged worktree while it is still usable, or `None` once it has been
    /// discarded or was never materialized.
    pub fn active_staged_worktree(&self) -> Option<&StagedWorktree> {
        self.staged_worktree
            .as_ref()
            .filter(|staged| staged.status == StagedWorktreeStatus::Active)
    }

    /// Working directory a launched runtime must be given. A staged Builder
    /// works inside its disposable worktree; every other scope works in the
    /// canonical workspace root.
    pub fn launch_working_directory(&self) -> &str {
        match self.active_staged_worktree() {
            Some(staged) => staged.root.as_str(),
            None => self.workspace_root.as_str(),
        }
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
    fn test_operator_decision_input_rejects_a_payload_asserting_its_own_provenance() {
        // The daemon stamps provenance; a client payload must not be able to
        // assert it. `OperatorDecisionInput` has no such field and denies
        // unknown ones, so the attempt fails at the boundary.
        let error = serde_json::from_value::<OperatorDecisionInput>(serde_json::json!({
            "actor": {"kind": "operator", "id": "operator-a"},
            "supervisor_verdict_id": "verdict-a",
            "decision": "approve",
            "rationale": "accepted",
            "authentication": "capability_authenticated",
        }))
        .unwrap_err();
        assert!(format!("{error}").contains("authentication"));

        // The legitimate shape still round-trips and carries no provenance.
        let input: OperatorDecisionInput = serde_json::from_value(serde_json::json!({
            "actor": {"kind": "operator", "id": "operator-a"},
            "supervisor_verdict_id": "verdict-a",
            "decision": "approve",
            "rationale": "accepted",
        }))
        .unwrap();
        let encoded = serde_json::to_value(&input).unwrap();
        assert!(encoded.get("authentication").is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(character: char) -> String {
        character.to_string().repeat(40)
    }

    fn test_shared_config_digest() -> SharedRepositoryConfigPin {
        SharedRepositoryConfigPin::Recorded(SharedRepositoryConfigDigest {
            repository_config: format!("sha256:{}", "d".repeat(64)),
            worktree_config: None,
            info_attributes: Some(format!("sha256:{}", "e".repeat(64))),
        })
    }

    fn staged_registration_builder() -> GovernedTaskRegistrationBuilder {
        GovernedTaskRegistration::builder(
            "request-1",
            "task-1",
            "impulse-rs",
            "/tmp/impulse-rs",
            "Ship the staged scope",
            "worker-1",
            "ion",
        )
        .world_scope(WorldScope::StagedAuthoritative)
        .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
        .acceptance_criteria(vec!["the gate is green".to_string()])
        .initial_subject_revision(oid('a'))
        .role_assignment(canonical_governed_builder_assignment())
        .role_compatibility(RoleCompatibility {
            platform: crate::agent_registry::AgentPlatformId::try_new("ion").unwrap(),
            role: canonical_governed_builder_assignment().role.clone(),
            checks: canonical_governed_builder_assignment()
                .requirements
                .iter()
                .map(
                    |requirement| crate::role_assignment::CapabilityCompatibility {
                        capability: requirement.capability.clone(),
                        required: requirement.minimum_enforcement,
                        available: requirement.minimum_enforcement,
                        mandatory: requirement.mandatory,
                    },
                )
                .collect(),
        })
    }

    #[test]
    fn test_world_scope_round_trips_through_serde() {
        for scope in [
            WorldScope::ReadOnlySnapshot,
            WorldScope::DisposableScratch,
            WorldScope::StagedAuthoritative,
            WorldScope::Authoritative,
        ] {
            let json = serde_json::to_string(&scope).unwrap();
            let recovered: WorldScope = serde_json::from_str(&json).unwrap();
            assert_eq!(recovered, scope);
            assert_eq!(json, format!("\"{}\"", scope.as_str()));
        }
    }

    #[test]
    fn test_world_scope_display_matches_wire_names() {
        assert_eq!(
            WorldScope::StagedAuthoritative.to_string(),
            "staged_authoritative"
        );
        assert_eq!(WorldScope::Authoritative.to_string(), "authoritative");
        assert_eq!(
            WorldScope::ReadOnlySnapshot.to_string(),
            "read_only_snapshot"
        );
        assert_eq!(
            WorldScope::DisposableScratch.to_string(),
            "disposable_scratch"
        );
    }

    #[test]
    fn test_world_scope_default_is_authoritative_so_old_records_load() {
        #[derive(Deserialize)]
        struct Legacy {
            #[serde(default)]
            world_scope: WorldScope,
        }

        let legacy: Legacy = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.world_scope, WorldScope::Authoritative);
        assert!(!legacy.world_scope.requires_staged_worktree());
    }

    #[test]
    fn test_world_scope_materializability_is_explicit() {
        assert!(WorldScope::StagedAuthoritative.is_materializable());
        assert!(WorldScope::Authoritative.is_materializable());
        assert!(!WorldScope::ReadOnlySnapshot.is_materializable());
        assert!(!WorldScope::DisposableScratch.is_materializable());
    }

    #[test]
    fn test_staged_worktree_round_trips_through_serde() {
        let staged = StagedWorktree {
            id: GovernedRecordId::try_new("staged-1").unwrap(),
            actor: GovernedActor {
                kind: GovernedActorKind::System,
                id: "impulse-daemon".to_string(),
            },
            root: "/tmp/impulse-rs/.impulse/worktrees/task-1".to_string(),
            initial_subject_revision: oid('a'),
            shared_config_digest: test_shared_config_digest(),
            status: StagedWorktreeStatus::Active,
            materialized_at: "2026-09-02T00:00:00Z".to_string(),
            based_on_revision: 0,
        };
        let recovered: StagedWorktree =
            serde_json::from_str(&serde_json::to_string(&staged).unwrap()).unwrap();
        assert_eq!(recovered, staged);
    }

    #[test]
    fn test_staged_worktree_input_round_trips_through_serde() {
        let input = StagedWorktreeInput {
            actor: GovernedActor {
                kind: GovernedActorKind::System,
                id: "impulse-daemon".to_string(),
            },
            root: "/tmp/impulse-rs/.impulse/worktrees/task-1".to_string(),
            initial_subject_revision: oid('a'),
            shared_config_digest: test_shared_config_digest(),
        };
        let recovered: StagedWorktreeInput =
            serde_json::from_str(&serde_json::to_string(&input).unwrap()).unwrap();
        assert_eq!(recovered, input);
    }

    #[test]
    fn test_promotion_records_round_trip_through_serde() {
        for outcome in [
            GovernedPromotionOutcome::Promoted {
                promoted_revision: oid('b'),
            },
            GovernedPromotionOutcome::PromotionBlocked {
                canonical_head: oid('c'),
                reason: PromotionBlockedReason::CanonicalHeadMoved,
            },
        ] {
            let promotion = GovernedPromotion {
                id: GovernedRecordId::try_new("promotion-1").unwrap(),
                actor: GovernedActor {
                    kind: GovernedActorKind::System,
                    id: "impulse-daemon".to_string(),
                },
                accepted_revision: oid('b'),
                initial_subject_revision: oid('a'),
                outcome: outcome.clone(),
                recorded_at: "2026-09-02T00:00:00Z".to_string(),
                based_on_revision: 6,
            };
            let recovered: GovernedPromotion =
                serde_json::from_str(&serde_json::to_string(&promotion).unwrap()).unwrap();
            assert_eq!(recovered, promotion);
            assert_eq!(outcome.is_promoted(), recovered.outcome.is_promoted());
        }
    }

    #[test]
    fn test_staged_registration_requires_a_verification_profile() {
        let error = GovernedTaskRegistration::builder(
            "request-1",
            "task-1",
            "impulse-rs",
            "/tmp/impulse-rs",
            "Ship the staged scope",
            "worker-1",
            "ion",
        )
        .world_scope(WorldScope::StagedAuthoritative)
        .build()
        .expect_err("a staged scope without an attested OID must not register");
        assert!(matches!(
            error,
            GovernedTaskContractError::InvalidField {
                field: "world_scope",
                ..
            }
        ));
        assert!(error.to_string().contains("verification profile"));
    }

    #[test]
    fn test_unmaterializable_world_scopes_fail_registration_closed() {
        for scope in [WorldScope::ReadOnlySnapshot, WorldScope::DisposableScratch] {
            let error = staged_registration_builder()
                .world_scope(scope)
                .build()
                .expect_err("an unimplemented world scope must not register");
            assert!(error.to_string().contains("not materializable"), "{error}");
        }
    }

    #[test]
    fn test_staged_registration_rejects_a_task_id_that_is_not_a_path_segment() {
        let error = GovernedTaskRegistration::builder(
            "request-1",
            "../escape",
            "impulse-rs",
            "/tmp/impulse-rs",
            "Ship the staged scope",
            "worker-1",
            "ion",
        )
        .world_scope(WorldScope::StagedAuthoritative)
        .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
        .acceptance_criteria(vec!["the gate is green".to_string()])
        .initial_subject_revision(oid('a'))
        .role_assignment(canonical_governed_builder_assignment())
        .build()
        .expect_err("a traversal task id must not become a worktree path");
        assert!(matches!(
            error,
            GovernedTaskContractError::InvalidField {
                field: "task_id",
                ..
            }
        ));
    }

    #[test]
    fn test_staged_registration_accepts_the_canonical_builder_shape() {
        let registration = staged_registration_builder().build().unwrap();
        assert_eq!(registration.world_scope, WorldScope::StagedAuthoritative);
        assert!(registration.world_scope.requires_staged_worktree());
    }

    #[test]
    fn test_validate_path_segment_rejects_traversal_and_separators() {
        for value in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            ".hidden",
            "a b",
            &"x".repeat(129),
        ] {
            assert!(
                validate_path_segment("task_id", value).is_err(),
                "expected `{value}` to be rejected"
            );
        }
        assert!(validate_path_segment("task_id", "task-1_A9").is_ok());
    }

    fn run_with(world_scope: WorldScope, staged: Option<StagedWorktree>) -> GovernedTaskRun {
        GovernedTaskRun {
            id: GovernedTaskId::try_new("task-1").unwrap(),
            revision: 0,
            project_id: "impulse-rs".to_string(),
            workspace_root: "/tmp/impulse-rs".to_string(),
            task: "Ship the staged scope".to_string(),
            acceptance_criteria: Vec::new(),
            approval_policy: ApprovalPolicy::OperatorRequired,
            world_scope,
            verification_profile: None,
            role_assignment: None,
            role_compatibility: None,
            runtime_id: "ion".to_string(),
            agent_id: "worker-1".to_string(),
            session_id: None,
            initial_subject_revision: None,
            staged_worktree: staged,
            promotions: Vec::new(),
            execution_state: GovernedExecutionState::Registered,
            review_state: GovernedReviewState::AwaitingClaim,
            claims: Vec::new(),
            verifications: Vec::new(),
            supervisor_verdicts: Vec::new(),
            operator_decisions: Vec::new(),
            events: Vec::new(),
            created_at: "2026-09-02T00:00:00Z".to_string(),
            updated_at: "2026-09-02T00:00:00Z".to_string(),
        }
    }

    fn staged_record(status: StagedWorktreeStatus) -> StagedWorktree {
        StagedWorktree {
            id: GovernedRecordId::try_new("staged-1").unwrap(),
            actor: GovernedActor {
                kind: GovernedActorKind::System,
                id: "impulse-daemon".to_string(),
            },
            root: "/tmp/impulse-rs/.impulse/worktrees/task-1".to_string(),
            initial_subject_revision: oid('a'),
            shared_config_digest: test_shared_config_digest(),
            status,
            materialized_at: "2026-09-02T00:00:00Z".to_string(),
            based_on_revision: 0,
        }
    }

    #[test]
    fn test_launch_working_directory_prefers_an_active_staged_worktree() {
        let staged = run_with(
            WorldScope::StagedAuthoritative,
            Some(staged_record(StagedWorktreeStatus::Active)),
        );
        assert_eq!(
            staged.launch_working_directory(),
            "/tmp/impulse-rs/.impulse/worktrees/task-1"
        );
    }

    #[test]
    fn test_launch_working_directory_falls_back_once_the_worktree_is_discarded() {
        let discarded = run_with(
            WorldScope::StagedAuthoritative,
            Some(staged_record(StagedWorktreeStatus::Discarded)),
        );
        assert!(discarded.active_staged_worktree().is_none());
        assert_eq!(discarded.launch_working_directory(), "/tmp/impulse-rs");

        let authoritative = run_with(WorldScope::Authoritative, None);
        assert_eq!(authoritative.launch_working_directory(), "/tmp/impulse-rs");
    }

    #[test]
    fn test_expected_staged_worktree_root_is_derived_from_the_record() {
        let task = run_with(WorldScope::StagedAuthoritative, None);
        assert_eq!(
            task.expected_staged_worktree_root().unwrap(),
            PathBuf::from("/tmp/impulse-rs/.impulse/worktrees/task-1")
        );
    }

    #[test]
    fn test_expected_staged_worktree_root_rejects_an_unsafe_task_id() {
        let mut task = run_with(WorldScope::StagedAuthoritative, None);
        task.id = GovernedTaskId::try_new("../escape").unwrap();
        assert!(task.expected_staged_worktree_root().is_err());
    }

    #[test]
    fn test_governed_task_run_round_trips_with_staged_fields() {
        let mut task = run_with(
            WorldScope::StagedAuthoritative,
            Some(staged_record(StagedWorktreeStatus::Active)),
        );
        task.promotions.push(GovernedPromotion {
            id: GovernedRecordId::try_new("promotion-1").unwrap(),
            actor: GovernedActor {
                kind: GovernedActorKind::System,
                id: "impulse-daemon".to_string(),
            },
            accepted_revision: oid('b'),
            initial_subject_revision: oid('a'),
            outcome: GovernedPromotionOutcome::Promoted {
                promoted_revision: oid('b'),
            },
            recorded_at: "2026-09-02T00:01:00Z".to_string(),
            based_on_revision: 0,
        });
        let recovered: GovernedTaskRun =
            serde_json::from_str(&serde_json::to_string(&task).unwrap()).unwrap();
        assert_eq!(recovered, task);
    }

    #[test]
    fn test_governed_task_run_loads_a_record_written_before_adr_0019() {
        let legacy = serde_json::json!({
            "id": "task-legacy",
            "revision": 0,
            "project_id": "impulse-rs",
            "workspace_root": "/tmp/impulse-rs",
            "task": "legacy run",
            "runtime_id": "codex",
            "agent_id": "worker-1",
            "created_at": "2026-07-13T00:00:00Z",
            "updated_at": "2026-07-13T00:00:00Z"
        });
        let task: GovernedTaskRun = serde_json::from_value(legacy).unwrap();
        assert_eq!(task.world_scope, WorldScope::Authoritative);
        assert!(task.staged_worktree.is_none());
        assert!(task.promotions.is_empty());
        assert_eq!(task.launch_working_directory(), "/tmp/impulse-rs");
    }

    #[test]
    fn test_claim_loads_without_a_loop_report_digest() {
        let legacy = serde_json::json!({
            "id": "claim-legacy",
            "actor": {"kind": "worker", "id": "worker-1"},
            "summary": "done",
            "subject_revision": oid('a'),
            "submitted_at": "2026-07-13T00:00:00Z",
            "based_on_revision": 1
        });
        let claim: WorkerCompletionClaim = serde_json::from_value(legacy).unwrap();
        assert!(claim.loop_report_digest.is_none());
    }

    #[test]
    fn test_new_mutations_round_trip_through_serde() {
        let mutations = vec![
            GovernedTaskMutation::MaterializeStagedWorktree {
                staged: StagedWorktreeInput {
                    actor: GovernedActor {
                        kind: GovernedActorKind::System,
                        id: "impulse-daemon".to_string(),
                    },
                    root: "/tmp/impulse-rs/.impulse/worktrees/task-1".to_string(),
                    initial_subject_revision: oid('a'),
                    shared_config_digest: test_shared_config_digest(),
                },
            },
            GovernedTaskMutation::DiscardStagedWorktree {
                actor: GovernedActor {
                    kind: GovernedActorKind::System,
                    id: "impulse-daemon".to_string(),
                },
                reason: "rejected".to_string(),
            },
            GovernedTaskMutation::RecordPromotion {
                promotion: GovernedPromotionInput {
                    actor: GovernedActor {
                        kind: GovernedActorKind::System,
                        id: "impulse-daemon".to_string(),
                    },
                    accepted_revision: oid('b'),
                    initial_subject_revision: oid('a'),
                    outcome: GovernedPromotionOutcome::PromotionBlocked {
                        canonical_head: oid('c'),
                        reason: PromotionBlockedReason::DetachedHead,
                    },
                },
            },
        ];
        for mutation in mutations {
            let recovered: GovernedTaskMutation =
                serde_json::from_str(&serde_json::to_string(&mutation).unwrap()).unwrap();
            assert_eq!(recovered, mutation);
        }
    }

    #[test]
    fn test_new_event_kinds_round_trip_through_serde() {
        for (kind, wire) in [
            (
                GovernedTaskEventKind::StagedWorktreeMaterialized,
                "\"staged_worktree_materialized\"",
            ),
            (
                GovernedTaskEventKind::StagedWorktreeDiscarded,
                "\"staged_worktree_discarded\"",
            ),
            (GovernedTaskEventKind::LoopTripped, "\"loop_tripped\""),
            (
                GovernedTaskEventKind::PromotionRecorded,
                "\"promotion_recorded\"",
            ),
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, wire);
            let recovered: GovernedTaskEventKind = serde_json::from_str(&json).unwrap();
            assert_eq!(recovered, kind);
        }
    }

    #[test]
    fn test_promotion_blocked_reason_round_trips_and_displays() {
        for (reason, wire) in [
            (
                PromotionBlockedReason::CanonicalHeadMoved,
                "canonical_head_moved",
            ),
            (PromotionBlockedReason::DetachedHead, "detached_head"),
            (
                PromotionBlockedReason::ConcurrentBranchUpdate,
                "concurrent_branch_update",
            ),
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            assert_eq!(json, format!("\"{wire}\""));
            assert_eq!(
                serde_json::from_str::<PromotionBlockedReason>(&json).unwrap(),
                reason
            );
            assert_eq!(reason.to_string(), wire);
        }
    }

    #[test]
    fn test_blocked_reason_is_reachable_from_the_outcome() {
        let blocked = GovernedPromotionOutcome::PromotionBlocked {
            canonical_head: oid('c'),
            reason: PromotionBlockedReason::DetachedHead,
        };
        assert_eq!(
            blocked.blocked_reason(),
            Some(PromotionBlockedReason::DetachedHead)
        );
        assert!(!blocked.is_promoted());

        let promoted = GovernedPromotionOutcome::Promoted {
            promoted_revision: oid('b'),
        };
        assert_eq!(promoted.blocked_reason(), None);
        assert!(promoted.is_promoted());
    }

    #[test]
    fn test_claim_loads_without_loop_evidence_version() {
        let legacy = serde_json::json!({
            "id": "claim-legacy",
            "actor": {"kind": "worker", "id": "worker-1"},
            "summary": "done",
            "subject_revision": oid('a'),
            "loop_report_digest": format!("sha256:{}", "a".repeat(64)),
            "submitted_at": "2026-07-13T00:00:00Z",
            "based_on_revision": 1
        });
        let claim: WorkerCompletionClaim = serde_json::from_value(legacy).unwrap();
        assert!(claim.loop_report_digest.is_some());
        assert_eq!(claim.loop_report_version, None);
    }

    #[test]
    fn test_shared_config_pin_round_trips_and_defaults_to_unknown() {
        let recorded = test_shared_config_digest();
        let json = serde_json::to_string(&recorded).unwrap();
        assert_eq!(
            serde_json::from_str::<SharedRepositoryConfigPin>(&json).unwrap(),
            recorded
        );
        assert!(recorded.recorded().is_some());
        assert!(!recorded.is_unknown());

        let unknown = SharedRepositoryConfigPin::Unknown;
        assert_eq!(serde_json::to_string(&unknown).unwrap(), "\"unknown\"");
        assert_eq!(
            serde_json::from_str::<SharedRepositoryConfigPin>("\"unknown\"").unwrap(),
            unknown
        );
        assert_eq!(SharedRepositoryConfigPin::default(), unknown);
        assert!(unknown.recorded().is_none());
        assert!(unknown.is_unknown());
    }

    /// The whole point of the pin being an enum: a worktree recorded before it
    /// existed must still deserialize, or one stale record fails the ledger.
    #[test]
    fn test_staged_worktree_loads_without_a_shared_config_pin() {
        let legacy = serde_json::json!({
            "id": "staged-1",
            "actor": {"kind": "system", "id": "impulse-daemon"},
            "root": "/tmp/impulse-rs/.impulse/worktrees/task-1",
            "initial_subject_revision": oid('a'),
            "status": "active",
            "materialized_at": "2026-09-02T00:00:00Z",
            "based_on_revision": 0
        });
        let staged: StagedWorktree = serde_json::from_value(legacy).unwrap();
        assert_eq!(
            staged.shared_config_digest,
            SharedRepositoryConfigPin::Unknown
        );
        assert_eq!(staged.status, StagedWorktreeStatus::Active);
    }

    #[test]
    fn test_staged_worktree_input_loads_without_a_shared_config_pin() {
        let legacy = serde_json::json!({
            "actor": {"kind": "system", "id": "impulse-daemon"},
            "root": "/tmp/impulse-rs/.impulse/worktrees/task-1",
            "initial_subject_revision": oid('a')
        });
        let input: StagedWorktreeInput = serde_json::from_value(legacy).unwrap();
        assert!(input.shared_config_digest.is_unknown());
    }

    #[test]
    fn test_unpinned_blocked_reason_round_trips() {
        let reason = PromotionBlockedReason::RepositoryConfigUnpinned;
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, "\"repository_config_unpinned\"");
        assert_eq!(
            serde_json::from_str::<PromotionBlockedReason>(&json).unwrap(),
            reason
        );
        assert_eq!(reason.to_string(), "repository_config_unpinned");
    }
}
