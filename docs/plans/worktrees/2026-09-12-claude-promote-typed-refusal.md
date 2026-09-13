---
title: Promote Typed Staged-Config Refusal Lane
description: Work card for claude-promote-typed-refusal-20260912 (PromoteGovernedOutcome returns the typed staged-config refusal ack, protocol v9 additive)
updated: 2026-09-12
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, governed, adr-0019, protocol-v9]
---

# Promote Typed Staged-Config Refusal Lane

## Lane Facts
- Owner: Claude (Fable 5.1)
- Role: implementation lane (small fix PR against `main`)
- Branch: `claude/promote-typed-refusal-20260912` (based on `origin/main` `e470767`)
- Worktree: `.claude/worktrees/recursing-clarke-b3eb50` — the desktop app's own worktree, kept
  rather than re-created under `.worktrees/` so the session's tooling keeps tracking it; the
  branch follows the lane convention.
- Owned paths: `impulse-rs/src/daemon/governed_wiring.rs`, `impulse-rs/src/client/mod.rs`
  (`promote_governed_outcome` only), `impulse-rs/src/handlers/daemon_dispatch.rs`
  (`handle_governed_promote` only), `docs/IPC-PROTOCOL.md` (typed-refusal paragraphs and the v9
  changelog), `CONTEXT.md` (world-scope entry only), this card.
- Blocked/shared paths: `impulse-rs/src/governed_producers.rs`, `impulse-rs/impulse-ops/**`,
  `impulse-rs/impulse-desktop/**`, `impulse-rs/src/state/**`, `Cargo.toml`/`Cargo.lock`,
  `AGENTS.md`, `CLAUDE.md`, the ADRs.
- Plan/spec: the follow-up chip filed on 2026-09-12 after PRs #52/#58 merged ("promote endpoint →
  typed `StagedConfigRefusal` ack"); `docs/IPC-PROTOCOL.md` "A drifted staged configuration is a
  typed refusal, not an error"; ADR-0019 rules 6 and 13.
- Verification: isolated `CARGO_TARGET_DIR`; `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `python3 ../docs/validate_docs.py --all`.
- Latest status: complete; gate green (evidence below); PR open.

## Decisions
- 2026-09-12: **Only the submodule gate is re-shaped.** The task chip described the refusal as
  reachable "via `ensure_no_submodule_configuration` / the pin comparison". At promotion the pin
  comparison does not raise `StagedConfigRefusal`: ADR-0019 rule 6 records a drifted pin as
  `promotion_blocked { repository_config_changed }` on the accepted run, `docs/IPC-PROTOCOL.md`
  documents it, and nine producer tests pin it. Changing that would be a design change, not a
  consistency fix, so it stays. The promote endpoint now routes the one error-shaped refusal it can
  raise — `unsupported_submodules`, a `.gitmodules` introduced after acceptance — through
  `respond_producer_error`, and a new endpoint test pins the blocked-outcome asymmetry explicitly.
- 2026-09-12: **Operator-class check before the refusal pre-read.** `handle_governed_promotion`
  checks the connection class before re-reading the task for the refusal echo, so a non-operator
  connection still causes no state read (the documented v9 property). `promote_governed_outcome`
  repeats the check; the repetition is cheap and keeps it safe to call directly.
- 2026-09-12: **Genuine errors now carry their context chain.** The promote arm previously
  answered `respond_err(error)` (`Display`, top layer only); routing through
  `respond_producer_error` answers `{error:#}` like the claim and verification endpoints. The
  socket test's substring assertions still hold.

## Changes
- `impulse-rs/src/daemon/governed_wiring.rs`: `handle_governed_promotion` (dispatch arm for
  `PromoteGovernedOutcome`); three tests.
- `impulse-rs/src/client/mod.rs`: `DaemonClient::promote_governed_outcome` returns
  `GovernedProducerOutcome<GovernedProducerAck>` via the shared `refused`-flag discrimination.
- `impulse-rs/src/handlers/daemon_dispatch.rs`: `governed-promote` prints a refusal through
  `print_staged_config_refusal` ("Promotion refused").
- `docs/IPC-PROTOCOL.md`, `CONTEXT.md`: wording as above.

## Tests
- `a_submodule_introduced_after_acceptance_refuses_the_promotion_with_a_typed_reason` — typed
  `unsupported_submodules` ack naming the path, remedy on the wire, no Git ran (smudge tripwire),
  reservation released, task unchanged, canonical branch and Builder commit unmoved.
- `a_drifted_config_pin_at_promotion_is_a_recorded_blocked_outcome_not_a_refusal` — the planted
  driver alone yields a producer ack with `promotion_blocked { repository_config_changed }` and no
  `refused` field.
- `a_genuine_promotion_failure_still_answers_as_an_error` — an unaccepted run is still an `Error`.

## Handoff Notes
- **Gate evidence (2026-09-12, isolated `CARGO_TARGET_DIR`, `origin/main` `e470767` + this lane):**
  `cargo build --workspace` clean; `cargo test --workspace` — impulse_desktop 168/0/0 + 82 + 8 +
  22 (1 ignored) + 7, impulse_ion 23 (1 ignored), impulse_ops 166 + 8 + 5 + 5, impulse_rs lib
  2338 passed / 0 failed / 5 ignored, integration binaries 5+2+30+5+5+5+11 (1 ignored)+4+2+17+5,
  doc-tests 3 (1 ignored); `cargo clippy --workspace --all-targets -- -D warnings` clean;
  `cargo fmt --all -- --check` clean. One first-pass failure,
  `agent::tests::test_harness_query_kills_hung_child_instead_of_orphaning` (timing-bound, the
  known flake already chipped on 2026-09-12, not touched by this lane), passed 3/3 on isolated
  rerun and passed in the full lib rerun. `python3 docs/validate_docs.py --all` reports one
  pre-existing metadata error outside this lane (ADR-0014 `status: proposed`); every touched
  doc validates.
- Refutation check: with the dispatch arm reverted to `respond_err(error)`, the new submodule
  test fails with the plain error string; with the fix it passes.
- **Desktop gap, out of this lane's scope:** `impulse-desktop/src/ui.rs` defines
  `staged_config_refusal_notice`/`StagedConfigRefusalNotice` but nothing calls them, and
  `daemon_ops.rs::promote_outcome` decodes every `Ok` as a `GovernedProducerAck` (`replayed`
  defaults), after which `runtime.rs::adopt_acknowledged_governed_task` rejects the unchanged
  revision a refusal echoes. The cockpit therefore reports a bridge error for a promote refusal
  rather than the notice. Needs a desktop-lane change: discriminate on `refused` in the gateway and
  wire the notice into the governed controls.
