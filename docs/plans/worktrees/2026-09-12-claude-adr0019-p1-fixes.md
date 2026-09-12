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

## Cross-lane pickups (2026-09-12)

Handed over by the sibling `claude/daemon-governed-wiring-20260912` lane (PR #52) and carried here
so the two PRs do not diverge:

- **Cherry-picked `8dca0a1` verbatim** — `cargo clippy --workspace --all-targets -- -D warnings`
  fails on `origin/main` with the installed toolchain (rustc 1.98.1) at two pre-existing one-line
  sites outside both lanes: `impulse-term/src/renderer.rs` (`float_literal_f32_fallback`, now
  hard-denied) and `impulse-desktop/tests/desktop_contract.rs` (`clippy::drain_collect`). Neither
  is caused by this lane; without the pick the gate cannot run to completion.
- **Cherry-picked `e533b09` verbatim** — exempts `.impulse/PRODUCER_RESERVATIONS.json` from the
  producers' cleanliness check and adds it to what `impulse init` gitignores.
- **Added on top, in this lane's own file:** `.impulse/MEMORY_CANDIDATES.json` (and its `.tmp.`
  siblings) had the identical gap — gitignored by `init`, never exempted from the cleanliness
  check — so in a project whose `.impulse` is not gitignored, recording an operator approval
  dirtied the canonical tree and the very next promotion failed on a tree the daemon had dirtied
  itself. Promotion is only reachable *after* an approval, so this was on the critical path.
  Test: `state::governed_task::tests::test_an_operator_approval_leaves_an_ungitignored_canonical_tree_promotable`,
  which drives the real chain (register → real `materialize_staged_worktree` → launch → Builder
  commit → claim → verify → recommend → approve → promote) against a real repository with no
  `.gitignore`, with a negative control asserting Git can actually see the candidate ledger.
  Reverting the exemption fails it.

## Review round 1 (2026-09-12, PR #53)

An adversarial review confirmed all five claims and the whole non-vacuity table, and returned two
P1s, three P2s, and two nits. All addressed on this branch.

| Finding | Fix | Test | Revert-check |
|---|---|---|---|
| **P1** — the staged worktree's own `<common>/worktrees/<id>/config.worktree` was unpinned; reachable whenever `extensions.worktreeConfig` is already on, with `.git/config` left byte-identical | `shared_repository_config_digest` takes the staged root and pins the canonical, common, and staged copies; recorded after the worktree exists; every comparison passes the same root | `test_promotion_blocks_a_driver_planted_in_the_staged_worktrees_own_config`, `test_worktree_config_extension_alone_does_not_block_promotion` | dropping the staged paths fails it |
| **P1** — no pin comparison on the claim or verification paths; a `filter.*.clean` executed during `governed-claim` | `ensure_staged_config_pin_holds` before the first Git call in both, typed `StagedConfigRefusal::{Changed,Unpinned}` | `test_derive_claim_refuses_a_staged_worktree_whose_shared_config_changed`, `test_run_verification_refuses_a_staged_worktree_whose_shared_config_changed`, `test_derive_claim_refuses_an_unpinned_staged_worktree` | removing the gate fails with *the planted driver executed inside the claim producer: CLEAN_FIRED* |
| **P2** — the `MarkRunning` precondition was enforced during replay, so a #50-era ledger failed to load entirely | `MutationContext::is_replay`; transition rejections are live-only | `test_a_staged_ledger_that_ran_before_materialization_still_replays` (also asserts the live path still refuses) | enforcing it on replay fails it |
| **P2** — global/system Git config in force for every producer | `hook_free_git` sets `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` to `/dev/null`, documented | `test_producer_git_invocations_suppress_global_and_system_config` | dropping it fails it |
| **P2** — include parser missed backslash continuation and `~user/` | continuation handled (`join_continued_lines`); `~user/` recorded as a residual in ADR-0019 | `test_config_include_paths_honors_backslash_line_continuation` | dropping the join fails it |
| **P2** — reftable repositories return `Err` instead of a typed blocked record | documented as a residual in ADR-0019 (fails closed, still names the component) | — | — |
| Nit — the launch error did not name the task | `LaunchWorkingDirectoryError::StagedWorktreeNotMaterialized { task_id, scope }` | assertion added to `test_launch_working_directory_refuses_a_staged_task_without_a_worktree` | — |
| Nit — `read_head_oid_without_git` read the same directory twice in a main worktree | deduplicated | existing `test_read_head_oid_without_git_matches_git_for_every_ref_shape` | — |

The digest docstring's "survives being wrong about Git" claim is toned down in both the code and
ADR-0019: within the files it covers the gate detects change without enumerating keys, but it is
*not* immune to being wrong about **which files Git reads** — which is exactly how both P1s
happened. The file list is now named as the security-relevant surface.

One lesson worth keeping: the test harness had been setting `GIT_CONFIG_GLOBAL=/dev/null` all
along while production did not. A harness safer than production hides the thing it is meant to
test.

## Review round 2 (2026-09-12, PR #53)

Round-2 verification confirmed every round-1 fix and both P1 reverts. Three P3s and two wording
corrections; the branch freezes after them.

| Finding | Fix | Test | Revert-check |
|---|---|---|---|
| **P3** — submodule config (`<common>/modules/<name>/config`, `<common>/worktrees/<id>/modules/<name>/config`, and their `info/attributes`) is outside the pinned set, while rule 13 claims every repository-level config file is pinned. Verified reachable on Git 2.50.1. | **refusal, not coverage**: `ensure_no_submodule_configuration` refuses a repository carrying a `.gitmodules` in either tree or a `modules/` directory in either Git directory — at materialization and on every producer path | `test_materialize_refuses_a_repository_with_submodule_configuration`, `test_producers_refuse_a_submodule_repository_introduced_mid_run` | both fail when the check is dropped |
| **P3** — the scheme version stayed 2 although round 1 changed the covered file set and the attributes domain separator, so a pin from the previous commit was *compared* (misleading `RepositoryConfigChanged`) instead of refused | bumped to 3; the constant's doc now says to bump on any change to the covered set, not only the algorithm | `test_a_scheme_two_pin_is_refused_rather_than_compared` | reverting to 2 fails it |
| **P3** — `join_continued_lines` let a comment ending in `\` swallow a following `[include]` header Git honors | comment lines never continue and never absorb a continuation in progress; docstring corrected | `test_config_include_paths_are_not_hidden_by_a_continued_comment` | removing the comment branch fails it |
| Nit — the materialization window was undocumented | one sentence in `materialize_staged_worktree` and rule 13: synchronous, no `.await`, no launch path (launch needs the active record, `MarkRunning` refuses without it, materialization is operator-only) | — | — |
| Wording — "every producer Git invocation" | now "every daemon-owned producer in `governed_producers.rs`", in the commit, the code, and ADR-0019 | — | — |

Why refusal over coverage for the submodule finding: extending the pin means replacing a fixed file
list with a walk of a directory tree whose shape is Git's to change, which weakens exactly the
property rule 13 rests on. A named capability gap is worth more than a coverage claim that has to be
re-derived at runtime.

Why the scheme bump matters more than it looks: comparing two pins computed over *different* file
sets does not fail safe in the useful sense — it reports a change that never happened, and sends an
operator looking for it. An uncomparable pin must say so.

## Follow-ups

- `staged_worktree_is_discardable` is duplicated: the state layer holds the enforcing copy, and
  `impulse_ops::governed_wiring` (PR #52) holds a preflight copy. Deliberately **not** unified in
  either PR — whichever lands second should not be rebased around a refactor. Unify in a dedicated
  follow-up once both are merged, keeping the state layer as the single enforcement point.

## Handoff Notes

- **Desktop track — a governed-path Git invocation with none of this hardening.**
  `impulse-desktop/src/runtime.rs`'s `run_bounded_governed_git` (the Dioxus governed-launch
  preflight) sets no `core.hooksPath`, no `core.fsmonitor`, and no `GIT_CONFIG_GLOBAL`/
  `GIT_CONFIG_SYSTEM`, and its env scrub keeps `HOME`. It runs in the operator's own checkout
  before any staged worktree exists, so it is outside ADR-0019's trust boundary, but it is a Git
  invocation on a governed path without the protections every producer has. **Not fixed here:
  `impulse-desktop/**` is blocked for this lane.** Recorded in ADR-0019's residuals too.
- **For `claude/daemon-governed-wiring-20260912`:** one breaking signature.
  `GovernedTaskRun::launch_working_directory()` now returns
  `Result<&str, LaunchWorkingDirectoryError>` instead of `&str`. Every other producer signature is
  unchanged: `run_verification`, `derive_claim`, `materialize_staged_worktree`,
  `promote_governed_outcome`, and `discard_staged_worktree` keep their argument and return types.
- A daemon endpoint that marks a staged task running must materialize the staged worktree first;
  the state layer now refuses the transition otherwise, with a typed invalid-transition message.
