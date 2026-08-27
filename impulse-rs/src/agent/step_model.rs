//! Adapter from Impulse governed-task types onto [`impulse_step_model`].
//!
//! Policy lives in `impulse-step-model`. This module maps `Governed*` enums
//! into the crate's local classification types, re-exports the decision API,
//! and keeps arena logging (`record_step_model` / `resolve_step_model`) here
//! so the thin crate has no tracing or ledger writes.

use impulse_ops::governed_task::{
    GovernedActorKind, GovernedReviewState, GovernedVerificationOutcome,
};
pub use impulse_step_model::{
    decide_step_model, ActorKind, HarnessStepContext, ReviewState, StepModelDecision,
    StepModelReason, VerificationOutcome,
};

/// Additive arena record logged beside ADR-0011 four-party attestation.
/// Durable ledger attachment is later work; this is not a SettlementRecord.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StepModelRecord {
    pub model: String,
    pub reason: StepModelReason,
    pub actor: ActorKind,
    pub tool_round: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    pub configured: String,
}

pub fn step_actor_from_governed(kind: GovernedActorKind) -> ActorKind {
    match kind {
        GovernedActorKind::System => ActorKind::System,
        GovernedActorKind::Worker => ActorKind::Worker,
        GovernedActorKind::Verifier => ActorKind::Verifier,
        GovernedActorKind::Supervisor => ActorKind::Supervisor,
        GovernedActorKind::Operator => ActorKind::Operator,
    }
}

pub fn step_review_from_governed(state: GovernedReviewState) -> ReviewState {
    match state {
        GovernedReviewState::VerificationFailed => ReviewState::VerificationFailed,
        _ => ReviewState::Other,
    }
}

pub fn step_verification_from_governed(
    outcome: GovernedVerificationOutcome,
) -> VerificationOutcome {
    match outcome {
        GovernedVerificationOutcome::Passed => VerificationOutcome::Passed,
        GovernedVerificationOutcome::Failed => VerificationOutcome::Failed,
        GovernedVerificationOutcome::Inconclusive => VerificationOutcome::Inconclusive,
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

    #[test]
    fn adapter_maps_governed_verification_failed_into_escalate() {
        let mut ctx = HarnessStepContext::supervisor("haiku");
        ctx.latest_verification = Some(step_verification_from_governed(
            GovernedVerificationOutcome::Failed,
        ));
        ctx.escalate_model = Some("sonnet".to_string());
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "sonnet");
        assert_eq!(decision.reason, StepModelReason::AfterVerifierFailure);
    }

    #[test]
    fn adapter_maps_operator_so_policy_stays_configured() {
        let mut ctx = HarnessStepContext::supervisor("haiku");
        ctx.actor = step_actor_from_governed(GovernedActorKind::Operator);
        ctx.latest_verification = Some(step_verification_from_governed(
            GovernedVerificationOutcome::Failed,
        ));
        ctx.escalate_model = Some("sonnet".to_string());
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "haiku");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn test_record_step_model_reuses_actor_id_when_present() {
        let ctx = HarnessStepContext::supervisor("haiku");
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
        assert_eq!(record.actor, ActorKind::Supervisor);
    }

    #[test]
    fn test_resolve_step_model_returns_decided_model() {
        let mut ctx = HarnessStepContext::supervisor("haiku");
        ctx.latest_verification = Some(step_verification_from_governed(
            GovernedVerificationOutcome::Failed,
        ));
        ctx.escalate_model = Some("sonnet".to_string());
        let model = resolve_step_model(&ctx, "haiku", None);
        assert_eq!(model, "sonnet");
    }

    #[test]
    fn round_trip_step_model_record() {
        let original = StepModelRecord {
            model: "sonnet".to_string(),
            reason: StepModelReason::Configured,
            actor: ActorKind::Supervisor,
            tool_round: 0,
            actor_id: Some("impulse-agent:api:anthropic:sonnet:sha256-abc".to_string()),
            configured: "haiku".to_string(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: StepModelRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }
}
