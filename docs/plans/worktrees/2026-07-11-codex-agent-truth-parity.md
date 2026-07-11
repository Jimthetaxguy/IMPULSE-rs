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
  - Claude's active Ion T6 files: `impulse-rs/Cargo.toml`, `impulse-rs/Cargo.lock`, `impulse-rs/src/bin/ion.rs`, `impulse-rs/src/lib.rs`, `impulse-rs/src/ion_repl/`, and `impulse-rs/impulse-ion/TUI_SPEC.md`.
  - `AGENTS.md`, `CLAUDE.md`, `CONTEXT.md`, the canonical contract, protocol docs, and docs indexes remain integration-owned shared files.
- Plan/spec: `docs/spec/USER-STORY-MAP.md` ST-13 and `docs/spec/TEST-TRACEABILITY.md` agent-control gap.
- Verification: `cargo test -p impulse-rs ops_workbench`, then `cargo check --workspace`, `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, and `cargo fmt --check` from `impulse-rs/`.
- Latest status: implementation and three review loops complete; branch is ready for integration after Claude hands off its active Ion T7 lane on `main`.

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
- KNOWN EXTERNAL BLOCKER: unskipped `cargo test --workspace` has exactly one failure, `impulse_ops::agent_registry::test_reconciled_clean_archive_has_contracts_snapshot`, for that absent worktree archive fixture.
- KNOWN EXTERNAL BLOCKER: `cargo clippy --workspace --all-targets -- -D warnings` reaches four pre-existing `await_holding_lock` warnings in Claude-owned `src/handlers/ion.rs` tests. Normal workspace Clippy is green.
- KNOWN EXTERNAL BLOCKER: `python3 docs/validate_docs.py --contract` reports 17 pre-existing documents older than the 120-day threshold; none is owned by this lane.
- KNOWN EXTERNAL BLOCKER: `python3 docs/validate_docs.py --all` additionally reports unrelated `research/2026-06-30-sites-map-phase1-spec.md` status `approved-for-planning` outside the allowed vocabulary. The new lane card itself validates.
- REVIEW: final local architecture review returned `NO_FINDINGS`; MiniMax-M2.7 found the empty `working:` / `blocked:` inverse edge, which is fixed and covered.

## Handoff Notes
- Claude Code now owns active Ion T7/tooling work on `main`; do not cherry-pick or merge this branch until that dirty lane is clean or handed off.
- Integration step: rebase `codex/agent-truth-parity` onto the latest clean `main`, rerun the focused tests plus workspace check/Clippy, then merge or cherry-pick the lane commit.
- Next product dependency after integration: publish Dioxus `DesktopRuntime` snapshots as `TerminalOpsReport` and subscribe the desktop host to daemon `ProjectOpsSnapshot`; `OpsUpdate` currently has no production Dioxus producer.
