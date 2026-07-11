//! Ion harness contract v0 — the transport-agnostic `HarnessRequest`/`HarnessResponse`
//! shapes that make any agent harness pluggable into Impulse.
//!
//! Schema source: `~/.ai-memory/docs/ion-harness/spec-a-harness-contract-v0.md` (§2, §3).
//! This crate pins the contract as Rust types plus the machine-checkable validation
//! rules from spec-a §6 so callers never hand-roll verdict parsing.

use serde::{Deserialize, Serialize};

pub mod pi_adapter;

pub const CONTRACT_VERSION: &str = "0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Verify,
    Review,
    Summarize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RepoRef {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_inline: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub description: String,
    #[serde(default)]
    pub verdict_priority: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryExcerpt {
    pub kind: String,
    pub source: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Context {
    pub read_only: bool,
    #[serde(default)]
    pub payload: Vec<MemoryExcerpt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HarnessRequest {
    pub contract_version: String,
    pub request_id: String,
    pub intent: Intent,
    pub repo: RepoRef,
    pub task: Task,
    pub capability_allowlist: Vec<String>,
    pub model_role: String,
    pub context: Context,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    Critical,
    Warning,
    Note,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Verdict {
    Approve,
    #[serde(rename = "CHANGES REQUESTED")]
    ChangesRequested,
    #[serde(rename = "NEEDS DISCUSSION")]
    NeedsDiscussion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub category: String,
    pub file: String,
    pub line: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandRun {
    pub command: String,
    pub exit_code: i32,
    pub output_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Metrics {
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default)]
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HarnessResponse {
    pub contract_version: String,
    pub request_id: String,
    pub verdict: Verdict,
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub commands_run: Vec<CommandRun>,
    #[serde(default)]
    pub output_logs: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub metrics: Metrics,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ContractViolation {
    #[error("verdict {0:?} forbidden: a CRITICAL finding is present")]
    CriticalBlocksApprove(Verdict),
    #[error("verdict {0:?} requires at least one entry in commands_run")]
    MissingCommandsRun(Verdict),
    #[error("capability_allowlist contains a write-capable tool: {0}")]
    WriteCapabilityRequested(String),
}

/// Tool names that must never appear in a verify-intent `capability_allowlist`.
/// Spec-a §2/§6: write denial is structural (by omission), this just makes the
/// omission checkable instead of trusting every caller to remember it.
const WRITE_CAPABILITIES: &[&str] = &["write", "edit", "apply_patch", "rm", "mv", "git_push"];

/// Tool names allowed in a verify-intent `capability_allowlist` (spec-a §2):
/// read-only inspection (`read`, `grep`, `find`, `ls`) plus run-only build/test
/// tools. No write tool ever appears here — see `WRITE_CAPABILITIES` above.
const VERIFY_CAPABILITY_ALLOWLIST: &[&str] = &["read", "grep", "find", "ls", "build", "test"];

/// Task verdict-priority order shared by every verify-intent request built via
/// `HarnessRequest::verify` (spec-a worked example order).
const VERIFY_VERDICT_PRIORITY: &[&str] = &["correctness", "security", "style", "performance"];

impl HarnessRequest {
    /// Build a validated verify-intent `HarnessRequest` using the spec-a §2
    /// defaults: read-only `capability_allowlist`, the standard verdict
    /// priority order, `model_role = "verifier-cheap"`, and a read-only
    /// `Context`. This is the single seam for the defaults that used to be
    /// hand-rolled at every call site (G4) — callers only supply what varies
    /// per request: the repo path, the diff to inspect, and a task description.
    ///
    /// `request_id` is freshly generated per call (`req-ion-verify-<uuid v4>`)
    /// so concurrent verify requests never collide.
    pub fn verify(
        repo_path: impl Into<String>,
        diff_ref: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            contract_version: CONTRACT_VERSION.to_string(),
            request_id: format!("req-ion-verify-{}", uuid::Uuid::new_v4()),
            intent: Intent::Verify,
            repo: RepoRef {
                path: repo_path.into(),
                diff_ref: Some(diff_ref.into()),
                diff_inline: None,
            },
            task: Task {
                description: description.into(),
                verdict_priority: VERIFY_VERDICT_PRIORITY
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            },
            capability_allowlist: VERIFY_CAPABILITY_ALLOWLIST
                .iter()
                .map(|s| s.to_string())
                .collect(),
            model_role: "verifier-cheap".to_string(),
            context: Context {
                read_only: true,
                payload: vec![],
            },
        }
    }

    /// Validate the write-denial-by-omission rule (spec-a §2, §6).
    pub fn validate(&self) -> Result<(), ContractViolation> {
        for cap in &self.capability_allowlist {
            if WRITE_CAPABILITIES.contains(&cap.as_str()) {
                return Err(ContractViolation::WriteCapabilityRequested(cap.clone()));
            }
        }
        Ok(())
    }
}

impl HarnessResponse {
    /// Validate the spec-a §6 acceptance criteria that are checkable on the
    /// response alone: CRITICAL findings forbid APPROVE, and any verdict other
    /// than NEEDS DISCUSSION must carry evidence (a non-empty `commands_run`).
    pub fn validate(&self) -> Result<(), ContractViolation> {
        let has_critical = self
            .findings
            .iter()
            .any(|f| f.severity == Severity::Critical);
        if has_critical && self.verdict == Verdict::Approve {
            return Err(ContractViolation::CriticalBlocksApprove(
                self.verdict.clone(),
            ));
        }
        if self.verdict != Verdict::NeedsDiscussion && self.commands_run.is_empty() {
            return Err(ContractViolation::MissingCommandsRun(self.verdict.clone()));
        }
        Ok(())
    }

    /// Machine-branchable pass/fail per spec-a §5: PASS iff verdict is APPROVE
    /// and no CRITICAL finding is present. Callers must never do NLP on prose.
    pub fn passed(&self) -> bool {
        self.verdict == Verdict::Approve
            && !self
                .findings
                .iter()
                .any(|f| f.severity == Severity::Critical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worked_example_request() -> HarnessRequest {
        HarnessRequest::verify(
            "/Users/.../auth-service",
            "HEAD~1..HEAD",
            "Verify token-expiry off-by-one fix.",
        )
    }

    fn worked_example_response() -> HarnessResponse {
        HarnessResponse {
            contract_version: CONTRACT_VERSION.to_string(),
            request_id: "req-2026-05-21-0001".to_string(),
            verdict: Verdict::Approve,
            findings: vec![Finding {
                severity: Severity::Note,
                category: "style".to_string(),
                file: "src/auth/token.ts".to_string(),
                line: 42,
                message: "Comparison now `<`; consider a comment naming the boundary case."
                    .to_string(),
            }],
            commands_run: vec![
                CommandRun {
                    command: "pnpm tsc --noEmit".to_string(),
                    exit_code: 0,
                    output_ref: "log-1".to_string(),
                },
                CommandRun {
                    command: "pnpm test -- token".to_string(),
                    exit_code: 0,
                    output_ref: "log-2".to_string(),
                },
            ],
            output_logs: [
                ("log-1".to_string(), "tsc: no errors.".to_string()),
                ("log-2".to_string(), "8 passed, 0 failed.".to_string()),
            ]
            .into_iter()
            .collect(),
            metrics: Metrics {
                tokens_in: 3120,
                tokens_out: 280,
                latency_ms: 7110,
            },
        }
    }

    #[test]
    fn worked_example_request_round_trips_through_json() {
        let req = worked_example_request();
        let json = serde_json::to_string(&req).unwrap();
        let back: HarnessRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
        assert!(req.validate().is_ok());
    }

    #[test]
    fn worked_example_response_round_trips_and_passes() {
        let resp = worked_example_response();
        let json = serde_json::to_string(&resp).unwrap();
        let back: HarnessResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
        assert!(resp.validate().is_ok());
        assert!(resp.passed());
    }

    #[test]
    fn write_capability_in_allowlist_is_rejected() {
        let mut req = worked_example_request();
        req.capability_allowlist.push("write".to_string());
        assert_eq!(
            req.validate(),
            Err(ContractViolation::WriteCapabilityRequested(
                "write".to_string()
            ))
        );
    }

    #[test]
    fn critical_finding_forbids_approve() {
        let mut resp = worked_example_response();
        resp.findings.push(Finding {
            severity: Severity::Critical,
            category: "security".to_string(),
            file: "src/auth/token.ts".to_string(),
            line: 10,
            message: "token never expires under leap-second skew".to_string(),
        });
        assert_eq!(
            resp.validate(),
            Err(ContractViolation::CriticalBlocksApprove(Verdict::Approve))
        );
        assert!(!resp.passed());
    }

    #[test]
    fn non_needs_discussion_verdict_requires_commands_run() {
        let mut resp = worked_example_response();
        resp.commands_run.clear();
        assert_eq!(
            resp.validate(),
            Err(ContractViolation::MissingCommandsRun(Verdict::Approve))
        );
    }

    #[test]
    fn needs_discussion_may_have_empty_commands_run() {
        let mut resp = worked_example_response();
        resp.verdict = Verdict::NeedsDiscussion;
        resp.commands_run.clear();
        assert!(resp.validate().is_ok());
        assert!(!resp.passed());
    }

    #[test]
    fn verify_constructor_round_trips_through_json() {
        let req = HarnessRequest::verify("/repo", "HEAD~1..HEAD", "Verify the diff.");
        let json = serde_json::to_string(&req).unwrap();
        let back: HarnessRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn verify_constructor_validates_clean() {
        let req = HarnessRequest::verify("/repo", "HEAD~1..HEAD", "Verify the diff.");
        assert!(req.validate().is_ok());
    }

    #[test]
    fn verify_constructor_sets_fields_per_spec_a() {
        let req = HarnessRequest::verify("/repo", "HEAD~1..HEAD", "Verify the diff.");
        assert_eq!(req.contract_version, CONTRACT_VERSION);
        assert!(req.request_id.starts_with("req-ion-verify-"));
        assert_eq!(req.intent, Intent::Verify);
        assert_eq!(req.repo.path, "/repo");
        assert_eq!(req.repo.diff_ref, Some("HEAD~1..HEAD".to_string()));
        assert_eq!(req.repo.diff_inline, None);
        assert_eq!(req.task.description, "Verify the diff.");
        assert_eq!(
            req.task.verdict_priority,
            vec!["correctness", "security", "style", "performance"]
        );
        assert_eq!(req.model_role, "verifier-cheap");
        assert!(req.context.read_only);
        assert!(req.context.payload.is_empty());
    }

    #[test]
    fn verify_constructor_capability_allowlist_matches_spec_a_read_only_set() {
        // spec-a §2 worked example: exactly the read-only inspection tools
        // plus run-only build/test — no write tool, no extras, no reordering.
        let req = HarnessRequest::verify("/repo", "HEAD~1..HEAD", "Verify the diff.");
        assert_eq!(
            req.capability_allowlist,
            vec!["read", "grep", "find", "ls", "build", "test"]
        );
    }

    #[test]
    fn verify_constructor_generates_distinct_request_ids() {
        let a = HarnessRequest::verify("/repo", "HEAD~1..HEAD", "Verify the diff.");
        let b = HarnessRequest::verify("/repo", "HEAD~1..HEAD", "Verify the diff.");
        assert_ne!(a.request_id, b.request_id);
    }
}
