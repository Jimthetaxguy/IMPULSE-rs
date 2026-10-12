---
title: Thin Step Model Policy Crate
description: Work card for extracting ADR-0015 policy from the Impulse application graph
updated: 2026-08-23
type: doc
category: planning
phase: all
status: complete
audience: builders
tags: [worktree, lane, step-model, adr-0015, rosa]
---

# Thin Step Model Policy Crate

## Lane Facts

- Owner: Codex
- Role: implementation and integration lane
- Branch: `codex/impulse-step-model-crate-20260823`
- Worktree: `/Users/jamespustorino/code/IMPULSE-rs/.worktrees/step-model-crate-20260823`
- Base: `origin/main` at `76ab525`
- Owned paths: `impulse-rs/Cargo.toml`, `impulse-rs/Cargo.lock`, new
  `impulse-rs/impulse-step-model/**`, `impulse-rs/src/agent/step_model.rs`,
  ADR-0015 and its indexes, this work card.
- Shared/blocked paths: no edits to other active worktrees; no TUI, desktop,
  daemon protocol, provider, or governed-task behavior outside the adapter.
- Plan/spec: ADR-0015 and this work card.
- Verification: focused crate and step-model tests, workspace build/fmt/clippy/tests,
  and `python3 docs/validate_docs.py --all`.
- Latest status: implementation and verification complete; implementation
  commit `220079e` is durable on
  `origin/codex/impulse-step-model-crate-20260823`.

## Decisions

- 2026-08-23: Keep provider selection, inference permission, candidate
  admissibility, and audit persistence in each host. Export only deterministic
  step-model policy and neutral context/reason types.
- 2026-08-23: Preserve Impulse-native governed-task adaptation and arena
  logging in `src/agent/step_model.rs`; the new crate does not import
  `impulse-ops`.

## Changes

- Added `impulse-step-model` with only `serde` as a runtime dependency.
- Delegated Impulse's existing ADR-0015 seam through the pure crate without
  changing its current identity/escalation behavior.
- Accepted ADR-0015 and documented the ROSA consumer boundary.

## Tests

- `cargo test -p impulse-step-model`: 11 passed, 0 failed.
- `cargo test -p impulse-rs --lib step_model`: 19 passed, 0 failed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo test --workspace --locked -- --format=terse`: passed on the final
  checkout with 2,228 passed, 0 failed, and 9 ignored across workspace and
  doc-test suites; the primary `impulse-rs` library accounted for 1,709 passed
  and 5 ignored.
- `cargo tree -p impulse-step-model --depth 2 --locked`: only `serde` at
  runtime; `serde_json` is test-only.
- `python3 docs/validate_docs.py --all`: no lane-introduced failures. The
  existing repository baseline remains ADR-0014's invalid `proposed` metadata
  plus three documents beyond the 120-day freshness threshold.
- `git diff --check`: passed.

## Handoff Notes

- The thin crate is independently consumable, Impulse behavior and audit
  logging are preserved, and the verified branch is remotely durable.
- ROSA must pin an exact commit, resolve provider defaults before invoking the
  policy, reject any result outside the admitted candidates, and emit
  ROSA-owned decision evidence.
