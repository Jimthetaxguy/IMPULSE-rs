---
title: Codex agent truth parity lane
description: Preserve structured live-agent state when terminal telemetry overlays daemon workbench truth.
updated: 2026-07-11
type: doc
category: planning
phase: all
status: complete
audience: builders
tags: [worktree, lane, daemon, telemetry, agents, coordination]
---

# Codex Agent Truth Parity

## Lane Facts
- Owner: Codex
- Role: Repair the daemon-backed agent-state projection that future lifecycle controls consume.
- Branch: `codex/agent-truth-parity`
- Worktree: `/Users/jamespustorino/code/IMPULSE-rs/.worktrees/agent-truth-parity`
- Owned paths:
  - `impulse-rs/src/ops_workbench.rs`
  - `docs/plans/worktrees/2026-07-11-codex-agent-truth-parity.md`
  - `_working-files/20260711-050350-codex-agent-truth-parity.md`
- Blocked/shared paths:
  - Claude's Ion T1-T9, env-scrub, ApprovalGrant, and FileWrite guardrail work is now committed through `a5184e2`; those paths remain outside this lane.
  - Claude's current daemon-agent timeout/cache work under `impulse-rs/src/{agent,daemon,error,mcp}` is an active semantic neighbor and must stabilize before merge.
  - `AGENTS.md`, `CLAUDE.md`, `CONTEXT.md`, the canonical contract, protocol docs, and docs indexes remain integration-owned shared files.
- Plan/spec: `docs/spec/USER-STORY-MAP.md` ST-13 and `docs/spec/TEST-TRACEABILITY.md` agent-control gap.
- Verification: `cargo test -p impulse-rs ops_workbench`, then `cargo check --workspace`, `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, and `cargo fmt --check` from `impulse-rs/`.
- Latest status: forward-ported onto live `main` at `a5184e2` as `dbac22f`; verification is green, but merge waits for Claude's active daemon-agent concurrency lane to stabilize.

## Goal and User-Visible Outcome
Preserve blocked/working status, role/group, tool activity, diff summary, and machine target when a live terminal agent overlays its durable daemon session. The Agents surface can then render and act on one authoritative state instead of reconstructing missing facts.

## Non-Goals
- No Ion REPL or Pi/MiniMax gate changes.
- No new UI controls, restart semantics, or protocol variants in this slice.
- No dependency or Cargo changes.
- No removal of the legacy string status field.

## Acceptance Criteria
- A telemetry agent matched to a durable session preserves the full structured manager fields.
- `AgentStatus::Blocked { reason }` and `AgentStatus::Working { task }` payloads survive overlay exactly.
- Existing identity, legacy status, context, recent file/tool, warning, and ephemeral merge behavior remains unchanged.
- A regression test exercises the real matched-session merge path, not only serde round-tripping.
- The lane verification gate is recorded before handoff.

## Decisions
- 2026-07-11: Repair the source-of-truth projection before adding lifecycle controls; controls built on lossy state would create another split-brain boundary.
- 2026-07-11: Keep backward-compatible legacy status strings while preserving the structured status in parallel.
- 2026-07-11: `TerminalOpsReport` has no schema version or field-presence markers, so serde-defaulted structured fields cannot safely clear durable metadata. Populated live values win; omitted role/group/tool/diff/target values preserve durable state.
- 2026-07-11: When `agent_status` defaults to `Idle`, parse every form emitted by `AgentStatus::to_legacy_string()` from the legacy status field. Unknown legacy strings remain available in `status` and do not erase structured daemon state.
- 2026-07-11: Explicit clearing of optional structured metadata is deferred until the wire contract can distinguish omission from an intentional empty value.

## Changes
- `overlay_agent_runtime` now preserves populated `agent_status`, role, group, tool invocations, diff summary, and machine target on matched daemon sessions.
- Legacy lifecycle strings (`starting`, `idle`, `working:`, `blocked:`, `interrupted`, `completed`) are converted back into structured status when older publishers omit `agent_status`.
- Compatibility tests prove old JSON omission preserves durable state; matched telemetry tests prove rich fields, identity, legacy fields, context, files/tools, warnings, and ephemeral semantics.

## Tests
- PASS: `cargo test -p impulse-rs ops_workbench::tests` — 12 passed.
- PASS: `cargo check --workspace`.
- PASS: `cargo clippy --workspace -- -D warnings`.
- PASS: `cargo fmt --all -- --check` and `git diff --check`.
- PASS: `cargo test --workspace -- --skip test_reconciled_clean_archive_has_contracts_snapshot` — all runnable workspace tests passed; the skipped proof test is worktree-hostile because its required gitignored archive exists only in the canonical checkout.
- KNOWN EXTERNAL BLOCKER: unskipped `cargo test --workspace` has exactly one failure, `agent_registry::tests::test_reconciled_clean_archive_has_contracts_snapshot`, for that absent worktree archive fixture.
- PASS on the live forward-port: `cargo clippy --workspace --all-targets -- -D warnings`; the four historical Ion test warnings no longer reproduce on `a5184e2` plus `dbac22f`/`ae8fcd0`.
- KNOWN EXTERNAL BLOCKER: `python3 docs/validate_docs.py --contract` reports 17 pre-existing documents older than the 120-day threshold; none is owned by this lane.
- KNOWN EXTERNAL BLOCKER: `python3 docs/validate_docs.py --all` additionally reports unrelated `research/2026-06-30-sites-map-phase1-spec.md` status `approved-for-planning` outside the allowed vocabulary. The new lane card itself validates.
- REVIEW: final local architecture review returned `NO_FINDINGS`; MiniMax-M2.7 found the empty `working:` / `blocked:` inverse edge, which is fixed and covered.

## Handoff Notes
- Forward-port branch: `codex/live-daemon-truth-integration`; commit `dbac22f` preserves this lane before `ae8fcd0` adds desktop publication/subscription.
- Do not merge while Claude's daemon-agent cache work is dirty. Its current `checkout_agent`/`checkin_agent` design needs explicit serialization or Busy semantics so concurrent requests cannot create two cached agents and lose one history.
- After Claude commits a concurrency-safe lane, rebase the forward-port onto that clean tip and rerun the focused tests plus workspace check, Clippy, and the complete runnable suite.
- The Dioxus publication/subscription dependency is implemented by the stacked daemon-truth commit; the next manager dependency is safe daemon-approved control plus multi-workspace daemon routing.
