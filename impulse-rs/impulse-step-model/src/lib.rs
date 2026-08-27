//! Harness-owned per-step model choice.
//!
//! Hosts map their own actor and verifier facts into [`HarnessStepContext`].
//! This crate never reads token counts, prices, or provider health.
//! No HTTP, TUI, SQLite, token tracker, config loading, ledger writes, or impulse-ops.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    System,
    Worker,
    Verifier,
    Supervisor,
    Operator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    VerificationFailed,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationOutcome {
    Passed,
    Failed,
    Inconclusive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessStepContext {
    pub actor: ActorKind,
    pub review_state: Option<ReviewState>,
    pub latest_verification: Option<VerificationOutcome>,
    pub tool_round: usize,
    pub current_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalate_model: Option<String>,
    /// Optional allow-list. When set and non-empty, escalate must be listed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_models: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepModelReason {
    Configured,
    AfterVerifierFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepModelDecision {
    pub model: String,
    pub reason: StepModelReason,
}

impl HarnessStepContext {
    pub fn ion_api(current_model: impl Into<String>) -> Self {
        Self {
            actor: ActorKind::Worker,
            review_state: None,
            latest_verification: None,
            tool_round: 0,
            current_model: current_model.into(),
            escalate_model: None,
            allowed_models: None,
        }
    }

    pub fn supervisor(current_model: impl Into<String>) -> Self {
        Self {
            actor: ActorKind::Supervisor,
            review_state: None,
            latest_verification: None,
            tool_round: 0,
            current_model: current_model.into(),
            escalate_model: None,
            allowed_models: None,
        }
    }
}

fn verifier_or_attestation_failed(ctx: &HarnessStepContext) -> bool {
    matches!(
        ctx.latest_verification,
        Some(VerificationOutcome::Failed) | Some(VerificationOutcome::Inconclusive)
    ) || matches!(ctx.review_state, Some(ReviewState::VerificationFailed))
}

fn model_allowed(ctx: &HarnessStepContext, model: &str) -> bool {
    match ctx.allowed_models.as_ref() {
        None => true,
        Some(list) if list.is_empty() => true,
        Some(list) => list.iter().any(|allowed| allowed == model),
    }
}

/// Admissibility first: Operator and Verifier never receive a different model.
/// Then stay on `current_model` unless verification failed and `escalate_model` is set.
pub fn decide_step_model(ctx: &HarnessStepContext, configured: &str) -> StepModelDecision {
    if matches!(ctx.actor, ActorKind::Operator | ActorKind::Verifier) {
        return stay_configured(ctx, configured);
    }
    if verifier_or_attestation_failed(ctx) {
        if let Some(escalate) = ctx
            .escalate_model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty())
        {
            if model_allowed(ctx, escalate) {
                return StepModelDecision {
                    model: escalate.to_string(),
                    reason: StepModelReason::AfterVerifierFailure,
                };
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_ctx(current: &str) -> HarnessStepContext {
        HarnessStepContext::supervisor(current)
    }

    #[test]
    fn identity_returns_configured() {
        let ctx = configured_ctx("claude-sonnet-4-6");
        let decision = decide_step_model(&ctx, "claude-sonnet-4-6");
        assert_eq!(decision.model, "claude-sonnet-4-6");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn stays_on_current_when_configured_differs() {
        let ctx = configured_ctx("already-escalated");
        let decision = decide_step_model(&ctx, "claude-sonnet-4-6");
        assert_eq!(decision.model, "already-escalated");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn after_verifier_failure_uses_escalate_model() {
        let mut ctx = configured_ctx("claude-sonnet-4-6");
        ctx.latest_verification = Some(VerificationOutcome::Failed);
        ctx.escalate_model = Some("claude-opus-4-6".to_string());
        let decision = decide_step_model(&ctx, "claude-sonnet-4-6");
        assert_eq!(decision.model, "claude-opus-4-6");
        assert_eq!(decision.reason, StepModelReason::AfterVerifierFailure);
    }

    #[test]
    fn inconclusive_verification_uses_escalate_model() {
        let mut ctx = configured_ctx("haiku");
        ctx.latest_verification = Some(VerificationOutcome::Inconclusive);
        ctx.escalate_model = Some("sonnet".to_string());
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "sonnet");
        assert_eq!(decision.reason, StepModelReason::AfterVerifierFailure);
    }

    #[test]
    fn verification_failed_review_state_uses_escalate_model() {
        let mut ctx = configured_ctx("haiku");
        ctx.review_state = Some(ReviewState::VerificationFailed);
        ctx.escalate_model = Some("sonnet".to_string());
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "sonnet");
        assert_eq!(decision.reason, StepModelReason::AfterVerifierFailure);
    }

    #[test]
    fn verifier_failure_without_escalate_stays_configured() {
        let mut ctx = configured_ctx("haiku");
        ctx.latest_verification = Some(VerificationOutcome::Failed);
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "haiku");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn does_not_escalate_on_tool_round() {
        let mut ctx = configured_ctx("haiku");
        ctx.tool_round = 99;
        ctx.escalate_model = Some("sonnet".to_string());
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "haiku");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn operator_never_receives_escalate_model() {
        let mut ctx = configured_ctx("haiku");
        ctx.actor = ActorKind::Operator;
        ctx.latest_verification = Some(VerificationOutcome::Failed);
        ctx.escalate_model = Some("sonnet".to_string());
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "haiku");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn verifier_actor_never_receives_llm_escalate() {
        let mut ctx = configured_ctx("haiku");
        ctx.actor = ActorKind::Verifier;
        ctx.latest_verification = Some(VerificationOutcome::Failed);
        ctx.escalate_model = Some("sonnet".to_string());
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "haiku");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn empty_current_falls_back_to_configured() {
        let ctx = configured_ctx("  ");
        let decision = decide_step_model(&ctx, "configured-model");
        assert_eq!(decision.model, "configured-model");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }

    #[test]
    fn reason_display_is_not_named_escalate() {
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
            actor: ActorKind::Supervisor,
            review_state: Some(ReviewState::VerificationFailed),
            latest_verification: Some(VerificationOutcome::Failed),
            tool_round: 2,
            current_model: "haiku".to_string(),
            escalate_model: Some("sonnet".to_string()),
            allowed_models: None,
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
    fn allowed_models_blocks_escalate_not_in_list() {
        let mut ctx = configured_ctx("haiku");
        ctx.latest_verification = Some(VerificationOutcome::Failed);
        ctx.escalate_model = Some("sonnet".to_string());
        ctx.allowed_models = Some(vec!["haiku".to_string()]);
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "haiku");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }
}
