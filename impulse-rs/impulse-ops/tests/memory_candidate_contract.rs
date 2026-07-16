use impulse_ops::governed_task::{GovernedRecordId, GovernedTaskId, GovernedVerificationProfile};
use impulse_ops::memory_candidate::{
    AcceptedRunCommandEvidence, AcceptedRunMemoryCandidate, AcceptedRunSourceAssurance,
    MemoryCandidateId, MemoryCandidateStatus, ACCEPTED_RUN_MEMORY_CANDIDATE_SCHEMA_VERSION,
    ACCEPTED_RUN_MEMORY_DERIVATION_VERSION,
};
use impulse_ops::ProjectOpsSnapshot;

#[derive(serde::Deserialize)]
struct LegacyProjectOpsSnapshotV5 {
    generated_at: String,
    #[serde(default)]
    governed_tasks: Vec<impulse_ops::governed_task::GovernedTaskSnapshot>,
}

fn digest(prefix: &str, character: char) -> String {
    format!("{prefix}{}", character.to_string().repeat(64))
}

fn accepted_run_candidate() -> AcceptedRunMemoryCandidate {
    AcceptedRunMemoryCandidate {
        id: MemoryCandidateId::try_new(format!(
            "memory-candidate-{}",
            "a".repeat(64)
        ))
        .unwrap(),
        schema_version: ACCEPTED_RUN_MEMORY_CANDIDATE_SCHEMA_VERSION,
        derivation_version: ACCEPTED_RUN_MEMORY_DERIVATION_VERSION,
        status: MemoryCandidateStatus::PendingReview,
        project_id: "impulse-rs".to_string(),
        workspace_root: "/tmp/impulse-rs".to_string(),
        governed_task_id: GovernedTaskId::try_new("task-memory-candidate").unwrap(),
        accepted_task_revision: 7,
        task: "Stage an accepted-run memory candidate".to_string(),
        acceptance_criteria: vec![
            "Keep the candidate review-only".to_string(),
            "Preserve governed evidence provenance".to_string(),
        ],
        proposed_summary: "Accepted governed outcome for task: Stage an accepted-run memory candidate. Daemon-profiled evidence passed; pending semantic-memory review.".to_string(),
        runtime_id: "codex".to_string(),
        agent_id: "builder-01".to_string(),
        session_id: Some("session-01".to_string()),
        verification_profile: Some(GovernedVerificationProfile::RustWorkspaceV1),
        verification_policy: "rust_workspace_v1".to_string(),
        subject_revision: "a".repeat(40),
        claim_id: GovernedRecordId::try_new("claim-01").unwrap(),
        verification_id: GovernedRecordId::try_new("verification-01").unwrap(),
        supervisor_verdict_id: GovernedRecordId::try_new("verdict-01").unwrap(),
        operator_decision_id: GovernedRecordId::try_new("decision-01").unwrap(),
        claimed_artifact_ids: vec!["artifact-claim-01".to_string()],
        verification_artifact_ids: vec!["artifact-verification-01".to_string()],
        commands: vec![AcceptedRunCommandEvidence {
            name: "cargo-test".to_string(),
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
fn accepted_run_candidate_public_contract_round_trips() {
    let candidate = accepted_run_candidate();
    candidate.validate_shape().unwrap();

    let encoded = serde_json::to_value(&candidate).unwrap();
    let decoded: AcceptedRunMemoryCandidate = serde_json::from_value(encoded).unwrap();

    assert_eq!(decoded, candidate);
}

#[test]
fn accepted_run_candidate_ignores_additive_unknown_fields() {
    let mut encoded = serde_json::to_value(accepted_run_candidate()).unwrap();
    encoded.as_object_mut().unwrap().insert(
        "future_review_metadata".to_string(),
        serde_json::json!({"v": 2}),
    );

    let decoded: AcceptedRunMemoryCandidate = serde_json::from_value(encoded).unwrap();

    decoded.validate_shape().unwrap();
    assert_eq!(decoded.status, MemoryCandidateStatus::PendingReview);
}

#[test]
fn legacy_ops_snapshot_defaults_to_no_memory_candidates() {
    let mut legacy = serde_json::to_value(ProjectOpsSnapshot::default()).unwrap();
    legacy.as_object_mut().unwrap().remove("memory_candidates");

    let snapshot: ProjectOpsSnapshot = serde_json::from_value(legacy).unwrap();

    assert!(snapshot.memory_candidates.is_empty());
}

#[test]
fn ops_snapshot_round_trips_review_only_memory_candidates() {
    let candidate = accepted_run_candidate();
    let snapshot = ProjectOpsSnapshot {
        memory_candidates: vec![candidate.clone()],
        ..Default::default()
    };

    let encoded = serde_json::to_string(&snapshot).unwrap();
    let decoded: ProjectOpsSnapshot = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded.memory_candidates, vec![candidate]);
}

#[test]
fn candidate_snapshot_is_additive_for_the_pre_candidate_snapshot_shape() {
    let snapshot = ProjectOpsSnapshot {
        generated_at: "2026-07-15T22:00:00Z".to_string(),
        memory_candidates: vec![accepted_run_candidate()],
        ..Default::default()
    };

    let encoded = serde_json::to_string(&snapshot).unwrap();
    let legacy: LegacyProjectOpsSnapshotV5 = serde_json::from_str(&encoded).unwrap();

    assert_eq!(legacy.generated_at, "2026-07-15T22:00:00Z");
    assert!(legacy.governed_tasks.is_empty());
}
