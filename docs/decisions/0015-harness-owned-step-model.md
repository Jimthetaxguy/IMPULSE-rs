---
title: "ADR-0015: Harness-Owned Step Model Choice"
status: draft
created: 2026-08-17
deciders: [Impulse Maintainers]
---

# ADR-0015: Harness-Owned Step Model Choice

## Status

Proposed.

## Context

Impulse is the harness. A gateway may do keys, rate limits, audit, or provider
failover. It must not pick the model. Routing stays in Impulse, not in an
external proxy such as LiteLLM, OpenRouter, or quirewiki.

Today there is no step-level router. The model is resolved once at construction
(`impulse_agent_model` / `IMPULSE_MODEL` / compiled default) and copied into
every `ChatRequest`. The seam already exists: `ChatRequest.model` is per-request;
fill sites copy a session-fixed string.

ADR-0011 already records four-party attestation (worker claim, verifier
evidence, supervisor judgment, operator approval). Model choice for a harness
step belongs beside that arena record. It does not belong on a settlement
ledger and it does not rewrite ADR-0014.

## Decision

1. **The harness owns step model choice.** `decide_step_model` is the only
   policy function. Gateways, `*_BASE_URL` overrides, and `monty/routing.rs`
   do not pick the model.
2. **Fill the existing seam.** Call `decide_step_model` at the three API
   `ChatRequest` fill sites: `Agent::chat`, `run_tool_loop`, and
   `ImpulseAgent::query_stateless`. Start on Supervisor + Ion/API. Worker CLI
   harness model stays opaque. Do not add a picker in `handle_chat` or daemon
   chat.
3. **Policy order is admissibility, then capability, then cost.**
   - Admissibility: Operator never gets a model pick. Verifier stays daemon
     commands, never an LLM. Supervisor stays API-only, tool-free, and
     history-free. A model cannot accept a governed task.
   - Capability: return the configured model, except when latest verification
     is `Failed`/`Inconclusive` or `review_state` is `VerificationFailed`. Then
     the optional configured escalate model (`impulse_agent_escalate_model`
     when a caller has that value) may be used. Escalate from verifier
     failure, not token count. Do not read `token_tracker`.
   - Stay on `current_model` unless verifier/attestation failed.
   - v0 may be identity (`Configured`) when no escalate model is set, as long
     as the hook is actually called and logged.
4. **Reason names stay honest.** Do not name a reason `Escalate`. The
   verifier-failure reason is `AfterVerifierFailure`.
5. **Arena log beside ADR-0011.** Emit a structured `StepModelRecord` (actor,
   model, reason, tool round, optional `governed_api_actor_id`). Do not attach
   it to `SettlementRecord`. Durable ledger attachment is later work.
6. **Default N=1.** One model per step. No ensemble, no router fan-out.

## Consequences

- Construction-time model resolution remains the configured default. Per-request
  choice is now an explicit harness function instead of a silent copy.
- A later caller can populate `HarnessStepContext` from governed-task state
  without inventing ExecutionPosture, AuthorityEnvelope, or a new crate.
- Optional `impulse_agent_escalate_model` is a State config key copied onto
  `HarnessStepContext` by `resolve_from_config`, so CLI query, TUI, and the
  daemon cache share the same escalate model. Policy is unchanged.
- Worker CLI protocol is unchanged. PRs that add provider base-URL overrides
  or settlement records are independent and must not be stacked under this
  decision.

## Validation

This decision is represented when tests prove:

1. `decide_step_model` is called at the three API fill sites;
2. v0 is identity unless verifier/attestation failed and an escalate model is
   set;
3. token count / tool-round volume does not escalate;
4. Operator and Verifier actors do not receive an escalate model;
5. `StepModelRecord` / decision types round-trip; and
6. the arena log reuses `governed_api_actor_id` when present.

## Related Documents

- [`0011-governed-task-run-lifecycle.md`](0011-governed-task-run-lifecycle.md)
- [`0012-daemon-owned-governed-runtime-producers.md`](0012-daemon-owned-governed-runtime-producers.md)
- [`../../VISION.md`](../../VISION.md)
- [`../spec/RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md)
