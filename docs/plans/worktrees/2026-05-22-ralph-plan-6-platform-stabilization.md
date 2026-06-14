---
title: Ralph Plan 6 Platform Stabilization Lane
description: Work card for the Impulse Ralph Plan 6 stabilization and sub-agent loop map.
updated: 2026-06-14
type: doc
category: planning
phase: all
status: superseded
audience: builders
tags: [worktree, lane, ralph-plan, platform-stabilization, handoff]
---

# Ralph Plan 6 Platform Stabilization Lane

> Superseded 2026-06-14: this lane is retained as historical Plan 6 execution context. Current desktop work targets the Dioxus Desktop `impulse-desktop` host, with Tauri-shaped code kept only as legacy compatibility while parity migrates.

## Lane Facts

- Owner: Codex
- Role: Ralph planning integrator and lane orchestrator
- Branch: `main`
- Worktree: `<legacy-worktree>`
- Other worktrees: `.worktrees/gui-roadmap` on `feature/gui-roadmap`; `.worktrees/impulse-1.0-memory-loop` on `feature/impulse-1.0-memory-loop`
- Owned paths for Loop 1: `docs/archive/ralph-plans/ralph-plan-6.md`, `docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`
- Owned paths for Loop 2: `README.md`, `AGENTS.md`, `CLAUDE.md`, `docs/spec/RUST-CANONICAL-CONTRACT.md`, `docs/guides/COLLABORATIVE-AGENTIC-CODING.md`, `impulse-rs/QUICKSTART.md`, `impulse-rs/impulse-gui/README.md`, `docs/plans/IMPLEMENTATION-HANDOFF.md`, `docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md`, `docs/metadata.yaml`, `docs/archive/ralph-plans/ralph-plan-6.md`, this work card
- Owned paths for Loop 3: `impulse-rs/impulse-desktop/**`, `docs/archive/ralph-plans/ralph-plan-6.md`, this work card
- Owned paths for Loop 4: `impulse-rs/impulse-term/**`, `docs/archive/ralph-plans/ralph-plan-6.md`, this work card
- Owned paths for Loop 5: `impulse-rs/impulse-desktop/src/runtime.rs`, `impulse-rs/impulse-desktop/src/ui.rs`, `impulse-rs/impulse-desktop/tests/*`, `docs/archive/ralph-plans/ralph-plan-6.md`, this work card
- Owned paths for Loop 6: `docs/plans/worktrees/2026-05-22-daemon-truth-boundary-loop6.md`, `docs/archive/ralph-plans/ralph-plan-6.md`, this work card
- Owned paths for Loop 7: `docs/plans/worktrees/2026-05-22-sub-agent-lane-map-loop7.md`, `docs/archive/ralph-plans/ralph-plan-6.md`, this work card
- Owned paths for Loop 8: `docs/archive/ralph-plans/ralph-plan-6.md`, this work card
- Allowed additions: new `docs/plans/worktrees/*.md` lane cards only if needed
- Blocked/shared paths: `docs/archive/ralph-plans/ralph-plan-5.md`, Rust code outside the active loop's owned paths, `Cargo.toml`, `Cargo.lock`, `AGENTS.md`, `CLAUDE.md`, `README.md`, docs indexes/specs, protocol docs, and all existing dirty files outside this lane
- Plan/spec: `docs/archive/ralph-plans/ralph-plan-6.md`
- Required Loop 1 verification: `bash <agent-skills>/ralph-plan/scripts/validate-plan.sh --strict-v2 docs/archive/ralph-plans/ralph-plan-6.md`; `git diff --check -- docs/archive/ralph-plans/ralph-plan-6.md docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`
- Required Loop 2 verification: `python3 docs/validate_docs.py --all`; `bash <agent-skills>/ralph-plan/scripts/validate-plan.sh --strict-v2 docs/archive/ralph-plans/ralph-plan-6.md`; `git diff --check -- README.md AGENTS.md CLAUDE.md docs/spec/RUST-CANONICAL-CONTRACT.md docs/guides/COLLABORATIVE-AGENTIC-CODING.md impulse-rs/QUICKSTART.md impulse-rs/impulse-gui/README.md docs/plans/IMPLEMENTATION-HANDOFF.md docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md docs/metadata.yaml docs/archive/ralph-plans/ralph-plan-6.md docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`
- Later implementation verification: `python3 docs/validate_docs.py --all` and the Rust workspace gate from `impulse-rs`
- Latest status: Loop 8 planning checkpoint completed; Root rows and detailed plans for loops 9-15 are concrete and the final Loop 8 docs/Rust/plan gate is green

## Decisions

- 2026-05-22: Create `docs/archive/ralph-plans/ralph-plan-6.md` instead of resuming `docs/archive/ralph-plans/ralph-plan-5.md` because Plan 5 still centers stale GUI/EGUI work.
- 2026-05-22: Preserve Plan 5 as historical context and mark supersession only outside its Root documents.
- 2026-05-22: Treat OpenCode references as compatibility debt to audit, not wholesale removal, until James confirms product-level removal inside Impulse.
- 2026-05-22: Treat the current dirty tree as multi-lane work. Loop 1 may document it but must not normalize, stage, revert, or edit anything outside the Plan 6 lane.
- 2026-05-22: Active docs should present Claude Code/Codex as primary platforms; OpenCode remains legacy compatibility only.
- 2026-06-14: Supersession update: current operator use remains ratatui TUI, active native desktop migration routes to the Dioxus Desktop `impulse-desktop` host, and Tauri-shaped code is legacy compatibility only.
- 2026-05-22: Treat daemon `ProjectOpsSnapshot`, artifact store/actions, supervisor policy/action approval, and telemetry overlays as authoritative truth; desktop runtime owns PTY mechanics and publishes `TerminalOpsReport` only as input to daemon reconciliation.
- 2026-05-22: Reserve loops 9-15 Root assignment changes for Loop 8; Loop 7 provides a concrete non-Root execution map and work card only.
- 2026-05-22: Sequence shared files explicitly: Loop 9 owns shared DTO/protocol parity, Loop 10 owns desktop daemon-client plumbing, Loop 11 owns runtime telemetry publishing, Loop 12 owns render-only panels, Loop 13 owns docs validator/index reconciliation, Loop 14 owns Cargo integration, and Loop 15 owns compatibility decision prep.

## Current Dirty-Tree Classification

Observed with `git status --short` on `main`:

| Bucket | Files | Ownership |
|--------|-------|-----------|
| Agent/project guidance and root docs | `AGENTS.md`, `CLAUDE.md`, `README.md`, `CONTRIBUTING.md` | Shared; blocked for Loop 1 |
| Docs indexes/specs/protocols | `docs/INDEX.md`, `docs/IPC-PROTOCOL.md`, `docs/LONG-RANGE-ENHANCEMENTS.md`, `docs/ROADMAP-PLAN.md`, `docs/SUMMARY.md`, `docs/SUMMARY.yaml`, `docs/spec/*`, `docs/validate_docs.py` | Shared; blocked for Loop 1 |
| Collaboration docs and lane cards | `docs/guides/COLLABORATIVE-AGENTIC-CODING.md`, `docs/plans/worktrees/` | Plan 6 may edit only its lane card; guide is shared |
| Research/docs support | `docs/guides/DESKTOP-BENCHMARK-METHODOLOGY.md`, `docs/research/RESEARCH-DIGEST.md` | Shared; blocked for Loop 1 |
| Rust workspace metadata | `impulse-rs/.gitignore`, `impulse-rs/Cargo.toml`, `impulse-rs/Cargo.lock`, crate `Cargo.toml` files | Shared; blocked for Loop 1 |
| Rust implementation | `impulse-rs/src/*`, `impulse-rs/impulse-term/src/*`, `impulse-rs/impulse-desktop/` | Implementation lanes only; blocked for Loop 1 |
| Prior plan | `docs/archive/ralph-plans/ralph-plan-5.md` | Blocked; preserve unchanged |
| Current plan lane | `docs/archive/ralph-plans/ralph-plan-6.md`, this work card | Owned by Loop 1 |

## Changes

- Added `docs/archive/ralph-plans/ralph-plan-6.md` with Root docs, Iteration Contents, dependency graph, sub-agent strategy, domain inventory, loop plans for loops 1-8, and verification plan.
- Added a non-Root archive note to `docs/archive/ralph-plans/ralph-plan-5.md`.
- Added this work card to identify ownership and shared-file constraints for the Ralph Plan 6 lane.
- Loop 1 updated this work card with the observed dirty-tree classification, active worktrees, blocked/shared paths, and verification gates.
- Loop 1 updated `docs/archive/ralph-plans/ralph-plan-6.md` with completion status and a working log.
- Loop 2 updated active docs for platform truth: top-level guidance, canonical contract, collaborative lane prefixes, quickstart, legacy GUI README, implementation handoff docs, and project metadata.
- Loop 2 updated `docs/archive/ralph-plans/ralph-plan-6.md` with completion status and a reverse-chronological Working Log.
- Loop 3 archived `impulse-rs/impulse-desktop/src/tauri_commands 2.rs` under `impulse-rs/impulse-desktop/_archive-2026-05-22-loop3/tauri_commands-2.rs`, preserved `src/tauri_commands.rs` as the active runtime-backed surface at the time; superseded on 2026-06-14 by `src/host_commands.rs`, and clarified the desktop README contract.
- Loop 4 added `impulse-rs/impulse-term/tests/boundary_tests.rs` and documented the framework-neutral terminal boundary in `impulse-rs/impulse-term/README.md`.
- Loop 5 converted xterm `onData` input to byte arrays before `agent_write`, exposed the interop script for contract testing, and added focused runtime bridge regression coverage.
- Loop 6 added `docs/plans/worktrees/2026-05-22-daemon-truth-boundary-loop6.md` documenting daemon-owned truth, desktop-runtime-owned PTY mechanics, UI-rendered surfaces, the `WorkbenchDaemonRequest` artifact list/get gap, and the `TerminalOpsReport` publish/subscribe plan.
- Loop 7 added `docs/plans/worktrees/2026-05-22-sub-agent-lane-map-loop7.md` documenting the recommended loops 9-15 execution map, shared-file sequencing, blocked-path rules, and verification gates.
- Loop 8 adopted the Loop 7 map into `docs/archive/ralph-plans/ralph-plan-6.md` Root rows and detailed plans for loops 9-15.

## Tests

- Passed: `bash <agent-skills>/ralph-plan/scripts/validate-plan.sh --strict-v2 docs/archive/ralph-plans/ralph-plan-6.md`
- Passed: `python3 docs/validate_docs.py --all`
- Passed: `git diff --check -- docs/archive/ralph-plans/ralph-plan-5.md docs/archive/ralph-plans/ralph-plan-6.md docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`
- Passed: `cargo check --workspace`
- Passed: `cargo test --workspace`
- Passed: `cargo clippy --workspace -- -D warnings`
- Passed: `cargo fmt --check`
- Passed: `bash <agent-skills>/ralph-plan/scripts/validate-plan.sh --strict-v2 docs/archive/ralph-plans/ralph-plan-6.md`
- Passed: `git diff --check -- docs/archive/ralph-plans/ralph-plan-6.md docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`
- Failed with pre-existing/out-of-scope validator drift: `python3 docs/validate_docs.py --all`
  - `docs/validate_docs.py` still requires the old roadmap marker text containing `Phase 0 docs reset` in `AGENTS.md` and `CLAUDE.md`.
  - The same run reports 30 stale docs last updated `2026-02-20` across research/spec/vision/decision/guide/phase files outside Loop 2 ownership.
- Passed Loop 2: `bash <agent-skills>/ralph-plan/scripts/validate-plan.sh --strict-v2 docs/archive/ralph-plans/ralph-plan-6.md`
- Passed Loop 2: `git diff --check -- README.md AGENTS.md CLAUDE.md docs/spec/RUST-CANONICAL-CONTRACT.md docs/guides/COLLABORATIVE-AGENTIC-CODING.md impulse-rs/QUICKSTART.md impulse-rs/impulse-gui/README.md docs/plans/IMPLEMENTATION-HANDOFF.md docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md docs/metadata.yaml docs/archive/ralph-plans/ralph-plan-6.md docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`
- Passed Loop 3: `cd impulse-rs && cargo test -p impulse-desktop` (10 tests passed, 0 failed)
- Passed Loop 3: `cd impulse-rs && cargo check -p impulse-desktop --features tauri-runtime`
- Passed Loop 3: `bash <agent-skills>/ralph-plan/scripts/validate-plan.sh --strict-v2 docs/archive/ralph-plans/ralph-plan-6.md`
- Passed Loop 3: `git diff --check`
- Passed Loop 4: `cd impulse-rs && cargo test -p impulse-term --no-default-features` (54 tests passed, 0 failed)
- Passed Loop 4: `cd impulse-rs && cargo test -p impulse-term` (114 tests passed, 0 failed)
- Passed Loop 4: `cargo fmt -p impulse-term`
- Passed Loop 4: `bash <agent-skills>/ralph-plan/scripts/validate-plan.sh --strict-v2 docs/archive/ralph-plans/ralph-plan-6.md`
- Passed Loop 4: `git diff --check`
- Passed Loop 5: `cd impulse-rs && cargo test -p impulse-desktop --test runtime --test tauri_surface --test desktop_contract` (16 tests passed, 0 failed)
- Passed Loop 5: `cd impulse-rs && cargo fmt -p impulse-desktop`
- Passed Loop 5: `bash <agent-skills>/ralph-plan/scripts/validate-plan.sh --strict-v2 docs/archive/ralph-plans/ralph-plan-6.md`
- Passed Loop 5: `git diff --check`
- Passed Loop 6: `cd impulse-rs && cargo test -p impulse-rs ops_workbench`
- Passed Loop 6: `bash <agent-skills>/ralph-plan/scripts/validate-plan.sh --strict-v2 docs/archive/ralph-plans/ralph-plan-6.md`
- Passed Loop 6: `git diff --check`
- Passed Loop 7: `bash <agent-skills>/ralph-plan/scripts/validate-plan.sh --strict-v2 docs/archive/ralph-plans/ralph-plan-6.md`
- Passed Loop 7: `git diff --check`
- Passed Loop 8: `python3 docs/validate_docs.py --all`
- Passed Loop 8: `cd impulse-rs && cargo check --workspace`
- Passed Loop 8: `cd impulse-rs && cargo test --workspace` (1,483 tests passed, 4 ignored)
- Passed Loop 8: `cd impulse-rs && cargo clippy --workspace -- -D warnings`
- Passed Loop 8: `cd impulse-rs && cargo fmt --check`
- Passed Loop 8: `bash <agent-skills>/ralph-plan/scripts/validate-plan.sh --strict-v2 docs/archive/ralph-plans/ralph-plan-6.md`
- Passed Loop 8: `git diff --check`

## Handoff Notes

- Existing dirty tree state predates this lane and must not be reverted.
- Future loops should begin with a fresh dirty-tree reconciliation before changing shared files.
- `impulse-rs/impulse-desktop/src/tauri_commands 2.rs` has been archived, not deleted. Historical note: the active command surface was `impulse-rs/impulse-desktop/src/tauri_commands.rs` at the time; it is now `impulse-rs/impulse-desktop/src/host_commands.rs`.
- Next loops must claim shared files before editing them. In particular, docs indexes/specs, Cargo manifests, and Rust source files outside the claimed lane are blocked unless a future lane card explicitly transfers ownership.
- Loop 4 historical assumption superseded: `impulse-desktop` now exposes host-oriented command names, and README wording identifies the Dioxus Desktop host as the active path.
- Loop 5 may rely on `impulse-term` core backend/context/paste exports compiling without egui while default `egui` compatibility remains enabled.
- Loop 6 may rely on `impulse-desktop` rejecting missing sessions and invalid terminal dimensions, enforcing exclusive focus, requiring supervisor confirmation before input, and sending xterm input as byte arrays across the Rust command boundary.
- Loop 7 may rely on the Loop 6 boundary artifact for lane assignments: shared DTO/protocol parity is separate from desktop runtime telemetry publishing and UI panel rendering.
- The concrete shared DTO gap is `WorkbenchDaemonRequest` missing `ListArtifacts` and `GetArtifact`; do not wire desktop artifact panels through ad hoc JSON if a protocol lane can close that parity gap first.
- Desktop should publish `TerminalOpsReport` as telemetry input and subscribe to daemon `ops_update`/snapshot responses for rendered truth; it must not maintain a second artifact/supervisor/project state owner.
- Loop 8 adopted the Loop 7 map into Root rows for loops 9-15.
- Loop 9 starts with `impulse-rs/impulse-ops/src/lib.rs` and `impulse-rs/src/daemon/protocol.rs`; desktop source and Cargo files stay blocked until their sequenced lanes.
- OpenCode remains compatibility debt; do not remove code/tests/docs for it without an explicit removal plan.
- Loop 2 docs validation is green after the controller-owned `docs/validate_docs.py` adjustment; Loop 3 did not edit or rerun that file.
