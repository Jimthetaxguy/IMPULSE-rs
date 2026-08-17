//! Harness-owned per-step model choice (ADR-0015).
//!
//! Impulse is the harness. A gateway may do keys, rate limits, audit, or
//! provider failover. It must not pick the model. There is no step-level
//! router today: construction resolves one session model and fill sites copy
//! it into every [`crate::llm_backends::ChatRequest`]. This module is the
//! hook those fill sites call.
//!
//! Policy v0 is identity (`Configured`) unless attestation/verification
//! failed and an optional escalate model is present. Escalation is
//! `AfterVerifierFailure`, never token-count. This module does not read
//! `token_tracker`.

use impulse_ops::governed_task::{
    GovernedActorKind, GovernedReviewState, GovernedVerificationOutcome,
};
use serde::{Deserialize, Serialize};

/// Per-step harness facts used to choose a request model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessStepContext {
    pub actor: GovernedActorKind,
    pub review_state: Option<GovernedReviewState>,
    pub latest_verification: Option<GovernedVerificationOutcome>,
    pub tool_round: usize,
    pub current_model: String,
    /// Optional configured escalate model (`impulse_agent_escalate_model` when
    /// a caller has that value). Absent means v0 identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalate_model: Option<String>,
}

/// Why the harness chose `model` for this step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepModelReason {
    Configured,
    AfterVerifierFailure,
}

/// Result of [`decide_step_model`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepModelDecision {
    pub model: String,
    pub reason: StepModelReason,
}

/// Additive arena record logged beside ADR-0011 four-party attestation.
/// Durable ledger attachment is later work; this is not a SettlementRecord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepModelRecord {
    pub model: String,
    pub reason: StepModelReason,
    pub actor: GovernedActorKind,
    pub tool_round: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    pub configured: String,
}

impl HarnessStepContext {
    /// Ion/API fill-site default: Worker actor, no review/verification yet.
    pub fn ion_api(current_model: impl Into<String>) -> Self {
        Self {
            actor: GovernedActorKind::Worker,
            review_state: None,
            latest_verification: None,
            tool_round: 0,
            current_model: current_model.into(),
            escalate_model: None,
        }
    }

    /// Supervisor API-only turn default.
    pub fn supervisor(current_model: impl Into<String>) -> Self {
        Self {
            actor: GovernedActorKind::Supervisor,
            review_state: None,
            latest_verification: None,
            tool_round: 0,
            current_model: current_model.into(),
            escalate_model: None,
        }
    }
}

fn verifier_or_attestation_failed(ctx: &HarnessStepContext) -> bool {
    matches!(
        ctx.latest_verification,
        Some(GovernedVerificationOutcome::Failed) | Some(GovernedVerificationOutcome::Inconclusive)
    ) || matches!(
        ctx.review_state,
        Some(GovernedReviewState::VerificationFailed)
    )
}

/// Choose the model for one harness step.
///
/// Admissibility first: Operator and Verifier never receive a different
/// model (Operator never gets a model pick; Verifier stays daemon commands).
/// Capability next: stay on `current_model` unless verification/attestation
/// failed and `escalate_model` is set. Cost is unused in v0. Token count is
/// not an input.
pub fn decide_step_model(ctx: &HarnessStepContext, configured: &str) -> StepModelDecision {
    if matches!(
        ctx.actor,
        GovernedActorKind::Operator | GovernedActorKind::Verifier
    ) {
        return stay_configured(ctx, configured);
    }

    if verifier_or_attestation_failed(ctx) {
        if let Some(escalate) = ctx
            .escalate_model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty())
        {
            return StepModelDecision {
                model: escalate.to_string(),
                reason: StepModelReason::AfterVerifierFailure,
            };
        }
    }

    stay_configured(ctx, configured)
}

fn stay_configured(ctx: &HarnessStepContext, configured: &str) -> StepModelDecision {
    let model = if ctx.current_model.trim().is_empty() {
        configured.to_string()
    } else {
        ctx.current_model.clone()
    };
    StepModelDecision {
        model,
        reason: StepModelReason::Configured,
    }
}

/// Decide and emit a structured arena log. Returns the chosen model string
/// for `ChatRequest.model`.
pub fn resolve_step_model(
    ctx: &HarnessStepContext,
    configured: &str,
    actor_id: Option<&str>,
) -> String {
    let decision = decide_step_model(ctx, configured);
    let _record = record_step_model(ctx, &decision, configured, actor_id);
    decision.model
}

/// Structured arena log beside ADR-0011 attestation. Not attached to a
/// governed-task ledger in this slice.
pub fn record_step_model(
    ctx: &HarnessStepContext,
    decision: &StepModelDecision,
    configured: &str,
    actor_id: Option<&str>,
) -> StepModelRecord {
    let record = StepModelRecord {
        model: decision.model.clone(),
        reason: decision.reason,
        actor: ctx.actor,
        tool_round: ctx.tool_round,
        actor_id: actor_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(ToOwned::to_owned),
        configured: configured.to_string(),
    };
    tracing::info!(
        target: "impulse.arena.step_model",
        actor = ?record.actor,
        actor_id = record.actor_id.as_deref(),
        model = %record.model,
        reason = ?record.reason,
        tool_round = record.tool_round,
        configured = %record.configured,
        "harness step model decision"
    );
    record
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_ctx(current: &str) -> HarnessStepContext {
        HarnessStepContext::supervisor(current)
    }

    #[test]
    fn test_decide_step_model_identity_returns_configured() {
        let ctx = configured_ctx("claude-sonnet-4-6");
        let decision = decide_step_model(&ctx, "claude-sonnet-4-6");
        assert_eq!(decision.model, "claude-sonnet-4-6");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn test_decide_step_model_stays_on_current_model_when_configured_differs() {
        let ctx = configured_ctx("already-escalated");
        let decision = decide_step_model(&ctx, "claude-sonnet-4-6");
        assert_eq!(decision.model, "already-escalated");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn test_decide_step_model_after_verifier_failure_uses_escalate_model() {
        let mut ctx = configured_ctx("claude-sonnet-4-6");
        ctx.latest_verification = Some(GovernedVerificationOutcome::Failed);
        ctx.escalate_model = Some("claude-opus-4-6".to_string());
        let decision = decide_step_model(&ctx, "claude-sonnet-4-6");
        assert_eq!(decision.model, "claude-opus-4-6");
        assert_eq!(decision.reason, StepModelReason::AfterVerifierFailure);
    }

    #[test]
    fn test_decide_step_model_inconclusive_verification_uses_escalate_model() {
        let mut ctx = configured_ctx("haiku");
        ctx.latest_verification = Some(GovernedVerificationOutcome::Inconclusive);
        ctx.escalate_model = Some("sonnet".to_string());
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "sonnet");
        assert_eq!(decision.reason, StepModelReason::AfterVerifierFailure);
    }

    #[test]
    fn test_decide_step_model_verification_failed_review_state_uses_escalate_model() {
        let mut ctx = configured_ctx("haiku");
        ctx.review_state = Some(GovernedReviewState::VerificationFailed);
        ctx.escalate_model = Some("sonnet".to_string());
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "sonnet");
        assert_eq!(decision.reason, StepModelReason::AfterVerifierFailure);
    }

    #[test]
    fn test_decide_step_model_verifier_failure_without_escalate_stays_configured() {
        let mut ctx = configured_ctx("haiku");
        ctx.latest_verification = Some(GovernedVerificationOutcome::Failed);
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "haiku");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn test_decide_step_model_does_not_escalate_on_token_count_or_tool_round() {
        let mut ctx = configured_ctx("haiku");
        ctx.tool_round = 99;
        ctx.escalate_model = Some("sonnet".to_string());
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "haiku");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn test_decide_step_model_operator_never_receives_escalate_model() {
        let mut ctx = configured_ctx("haiku");
        ctx.actor = GovernedActorKind::Operator;
        ctx.latest_verification = Some(GovernedVerificationOutcome::Failed);
        ctx.escalate_model = Some("sonnet".to_string());
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "haiku");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn test_decide_step_model_verifier_actor_never_receives_llm_escalate() {
        let mut ctx = configured_ctx("haiku");
        ctx.actor = GovernedActorKind::Verifier;
        ctx.latest_verification = Some(GovernedVerificationOutcome::Failed);
        ctx.escalate_model = Some("sonnet".to_string());
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "haiku");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn test_decide_step_model_empty_current_falls_back_to_configured() {
        let mut ctx = configured_ctx("");
        let decision = decide_step_model(&ctx, "configured-model");
        assert_eq!(decision.model, "configured-model");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn test_step_model_reason_display_is_not_named_escalate() {
        let reason = format!("{:?}", StepModelReason::AfterVerifierFailure);
        assert!(reason.contains("AfterVerifierFailure"));
        assert!(!reason.contains("Escalate"));
        let json = serde_json::to_string(&StepModelReason::AfterVerifierFailure).unwrap();
        assert_eq!(json, "\"after_verifier_failure\"");
        assert!(!json.contains("escalate"));
    }

    #[test]
    fn round_trip_harness_step_context() {
        let original = HarnessStepContext {
            actor: GovernedActorKind::Supervisor,
            review_state: Some(GovernedReviewState::VerificationFailed),
            latest_verification: Some(GovernedVerificationOutcome::Failed),
            tool_round: 2,
            current_model: "haiku".to_string(),
            escalate_model: Some("sonnet".to_string()),
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: HarnessStepContext = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn round_trip_step_model_decision() {
        let original = StepModelDecision {
            model: "sonnet".to_string(),
            reason: StepModelReason::AfterVerifierFailure,
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: StepModelDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn round_trip_step_model_record() {
        let original = StepModelRecord {
            model: "sonnet".to_string(),
            reason: StepModelReason::Configured,
            actor: GovernedActorKind::Supervisor,
            tool_round: 0,
            actor_id: Some("impulse-agent:api:anthropic:sonnet:sha256-abc".to_string()),
            configured: "haiku".to_string(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: StepModelRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_record_step_model_reuses_actor_id_when_present() {
        let ctx = configured_ctx("haiku");
        let decision = decide_step_model(&ctx, "haiku");
        let record = record_step_model(
            &ctx,
            &decision,
            "haiku",
            Some("impulse-agent:api:anthropic:haiku:sha256-deadbeef"),
        );
        assert_eq!(
            record.actor_id.as_deref(),
            Some("impulse-agent:api:anthropic:haiku:sha256-deadbeef")
        );
        assert_eq!(record.actor, GovernedActorKind::Supervisor);
    }

    #[test]
    fn test_resolve_step_model_returns_decided_model() {
        let mut ctx = configured_ctx("haiku");
        ctx.latest_verification = Some(GovernedVerificationOutcome::Failed);
        ctx.escalate_model = Some("sonnet".to_string());
        let model = resolve_step_model(&ctx, "haiku", None);
        assert_eq!(model, "sonnet");
    }
}
