---
title: Sub-Agent Lane Map Loop 7
description: Execution lane map for Ralph Plan 6 loops 9-15.
updated: 2026-05-22
type: doc
category: planning
phase: all
status: superseded
audience: builders
tags: [worktree, lane, ralph-plan, sub-agent, sequencing, handoff]
---

# Sub-Agent Lane Map Loop 7

> Historical lane map from the May 2026 Tauri+Dioxus migration phase. Use the
> 2026-06-14 Dioxus Desktop work cards and ADR-0008 for current host direction.

## Lane Facts

- Owner: Codex Loop 7 worker
- Role: Lane integrator and sequencing mapper
- Branch: `main`
- Worktree: `<legacy-worktree>`
- Owned paths: this artifact, `docs/archive/ralph-plans/ralph-plan-6.md`, `docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`
- Read-only inputs: `docs/plans/worktrees/2026-05-22-daemon-truth-boundary-loop6.md`, Loop 1-6 Working Logs in `docs/archive/ralph-plans/ralph-plan-6.md`
- Blocked/shared paths: `impulse-rs/**` source and Cargo files, `docs/validate_docs.py`, `AGENTS.md`, `CLAUDE.md`, `README.md`, `docs/spec/**`, docs indexes, protocol docs, and unrelated dirty files
- Verification: `bash <agent-skills>/ralph-plan/scripts/validate-plan.sh --strict-v2 docs/archive/ralph-plans/ralph-plan-6.md`; `git diff --check`
- Latest status: Loop 7 lane map completed as a docs-only artifact; no Rust, Cargo, validator, spec, index, root guidance, or unrelated dirty files were edited

## Execution Map For Loops 9-15

Loop 8 should adopt or adjust this map, then update the Root Iteration Contents. Until Loop 8 does that, these are concrete recommended lanes, not Root table status.

| Loop | Focus | Agent Type | Owned Paths | Blocked/Shared Paths | Dependencies | Verification Gate |
|---|---|---|---|---|---|---|
| 9 | Shared protocol parity for artifact list/get/action DTOs | Rust protocol implementer | `impulse-rs/impulse-ops/src/lib.rs`; `impulse-rs/src/daemon/protocol.rs`; focused protocol tests in the same crates | `impulse-rs/Cargo.toml`; `Cargo.lock`; `impulse-rs/impulse-desktop/**`; docs indexes/spec docs unless separately claimed | Loop 6 artifact gap; Loop 8 adopts lane; no desktop artifact panel work starts first | `cd impulse-rs && cargo test -p impulse-ops`; `cd impulse-rs && cargo test -p impulse-rs daemon::protocol::tests::test_shared_workbench_requests_deserialize_into_daemon_protocol`; `cd impulse-rs && cargo test -p impulse-rs ops_workbench`; plan validation; `git diff --check` |
| 10 | Desktop daemon client adapter for workbench requests | Desktop runtime implementer | New or existing daemon-client adapter under `impulse-rs/impulse-desktop/src/**`; focused desktop tests under `impulse-rs/impulse-desktop/tests/**` | `impulse-rs/impulse-ops/src/lib.rs`; `impulse-rs/src/daemon/protocol.rs`; Cargo files unless Loop 14 grants ownership | Loop 9 green; no UI panel rendering work depends on bespoke JSON before this adapter exists | `cd impulse-rs && cargo test -p impulse-desktop`; focused daemon-client tests; plan validation; `git diff --check` |
| 11 | Runtime telemetry adapter and publish cadence | Desktop runtime implementer with daemon reviewer | `impulse-rs/impulse-desktop/src/runtime.rs`; telemetry adapter module/tests under `impulse-rs/impulse-desktop/src/**` and `tests/**` | `impulse-rs/impulse-ops/src/lib.rs`; `impulse-rs/src/daemon/protocol.rs`; UI panel state files unless Loop 12 starts after handoff | Loop 9 DTOs stable; Loop 10 daemon client path exists | `cd impulse-rs && cargo test -p impulse-desktop --test runtime`; focused tests for spawn/resize/focus/exit publish triggers; plan validation; `git diff --check` |
| 12 | UI ops-update rendering and artifact/supervisor panels | Desktop UI implementer | `impulse-rs/impulse-desktop/src/ui.rs`; desktop UI contract tests under `impulse-rs/impulse-desktop/tests/**`; render-only panel modules if introduced | Runtime telemetry internals from Loop 11 unless handed off; protocol DTOs from Loop 9; docs specs/indexes | Loop 10 request adapter; Loop 11 telemetry adapter; daemon truth remains source for panel state | Historical gate: `cd impulse-rs && cargo test -p impulse-desktop --test desktop_contract --test tauri_surface`. Current equivalent after the 2026-06-14 host rename is `cd impulse-rs && cargo test -p impulse-desktop --test desktop_contract --test host_surface`; UI contract tests prove panels render daemon payloads without client-side snapshot mutation; plan validation; `git diff --check` |
| 13 | Docs validator and index reconciliation after implementation semantics settle | Docs contract implementer | `docs/validate_docs.py`; `docs/INDEX.md`; `docs/SUMMARY.md`; `docs/SUMMARY.yaml`; `docs/metadata.yaml`; targeted docs under `docs/plans/**` | `AGENTS.md`; `CLAUDE.md`; `README.md`; `docs/spec/**`; Rust source/Cargo files unless explicitly claimed | Loops 9-12 define actual protocol/desktop truth; validator marker drift from Loop 2 is still known debt | `python3 docs/validate_docs.py --contract`; `python3 docs/validate_docs.py --all`; plan validation; `git diff --check` |
| 14 | Cargo integration and workspace verification | Integration implementer/verifier | `impulse-rs/Cargo.toml`; `impulse-rs/Cargo.lock`; crate manifests only if required by accepted Loop 9-12 changes; no feature work beyond dependency wiring | All feature source paths except mechanical manifest-driven compile fixes; docs specs/indexes unless Loop 13 hands off | Loops 9-12 declare final dependency needs; no parallel lane edits Cargo files | `cd impulse-rs && cargo check --workspace`; `cd impulse-rs && cargo test --workspace`; `cd impulse-rs && cargo clippy --workspace -- -D warnings`; `cd impulse-rs && cargo fmt --check`; plan validation; `git diff --check` |
| 15 | OpenCode compatibility cleanup decision and Loop 16 prep | Reviewer/planner with docs support | New work card under `docs/plans/worktrees/**`; `docs/archive/ralph-plans/ralph-plan-6.md` Working Log; optional compatibility audit artifact under `docs/plans/worktrees/**` | OpenCode code/tests/docs, `AGENTS.md`, `CLAUDE.md`, `README.md`, `docs/spec/**`, `docs/validate_docs.py`, Cargo files unless Loop 16 explicitly claims them | Loops 9-14 complete or have exact blockers; Loop 13 docs status known | Plan validation; `git diff --check`; optional read-only `rg -n "OpenCode|opencode" AGENTS.md CLAUDE.md README.md docs impulse-rs --glob '!target/**'` with findings only |

## Shared-File Sequencing Rules

These files must not be edited opportunistically by future workers:

| Shared File Or Area | Exclusive Owner In This Map | Sequencing Rule |
|---|---|---|
| `impulse-rs/Cargo.toml` | Loop 14 only | Loops 9-12 must record dependency needs in their handoff. Loop 14 applies manifest changes in one batch after feature-source lanes settle. |
| `impulse-rs/Cargo.lock` | Loop 14 only | Lockfile changes happen only as a consequence of Loop 14 manifest resolution or approved dependency commands. No feature lane updates the lockfile incidentally. |
| `impulse-rs/impulse-ops/src/lib.rs` | Loop 9 only | Add shared `WorkbenchDaemonRequest` parity before any desktop adapter or panel relies on artifact list/get. Later lanes treat this file as read-only unless Loop 9 hands off an unresolved blocker. |
| `impulse-rs/src/daemon/protocol.rs` | Loop 9 only | Protocol compatibility tests move with shared DTO changes. Desktop lanes must not update daemon protocol tests to make UI code pass. |
| `impulse-rs/impulse-desktop/**` | Loops 10-12, sequential | Loop 10 owns daemon client request plumbing, Loop 11 owns runtime telemetry publishing, Loop 12 owns UI rendering. Each loop hands off before the next edits overlapping desktop files. |
| Docs indexes and spec docs | Loop 13 only, except plan files | `docs/INDEX.md`, `docs/SUMMARY.md`, `docs/SUMMARY.yaml`, `docs/metadata.yaml`, and `docs/spec/**` wait until implementation truth is stable. |
| Plan files | Current loop owner only | Each loop may update its work card and `docs/archive/ralph-plans/ralph-plan-6.md` status/log. Root loop assignments for 9-15 are reserved for Loop 8. |

## Blocked-Path Rules

- No loop may edit another loop's owned path while that loop is active.
- No loop may edit `impulse-rs/Cargo.toml` or `impulse-rs/Cargo.lock` before Loop 14 unless it records a blocker and receives explicit ownership transfer in a work card.
- No desktop lane may invent a second artifact, supervisor, telemetry, or project state owner. Panels render daemon truth; runtime publishes telemetry input.
- No UI lane may use bespoke artifact JSON requests after Loop 9 closes the shared DTO gap.
- No docs lane may rewrite Root docs or broad specs to match incomplete code. Docs indexes/specs follow verified implementation state.
- No worker may normalize, format, stage, revert, or archive unrelated dirty files.
- If a lane must touch a blocked path, it must first update a work card with the exact file, reason, prior owner, dependency, and verification command.

## Dependency Chain

```text
Loop 9 protocol parity
  -> Loop 10 desktop daemon client
  -> Loop 11 runtime telemetry publish adapter
  -> Loop 12 UI render-only panels
  -> Loop 13 docs validator/index reconciliation
  -> Loop 14 Cargo/workspace integration
  -> Loop 15 compatibility decision + Loop 16 prep
```

Loop 13 may start read-only scanning earlier, but it must not write docs indexes, specs, or validator code until the Loop 9-12 code lanes have either passed or handed off exact blockers.

## Verification Policy

Every future lane keeps the plan checks:

```bash
bash <agent-skills>/ralph-plan/scripts/validate-plan.sh --strict-v2 docs/archive/ralph-plans/ralph-plan-6.md
git diff --check
```

Rust implementation lanes add focused crate tests first, then broader workspace checks when they touch shared behavior. Docs lanes run `python3 docs/validate_docs.py --contract` and `python3 docs/validate_docs.py --all` only after they claim the validator/index lane.

## Handoff Notes

- Loop 8 should update the Root Iteration Contents only after reviewing this map.
- Loop 9 is the first code lane because shared DTO parity unblocks all artifact-panel and request-adapter work.
- Loop 14 exists to prevent incidental Cargo churn from multiple feature lanes.
- Loop 15 is intentionally decision-oriented; OpenCode compatibility removal is not authorized by this map.
