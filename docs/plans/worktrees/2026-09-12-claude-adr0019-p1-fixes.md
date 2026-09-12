---
title: ADR-0019 Post-Merge P1 Fixes
description: Work card for claude-adr0019-p1-fixes-20260912 (the four unresolved Codex P1 threads on merged PR #50)
updated: 2026-09-12
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, adr-0019, governance, review, staged-worktree]
---

# ADR-0019 Post-Merge P1 Fixes

## Lane Facts

- Owner: Claude (Fable 5.1).
- Role: implementation lane closing the four unresolved P1 review threads on merged PR #50
  (ADR-0019, Builder staged-worktree world scope).
- Branch: `claude/adr0019-p1-fixes-20260912`.
- Worktree: `.worktrees/adr0019-p1-fixes-20260912`.
- Base: `origin/main` at `8dfd2ab`.
- Owned paths:
  - `impulse-rs/src/governed_producers.rs`
  - `impulse-rs/impulse-ops/src/governed_task.rs`
  - `impulse-rs/src/state/governed_task.rs`
  - `impulse-rs/tests/governed_staged_worktree.rs`
  - `docs/decisions/0019-builder-staged-worktree-world-scope.md`
  - `CONTEXT.md` (world-scope entry only), this work card
- Blocked/shared paths (owned by the sibling `claude/daemon-governed-wiring-20260912` lane or by
  repository policy): `impulse-rs/src/daemon/**`, `impulse-rs/src/client/**`,
  `impulse-rs/src/handlers/**`, `impulse-rs/impulse-ops/src/daemon*.rs`,
  `impulse-rs/impulse-desktop/**`, `.github/**`, `Cargo.toml`, `Cargo.lock`, `CLAUDE.md`,
  `AGENTS.md`.
- Plan/spec: the four P1 threads on PR #50, and ADR-0019 rules 4, 5, 6 and 13.
- Verification: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace
  --all-targets -- -D warnings && cargo fmt --all -- --check`, plus
  `python3 ../docs/validate_docs.py --all`.
- Latest status: all four findings reproduced, fixed, tested, and revert-checked; gate green.

## Decisions

- 2026-09-12: **The configuration comparison never spawns Git.** The digest is computed by reading
  files, and the Git directory / common directory are resolved by hand (`.git` directory or
  `gitdir:` pointer, then `commondir`) exactly as Git resolves them. This is what makes
  "compare before any Git runs" implementable at all, and it is also what fixes the ordering
  finding: there is no Git process left to order against.
- 2026-09-12: **`-c core.fsmonitor=false` is belt and braces, not the guarantee.** It is added to
  `hook_free_git` because materialization legitimately runs Git against pre-existing operator
  configuration, where no pin exists yet to compare. It is documented as supporting defense so a
  future reader does not mistake it for the load-bearing control.
- 2026-09-12: **The pin carries a scheme version rather than a renamed serde variant.** Renaming
  the variant would make an existing ledger fail to deserialize — the exact regression ADR-0019
  review round 3 fixed. `scheme_version` defaults to the legacy value and is skipped on
  serialization when legacy, so a pre-existing record re-serializes to the same bytes and its
  receipt fingerprint still matches.
- 2026-09-12: **`launch_working_directory` returns `Result`.** The silent fallback was the bug; a
  typed `LaunchWorkingDirectoryError` is the fix. This is the one signature change the daemon
  lane must absorb.
- 2026-09-12: **`run_verification` observes `launch_working_directory`,** and its detached verifier
  is created from that tree. The canonical checkout is neither read nor written by the verifier for
  a staged run.

## Changes

- `impulse-ops/src/governed_task.rs`: `SHARED_REPOSITORY_CONFIG_SCHEME_VERSION` (=2) and
  `LEGACY_SHARED_REPOSITORY_CONFIG_SCHEME_VERSION` (=0); `SharedRepositoryConfigDigest::current`,
  `::is_current_scheme`, and a defaulted, legacy-skipping `scheme_version` field;
  `SharedRepositoryConfigPin::{comparable, is_comparable}`; `launch_working_directory` now returns
  `Result<&str, LaunchWorkingDirectoryError>`.
- `src/governed_producers.rs`: `SharedGitPaths` (Git-free `.git` / `commondir` resolution);
  raw-bytes `digest_config_chain` with `include`/`includeIf` coverage, replacing the sorted
  `git config --list` digest; `read_head_oid_without_git`; `hook_free_git` also sets
  `core.fsmonitor=false`; `promote_governed_outcome` compares configuration before any Git
  invocation; `governed_source_root` used by both `derive_claim` and `run_verification`.
- `src/state/governed_task.rs`: `MarkRunning` refuses a `staged_authoritative` task with no active
  staged worktree; `staged_worktree_is_discardable` treats an uncomparable pin (absent *or* legacy
  scheme) as always discardable.
- `docs/decisions/0019-*.md`: Consequences corrected, plus a dated "Post-merge fixes (2026-09-12)"
  section.

## Tests

| Finding | Test |
|---|---|
| 1 — staged claims never verified | `governed_producers::tests::staged_verification_runs_against_the_staged_worktree_and_reaches_review`, `…::staged_verification_refuses_a_task_with_no_materialized_worktree` |
| 2 — Git ran before the config check | `test_promotion_compares_shared_config_before_running_any_git_command`, `test_promotion_blocks_a_builder_planted_fsmonitor_without_executing_it`, `test_materialization_never_executes_a_pre_existing_fsmonitor_hook` |
| 3 — order-insensitive digest | `test_promotion_blocks_a_reordered_repeated_config_key`, `test_promotion_blocks_a_change_to_an_included_config_file`, `test_an_unchanged_shared_config_with_includes_still_promotes`, `test_promotion_blocks_a_pin_recorded_under_a_superseded_scheme`, `…::test_shared_config_digest_is_stable_and_order_sensitive`, `…::test_shared_config_digest_covers_included_files`, `governed_task::tests::test_a_legacy_scheme_pin_loads_but_is_not_comparable` |
| 4 — `MarkRunning` without a worktree | `state::governed_task::tests::test_mark_running_requires_a_materialized_staged_worktree`, `…::test_mark_running_is_unchanged_for_an_authoritative_task`, `…::test_an_authoritative_ledger_with_a_running_task_still_replays`, `governed_task::tests::test_launch_working_directory_refuses_a_staged_task_without_a_worktree` |

Every fix was reverted once and its test watched to fail, including the two-part revert that
reproduces finding 2 exactly as reviewed (old ordering plus no `core.fsmonitor=false`: the
Builder-planted hook executes during promotion).

## Handoff Notes

- **For `claude/daemon-governed-wiring-20260912`:** one breaking signature.
  `GovernedTaskRun::launch_working_directory()` now returns
  `Result<&str, LaunchWorkingDirectoryError>` instead of `&str`. Every other producer signature is
  unchanged: `run_verification`, `derive_claim`, `materialize_staged_worktree`,
  `promote_governed_outcome`, and `discard_staged_worktree` keep their argument and return types.
- A daemon endpoint that marks a staged task running must materialize the staged worktree first;
  the state layer now refuses the transition otherwise, with a typed invalid-transition message.
