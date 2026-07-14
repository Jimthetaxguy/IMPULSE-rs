use impulse_ops::agent_registry::AgentPlatformId;
use impulse_ops::governed_task::{
    GovernedClaimRequest, GovernedRecordId, GovernedRequestId, GovernedSupervisorReviewEnvelope,
    GovernedSupervisorReviewRequest, GovernedTaskId, GovernedTaskRegistration,
    GovernedVerificationProfile, GovernedVerificationRequest, SupervisorVerdictKind,
    GOVERNED_SUPERVISOR_REVIEW_CONTRACT_VERSION,
};
use impulse_ops::role_assignment::{
    canonical_governed_builder_assignment, AgentRoleAssignment, AgentRoleId,
    CapabilityCompatibility, RoleCompatibility,
};

const OID: &str = "0123456789abcdef0123456789abcdef01234567";

fn request_id(value: &str) -> GovernedRequestId {
    GovernedRequestId::try_new(value).unwrap()
}

fn task_id() -> GovernedTaskId {
    GovernedTaskId::try_new("task-producer-contract").unwrap()
}

fn allowed_profiled_role(runtime: &str) -> (AgentRoleAssignment, RoleCompatibility) {
    let assignment = canonical_governed_builder_assignment();
    let compatibility = RoleCompatibility {
        platform: AgentPlatformId::try_new(runtime).unwrap(),
        role: assignment.role.clone(),
        checks: assignment
            .requirements
            .iter()
            .map(|requirement| CapabilityCompatibility {
                capability: requirement.capability.clone(),
                required: requirement.minimum_enforcement,
                available: requirement.minimum_enforcement,
                mandatory: requirement.mandatory,
            })
            .collect(),
    };
    (assignment, compatibility)
}

#[test]
fn unprofiled_registration_remains_wire_compatible() {
    let registration = GovernedTaskRegistration::builder(
        "register-old",
        task_id().to_string(),
        "project",
        "/tmp/project",
        "legacy manual task",
        "worker-1",
        "codex",
    )
    .build()
    .unwrap();

    let json = serde_json::to_value(&registration).unwrap();
    assert!(json.get("verification_profile").is_none());
    assert!(json.get("initial_subject_revision").is_none());
}

#[test]
fn profiled_registration_requires_criteria_and_git_oid() {
    let missing_criteria = GovernedTaskRegistration::builder(
        "register-profile-1",
        task_id().to_string(),
        "project",
        "/tmp/project",
        "closed loop task",
        "worker-1",
        "ion",
    )
    .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
    .initial_subject_revision(OID)
    .build()
    .unwrap_err();
    assert!(missing_criteria.to_string().contains("acceptance_criteria"));

    let missing_oid = GovernedTaskRegistration::builder(
        "register-profile-2",
        task_id().to_string(),
        "project",
        "/tmp/project",
        "closed loop task",
        "worker-1",
        "ion",
    )
    .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
    .acceptance_criteria(vec!["workspace tests pass".to_string()])
    .build()
    .unwrap_err();
    assert!(missing_oid.to_string().contains("initial_subject_revision"));

    let uppercase_oid = GovernedTaskRegistration::builder(
        "register-profile-3",
        task_id().to_string(),
        "project",
        "/tmp/project",
        "closed loop task",
        "worker-1",
        "ion",
    )
    .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
    .acceptance_criteria(vec!["workspace tests pass".to_string()])
    .initial_subject_revision(OID.to_ascii_uppercase())
    .build()
    .unwrap_err();
    assert!(uppercase_oid.to_string().contains("lowercase hexadecimal"));
}

#[test]
fn profiled_registration_roundtrips() {
    let (assignment, compatibility) = allowed_profiled_role("ion");
    let registration = GovernedTaskRegistration::builder(
        "register-profile-ok",
        task_id().to_string(),
        "project",
        "/tmp/project",
        "closed loop task",
        "worker-1",
        "ion",
    )
    .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
    .acceptance_criteria(vec!["workspace tests pass".to_string()])
    .initial_subject_revision(OID)
    .role_assignment(assignment)
    .role_compatibility(compatibility)
    .build()
    .unwrap();

    let wire = serde_json::to_string(&registration).unwrap();
    assert!(wire.contains("rust_workspace_v1"));
    assert_eq!(
        serde_json::from_str::<GovernedTaskRegistration>(&wire).unwrap(),
        registration
    );
}

#[test]
fn profiled_registration_requires_the_canonical_builder_contract() {
    let missing = GovernedTaskRegistration::builder(
        "register-profile-role-missing",
        "task-profile-role-missing",
        "project",
        "/tmp/project",
        "closed loop task",
        "worker-1",
        "ion",
    )
    .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
    .acceptance_criteria(vec!["workspace tests pass".to_string()])
    .initial_subject_revision(OID)
    .build()
    .unwrap_err();
    assert!(missing.to_string().contains("canonical Builder role"));

    let (mut wrong_assignment, mut wrong_compatibility) = allowed_profiled_role("ion");
    wrong_assignment.role = AgentRoleId::try_new("reviewer").unwrap();
    wrong_compatibility.role = wrong_assignment.role.clone();
    let wrong = GovernedTaskRegistration::builder(
        "register-profile-role-wrong",
        "task-profile-role-wrong",
        "project",
        "/tmp/project",
        "closed loop task",
        "worker-1",
        "ion",
    )
    .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
    .acceptance_criteria(vec!["workspace tests pass".to_string()])
    .initial_subject_revision(OID)
    .role_assignment(wrong_assignment)
    .role_compatibility(wrong_compatibility)
    .build()
    .unwrap_err();
    assert!(wrong
        .to_string()
        .contains("canonical Builder role requirements"));
}

#[test]
fn producer_triggers_exclude_derived_truth_fields() {
    let claim = GovernedClaimRequest {
        request_id: request_id("claim-request"),
        project_id: "project".to_string(),
        task_id: task_id(),
        expected_revision: 1,
        summary: "implemented the assigned change".to_string(),
        artifact_ids: vec!["artifact-1".to_string()],
    };
    let claim_json = serde_json::to_value(claim).unwrap();
    assert!(claim_json.get("actor").is_none());
    assert!(claim_json.get("subject_revision").is_none());
    assert!(claim_json.get("diff_ref").is_none());

    let verify = GovernedVerificationRequest {
        request_id: request_id("verify-request"),
        project_id: "project".to_string(),
        task_id: task_id(),
        expected_revision: 2,
    };
    let verify_json = serde_json::to_value(verify).unwrap();
    assert!(verify_json.get("commands").is_none());
    assert!(verify_json.get("verification").is_none());

    let review = GovernedSupervisorReviewRequest {
        request_id: request_id("review-request"),
        project_id: "project".to_string(),
        task_id: task_id(),
        expected_revision: 3,
    };
    let review_json = serde_json::to_value(review).unwrap();
    assert!(review_json.get("verdict").is_none());
    assert!(review_json.get("actor").is_none());
}

#[test]
fn supervisor_envelope_is_strict_and_versioned() {
    let envelope = GovernedSupervisorReviewEnvelope {
        contract_version: GOVERNED_SUPERVISOR_REVIEW_CONTRACT_VERSION.to_string(),
        task_id: task_id(),
        task_revision: 3,
        claim_id: GovernedRecordId::try_new("claim-1").unwrap(),
        verification_id: GovernedRecordId::try_new("verification-1").unwrap(),
        subject_revision: OID.to_string(),
        acceptance_criteria_count: 1,
        acceptance_criteria_digest: format!("sha256:{}", "a".repeat(64)),
        verdict: SupervisorVerdictKind::RecommendAccept,
        rationale: "all criteria are supported by passing evidence".to_string(),
    };
    assert!(envelope.validate_shape().is_ok());

    let mut json = serde_json::to_value(&envelope).unwrap();
    json.as_object_mut()
        .unwrap()
        .insert("unexpected".to_string(), serde_json::json!(true));
    assert!(serde_json::from_value::<GovernedSupervisorReviewEnvelope>(json).is_err());

    let mut wrong_version = envelope;
    wrong_version.contract_version = "2".to_string();
    assert!(wrong_version.validate_shape().is_err());
}

#[test]
fn profiled_registration_rejects_lossy_supervisor_inputs() {
    let too_many = GovernedTaskRegistration::builder(
        "register-profile-bounds-1",
        "task-profile-bounds-1",
        "project",
        "/tmp/project",
        "task",
        "worker",
        "ion",
    )
    .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
    .initial_subject_revision(OID)
    .acceptance_criteria(vec!["criterion".to_string(); 17])
    .build()
    .unwrap_err();
    assert!(too_many.to_string().contains("at most 16 exact criteria"));

    let too_long = GovernedTaskRegistration::builder(
        "register-profile-bounds-2",
        "task-profile-bounds-2",
        "project",
        "/tmp/project",
        "task",
        "worker",
        "ion",
    )
    .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
    .initial_subject_revision(OID)
    .acceptance_criteria(vec!["x".repeat(513)])
    .build()
    .unwrap_err();
    assert!(too_long.to_string().contains("at most 512"));
}

#[test]
fn producer_requests_reject_unknown_derived_truth_fields() {
    let claim = serde_json::json!({
        "request_id": "claim-strict",
        "project_id": "project",
        "task_id": "task-1",
        "expected_revision": 1,
        "summary": "done",
        "artifact_ids": [],
        "actor": {"kind": "worker", "id": "forged"}
    });
    assert!(serde_json::from_value::<GovernedClaimRequest>(claim).is_err());

    let verify = serde_json::json!({
        "request_id": "verify-strict",
        "project_id": "project",
        "task_id": "task-1",
        "expected_revision": 2,
        "commands": []
    });
    assert!(serde_json::from_value::<GovernedVerificationRequest>(verify).is_err());

    let review = serde_json::json!({
        "request_id": "review-strict",
        "project_id": "project",
        "task_id": "task-1",
        "expected_revision": 3,
        "verdict": "recommend_accept"
    });
    assert!(serde_json::from_value::<GovernedSupervisorReviewRequest>(review).is_err());
}
