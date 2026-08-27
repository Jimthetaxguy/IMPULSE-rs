---
title: "ADR-0015: Harness-Owned Step Model Choice"
status: accepted
created: 2026-08-17
updated: 2026-08-23
deciders: [Impulse Maintainers]
---

# ADR-0015: Harness-Owned Step Model Choice

## Status

Accepted.

## Context

Impulse is the harness. A gateway may do keys, rate limits, availability,
audit, or provider failover. An application decides whether inference is
permitted and retains its semantic acceptance gate. Neither may independently
invent a per-step model policy. Final per-step selection stays in the
Impulse-owned deterministic policy, not in an external proxy such as LiteLLM,
OpenRouter, ROSA, or quirewiki.

Today there is no step-level router. The model is resolved once at construction
(`impulse_agent_model` / `IMPULSE_MODEL` / compiled default) and copied into
every `ChatRequest`. The seam already exists: `ChatRequest.model` is per-request;
fill sites copy a session-fixed string.

ADR-0011 already records four-party attestation (worker claim, verifier
evidence, supervisor judgment, operator approval). Model choice for a harness
step belongs beside that arena record. It does not belong on a settlement
ledger and it does not rewrite ADR-0014.

## Decision

1. **The harness owns final step model choice.** `decide_step_model` is the only
   policy function. Applications decide whether an LLM runs and resolve a
   concrete configured/default candidate first. Gateways, `*_BASE_URL`
   overrides, and `monty/routing.rs` do not pick the final model.
2. **Publish a pure policy boundary.** `impulse-step-model` contains only the
   minimal actor/review/verification context, decision, reason, and pure policy.
   It does not depend on `impulse-ops` and has no HTTP, provider failover,
   configuration, token/cost tracking, tracing, persistence, TUI, PTY, SQLite,
   office, or credential authority. Hosts adapt native facts and record the
   returned evidence in their own audit domains.
3. **Fill the existing seam.** Call `decide_step_model` at the three API
   `ChatRequest` fill sites: `Agent::chat`, `run_tool_loop`, and
   `ImpulseAgent::query_stateless`. Start on Supervisor + Ion/API. Worker CLI
   harness model stays opaque. Do not add a picker in `handle_chat` or daemon
   chat.
4. **Policy order is host admissibility, then harness capability.**
   - Host admissibility: the application or provider layer chooses whether
     inference may occur, selects the provider, and supplies only non-empty,
     provider-compatible current/configured/escalation candidates. Availability,
     rate limits, current prices, and provider failover stay outside the pure
     policy. The host must reject a result outside its admitted candidate set.
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
5. **Reason names stay honest.** Do not name a reason `Escalate`. The
   verifier-failure reason is `AfterVerifierFailure`.
6. **Host-owned evidence.** Impulse emits a structured `StepModelRecord` (actor,
   model, reason, tool round, optional `governed_api_actor_id`). Do not attach
   it to `SettlementRecord`. ROSA and other consumers must emit equivalent
   host-owned evidence rather than importing Impulse persistence. Durable
   ledger attachment is later work.
7. **Default N=1.** One model per step. No ensemble, no router fan-out.

## Consequences

- Construction-time model resolution remains the configured default. Per-request
  choice is now an explicit harness function instead of a silent copy.
- Cross-repository consumers can depend on `impulse-step-model` without
  compiling or linking the Impulse application graph.
- A later caller can populate `HarnessStepContext` from governed-task state
  through Impulse's adapter without exporting `impulse-ops` as application
  vocabulary.
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
6. the arena log reuses `governed_api_actor_id` when present;
7. `impulse-step-model` has no dependency on `impulse-rs` or `impulse-ops`; and
8. an external consumer can compile the policy without the Impulse application
   dependency graph.

## ROSA consumer boundary

ROSA has no independent harness-step model policy. It resolves its provider and
concrete configured/default model, calls `impulse-step-model`, then emits a
ROSA-owned trace record before constructing the provider request. ROSA must not
grow `selector.rs` into a second picker and must not depend on the full
`impulse-rs` application for this call.

| Owner | Responsibility | Not owned |
|---|---|---|
| Impulse | `decide_step_model`: actor/verification-aware final model + reason | Whether ROSA runs inference, provider transport, ROSA audit persistence |
| ROSA | Inference permission, provider/default resolution, admitted candidates, host trace | Independent step-model policy |
| Provider adapter | Serialize and send the exact selected model | Silent defaulting after policy, provider failover inside policy |

James locked the single-picker boundary on 2026-08-19. The 2026-08-23 crate
extraction makes that decision portable instead of requiring ROSA to import the
Impulse product.

## Related Documents

- [`0011-governed-task-run-lifecycle.md`](0011-governed-task-run-lifecycle.md)
- [`0012-daemon-owned-governed-runtime-producers.md`](0012-daemon-owned-governed-runtime-producers.md)
- [`../../impulse-rs/impulse-step-model/README.md`](../../impulse-rs/impulse-step-model/README.md)
- [`../../VISION.md`](../../VISION.md)
- [`../spec/RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md)


## Addendum: ROSA imports `decide_step_model`

ROSA has no harness-step model policy. Impulse already owns that policy in `decide_step_model`. ROSA must call that function. ROSA must not grow `selector.rs` into a second picker.

Codee mapped rosa-renew-build @ `0f1d4e2` against Impulse main `76ab525`:

| Who | File / fn | What it sets | Step policy? |
|---|---|---|---|
| Impulse | `step_model.rs` `decide_step_model` | `ChatRequest.model` | Yes. Admissibility, then verifier-failure escalate, then stay on `current_model`. |
| Impulse | `Agent::chat`, `run_tool_loop`, `ImpulseAgent::query_stateless` | Calls the hook | Yes. |
| ROSA | `selector.rs` `select()` | `BackendId` from `ROSA_BACKEND` / catalog score | No. Exported. No production caller. Same class as `monty/routing.rs`. |
| ROSA | `team.rs` `AgentRole::to_request` | `RunRequest.model` from role model or `Team.default_model` | No. Construction-time string. |
| ROSA | anthropic/gemini `start_run` | empty → compiled `DEFAULT_MODEL`; same string for the tool loop | No. |
| ROSA | `dispatch_spawn_subagent` | copies `parent_model` | No. Tool-in-loop inherit. |

`spawn_subagent` is a tool-in-loop. It is not an A2A row.

Jim locked 2026-08-19: Impulse stays the only picker. ROSA has no step picker and should call Impulse's `decide_step_model`.

Do not add LiteLLM, OpenRouter, or a ROSA sibling of `decide_step_model`.
