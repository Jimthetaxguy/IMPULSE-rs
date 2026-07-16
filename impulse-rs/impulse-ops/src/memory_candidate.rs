//! Review-only memory candidates derived from accepted governed runs.
//!
//! A candidate is a semantic proposal backed by episodic governed-task
//! evidence. It is deliberately not curated project memory: this contract has
//! no promotion action and carries no worker summary or Supervisor rationale.

use chrono::DateTime;
use serde::de::Error as _;
use serde::{Deserialize, Serialize};

use crate::governed_task::{GovernedRecordId, GovernedTaskId, GovernedVerificationProfile};

pub const ACCEPTED_RUN_MEMORY_CANDIDATE_SCHEMA_VERSION: u32 = 1;
pub const ACCEPTED_RUN_MEMORY_DERIVATION_VERSION: u32 = 1;

const MAX_OPEN_ID_BYTES: usize = 256;
const MAX_PROJECT_TEXT_BYTES: usize = 16 * 1024;
const MAX_TASK_BYTES: usize = 8 * 1024;
const MAX_CRITERIA: usize = 64;
const MAX_CRITERION_BYTES: usize = 16 * 1024;
const MAX_PROPOSED_SUMMARY_BYTES: usize = 12 * 1024;
const MAX_ARTIFACT_REFERENCES: usize = 64;
const MAX_ARTIFACT_REFERENCE_BYTES: usize = 4 * 1024;
const MAX_COMMANDS: usize = 64;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateStatus {
    PendingReview,
}

/// Describes what Impulse can honestly attest about the source chain.
///
/// Operator identity is declared inside the current same-user socket trust
/// boundary; neither variant implies cryptographic human authentication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptedRunSourceAssurance {
    DaemonProfiledEvidenceDeclaredOperator,
    CallerComposedEvidenceDeclaredOperator,
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
        validate_text("project_id", &self.project_id, MAX_OPEN_ID_BYTES)?;
        validate_text(
            "workspace_root",
            &self.workspace_root,
            MAX_PROJECT_TEXT_BYTES,
        )?;
        validate_text("task", &self.task, MAX_TASK_BYTES)?;
        if self.acceptance_criteria.len() > MAX_CRITERIA
            || (self.acceptance_criteria.is_empty()
                && self.source_assurance
                    == AcceptedRunSourceAssurance::DaemonProfiledEvidenceDeclaredOperator)
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
        match self.source_assurance {
            AcceptedRunSourceAssurance::DaemonProfiledEvidenceDeclaredOperator => {
                validate_git_oid("subject_revision", &self.subject_revision)?;
            }
            AcceptedRunSourceAssurance::CallerComposedEvidenceDeclaredOperator => {
                validate_text(
                    "subject_revision",
                    &self.subject_revision,
                    MAX_ARTIFACT_REFERENCE_BYTES,
                )?;
            }
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
    fn command_references_accept_governed_uppercase_hex_without_weakening_source_digest() {
        let mut candidate = candidate();
        candidate.commands[0].command_digest = digest("sha256:", 'A');
        candidate.commands[0].output_digest = digest("sha256:", 'B');
        candidate.validate_shape().unwrap();

        candidate.source_digest = digest("sha256-v1:", 'C');
        assert!(candidate.validate_shape().is_err());
    }
}
