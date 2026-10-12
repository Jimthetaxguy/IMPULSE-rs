---
title: Plugin Registry Feature-Honest Test
description: Work card for claude/practical-elbakyan-51676b (make the daemon plugin-registry init test honest under --no-default-features, and add that configuration to CI)
updated: 2026-09-12
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, testing, features, ci, daemon, plugin]
---

# Plugin Registry Feature-Honest Test

## Lane Facts

- Owner: Claude (Fable 5.1).
- Role: small test-correctness lane. `daemon::tests::tests::test_plugin_registry_initialized_after_init`
  fails on `main` (confirmed at `7c2086c` through `22f9630` by two independent
  lanes on 2026-09-12, reproduced again here at `e470767`) when run with
  `cargo test --no-default-features --lib`, because `init_global_registry`
  registers the office context provider only under `#[cfg(feature = "office-support")]`
  (`impulse-rs/src/plugin/registry.rs`), yet the test asserted a non-empty
  provider list unconditionally.
- Branch: `claude/practical-elbakyan-51676b`, based on `main` at `e470767`.
- Worktree: `.claude/worktrees/practical-elbakyan-51676b`.
- Owned paths:
  - `impulse-rs/src/daemon/tests.rs` (the one test block, replaced)
  - `.github/workflows/ci.yml` (separate commit: add `cargo test --no-default-features --lib`)
  - This work card
- Blocked/shared paths (not touched): `impulse-rs/src/plugin/**`, `impulse-rs/src/daemon/mod.rs`,
  `impulse-rs/src/daemon/handlers.rs`, `CLAUDE.md`, `AGENTS.md`, any production code.
- Plan/spec: the task statement itself (single test, decision-complete). No separate spec.
- Verification (isolated target dir, see `shared-cargo-target-dir-hazard` memory):
  ```
  cd impulse-rs
  export CARGO_TARGET_DIR=$HOME/.cargo-target-lanes/practical-elbakyan
  cargo test --lib -- daemon::tests
  cargo test --no-default-features --lib -- daemon::tests
  cargo build --workspace && cargo test --workspace && \
    cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check
  cargo build --no-default-features
  ```

## Decisions

- 2026-09-12: **Gate the assertion, do not add a feature-independent provider.**
  Nothing in the crate registers a context provider or an action handler outside
  the `office-support` block (grep for `register_context_provider(` /
  `register_action_handler(` outside `registry.rs` returns nothing), so there is
  no genuine feature-independent provider whose registration would make the old
  assertion true in both configurations. Inventing one only to satisfy a test
  would be a production change made for test convenience.
- 2026-09-12: **Three tests replace the one.** A `cfg(feature = "office-support")`
  variant asserts the office provider is present by name and that all four
  office formats are supported (stronger than the old "not empty"); a
  `cfg(not(feature = "office-support"))` variant asserts the provider and
  action-handler lists are both empty (what the no-feature build actually
  produces); and an unconditional idempotence test asserts `init_global_registry`
  returns the single global instance and does not grow the provider set on
  re-initialization. The `is_empty` assertion is stable because no other test
  or production path registers into the global registry.
- 2026-09-12: **CI already builds `--no-default-features` but never tests it**
  (`.github/workflows/ci.yml` "Build (minimal, no default features)" step). The
  test step is added as a separate commit so the test fix can be judged alone.

## Changes

- `impulse-rs/src/daemon/tests.rs`: `test_plugin_registry_initialized_after_init`
  replaced by `test_init_global_registry_with_office_support_registers_office_provider`
  (feature-gated), `test_init_global_registry_without_office_support_registers_no_plugins`
  (gated on the feature being absent), and
  `test_init_global_registry_is_idempotent_and_returns_global` (unconditional).
- `.github/workflows/ci.yml`: new `cargo test --no-default-features --lib` step in
  the `test` job (separate commit).

## Tests

See the PR body for the before/after output captured on this checkout.

## Handoff Notes

- No production code changed. The `office-support` gating in
  `src/plugin/registry.rs` is unchanged and is the documented behavior.
