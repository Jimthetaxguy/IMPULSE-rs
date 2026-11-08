---
title: Remote Admission Source Reconciliation
description: Preserve existing deny-by-default daemon and MCP behavior through a shared helper and regression coverage.
author: codex/Codex
updated: 2026-11-08
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, safety, verification]
---

# Remote Admission Source Reconciliation

## Lane Facts
- Owner: codex/Codex; source reconciliation and local verification, root agent independent review.
- Role: bounded implementation; no deployment or runtime adoption.
- Branch: `codex/remote-admission-reconciliation-20261108`.
- Worktree: `.worktrees/remote-admission-reconciliation-20261108`.
- Base: origin/main `c46e9ee28346356736b76d865a7ce0c647b9ffb4`.
- Owned paths: `impulse-rs/src/daemon/mod.rs`, `impulse-rs/src/handlers/common.rs`, `impulse-rs/src/handlers/system.rs`, `impulse-rs/src/mcp/server.rs`, and this card.
- Blocked/shared paths: Cargo manifests and lockfiles, protocol/authentication/registry policy, instructions, dirty originals, pagination lane and every other worktree.
- Plan/spec: reconcile only the four Rust path deltas from backup `9294193488949c1b609865882349dfc7f87f8f99`; canonical Rust contract governs.
- Verification: lane-isolated Cargo output; targeted synthetic MCP/daemon admission test, then workspace build/check/test/strict all-target Clippy/format. Inspect tests before execution; no live providers.
- Latest status: source reconciled and local gates passed; awaiting independent review before commit.

## Decisions
- Both current transports already deny write capability by default. Consolidate their context helper and prove shared admission, without new tools, endpoints, policy grants or public interfaces.
- Exclude the old snapshot lockfile entirely; retain current dependency/security resolution.
- Real local request handlers plus temporary state and test probes establish admission behavior, not socket peer authentication or live-provider adoption.

## Acceptance
- Existing capability denial, actor provenance and registry admission ordering remain unchanged.
- Both transports admit read probes and reject unauthorized writes and malformed arguments before execution.
- Exact source diff receives independent review after required local verification and before commit.

## Changes
- Applied the exact four-source-path backup delta; the current dependency lockfile is unchanged.

## Tests
- Current isolated workspace build, check, tests, strict all-target Clippy and formatting passed on 2026-11-08.
- Workspace tests: 4,018 passed, zero failed, 11 ignored. Ignored tests were not exercised; no live-provider or live-keychain test was requested.
- Fully qualified admission test independently passed: one passed, zero failed. An initial unqualified exact filter selected zero tests; it is not counted as targeted verification.
- Redacted scans of the four Rust paths and this card found no secrets. Exact diff and per-suite results are retained in the existing machine-wide audit lane.
- These gates establish local admission behavior; they do not prove Unix-socket peer authentication, provider adoption or deployment.

## Handoff Notes
- No merges, hook activation or original worktree retirement.
- Rollback uses preserved preimages or a reviewed reverse patch for owned paths; never reset unrelated work.
