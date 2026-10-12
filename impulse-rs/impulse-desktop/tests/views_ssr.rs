//! SSR contracts for the center-stage views grafted from the desktop-views lane.
//! These stay DTO-bound: Rust state shapes drive Dioxus rendering.

use dioxus::prelude::*;
use impulse_desktop::{ArtifactsView, MemoryView, MemoryViewProps};
use impulse_ops::governed_task::{GovernedRecordId, GovernedTaskId, GovernedVerificationProfile};
use impulse_ops::memory_candidate::{
    AcceptedRunCommandEvidence, AcceptedRunMemoryCandidate, AcceptedRunSourceAssurance,
    MemoryCandidateId, MemoryCandidateStatus, ACCEPTED_RUN_MEMORY_CANDIDATE_SCHEMA_VERSION,
    ACCEPTED_RUN_MEMORY_DERIVATION_VERSION,
};
use impulse_ops::{
    ArtifactAction, ArtifactEnvelope, ArtifactStatus, ArtifactViewHint, ContextHealthSummary,
    InsightRecord, MemorySummary, RetrievalSummary,
};

fn render_dom(mut vdom: VirtualDom) -> String {
    vdom.rebuild_in_place();
    dioxus_ssr::render(&vdom)
}

fn seed_insight(content: &str) -> InsightRecord {
    InsightRecord {
        timestamp: Some("12:00".to_string()),
        agent_label: "codex".to_string(),
        kind: "decision".to_string(),
        content: content.to_string(),
    }
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
        acceptance_criteria: vec!["Keep the candidate review-only".to_string()],
        proposed_summary: "Accepted governed outcome backed by daemon-profiled verification; pending semantic-memory review.".to_string(),
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
fn test_memory_view_binds_context_memory_and_retrieval() {
    let context = ContextHealthSummary {
        tier: "operator".to_string(),
        usage_fraction: 0.236,
        estimated_tokens: 47_238,
        window_tokens: 200_000,
        injection_count: 7,
        compaction_count: 2,
        recent_insights: vec![seed_insight("split daemon module")],
        ..Default::default()
    };
    let memory = MemorySummary {
        genome_decisions: 12,
        active_sessions: 3,
        history_entries: 88,
        ..Default::default()
    };
    let retrieval = RetrievalSummary {
        backend: "sqlite-vector".to_string(),
        mode: "hybrid".to_string(),
        vector_enabled: true,
        semantic_strategy: "hyde".to_string(),
    };

    let html = render_dom(VirtualDom::new_with_props(
        MemoryView,
        MemoryViewProps {
            context,
            memory,
            retrieval,
            memory_candidates: Vec::new(),
        },
    ));

    assert!(html.contains("view-memory"));
    assert!(html.contains("Context health"));
    assert!(html.contains("24%"));
    assert!(html.contains("tier · operator"));
    assert!(html.contains("meter-fill"));
    assert!(html.contains("12"));
    assert!(html.contains("sqlite-vector"));
    assert!(html.contains("hyde"));
    assert!(html.contains("split daemon module"));
}

#[test]
fn test_memory_view_fallbacks_for_empty_and_none_fields() {
    let context = ContextHealthSummary {
        tier: String::new(),
        usage_fraction: 0.0,
        recent_insights: vec![InsightRecord {
            timestamp: None,
            agent_label: "codex".to_string(),
            kind: String::new(),
            content: "untyped note".to_string(),
        }],
        ..Default::default()
    };
    let memory = MemorySummary {
        last_genome_update: None,
        ..Default::default()
    };
    let retrieval = RetrievalSummary::default();

    let html = render_dom(VirtualDom::new_with_props(
        MemoryView,
        MemoryViewProps {
            context,
            memory,
            retrieval,
            memory_candidates: Vec::new(),
        },
    ));

    assert!(html.contains("tier · idle"));
    assert!(html.contains("last never"));
    assert!(html.contains("—"));
    assert!(html.contains("note"));
    assert!(html.contains("untyped note"));
}

#[test]
fn test_memory_view_renders_review_only_candidate_provenance_without_actions() {
    let candidate = accepted_run_candidate();
    let candidate_id = candidate.id.to_string();
    let html = render_dom(VirtualDom::new_with_props(
        MemoryView,
        MemoryViewProps {
            context: ContextHealthSummary::default(),
            memory: MemorySummary::default(),
            retrieval: RetrievalSummary::default(),
            memory_candidates: vec![candidate],
        },
    ));

    assert!(html.contains("Accepted-run candidates"));
    assert!(html.contains(&format!("data-candidate-id=\"{candidate_id}\"")));
    assert!(html.contains("Pending review — not stored in GENOME"));
    assert!(html.contains("Stage an accepted-run memory candidate"));
    assert!(html.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
    assert!(html.contains("rust_workspace_v1"));
    assert!(html.contains("daemon-profiled evidence · declared operator"));
    assert!(html.contains("task-memory-candidate"));
    assert!(html.contains("verification-01"));
    assert!(html.contains("decision-01"));
    assert!(html.contains("Keep the candidate review-only"));

    assert!(!html.contains("<button"));
    assert!(!html.contains("Promote"));
    assert!(!html.contains(">Apply<"));
    assert!(!html.contains("artifact-actions"));
}

#[component]
fn ArtifactsHarness(artifacts: Vec<ArtifactEnvelope>) -> Element {
    rsx! {
        ArtifactsView { artifacts, on_intent: move |_| {} }
    }
}

#[test]
fn test_artifacts_view_groups_by_status() {
    let artifacts = vec![
        ArtifactEnvelope {
            id: "art-pending".to_string(),
            agent_id: "codex".to_string(),
            kind: "diff".to_string(),
            title: "Refactor daemon".to_string(),
            summary: "split process_request".to_string(),
            status: ArtifactStatus::Pending,
            ..Default::default()
        },
        ArtifactEnvelope {
            id: "art-applied".to_string(),
            agent_id: "claude".to_string(),
            kind: "note".to_string(),
            title: "Genome update".to_string(),
            status: ArtifactStatus::Applied,
            ..Default::default()
        },
    ];

    let html = render_dom(VirtualDom::new_with_props(
        ArtifactsHarness,
        ArtifactsHarnessProps { artifacts },
    ));

    assert!(html.contains("view-artifacts"));
    assert!(html.contains("data-status=\"pending\""));
    assert!(html.contains("data-status=\"applied\""));
    assert!(html.contains("Refactor daemon"));
    assert!(html.contains("artifact-badge status-pending"));
    assert!(html.contains("split process_request"));
}

#[test]
fn test_artifact_card_renders_summary_hints_and_mutating_action() {
    let artifacts = vec![ArtifactEnvelope {
        id: "art-1".to_string(),
        agent_id: "codex".to_string(),
        kind: "patch".to_string(),
        title: String::new(),
        summary: "applies the daemon split".to_string(),
        view_hints: vec![ArtifactViewHint::Diff],
        actions: vec![ArtifactAction {
            id: "apply".to_string(),
            label: "Apply".to_string(),
            kind: "apply".to_string(),
            requires_confirmation: true,
            ..Default::default()
        }],
        status: ArtifactStatus::Staged,
        ..Default::default()
    }];

    let html = render_dom(VirtualDom::new_with_props(
        ArtifactsHarness,
        ArtifactsHarnessProps { artifacts },
    ));

    assert!(html.contains(">patch<"));
    assert!(html.contains("artifact-summary"));
    assert!(html.contains("applies the daemon split"));
    assert!(html.contains("hint-chips"));
    assert!(html.contains(">diff<"));
    assert!(html.contains("action-ghost mutating"));
}

#[test]
fn test_artifacts_view_orders_pending_before_applied() {
    let artifacts = vec![
        ArtifactEnvelope {
            id: "a-applied".to_string(),
            status: ArtifactStatus::Applied,
            title: "done".to_string(),
            ..Default::default()
        },
        ArtifactEnvelope {
            id: "a-pending".to_string(),
            status: ArtifactStatus::Pending,
            title: "todo".to_string(),
            ..Default::default()
        },
    ];

    let html = render_dom(VirtualDom::new_with_props(
        ArtifactsHarness,
        ArtifactsHarnessProps { artifacts },
    ));

    let pending_pos = html
        .find("data-status=\"pending\"")
        .expect("pending group present");
    let applied_pos = html
        .find("data-status=\"applied\"")
        .expect("applied group present");
    assert!(
        pending_pos < applied_pos,
        "pending must render before applied"
    );
}

#[test]
fn test_view_css_contains_stage_and_artifact_contracts() {
    let css = include_str!("../assets/impulse_crt.css");

    for selector in [
        "html,\nbody",
        ".stage-view",
        ".stage-view.active",
        ".view-rail",
        ".view-memory.active",
        ".view-artifacts.active",
        ".artifact-card",
        ".shell-notice",
    ] {
        assert!(css.contains(selector), "missing CSS selector {selector}");
    }

    for forbidden in [
        "@import",
        "fonts.googleapis",
        "fonts.gstatic",
        "https://",
        "http://",
    ] {
        assert!(
            !css.contains(forbidden),
            "desktop CSS must not require remote asset `{forbidden}`"
        );
    }
}
