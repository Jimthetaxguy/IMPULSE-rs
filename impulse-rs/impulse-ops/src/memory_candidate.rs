//! Review-only memory candidates derived from accepted governed runs.
//!
//! A candidate is a semantic proposal backed by episodic governed-task
//! evidence. It is deliberately not curated project memory: this contract has
//! no promotion action and carries no worker summary or Supervisor rationale.

use chrono::DateTime;
use serde::de::Error as _;
use serde::{Deserialize, Serialize};

use crate::governed_task::{
    GovernedActor, GovernedRecordId, GovernedRequestId, GovernedTaskId, GovernedVerificationProfile,
};

pub const ACCEPTED_RUN_MEMORY_CANDIDATE_SCHEMA_VERSION: u32 = 1;
/// Bumped to 2 by ADR-0018: the derivation now reads the operator decision's
/// connection provenance, so the same governed task can derive a different
/// `source_assurance` — and therefore a different `source_digest` and id —
/// than it did under version 1. Candidates persisted at an older derivation
/// version are pruned and re-derived from governed-task truth on load.
pub const ACCEPTED_RUN_MEMORY_DERIVATION_VERSION: u32 = 2;

const MAX_OPEN_ID_BYTES: usize = 256;
const MAX_PROJECT_TEXT_BYTES: usize = 16 * 1024;
const MAX_TASK_BYTES: usize = 8 * 1024;
const MAX_CRITERIA: usize = 64;
const MAX_CRITERION_BYTES: usize = 16 * 1024;
const MAX_PROPOSED_SUMMARY_BYTES: usize = 12 * 1024;
const MAX_ARTIFACT_REFERENCES: usize = 64;
const MAX_ARTIFACT_REFERENCE_BYTES: usize = 4 * 1024;
const MAX_COMMANDS: usize = 64;
const MAX_DISMISSAL_REASON_BYTES: usize = 4 * 1024;
const MAX_RECORD_TITLE_BYTES: usize = 4 * 1024;
const MAX_RECORD_BODY_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MemoryCandidateContractError {
    #[error("invalid memory candidate id `{0}`: ids must be nonempty and contain no whitespace or control characters")]
    InvalidId(String),
    #[error("invalid memory candidate field `{field}`: {message}")]
    InvalidField {
        field: &'static str,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct MemoryCandidateId(String);

impl MemoryCandidateId {
    pub fn try_new(value: impl Into<String>) -> Result<Self, MemoryCandidateContractError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_OPEN_ID_BYTES
            || value
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err(MemoryCandidateContractError::InvalidId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MemoryCandidateId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MemoryCandidateId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_new(value).map_err(D::Error::custom)
    }
}

/// Review lifecycle of one candidate (ADR-0020).
///
/// `PendingReview` is the serde default so a ledger written before ADR-0020 —
/// where the field was a single-variant enum and every record carried
/// `"pending_review"` — loads unchanged. The two decided variants are struct
/// variants, so they serialize as `{"promoted": {..}}` / `{"dismissed": {..}}`
/// and can never be confused with the older unit encoding.
///
/// A decided status is terminal: it is carried forward across a
/// derivation-version migration rather than reset (ADR-0018 follow-up 2), and
/// a second differing decision on the same candidate is refused.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateStatus {
    #[default]
    PendingReview,
    Promoted {
        record_id: MemoryRecordId,
        decided_at: String,
        decided_by: GovernedActor,
    },
    Dismissed {
        reason: String,
        decided_at: String,
        decided_by: GovernedActor,
    },
}

impl MemoryCandidateStatus {
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::PendingReview)
    }

    pub fn is_promoted(&self) -> bool {
        matches!(self, Self::Promoted { .. })
    }

    /// The promoted record this candidate produced, if any.
    pub fn promoted_record_id(&self) -> Option<&MemoryRecordId> {
        match self {
            Self::Promoted { record_id, .. } => Some(record_id),
            _ => None,
        }
    }

    /// Stable label for the review state, for operator surfaces and logs.
    pub fn label(&self) -> &'static str {
        match self {
            Self::PendingReview => "pending_review",
            Self::Promoted { .. } => "promoted",
            Self::Dismissed { .. } => "dismissed",
        }
    }

    fn validate_shape(&self) -> Result<(), MemoryCandidateContractError> {
        match self {
            Self::PendingReview => Ok(()),
            Self::Promoted {
                record_id,
                decided_at,
                decided_by,
            } => {
                validate_record_id(record_id)?;
                validate_decided_at(decided_at)?;
                validate_actor(decided_by)
            }
            Self::Dismissed {
                reason,
                decided_at,
                decided_by,
            } => {
                // A dismissal is the one review outcome that leaves no durable
                // record behind, so its reason is the only surviving account of
                // why an accepted run was refused. It is required to be real.
                validate_text("status.reason", reason, MAX_DISMISSAL_REASON_BYTES)?;
                validate_decided_at(decided_at)?;
                validate_actor(decided_by)
            }
        }
    }
}

fn validate_decided_at(value: &str) -> Result<(), MemoryCandidateContractError> {
    DateTime::parse_from_rfc3339(value)
        .map(|_| ())
        .map_err(|_| MemoryCandidateContractError::InvalidField {
            field: "status.decided_at",
            message: "must be a valid RFC 3339 timestamp".to_string(),
        })
}

fn validate_actor(actor: &GovernedActor) -> Result<(), MemoryCandidateContractError> {
    validate_text("status.decided_by", &actor.id, MAX_OPEN_ID_BYTES)
}

/// Describes what Impulse can honestly attest about the source chain.
///
/// `Declared` variants mean operator identity was asserted inside the same-user
/// socket trust boundary with no connection-level proof. The `Authenticated`
/// variant (ADR-0018) additionally means the approving connection presented
/// this daemon run's operator capability and its peer uid matched the daemon's
/// own; it still does not imply cryptographic *human* authentication, and it
/// does not defend against a same-uid process that deliberately reads the
/// capability file.
///
/// There is deliberately no authenticated variant for caller-composed
/// evidence: when the evidence chain itself was composed by a client, the
/// weaker half of the chain sets the assurance label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptedRunSourceAssurance {
    DaemonProfiledEvidenceDeclaredOperator,
    DaemonProfiledEvidenceAuthenticatedOperator,
    CallerComposedEvidenceDeclaredOperator,
}

impl AcceptedRunSourceAssurance {
    /// True when the evidence chain was produced by daemon-owned profiled
    /// producers rather than composed by the caller.
    pub fn is_daemon_profiled(self) -> bool {
        matches!(
            self,
            Self::DaemonProfiledEvidenceDeclaredOperator
                | Self::DaemonProfiledEvidenceAuthenticatedOperator
        )
    }

    /// True when the approving connection proved operator class (ADR-0018).
    pub fn is_authenticated_operator(self) -> bool {
        matches!(self, Self::DaemonProfiledEvidenceAuthenticatedOperator)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedRunCommandEvidence {
    pub name: String,
    pub command_digest: String,
    pub output_digest: String,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub output_bytes: u64,
    #[serde(default)]
    pub output_truncated: bool,
}

/// Insert-only v1 review proposal derived from one accepted governed run.
///
/// The proposed text is restricted to registration-time task/criteria plus a
/// daemon-generated evidence statement. Worker claim prose and Supervisor or
/// operator rationale remain only in the referenced governed task record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedRunMemoryCandidate {
    pub id: MemoryCandidateId,
    pub schema_version: u32,
    pub derivation_version: u32,
    /// Serde-defaulted so a pre-ADR-0020 ledger, and any record written without
    /// the field, loads as `PendingReview` instead of failing the daemon.
    #[serde(default)]
    pub status: MemoryCandidateStatus,
    pub project_id: String,
    pub workspace_root: String,
    pub governed_task_id: GovernedTaskId,
    pub accepted_task_revision: u64,
    pub task: String,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    pub proposed_summary: String,
    pub runtime_id: String,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_profile: Option<GovernedVerificationProfile>,
    pub verification_policy: String,
    pub subject_revision: String,
    pub claim_id: GovernedRecordId,
    pub verification_id: GovernedRecordId,
    pub supervisor_verdict_id: GovernedRecordId,
    pub operator_decision_id: GovernedRecordId,
    #[serde(default)]
    pub claimed_artifact_ids: Vec<String>,
    #[serde(default)]
    pub verification_artifact_ids: Vec<String>,
    #[serde(default)]
    pub commands: Vec<AcceptedRunCommandEvidence>,
    pub source_assurance: AcceptedRunSourceAssurance,
    pub source_digest: String,
    pub staged_at: String,
}

impl AcceptedRunMemoryCandidate {
    pub fn validate_shape(&self) -> Result<(), MemoryCandidateContractError> {
        if self.schema_version != ACCEPTED_RUN_MEMORY_CANDIDATE_SCHEMA_VERSION {
            return invalid(
                "schema_version",
                format!("must equal {ACCEPTED_RUN_MEMORY_CANDIDATE_SCHEMA_VERSION}"),
            );
        }
        if self.derivation_version != ACCEPTED_RUN_MEMORY_DERIVATION_VERSION {
            return invalid(
                "derivation_version",
                format!("must equal {ACCEPTED_RUN_MEMORY_DERIVATION_VERSION}"),
            );
        }
        validate_candidate_id(&self.id)?;
        self.status.validate_shape()?;
        validate_text("project_id", &self.project_id, MAX_OPEN_ID_BYTES)?;
        validate_text(
            "workspace_root",
            &self.workspace_root,
            MAX_PROJECT_TEXT_BYTES,
        )?;
        validate_text("task", &self.task, MAX_TASK_BYTES)?;
        if self.acceptance_criteria.len() > MAX_CRITERIA
            || (self.acceptance_criteria.is_empty() && self.source_assurance.is_daemon_profiled())
        {
            return invalid(
                "acceptance_criteria",
                format!(
                    "must contain at most {MAX_CRITERIA} entries and cannot be empty for daemon-profiled evidence"
                ),
            );
        }
        for criterion in &self.acceptance_criteria {
            validate_text("acceptance_criteria", criterion, MAX_CRITERION_BYTES)?;
        }
        validate_text(
            "proposed_summary",
            &self.proposed_summary,
            MAX_PROPOSED_SUMMARY_BYTES,
        )?;
        validate_text("runtime_id", &self.runtime_id, MAX_OPEN_ID_BYTES)?;
        validate_text("agent_id", &self.agent_id, MAX_OPEN_ID_BYTES)?;
        if let Some(session_id) = &self.session_id {
            validate_text("session_id", session_id, MAX_OPEN_ID_BYTES)?;
        }
        validate_text(
            "verification_policy",
            &self.verification_policy,
            MAX_ARTIFACT_REFERENCE_BYTES,
        )?;
        if self.source_assurance.is_daemon_profiled() {
            validate_git_oid("subject_revision", &self.subject_revision)?;
        } else {
            validate_text(
                "subject_revision",
                &self.subject_revision,
                MAX_ARTIFACT_REFERENCE_BYTES,
            )?;
        }
        validate_references("claimed_artifact_ids", &self.claimed_artifact_ids)?;
        validate_references("verification_artifact_ids", &self.verification_artifact_ids)?;
        if self.commands.is_empty() || self.commands.len() > MAX_COMMANDS {
            return invalid(
                "commands",
                format!("must contain between 1 and {MAX_COMMANDS} entries"),
            );
        }
        for command in &self.commands {
            validate_text("command.name", &command.name, MAX_ARTIFACT_REFERENCE_BYTES)?;
            validate_sha256_reference("command.command_digest", &command.command_digest)?;
            validate_sha256_reference("command.output_digest", &command.output_digest)?;
            if !command.success || command.exit_code != Some(0) {
                return invalid(
                    "commands",
                    "accepted-run candidates require successful zero-exit command evidence",
                );
            }
        }
        validate_lower_sha256("source_digest", &self.source_digest, "sha256-v1:")?;
        DateTime::parse_from_rfc3339(&self.staged_at).map_err(|_| {
            MemoryCandidateContractError::InvalidField {
                field: "staged_at",
                message: "must be a valid RFC 3339 timestamp".to_string(),
            }
        })?;
        Ok(())
    }
}

fn validate_candidate_id(id: &MemoryCandidateId) -> Result<(), MemoryCandidateContractError> {
    let Some(hex) = id.as_str().strip_prefix("memory-candidate-") else {
        return invalid("id", "must use the memory-candidate-<sha256> format");
    };
    if hex.len() != 64 || !is_lower_hex(hex) {
        return invalid("id", "must end with 64 lowercase hexadecimal characters");
    }
    Ok(())
}

fn validate_references(
    field: &'static str,
    values: &[String],
) -> Result<(), MemoryCandidateContractError> {
    if values.len() > MAX_ARTIFACT_REFERENCES {
        return invalid(
            field,
            format!("must contain at most {MAX_ARTIFACT_REFERENCES} entries"),
        );
    }
    for value in values {
        validate_text(field, value, MAX_ARTIFACT_REFERENCE_BYTES)?;
    }
    Ok(())
}

fn validate_text(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), MemoryCandidateContractError> {
    if value.trim().is_empty() || value.contains('\0') || value.len() > max_bytes {
        return invalid(
            field,
            format!("must be nonblank, NUL-free, and at most {max_bytes} bytes"),
        );
    }
    Ok(())
}

fn validate_git_oid(field: &'static str, value: &str) -> Result<(), MemoryCandidateContractError> {
    if !matches!(value.len(), 40 | 64) || !is_lower_hex(value) {
        return invalid(
            field,
            "must be a 40- or 64-character lowercase hexadecimal Git commit OID",
        );
    }
    Ok(())
}

fn validate_sha256_reference(
    field: &'static str,
    value: &str,
) -> Result<(), MemoryCandidateContractError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return invalid(field, "must use the sha256:<hex> format");
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return invalid(field, "must contain 64 hexadecimal characters");
    }
    Ok(())
}

fn validate_lower_sha256(
    field: &'static str,
    value: &str,
    prefix: &'static str,
) -> Result<(), MemoryCandidateContractError> {
    let Some(hex) = value.strip_prefix(prefix) else {
        return invalid(field, format!("must use the {prefix}<hex> format"));
    };
    if hex.len() != 64 || !is_lower_hex(hex) {
        return invalid(field, "must contain 64 lowercase hexadecimal characters");
    }
    Ok(())
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn invalid<T>(
    field: &'static str,
    message: impl Into<String>,
) -> Result<T, MemoryCandidateContractError> {
    Err(MemoryCandidateContractError::InvalidField {
        field,
        message: message.into(),
    })
}

pub const MEMORY_RECORD_SCHEMA_VERSION: u32 = 1;

/// Genesis link of the `MEMORY.jsonl` digest chain.
///
/// The first entry's `previous_digest` is this constant rather than an empty
/// string, so "no previous entry" is a value the chain verifier checks rather
/// than a case it skips.
pub const MEMORY_LOG_GENESIS_DIGEST: &str = "sha256-memlog-v1:genesis";

/// Prefix of a memory record's content digest.
pub const MEMORY_RECORD_DIGEST_PREFIX: &str = "sha256-mem-v1:";
/// Prefix of a memory record id; the remainder is the same hex as the digest.
pub const MEMORY_RECORD_ID_PREFIX: &str = "memory-record-";
/// Prefix of a chain-link digest in `MEMORY.jsonl`.
pub const MEMORY_LOG_DIGEST_PREFIX: &str = "sha256-memlog-v1:";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct MemoryRecordId(String);

impl MemoryRecordId {
    pub fn try_new(value: impl Into<String>) -> Result<Self, MemoryCandidateContractError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_OPEN_ID_BYTES
            || value
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err(MemoryCandidateContractError::InvalidId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MemoryRecordId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MemoryRecordId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_new(value).map_err(D::Error::custom)
    }
}

/// Blast radius of a promoted record.
///
/// Only [`MemoryScope::Project`] is written in this stage. The other two are
/// declared, refused at the write boundary, and exist so a later stage widens
/// an existing field instead of migrating every persisted record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    #[default]
    Project,
    Workspace,
    Global,
}

impl MemoryScope {
    /// True for the one scope this stage is allowed to write.
    pub fn is_writable_now(self) -> bool {
        matches!(self, Self::Project)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Workspace => "workspace",
            Self::Global => "global",
        }
    }
}

/// What kind of durable fact a record carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    /// Promoted from an accepted governed run's review candidate.
    AcceptedRunOutcome,
    /// Written directly by an operator, with no candidate behind it. Declared
    /// for the contract's sake; no producer mints it in this stage.
    OperatorNote,
}

impl MemoryKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::AcceptedRunOutcome => "accepted_run_outcome",
            Self::OperatorNote => "operator_note",
        }
    }
}

/// Where a record came from. `CandidateRef` pins the exact candidate *and* its
/// `source_digest`, so a record can always be traced back to the accepted run
/// it was derived from and a re-derived candidate is detectably not the same
/// source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum MemorySource {
    CandidateRef {
        candidate_id: MemoryCandidateId,
        candidate_source_digest: String,
        governed_task_id: GovernedTaskId,
    },
    OperatorManual {
        note: String,
    },
}

/// One durable, curated memory fact.
///
/// `digest` covers identity and content only — deliberately not `valid_from`.
/// That keeps the digest (and therefore `id`) replay-stable: the same decision
/// request replayed after a crash re-derives the same record id even though the
/// wall clock moved. `valid_from` is still tamper-evident, because the log
/// entry digest covers the whole serialized line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: MemoryRecordId,
    pub schema_version: u32,
    pub kind: MemoryKind,
    pub scope: MemoryScope,
    pub project_id: String,
    pub source: MemorySource,
    /// Decision request that minted the record; part of the identity digest,
    /// so a replay is idempotent and two distinct decisions never collide.
    pub request_id: GovernedRequestId,
    /// Candidate-ledger revision the decision was taken against.
    pub based_on_ledger_revision: u64,
    pub title: String,
    pub body: String,
    pub valid_from: String,
    /// Reserved for a later supersession entry kind; always `None` in this
    /// stage, because the log has no supersede operation yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<MemoryRecordId>,
    pub digest: String,
}

/// Fixed, ordered digest source for a memory record. No maps, no floats, no
/// Unicode normalization — the same discipline as `CandidateSourceV1`.
#[derive(Serialize)]
struct MemoryRecordSourceV1<'a> {
    schema_version: u32,
    kind: MemoryKind,
    scope: MemoryScope,
    project_id: &'a str,
    source: &'a MemorySource,
    request_id: &'a GovernedRequestId,
    based_on_ledger_revision: u64,
    title: &'a str,
    body: &'a str,
}

impl MemoryRecord {
    /// Canonical digest-source bytes for a record's identity and content.
    ///
    /// The hash function deliberately lives outside this crate: `impulse-ops`
    /// carries no cryptographic dependency, so it defines *what* is hashed and
    /// the state layer defines *how*. Callers hash these bytes with SHA-256 and
    /// build both the `sha256-mem-v1:<hex>` digest and the
    /// `memory-record-<hex>` id from the one resulting hex string, so the two
    /// can never drift apart.
    #[allow(clippy::too_many_arguments)] // one argument per digest-source field; a params struct would just restate MemoryRecordSourceV1
    pub fn identity_source_bytes(
        kind: MemoryKind,
        scope: MemoryScope,
        project_id: &str,
        source: &MemorySource,
        request_id: &GovernedRequestId,
        based_on_ledger_revision: u64,
        title: &str,
        body: &str,
    ) -> Result<Vec<u8>, MemoryCandidateContractError> {
        let digest_source = MemoryRecordSourceV1 {
            schema_version: MEMORY_RECORD_SCHEMA_VERSION,
            kind,
            scope,
            project_id,
            source,
            request_id,
            based_on_ledger_revision,
            title,
            body,
        };
        serde_json::to_vec(&digest_source).map_err(|error| {
            MemoryCandidateContractError::InvalidField {
                field: "digest",
                message: format!("failed to serialize memory record digest source: {error}"),
            }
        })
    }

    /// Canonical digest-source bytes recomputed from this record's own fields.
    pub fn recompute_identity_source_bytes(&self) -> Result<Vec<u8>, MemoryCandidateContractError> {
        Self::identity_source_bytes(
            self.kind,
            self.scope,
            &self.project_id,
            &self.source,
            &self.request_id,
            self.based_on_ledger_revision,
            &self.title,
            &self.body,
        )
    }

    /// The shared hex that both `id` and `digest` are built from.
    pub fn identity_hex(&self) -> Option<&str> {
        self.id.as_str().strip_prefix(MEMORY_RECORD_ID_PREFIX)
    }

    pub fn validate_shape(&self) -> Result<(), MemoryCandidateContractError> {
        if self.schema_version != MEMORY_RECORD_SCHEMA_VERSION {
            return invalid(
                "schema_version",
                format!("must equal {MEMORY_RECORD_SCHEMA_VERSION}"),
            );
        }
        validate_record_id(&self.id)?;
        if !self.scope.is_writable_now() {
            return invalid(
                "scope",
                "only the project scope is writable in this stage; workspace and global are declared but refused",
            );
        }
        validate_text("project_id", &self.project_id, MAX_OPEN_ID_BYTES)?;
        match &self.source {
            MemorySource::CandidateRef {
                candidate_id,
                candidate_source_digest,
                ..
            } => {
                validate_candidate_id(candidate_id)?;
                validate_lower_sha256(
                    "source.candidate_source_digest",
                    candidate_source_digest,
                    "sha256-v1:",
                )?;
                if self.kind != MemoryKind::AcceptedRunOutcome {
                    return invalid(
                        "kind",
                        "a candidate-backed record must be an accepted_run_outcome",
                    );
                }
            }
            MemorySource::OperatorManual { note } => {
                validate_text("source.note", note, MAX_RECORD_BODY_BYTES)?;
                if self.kind != MemoryKind::OperatorNote {
                    return invalid("kind", "an operator-manual record must be an operator_note");
                }
            }
        }
        validate_text("title", &self.title, MAX_RECORD_TITLE_BYTES)?;
        validate_text("body", &self.body, MAX_RECORD_BODY_BYTES)?;
        DateTime::parse_from_rfc3339(&self.valid_from).map_err(|_| {
            MemoryCandidateContractError::InvalidField {
                field: "valid_from",
                message: "must be a valid RFC 3339 timestamp".to_string(),
            }
        })?;
        if let Some(superseded_by) = &self.superseded_by {
            validate_record_id(superseded_by)?;
        }
        validate_lower_sha256("digest", &self.digest, MEMORY_RECORD_DIGEST_PREFIX)?;
        // Structural coherence is enforceable without hashing: the id and the
        // digest must be two spellings of the same hex. Whether that hex is the
        // real SHA-256 of `identity_source_bytes` is checked by the state layer,
        // which owns the hash function.
        let id_hex =
            self.identity_hex()
                .ok_or_else(|| MemoryCandidateContractError::InvalidField {
                    field: "id",
                    message: "must use the memory-record-<sha256> format".to_string(),
                })?;
        if self.digest.strip_prefix(MEMORY_RECORD_DIGEST_PREFIX) != Some(id_hex) {
            return invalid(
                "digest",
                "record id and digest must be built from the same hex",
            );
        }
        Ok(())
    }
}

/// One line of the append-only `MEMORY.jsonl` log.
///
/// `entry_digest` covers `{seq, previous_digest, record}` serialized through a
/// fixed, ordered struct. Each entry carries the previous entry's digest, so
/// the file is a hash chain: editing any byte of any record, reordering two
/// lines, or cutting the file mid-chain breaks the next link and the load path
/// refuses the file rather than silently serving a shortened history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLogEntry {
    pub seq: u64,
    pub previous_digest: String,
    pub record: MemoryRecord,
    pub entry_digest: String,
}

#[derive(Serialize)]
struct MemoryLogEntrySourceV1<'a> {
    seq: u64,
    previous_digest: &'a str,
    record: &'a MemoryRecord,
}

impl MemoryLogEntry {
    /// Canonical digest-source bytes for one chain link.
    pub fn digest_source_bytes(
        seq: u64,
        previous_digest: &str,
        record: &MemoryRecord,
    ) -> Result<Vec<u8>, MemoryCandidateContractError> {
        let source = MemoryLogEntrySourceV1 {
            seq,
            previous_digest,
            record,
        };
        serde_json::to_vec(&source).map_err(|error| MemoryCandidateContractError::InvalidField {
            field: "entry_digest",
            message: format!("failed to serialize memory log entry source: {error}"),
        })
    }

    /// Canonical digest-source bytes recomputed from this entry's own fields.
    pub fn recompute_digest_source_bytes(&self) -> Result<Vec<u8>, MemoryCandidateContractError> {
        Self::digest_source_bytes(self.seq, &self.previous_digest, &self.record)
    }

    /// Verify everything about this entry that does not require hashing: its
    /// position in the chain, the link to its predecessor, its digest format,
    /// and the record it carries. The state layer additionally recomputes the
    /// digest over [`Self::recompute_digest_source_bytes`].
    pub fn verify_links(
        &self,
        expected_seq: u64,
        expected_previous_digest: &str,
    ) -> Result<(), MemoryCandidateContractError> {
        if self.seq != expected_seq {
            return invalid(
                "seq",
                format!(
                    "memory log entry is out of order: expected seq {expected_seq}, found {}",
                    self.seq
                ),
            );
        }
        if self.previous_digest != expected_previous_digest {
            return invalid(
                "previous_digest",
                format!("memory log entry {expected_seq} does not chain to its predecessor"),
            );
        }
        validate_lower_sha256("entry_digest", &self.entry_digest, MEMORY_LOG_DIGEST_PREFIX)?;
        self.record.validate_shape()
    }
}

fn validate_record_id(id: &MemoryRecordId) -> Result<(), MemoryCandidateContractError> {
    let Some(hex) = id.as_str().strip_prefix(MEMORY_RECORD_ID_PREFIX) else {
        return invalid("record_id", "must use the memory-record-<sha256> format");
    };
    if hex.len() != 64 || !is_lower_hex(hex) {
        return invalid(
            "record_id",
            "must end with 64 lowercase hexadecimal characters",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(prefix: &str, character: char) -> String {
        format!("{prefix}{}", character.to_string().repeat(64))
    }

    fn candidate() -> AcceptedRunMemoryCandidate {
        AcceptedRunMemoryCandidate {
            id: MemoryCandidateId::try_new(format!(
                "memory-candidate-{}",
                "a".repeat(64)
            ))
            .unwrap(),
            schema_version: ACCEPTED_RUN_MEMORY_CANDIDATE_SCHEMA_VERSION,
            derivation_version: ACCEPTED_RUN_MEMORY_DERIVATION_VERSION,
            status: MemoryCandidateStatus::PendingReview,
            project_id: "project-a".to_string(),
            workspace_root: "/tmp/project-a".to_string(),
            governed_task_id: GovernedTaskId::try_new("task-a").unwrap(),
            accepted_task_revision: 4,
            task: "Add accepted-run memory candidates".to_string(),
            acceptance_criteria: vec!["Candidate remains review-only".to_string()],
            proposed_summary: "Accepted governed outcome for task: Add accepted-run memory candidates. Daemon-profiled evidence passed; pending semantic-memory review.".to_string(),
            runtime_id: "codex".to_string(),
            agent_id: "builder-a".to_string(),
            session_id: Some("session-a".to_string()),
            verification_profile: Some(GovernedVerificationProfile::RustWorkspaceV1),
            verification_policy: "rust_workspace_v1".to_string(),
            subject_revision: "a".repeat(40),
            claim_id: GovernedRecordId::try_new("claim-a").unwrap(),
            verification_id: GovernedRecordId::try_new("verification-a").unwrap(),
            supervisor_verdict_id: GovernedRecordId::try_new("verdict-a").unwrap(),
            operator_decision_id: GovernedRecordId::try_new("decision-a").unwrap(),
            claimed_artifact_ids: vec!["artifact-a".to_string()],
            verification_artifact_ids: vec!["verification-artifact-a".to_string()],
            commands: vec![AcceptedRunCommandEvidence {
                name: "test".to_string(),
                command_digest: digest("sha256:", 'b'),
                output_digest: digest("sha256:", 'c'),
                exit_code: Some(0),
                success: true,
                output_bytes: 42,
                output_truncated: false,
            }],
            source_assurance: AcceptedRunSourceAssurance::DaemonProfiledEvidenceDeclaredOperator,
            source_digest: digest("sha256-v1:", 'd'),
            staged_at: "2026-07-15T22:00:00Z".to_string(),
        }
    }

    #[test]
    fn valid_candidate_round_trips() {
        let candidate = candidate();
        candidate.validate_shape().unwrap();
        let json = serde_json::to_string(&candidate).unwrap();
        let decoded: AcceptedRunMemoryCandidate = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, candidate);
    }

    #[test]
    fn candidate_contract_has_no_worker_or_supervisor_semantic_text_fields() {
        let value = serde_json::to_value(candidate()).unwrap();
        let object = value.as_object().unwrap();
        assert!(!object.contains_key("worker_claim_summary"));
        assert!(!object.contains_key("supervisor_rationale"));
        assert!(!object.contains_key("operator_rationale"));
    }

    #[test]
    fn rejected_command_evidence_cannot_shape_an_accepted_candidate() {
        let mut candidate = candidate();
        candidate.commands[0].success = false;
        assert!(candidate.validate_shape().is_err());
    }

    #[test]
    fn authenticated_operator_assurance_round_trips_and_keeps_profiled_rules() {
        let mut candidate = candidate();
        candidate.source_assurance =
            AcceptedRunSourceAssurance::DaemonProfiledEvidenceAuthenticatedOperator;
        candidate.validate_shape().unwrap();

        let json = serde_json::to_string(&candidate).unwrap();
        assert!(json.contains("daemon_profiled_evidence_authenticated_operator"));
        let decoded: AcceptedRunMemoryCandidate = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, candidate);
        assert!(decoded.source_assurance.is_daemon_profiled());
        assert!(decoded.source_assurance.is_authenticated_operator());

        // Daemon-profiled rules still apply to the authenticated variant.
        let mut without_criteria = candidate.clone();
        without_criteria.acceptance_criteria.clear();
        assert!(without_criteria.validate_shape().is_err());

        let mut without_git_oid = candidate;
        without_git_oid.subject_revision = "not-a-git-oid".to_string();
        assert!(without_git_oid.validate_shape().is_err());
    }

    #[test]
    fn declared_assurances_are_not_reported_as_authenticated() {
        for assurance in [
            AcceptedRunSourceAssurance::DaemonProfiledEvidenceDeclaredOperator,
            AcceptedRunSourceAssurance::CallerComposedEvidenceDeclaredOperator,
        ] {
            assert!(!assurance.is_authenticated_operator());
        }
        assert!(
            !AcceptedRunSourceAssurance::CallerComposedEvidenceDeclaredOperator
                .is_daemon_profiled()
        );
    }

    #[test]
    fn candidate_at_a_superseded_derivation_version_is_rejected() {
        let mut candidate = candidate();
        candidate.derivation_version = ACCEPTED_RUN_MEMORY_DERIVATION_VERSION - 1;
        let error = candidate.validate_shape().unwrap_err();
        assert!(format!("{error}").contains("derivation_version"));
    }

    #[test]
    fn command_references_accept_governed_uppercase_hex_without_weakening_source_digest() {
        let mut candidate = candidate();
        candidate.commands[0].command_digest = digest("sha256:", 'A');
        candidate.commands[0].output_digest = digest("sha256:", 'B');
        candidate.validate_shape().unwrap();

        candidate.source_digest = digest("sha256-v1:", 'C');
        assert!(candidate.validate_shape().is_err());
    }
    fn actor() -> GovernedActor {
        GovernedActor {
            kind: crate::governed_task::GovernedActorKind::Operator,
            id: "operator-a".to_string(),
        }
    }

    fn record_id(character: char) -> MemoryRecordId {
        MemoryRecordId::try_new(format!(
            "memory-record-{}",
            character.to_string().repeat(64)
        ))
        .unwrap()
    }

    /// Build a shape-valid record whose id and digest share the same hex. The
    /// hex is not a real SHA-256 here; only the state layer, which owns the
    /// hash function, checks that.
    fn record(character: char) -> MemoryRecord {
        let hex = character.to_string().repeat(64);
        MemoryRecord {
            id: MemoryRecordId::try_new(format!("memory-record-{hex}")).unwrap(),
            schema_version: MEMORY_RECORD_SCHEMA_VERSION,
            kind: MemoryKind::AcceptedRunOutcome,
            scope: MemoryScope::Project,
            project_id: "project-a".to_string(),
            source: MemorySource::CandidateRef {
                candidate_id: MemoryCandidateId::try_new(format!(
                    "memory-candidate-{}",
                    "a".repeat(64)
                ))
                .unwrap(),
                candidate_source_digest: digest("sha256-v1:", 'd'),
                governed_task_id: GovernedTaskId::try_new("task-a").unwrap(),
            },
            request_id: GovernedRequestId::try_new("request-a").unwrap(),
            based_on_ledger_revision: 3,
            title: "Add accepted-run memory candidates".to_string(),
            body: "Accepted governed outcome.".to_string(),
            valid_from: "2026-09-12T10:00:00Z".to_string(),
            superseded_by: None,
            digest: format!("{MEMORY_RECORD_DIGEST_PREFIX}{hex}"),
        }
    }

    #[test]
    fn test_candidate_status_pending_review_keeps_its_pre_adr0020_encoding() {
        let json = serde_json::to_string(&MemoryCandidateStatus::PendingReview).unwrap();
        assert_eq!(json, "\"pending_review\"");
        let decoded: MemoryCandidateStatus = serde_json::from_str("\"pending_review\"").unwrap();
        assert_eq!(decoded, MemoryCandidateStatus::PendingReview);
        assert_eq!(MemoryCandidateStatus::default(), decoded);
    }

    #[test]
    fn test_candidate_status_decided_variants_round_trip() {
        for status in [
            MemoryCandidateStatus::Promoted {
                record_id: record_id('b'),
                decided_at: "2026-09-12T10:00:00Z".to_string(),
                decided_by: actor(),
            },
            MemoryCandidateStatus::Dismissed {
                reason: "duplicates an existing record".to_string(),
                decided_at: "2026-09-12T10:00:00Z".to_string(),
                decided_by: actor(),
            },
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let decoded: MemoryCandidateStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, status);
            status.validate_shape().unwrap();
        }
    }

    #[test]
    fn test_candidate_without_a_status_key_loads_as_pending_review() {
        let mut value = serde_json::to_value(candidate()).unwrap();
        value.as_object_mut().unwrap().remove("status");
        let decoded: AcceptedRunMemoryCandidate = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.status, MemoryCandidateStatus::PendingReview);
        decoded.validate_shape().unwrap();
    }

    #[test]
    fn test_candidate_with_a_promoted_status_validates_and_round_trips() {
        let mut promoted = candidate();
        promoted.status = MemoryCandidateStatus::Promoted {
            record_id: record_id('b'),
            decided_at: "2026-09-12T10:00:00Z".to_string(),
            decided_by: actor(),
        };
        promoted.validate_shape().unwrap();
        let decoded: AcceptedRunMemoryCandidate =
            serde_json::from_str(&serde_json::to_string(&promoted).unwrap()).unwrap();
        assert_eq!(decoded, promoted);
        assert!(decoded.status.is_promoted());
        assert_eq!(decoded.status.promoted_record_id(), Some(&record_id('b')));
        assert_eq!(decoded.status.label(), "promoted");
    }

    #[test]
    fn test_candidate_with_a_blank_dismissal_reason_is_rejected() {
        let mut dismissed = candidate();
        dismissed.status = MemoryCandidateStatus::Dismissed {
            reason: "   ".to_string(),
            decided_at: "2026-09-12T10:00:00Z".to_string(),
            decided_by: actor(),
        };
        let error = dismissed.validate_shape().unwrap_err();
        assert!(format!("{error}").contains("status.reason"));
    }

    #[test]
    fn test_candidate_with_a_non_rfc3339_decided_at_is_rejected() {
        let mut promoted = candidate();
        promoted.status = MemoryCandidateStatus::Promoted {
            record_id: record_id('b'),
            decided_at: "yesterday".to_string(),
            decided_by: actor(),
        };
        let error = promoted.validate_shape().unwrap_err();
        assert!(format!("{error}").contains("status.decided_at"));
    }

    #[test]
    fn test_memory_record_round_trips_and_validates() {
        let record = record('b');
        record.validate_shape().unwrap();
        let decoded: MemoryRecord =
            serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap();
        assert_eq!(decoded, record);
        assert_eq!(decoded.identity_hex(), Some("b".repeat(64).as_str()));
    }

    #[test]
    fn test_memory_record_rejects_a_digest_that_does_not_match_its_id() {
        let mut record = record('b');
        record.digest = format!("{MEMORY_RECORD_DIGEST_PREFIX}{}", "c".repeat(64));
        let error = record.validate_shape().unwrap_err();
        assert!(format!("{error}").contains("same hex"));
    }

    #[test]
    fn test_memory_record_rejects_a_non_project_scope_in_this_stage() {
        for scope in [MemoryScope::Workspace, MemoryScope::Global] {
            let mut record = record('b');
            record.scope = scope;
            let error = record.validate_shape().unwrap_err();
            assert!(format!("{error}").contains("scope"));
            assert!(!scope.is_writable_now());
        }
        assert!(MemoryScope::Project.is_writable_now());
        assert_eq!(MemoryScope::default(), MemoryScope::Project);
    }

    #[test]
    fn test_memory_record_rejects_a_kind_that_contradicts_its_source() {
        let mut record = record('b');
        record.kind = MemoryKind::OperatorNote;
        assert!(format!("{}", record.validate_shape().unwrap_err()).contains("kind"));

        let mut manual = record.clone();
        manual.source = MemorySource::OperatorManual {
            note: "operator wrote this".to_string(),
        };
        manual.kind = MemoryKind::AcceptedRunOutcome;
        assert!(format!("{}", manual.validate_shape().unwrap_err()).contains("kind"));
    }

    #[test]
    fn test_memory_record_rejects_blank_title_or_body() {
        let mut blank_title = record('b');
        blank_title.title = "  ".to_string();
        assert!(format!("{}", blank_title.validate_shape().unwrap_err()).contains("title"));

        let mut blank_body = record('b');
        blank_body.body = String::new();
        assert!(format!("{}", blank_body.validate_shape().unwrap_err()).contains("body"));
    }

    #[test]
    fn test_memory_record_identity_source_bytes_are_deterministic_and_field_sensitive() {
        let record = record('b');
        let first = record.recompute_identity_source_bytes().unwrap();
        let second = record.recompute_identity_source_bytes().unwrap();
        assert_eq!(first, second);

        // valid_from is deliberately absent from the identity source, so a
        // replay after a crash re-derives the same id despite a moved clock.
        let mut later = record.clone();
        later.valid_from = "2030-01-01T00:00:00Z".to_string();
        assert_eq!(later.recompute_identity_source_bytes().unwrap(), first);

        let mut other_body = record;
        other_body.body = "something else".to_string();
        assert_ne!(other_body.recompute_identity_source_bytes().unwrap(), first);
    }

    #[test]
    fn test_memory_log_entry_round_trips_and_verifies_its_links() {
        let entry = MemoryLogEntry {
            seq: 0,
            previous_digest: MEMORY_LOG_GENESIS_DIGEST.to_string(),
            record: record('b'),
            entry_digest: format!("{MEMORY_LOG_DIGEST_PREFIX}{}", "e".repeat(64)),
        };
        entry.verify_links(0, MEMORY_LOG_GENESIS_DIGEST).unwrap();
        let decoded: MemoryLogEntry =
            serde_json::from_str(&serde_json::to_string(&entry).unwrap()).unwrap();
        assert_eq!(decoded, entry);
    }

    #[test]
    fn test_memory_log_entry_rejects_a_broken_chain() {
        let entry = MemoryLogEntry {
            seq: 3,
            previous_digest: format!("{MEMORY_LOG_DIGEST_PREFIX}{}", "f".repeat(64)),
            record: record('b'),
            entry_digest: format!("{MEMORY_LOG_DIGEST_PREFIX}{}", "e".repeat(64)),
        };
        assert!(format!(
            "{}",
            entry.verify_links(4, &entry.previous_digest).unwrap_err()
        )
        .contains("out of order"));
        assert!(format!(
            "{}",
            entry
                .verify_links(3, MEMORY_LOG_GENESIS_DIGEST)
                .unwrap_err()
        )
        .contains("does not chain"));
    }

    #[test]
    fn test_memory_log_entry_digest_source_covers_seq_and_previous_digest() {
        let record = record('b');
        let base =
            MemoryLogEntry::digest_source_bytes(0, MEMORY_LOG_GENESIS_DIGEST, &record).unwrap();
        assert_ne!(
            MemoryLogEntry::digest_source_bytes(1, MEMORY_LOG_GENESIS_DIGEST, &record).unwrap(),
            base
        );
        assert_ne!(
            MemoryLogEntry::digest_source_bytes(0, "sha256-memlog-v1:other", &record).unwrap(),
            base
        );
    }
}
