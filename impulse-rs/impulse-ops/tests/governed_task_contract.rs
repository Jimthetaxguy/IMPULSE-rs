use impulse_ops::agent_registry::AgentPlatformId;
use impulse_ops::governed_task::{
    ApprovalPolicy, GovernedExecutionState, GovernedReviewState, GovernedTaskId,
    GovernedTaskRegistration, GovernedTaskRun, GovernedTaskSnapshot, WorldScope,
};
use impulse_ops::role_assignment::{
    AgentRoleAssignment, AgentRoleId, EnforcementStrength, RoleCapabilityRequirement,
    RoleCompatibility, RuntimeCapabilityId,
};
use impulse_ops::{ProjectOpsSnapshot, WorkbenchDaemonRequest};

#[test]
fn governed_ids_reject_blank_whitespace_and_control_characters() {
    assert!(GovernedTaskId::try_new("").is_err());
    assert!(GovernedTaskId::try_new("task id").is_err());
    assert!(GovernedTaskId::try_new("task\nid").is_err());
    assert_eq!(
        GovernedTaskId::try_new("task-01").unwrap().as_str(),
        "task-01"
    );
}

#[test]
fn old_ops_snapshot_defaults_to_no_governed_tasks() {
    let mut legacy = serde_json::to_value(ProjectOpsSnapshot::default()).unwrap();
    legacy.as_object_mut().unwrap().remove("governed_tasks");
    let snapshot: ProjectOpsSnapshot = serde_json::from_value(legacy).unwrap();

    assert!(snapshot.governed_tasks.is_empty());
}

#[test]
fn registration_request_roundtrips_without_conflating_task_and_agent_ids() {
    let request = WorkbenchDaemonRequest::RegisterGovernedTask {
        registration: GovernedTaskRegistration::builder(
            "request-01",
            "task-01",
            "impulse-rs",
            "/tmp/impulse-rs",
            "Implement daemon-owned task truth",
            "agent-01",
            "codex",
        )
        .acceptance_criteria(vec!["state survives restart".into()])
        .approval_policy(ApprovalPolicy::OperatorRequired)
        .build()
        .unwrap(),
    };

    let encoded = serde_json::to_string(&request).unwrap();
    let decoded: WorkbenchDaemonRequest = serde_json::from_str(&encoded).unwrap();
    let WorkbenchDaemonRequest::RegisterGovernedTask { registration } = decoded else {
        panic!("expected register request");
    };
    assert_eq!(registration.agent_id, "agent-01");
    assert!(encoded.contains("RegisterGovernedTask"));
}

#[test]
fn task_snapshot_keeps_execution_and_review_state_independent() {
    let run = GovernedTaskRun {
        id: GovernedTaskId::try_new("task-01").unwrap(),
        revision: 4,
        project_id: "impulse-rs".into(),
        workspace_root: "/tmp/impulse-rs".into(),
        task: "Prove the lifecycle".into(),
        acceptance_criteria: vec![],
        approval_policy: ApprovalPolicy::OperatorRequired,
        world_scope: WorldScope::default(),
        verification_profile: None,
        role_assignment: None,
        role_compatibility: None,
        runtime_id: "codex".into(),
        agent_id: "agent-01".into(),
        session_id: None,
        initial_subject_revision: None,
        staged_worktree: None,
        promotions: vec![],
        execution_state: GovernedExecutionState::RuntimeExited,
        review_state: GovernedReviewState::AwaitingOperator,
        claims: vec![],
        verifications: vec![],
        supervisor_verdicts: vec![],
        operator_decisions: vec![],
        events: vec![],
        created_at: "2026-07-13T00:00:00Z".into(),
        updated_at: "2026-07-13T00:01:00Z".into(),
    };

    let snapshot = GovernedTaskSnapshot::from(run);
    assert_eq!(
        snapshot.execution_state,
        GovernedExecutionState::RuntimeExited
    );
    assert_eq!(snapshot.review_state, GovernedReviewState::AwaitingOperator);
}

#[test]
fn registration_rejects_fabricated_or_cross_runtime_role_compatibility() {
    let assignment = AgentRoleAssignment {
        role: AgentRoleId::try_new("builder").unwrap(),
        requirements: vec![RoleCapabilityRequirement {
            capability: RuntimeCapabilityId::try_new("workspace.target").unwrap(),
            minimum_enforcement: EnforcementStrength::Mediated,
            mandatory: true,
        }],
    };
    let fabricated = RoleCompatibility {
        platform: AgentPlatformId::try_new("codex").unwrap(),
        role: AgentRoleId::try_new("builder").unwrap(),
        checks: vec![],
    };
    let error = GovernedTaskRegistration::builder(
        "request-fabricated",
        "task-fabricated",
        "impulse-rs",
        "/tmp/impulse-rs",
        "Reject fabricated compatibility",
        "agent-01",
        "codex",
    )
    .role_assignment(assignment.clone())
    .role_compatibility(fabricated)
    .build()
    .unwrap_err();
    assert!(error.to_string().contains("exactly cover"));

    let wrong_runtime = RoleCompatibility {
        platform: AgentPlatformId::try_new("claude").unwrap(),
        role: assignment.role.clone(),
        checks: vec![impulse_ops::role_assignment::CapabilityCompatibility {
            capability: assignment.requirements[0].capability.clone(),
            required: EnforcementStrength::Mediated,
            available: EnforcementStrength::Mediated,
            mandatory: true,
        }],
    };
    let error = GovernedTaskRegistration::builder(
        "request-runtime",
        "task-runtime",
        "impulse-rs",
        "/tmp/impulse-rs",
        "Reject cross-runtime compatibility",
        "agent-01",
        "codex",
    )
    .role_assignment(assignment)
    .role_compatibility(wrong_runtime)
    .build()
    .unwrap_err();
    assert!(error.to_string().contains("platform must match runtime_id"));
}
