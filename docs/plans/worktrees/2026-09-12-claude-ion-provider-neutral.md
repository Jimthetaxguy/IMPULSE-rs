---
title: Ion Provider Neutrality and Context Budget
description: Work card for claude/ion-provider-neutral-20260912 (Stage 1b-A — provider-neutral Ion tool calls plus a context budget on the loop contract)
updated: 2026-09-12
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, ion, llm-backends, loop-contract, adr-0017, context-budget]
---

# Ion Provider Neutrality and Context Budget

## Lane Facts

- Owner: Claude (Fable 5.1), Stage 1b-A of `docs/plans/2026-09-02-impulse-next-stages.md`.
- Role: implementation lane, one stage slice.
- Branch: `claude/ion-provider-neutral-20260912`.
- Worktree: `.worktrees/ion-provider-neutral-20260912` (repository-relative).
- Base: `origin/main` at `7c2086c`.
- Owned paths:
  - `impulse-rs/src/llm_backends/{mod,anthropic}.rs`
  - `impulse-rs/src/loop_contract.rs` (additive only)
  - `impulse-rs/src/ion_repl/chat.rs` (`from_env` and module docs only)
  - `docs/decisions/0017-canonical-loop-contract.md` (dated addendum, no status change)
  - `docs/superpowers/specs/2026-09-01-loop-contract-design.md` (dated addendum)
  - `impulse-rs/impulse-ion/TUI_SPEC.md` (one T9 entry), `CONTEXT.md` (two glossary entries)
  - this work card
- Blocked/shared paths honored: `impulse-rs/src/ion_repl/{tool_document,registry,tools,mod}.rs`
  and `src/tooling/**` (sibling lane `claude/ion-documents-memory-20260912`), `src/daemon/**`,
  `impulse-desktop/**`, `Cargo.toml`, `Cargo.lock`, `.github/**`, `CLAUDE.md`, `AGENTS.md`.
- **Ownership exception (flagged):** `impulse-rs/src/state/governed_task.rs` is blocked, but one
  line had to change — `LoopReport` gained a field, and that file constructs a `LoopReport`
  literal, so the crate does not compile without `compactions: 0` plus its explanatory comment.
  No behavior change: the field is `skip_serializing_if` zero, so the governed report serializes
  byte-identically and every stored `loop_report_digest` still reproduces.
- Plan/spec: Stage 1b in `docs/plans/2026-09-02-impulse-next-stages.md`; ADR-0017 and
  `docs/superpowers/specs/2026-09-01-loop-contract-design.md`, both with 2026-09-12 addenda.
- Verification (isolated `CARGO_TARGET_DIR`, per the shared-target-dir memory note):
  `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `cargo build --no-default-features`, `python3 ../docs/validate_docs.py --all`.

## Decisions

- 2026-09-12: Make the wire format an explicit parameter (`WireFormat` + `format_messages_for`)
  and delete `BaseProvider::format_messages` outright rather than leaving it beside the new
  path. The old function read only `Message::content`, so a tool-use turn sent through OpenAI or
  MiniMax reached the model as an empty assistant message followed by an empty user message. A
  new provider must now choose a shape that carries tool blocks; it cannot inherit one that
  discards them.
- 2026-09-12: Treat MiniMax as OpenAI-shaped. Its `chatcompletion_v2` request and response
  envelopes match OpenAI's apart from two things handled by parameters — `max_tokens` is always
  sent, and the response omits `model` — so both providers share
  `build_openai_style_body`/`OpenAiStyleResponse`/`openai_style_chat_response` instead of
  carrying a second near-identical set of structs.
- 2026-09-12: A `function.arguments` payload that is not valid JSON is a malformed response, not
  an empty call: fail with `AgentError::ApiResponse` rather than coercing it to `{}` and letting
  the tool execute with an input the model never sent.
- 2026-09-12: `ToolResult::is_error` is sent to OpenAI as plain content, with no invented marker.
  The wire has no counterpart field, and the loop breaker reads the executor's result rather than
  the wire form, so same-error detection is unaffected.
- 2026-09-12: `IMPULSE_PROVIDER` reuses `agent::ImpulseProvider::{parse, resolve_api_key,
  default_model}` rather than adding a second parser. It selects a *transport* only — ADR-0015's
  step model and `IMPULSE_MODEL` remain the only model pickers.
- 2026-09-12: An unknown `IMPULSE_PROVIDER` fails closed. `ChatState::try_from_env` returns the
  typed `ProviderSelectionError`; the infallible `from_env` (whose signature is fixed by the
  blocked `ion_repl/mod.rs` call site) carries it to the first turn through
  `UnconfiguredProvider`, which refuses every request with the typed reason. This is the same
  deferral the missing-API-key path already uses, not a stub: it never succeeds and never returns
  fabricated content.
- 2026-09-12: One ephemeral `cache_control` breakpoint, at the end of the system-and-tools prefix
  — on the system block when a system prompt exists (which covers the tools ordered before it),
  on the last tool otherwise, and never inside `messages`, which changes every round. On by
  default; `AnthropicProvider::with_prompt_cache(false)` disables it. Request-body construction
  was extracted into `build_anthropic_body` so the wire JSON is assertable without a network call.
- 2026-09-12: Measure the context budget in characters, not tokens — one number that is exact,
  reproducible, and identical across three providers that tokenize differently. 200,000 is about a
  quarter of the smallest supported window, leaving room for the system prompt, the tool schemas,
  and the reply, none of which the measurement counts.
- 2026-09-12: Compaction and measurement live in `llm_backends`, not `loop_contract`, preserving
  ADR-0017 rule 6 (no provider, tool, or daemon types in that module). `loop_contract` declares
  the limit, counts compactions, and names the trip.
- 2026-09-12: `LoopReport::compactions` is `skip_serializing_if` zero, so the additive field stays
  out of every already-persisted governed `loop_report_digest` and
  `GOVERNED_BUILDER_LOOP_VERSION` does not have to move.
- 2026-09-12: A `ContextBudget` trip surfaces through the existing
  `AgentError::ToolLoopStalled { trip }` rather than a new variant, leaving `src/error.rs`
  (unowned by this lane) untouched. Known doc drift: that variant's comment still describes only
  the no-progress detectors and should be widened by the lane that next owns the file.

## Changes

**`impulse-rs/src/llm_backends/anthropic.rs`**
- `WireFormat` + `format_messages_for`; `format_openai_messages`; `openai_tools_value`.
- `BaseProvider::format_messages` removed (no callers outside this file).
- `build_anthropic_body(request, cache_system_and_tools)` extracted from `AnthropicProvider::chat`;
  `AnthropicProvider` is now a named-field struct with `with_prompt_cache`/`prompt_cache_enabled`.
- `OpenAiStyleResponse` family + `build_openai_style_body` + `openai_style_chat_response`, shared
  by OpenAI and MiniMax; the per-provider `OpenAi*`/`Minimax*` structs are gone.

**`impulse-rs/src/llm_backends/mod.rs`**
- `PROVIDER_ENV`, `ProviderSelectionError`, `provider_from_env`, `build_provider`,
  `UnconfiguredProvider`.
- `message_chars`, `history_chars`, `compaction_stub`, `is_compaction_stub`,
  `enforce_context_budget`; called once per round in `run_tool_loop`, after `begin_round` and
  before the provider call.

**`impulse-rs/src/loop_contract.rs`**
- `ION_DEFAULT_MAX_CONTEXT_CHARS`, `LoopBudget::max_context_chars`,
  `LoopContractError::ZeroContextBudget` + `validate`, `LoopTrip::ContextBudget` + `Display`,
  `LoopReport::compactions`, `LoopBreaker::{observe_compaction, compactions}`.

**`impulse-rs/src/ion_repl/chat.rs`**
- `try_from_env` (typed) and `from_env` (deferring wrapper); module docs updated.

**`impulse-rs/src/state/governed_task.rs`** — one field in one struct literal (see the ownership
exception above).

## Compaction algorithm (as implemented)

Once per round, before the provider call:

1. No `max_context_chars` set → return.
2. `history_chars` = prose + each call's id, name, and canonical-JSON input + each result's id and
   content. At or under budget → return.
3. Replace tool-result content with `[compacted <N> chars from tool '<name>']`, **oldest first**
   (message order, then result order within a message), stopping as soon as the running total is
   under budget.
4. Skip a result that is already a stub, or whose stub would not be shorter than what it replaces.
5. Still over budget → `LoopTrip::ContextBudget { chars, limit }`.

`tool_use_id` is never modified, so provider-side pairing stays valid. Prose is never compacted.
Only the caller's `working` copy is mutated; `self.history` is assigned on the success path only.

## Verification

Recorded on this checkout at the SHA in the PR body; re-run the gate before citing any aggregate.
