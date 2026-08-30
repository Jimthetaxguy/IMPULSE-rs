//! Pure harness-owned per-step model choice (ADR-0015).
//!
//! This crate is deliberately smaller than the Impulse application and its
//! control-plane protocol. A host decides whether inference may occur, resolves
//! provider-compatible candidates, invokes [`decide_step_model`], and records
//! the returned evidence in its own audit domain.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// Actor class relevant to model admissibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepActor {
    System,
    Worker,
    Verifier,
    Supervisor,
    Operator,
}

/// Review signal that can affect a model decision.
///
/// Other review states are intentionally omitted because policy v0 only reacts
/// to failed verification. Hosts map all unrelated states to `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepReviewSignal {
    VerificationFailed,
}

/// Latest verification signal relevant to model choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepVerificationSignal {
    Passed,
    Failed,
    Inconclusive,
}

/// Minimal per-step facts used by the deterministic policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepModelContext {
    pub actor: StepActor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<StepReviewSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<StepVerificationSignal>,
    pub tool_round: usize,
    pub current_model: String,
    /// Optional escalation candidate already admitted by the host for the
    /// selected provider. The policy never changes providers itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalate_model: Option<String>,
}

impl StepModelContext {
    /// Default API worker context with no review or verification signal.
    pub fn worker(current_model: impl Into<String>) -> Self {
        Self {
            actor: StepActor::Worker,
            review: None,
            verification: None,
            tool_round: 0,
            current_model: current_model.into(),
            escalate_model: None,
        }
    }

    /// Default API supervisor context with no review or verification signal.
    pub fn supervisor(current_model: impl Into<String>) -> Self {
        Self {
            actor: StepActor::Supervisor,
            review: None,
            verification: None,
            tool_round: 0,
            current_model: current_model.into(),
            escalate_model: None,
        }
    }
}

/// Why the harness selected `model` for this step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepModelReason {
    Configured,
    AfterVerifierFailure,
}

/// Provider-neutral model decision for one step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepModelDecision {
    pub model: String,
    pub reason: StepModelReason,
}

fn verifier_or_attestation_failed(ctx: &StepModelContext) -> bool {
    matches!(
        ctx.verification,
        Some(StepVerificationSignal::Failed | StepVerificationSignal::Inconclusive)
    ) || matches!(ctx.review, Some(StepReviewSignal::VerificationFailed))
}

/// Choose the model for one harness step.
///
/// The host must resolve `configured` before calling and must only supply an
/// `escalate_model` valid for the already-selected provider. Operator and
/// Verifier actors never receive a different model. Policy v0 ignores token
/// count, cost, availability, and tool-round volume.
pub fn decide_step_model(ctx: &StepModelContext, configured: &str) -> StepModelDecision {
    if matches!(ctx.actor, StepActor::Operator | StepActor::Verifier) {
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

fn stay_configured(ctx: &StepModelContext, configured: &str) -> StepModelDecision {
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

    fn configured_ctx(current: &str) -> StepModelContext {
        StepModelContext::supervisor(current)
    }

    #[test]
    fn test_decide_step_model_identity_returns_current_model() {
        let decision = decide_step_model(&configured_ctx("sonnet"), "sonnet");
        assert_eq!(decision.model, "sonnet");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }


    #[test]
    fn test_decide_step_model_worker_stays_on_current_when_verifier_has_not_failed() {
        // Ion/API fill sites use Worker with no verification yet. Escalate must
        // stay unused until verifier/attestation actually failed.
        let mut ctx = StepModelContext::worker("sonnet");
        ctx.escalate_model = Some("opus".to_string());
        let decision = decide_step_model(&ctx, "haiku");
        assert_eq!(decision.model, "sonnet");
        assert_eq!(decision.reason, StepModelReason::Configured);
    }


    #[test]
    fn test_decide_step_model_current_model_wins_over_configured_fallback() {
        let decision = decide_step_model(&configured_ctx("already-selected"), "default");
        assert_eq!(decision.model, "already-selected");
    }

    #[test]
    fn test_decide_step_model_failed_verification_uses_admitted_escalation_model() {
        let mut ctx = configured_ctx("sonnet");
        ctx.verification = Some(StepVerificationSignal::Failed);
        ctx.escalate_model = Some("opus".to_string());
        let decision = decide_step_model(&ctx, "sonnet");
        assert_eq!(decision.model, "opus");
        assert_eq!(decision.reason, StepModelReason::AfterVerifierFailure);
    }

    #[test]
    fn test_decide_step_model_inconclusive_verification_uses_admitted_escalation_model() {
        let mut ctx = configured_ctx("sonnet");
        ctx.verification = Some(StepVerificationSignal::Inconclusive);
        ctx.escalate_model = Some("opus".to_string());
        assert_eq!(decide_step_model(&ctx, "sonnet").model, "opus");
    }

    #[test]
    fn test_decide_step_model_review_failure_uses_admitted_escalation_model() {
        let mut ctx = configured_ctx("sonnet");
        ctx.review = Some(StepReviewSignal::VerificationFailed);
        ctx.escalate_model = Some("opus".to_string());
        assert_eq!(decide_step_model(&ctx, "sonnet").model, "opus");
    }

    #[test]
    fn test_decide_step_model_failure_without_escalation_stays_current() {
        let mut ctx = configured_ctx("sonnet");
        ctx.verification = Some(StepVerificationSignal::Failed);
        assert_eq!(decide_step_model(&ctx, "sonnet").model, "sonnet");
    }

    #[test]
    fn test_decide_step_model_blank_escalation_stays_current() {
        let mut ctx = configured_ctx("sonnet");
        ctx.verification = Some(StepVerificationSignal::Failed);
        ctx.escalate_model = Some("   ".to_string());
        assert_eq!(decide_step_model(&ctx, "sonnet").model, "sonnet");
    }

    #[test]
    fn test_decide_step_model_tool_round_does_not_escalate() {
        let mut ctx = configured_ctx("sonnet");
        ctx.tool_round = 99;
        ctx.escalate_model = Some("opus".to_string());
        assert_eq!(decide_step_model(&ctx, "sonnet").model, "sonnet");
    }

    #[test]
    fn test_decide_step_model_operator_and_verifier_never_receive_escalation() {
        for actor in [StepActor::Operator, StepActor::Verifier] {
            let mut ctx = configured_ctx("sonnet");
            ctx.actor = actor;
            ctx.verification = Some(StepVerificationSignal::Failed);
            ctx.escalate_model = Some("opus".to_string());
            assert_eq!(decide_step_model(&ctx, "sonnet").model, "sonnet");
        }
    }

    #[test]
    fn test_decide_step_model_empty_current_uses_configured_fallback() {
        let decision = decide_step_model(&configured_ctx(""), "configured");
        assert_eq!(decision.model, "configured");
    }

    #[test]
    fn test_step_model_types_serde_round_trip() {
        let mut context = StepModelContext::worker("sonnet");
        context.review = Some(StepReviewSignal::VerificationFailed);
        context.verification = Some(StepVerificationSignal::Failed);
        context.escalate_model = Some("opus".to_string());
        let context_json = serde_json::to_string(&context).unwrap();
        let recovered_context: StepModelContext = serde_json::from_str(&context_json).unwrap();
        assert_eq!(recovered_context, context);

        let decision = decide_step_model(&context, "sonnet");
        let decision_json = serde_json::to_string(&decision).unwrap();
        let recovered_decision: StepModelDecision = serde_json::from_str(&decision_json).unwrap();
        assert_eq!(recovered_decision, decision);
        assert_eq!(
            decision_json,
            r#"{"model":"opus","reason":"after_verifier_failure"}"#
        );
    }
}
