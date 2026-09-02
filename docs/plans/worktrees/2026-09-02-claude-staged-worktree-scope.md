---
title: Builder Staged-Worktree World Scope
description: Work card for claude-staged-worktree-scope-20260902 (ADR-0019 state and producer layer)
updated: 2026-09-02
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, world-scope, adr-0019, governance, sandbox, loop-contract]
---

# Builder Staged-Worktree World Scope

## Lane Facts

- Owner: Claude (Fable 5.1), Stage 4 of `docs/plans/2026-09-02-impulse-next-stages.md`.
- Role: implementation lane for the **state and producer layer only**. The daemon endpoint and the
  desktop launch wiring are separate lanes.
- Branch: `claude/staged-worktree-scope-20260902`.
- Worktree: `.worktrees/staged-worktree-scope-20260902` (repository-relative).
- Base: `origin/main` at `36bda00`.
- Owned paths:
  - `impulse-rs/impulse-ops/src/governed_task.rs`, `impulse-rs/impulse-ops/src/role_assignment.rs`
  - `impulse-rs/src/governed_producers.rs`, `impulse-rs/src/state/governed_task.rs`,
    `impulse-rs/src/loop_contract.rs` (additive only)
  - `impulse-rs/tests/governed_staged_worktree.rs` (new)
  - `docs/decisions/0019-builder-staged-worktree-world-scope.md`, `docs/decisions/README.md`,
    `docs/INDEX.md`, `docs/SUMMARY.md`, `docs/SUMMARY.yaml` (one row/entry each)
  - `CONTEXT.md` (one glossary entry), this work card
- Paths touched outside the owned list, and why (all disclosed, all mechanical or required):
  - `impulse-rs/src/state/memory_candidate.rs` — **required**, not cosmetic. See
    [Cross-lane changes](#cross-lane-changes).
  - `impulse-rs/src/lib.rs` — one line: `pub(crate) mod governed_producers;` →
    `pub mod governed_producers;`, so `tests/governed_staged_worktree.rs` can drive the three new
    `pub` producers against real Git repositories before any daemon endpoint exists. Every other
    item in the module remains `pub(crate)`.
  - `impulse-rs/impulse-desktop/src/{daemon_ops,runtime}.rs`,
    `impulse-rs/impulse-desktop/tests/runtime.rs`,
    `impulse-rs/impulse-ops/tests/governed_task_contract.rs` — **test fixtures only**, three added
    fields per `GovernedTaskRun` literal and one per `WorkerCompletionClaim` literal (7 + 1 sites).
    Unavoidable: adding fields to a shared struct breaks every struct literal. No production
    desktop code was read or modified.
- Blocked paths honored: `impulse-rs/src/daemon/**` (socket-provenance lane), all
  `impulse-desktop` production code, `.github/**`, `impulse-rs/scripts/**`, `Cargo.toml`,
  `Cargo.lock` (no new dependencies), `CLAUDE.md`, `AGENTS.md`.
- Plan/spec: Stage 4 of `docs/plans/2026-09-02-impulse-next-stages.md`; ADR-0019.
- Verification (isolated `CARGO_TARGET_DIR`, per the shared-target-dir memory note):
  `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `python3 docs/validate_docs.py --all`.

## Gate

Run on this lane's tree with
`CARGO_TARGET_DIR=/private/tmp/.../scratchpad/target-staged`.

| Command | Result |
|---|---|
| `cargo build --workspace` | clean, zero warnings |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean, zero warnings |
| `cargo fmt --all -- --check` | clean, zero diffs |
| `cargo test --workspace` | **2399 passed, 0 failed, 9 ignored** (base `36bda00`: 2310 passed; +89) |
| `python3 docs/validate_docs.py --all` | only the four failures that pre-exist on `main` |

Package-level totals from the same run:

| Package / target | passed | failed | ignored |
|---|---|---|---|
| `impulse-desktop` lib | 147 | 0 | 0 |
| `impulse-desktop` `desktop_contract` | 64 | 0 | 0 |
| `impulse-desktop` `host_surface` | 8 | 0 | 0 |
| `impulse-desktop` `runtime` | 22 | 0 | 1 |
| `impulse-desktop` `views_ssr` | 7 | 0 | 0 |
| `impulse-ion` lib | 23 | 0 | 1 |
| `impulse-ops` lib | 97 | 0 | 0 |
| `impulse-ops` `governed_producer_contract` | 8 | 0 | 0 |
| `impulse-ops` `governed_task_contract` | 5 | 0 | 0 |
| `impulse-ops` `memory_candidate_contract` | 5 | 0 | 0 |
| `impulse-rs` lib | 1830 | 0 | 5 |
| `impulse-rs` `governed_process_flow` | 2 | 0 | 0 |
| `impulse-rs` `governed_staged_worktree` (new) | 19 | 0 | 0 |
| `impulse-rs` hook-validation suites (3 files) | 15 | 0 | 0 |
| `impulse-rs` `integration_enhancements` | 11 | 0 | 1 |
| `impulse-rs` `ion_binary` | 4 | 0 | 0 |
| `impulse-rs` `ion_verify_cli` | 2 | 0 | 0 |
| `impulse-step-model` lib | 12 | 0 | 0 |
| `impulse-term` lib | 93 | 0 | 0 |
| `impulse-term` `backend_tests` | 19 | 0 | 0 |
| `impulse-term` `boundary_tests` | 3 | 0 | 0 |
| `impulse-gui` (legacy) | 3 | 0 | 1 |

The docs-validator failures that pre-exist on `main`: ADR-0014's `proposed` status, plus three
March guides past the 120-day staleness threshold.

## Decisions

- 2026-09-02: `WorldScope` is a four-variant enum on the record, but registration **fails closed**
  on `read_only_snapshot` and `disposable_scratch` rather than silently downgrading them. Declaring
  a contract you cannot honor is the failure mode the real-systems rule exists to prevent.
- 2026-09-02: The staged root is derived once, in `impulse-ops`
  (`GovernedTaskRun::expected_staged_worktree_root`), and both the producer that creates it and
  the ledger that validates it call that one function. A caller can never choose the path. A
  staged task id must be a single ASCII path segment, checked at registration.
- 2026-09-02: A blocked promotion is a recorded outcome, not an error. Review state stays
  `Accepted`, the staged worktree stays active, and a retry is allowed after exactly one
  `PromotionBlocked`. A second *successful* promotion is refused.
- 2026-09-02: `apply_mutation` takes the clock as a parameter. This is the change that makes the
  loop binding safe: replay passes each event's own stored timestamp, so a clock-derived verdict
  reproduces exactly instead of drifting with wall time. Validation was tightened to prove it —
  replayed event kind, replayed loop-report digest, and replayed staged/promotion records must all
  match what is stored.
- 2026-09-02: The loop contract is evaluated **only** for `staged_authoritative` tasks. Evaluating
  it for every task would re-verdict historical claims in existing ledgers under a budget that did
  not exist when they were written, and a wall-clock trip would then make an old ledger fail to
  load. Scoping it to the new world scope makes "old `GOVERNED_TASKS.json` loads" structural rather
  than lucky.
- 2026-09-02: `filesystem.scoped` reports `mediated`. A Git worktree is not containment; a Builder
  process can write anywhere the user can. `world_scope_filesystem_enforcement` can never return
  `Structural`, and a test asserts the ordering.
- 2026-09-02: No new trip variant was needed. Claim cycles map onto `LoopTrip::RoundCap`, the
  per-task budget onto `WallClock`, and repeated verification failures onto `SameError` with
  `tool: "governed_verification"`. The addition is a *pure* `LoopBudget::evaluate_observed` that
  reads no clock, alongside the existing `Instant`-based `LoopBreaker`.

## Changes

**`impulse-ops/src/governed_task.rs`** — `WorldScope`, `StagedWorktree` / `StagedWorktreeStatus` /
`StagedWorktreeInput`, `GovernedPromotion` / `GovernedPromotionInput` /
`GovernedPromotionOutcome`; `world_scope` on the registration and the run (serde default
`authoritative`); `staged_worktree` and `promotions` on the run; `loop_report_digest` on the claim;
`MaterializeStagedWorktree`, `DiscardStagedWorktree`, `RecordPromotion` mutations;
`StagedWorktreeMaterialized`, `StagedWorktreeDiscarded`, `LoopTripped`, `PromotionRecorded` event
kinds; `validate_path_segment`; `expected_staged_worktree_root`, `active_staged_worktree`,
`latest_promotion`, `launch_working_directory`.

**`impulse-ops/src/role_assignment.rs`** — `FILESYSTEM_SCOPED_CAPABILITY`,
`world_scope_filesystem_enforcement`, `world_scope_capability_support`, and
`evaluate_role_compatibility_in_world`, which merges the scope's contribution into a runtime's
declared support taking the stronger of the two per capability.

**`impulse-rs/src/loop_contract.rs`** (additive) — `GOVERNED_BUILDER_*` constants,
`LoopBudget::governed_builder`, `LoopContract::governed_builder`, `LoopObservation`,
`SameErrorObservation`, and the pure `LoopBudget::evaluate_observed`.

**`impulse-rs/src/state/governed_task.rs`** — the three new mutation arms with their transition
guards; `apply_mutation` takes the clock; the governed Builder loop verdict and
`loop_report_digest` on `SubmitClaim`; replay support for the four new event kinds; tightened
replay verification (event kind, loop digest, staged worktree, promotions).

**`impulse-rs/src/governed_producers.rs`** — `add_detached_worktree` extracted and now shared by
verification and staging; `materialize_staged_worktree`, `promote_governed_outcome`,
`discard_staged_worktree` (plus `_async` wrappers) and the isolated
`fast_forward_canonical_branch`; the cleanliness check ignores untracked paths under
`.impulse/worktrees/`.

## Tests

- `impulse-ops/src/governed_task.rs` (new `mod tests`, 17 tests): serde round trips for every new
  type, mutation, and event kind; `WorldScope` `Display`; the `authoritative` default from `{}`; a
  full pre-ADR-0019 `GovernedTaskRun` and `WorkerCompletionClaim` deserializing unchanged;
  registration error paths (no profile, unmaterializable scope, traversal task id);
  `validate_path_segment` rejections; `launch_working_directory` for active, discarded, and
  authoritative.
- `impulse-ops/src/role_assignment.rs` (6 tests): mediated-never-structural, the scope's single
  declared capability, the staged preview raising `filesystem.scoped` to `mediated` while staying
  *degraded*, an authoritative scope changing nothing, a stronger declared enforcement never being
  lowered, and duplicate-capability errors preserved.
- `impulse-rs/src/loop_contract.rs` (9 tests): contract validation, budget serde round trip, and
  `evaluate_observed` for admit / round cap / wall clock / same-failure streak / short streak /
  disabled detector / repeatability.
- `impulse-rs/src/state/governed_task.rs` (17 tests): materialization happy path and five refusal
  paths; discard guards in both directions; promotion state, identity, and coherence guards;
  blocked-then-retry; the full staged lifecycle surviving reload byte-for-byte; staged claims
  carrying a digest and authoritative claims not; the loop tripping at the claim-cycle cap and on a
  repeated verification failure, with the tripped history replaying equal on reload; a
  pre-ADR-0019 ledger (fields stripped from the persisted JSON) still loading; a rewritten loop
  digest and a rewritten staged root each failing the ledger closed.
- `impulse-rs/tests/governed_staged_worktree.rs` (new, 12 tests, real Git repositories in temp
  dirs): materialization creates a detached worktree at the attested OID and leaves the canonical
  branch on `main`; refusals for a non-staged scope, a moved head, and an occupied path; promotion
  fast-forwards `main` to the Builder's commit; a moved canonical head blocks without touching the
  branch or the working tree; refusals for a non-accepted task, a claim the staged worktree does
  not hold, and a dirty staged worktree; discard removes the checkout and its `git worktree list`
  entry; a rejected run leaves the canonical tree byte-identical with `git status --porcelain`
  empty.

Local `init_git_repo`-style helper deliberately kept inside the new test file: a sibling lane is
unifying the five existing copies, and this lane must not collide with that work.

## Cross-lane changes

**`impulse-rs/src/state/memory_candidate.rs` — required, please read.**

ADR-0013's accepted-run projection digested `task.revision` as `accepted_task_revision`. That was
equivalent to "the revision the acceptance landed on" only because nothing could mutate a task
after acceptance. ADR-0019 promotion mutations can. Without a fix, the first promotion after an
accept makes `derive_accepted_run_memory_candidate` bail with "incoherent candidate evidence
chain" — on the mutation itself *and* on every later reload, because
`reconcile_accepted_run_memory_candidates` re-derives from the current revision.

The fix is five lines inside `derive_accepted_run_memory_candidate`: derive
`accepted_task_revision` from `operator.based_on_revision + 1` and relax the coherence check from
`== task.revision` to `<= task.revision`. **For every record written before ADR-0019 the two
numbers are identical, so no stored candidate digest changes** — proven by the existing
`memory_candidate` suite passing unchanged.

Flagged for the producer-reservation-journal lane, which may also be touching state helpers.

**Confirmed compatible with ADR-0018** (`claude/socket-actor-provenance-20260902`, PR #48, also a
draft off `36bda00`). That lane edits `memory_candidate.rs` in two places that do not overlap this
hunk: the `source_assurance` / `proposed_summary` selection a few lines below the coherence check,
and a `prune_superseded_derivations()` in `MemoryCandidateLedger::load` for the
`ACCEPTED_RUN_MEMORY_DERIVATION_VERSION` 1 → 2 bump. `git merge-tree` auto-merges that file with no
conflict. Keep both hunks.

## Merging with ADR-0018

Neither PR is merged and `origin/main` is still `36bda00`, so nothing is rebased yet. A dry-run
`git merge-tree` against `origin/claude/socket-actor-provenance-20260902` reports six conflicts,
all mechanical and all **both-keep**:

| File | Conflict | Resolution |
|---|---|---|
| `docs/{INDEX,SUMMARY}.md`, `docs/SUMMARY.yaml`, `docs/decisions/README.md` | Both lanes added an ADR row directly after 0017 | Keep both rows, 0018 then 0019 |
| `impulse-rs/impulse-ops/src/governed_task.rs` | Both added types and a new `mod tests` | Union the imports, the types, and the test modules |
| `impulse-rs/src/state/governed_task.rs` | Both added a parameter to `apply_mutation`, and both appended tests | See below |

`apply_mutation`'s extra inputs are now bundled, which makes this merge smaller than it first
looked. Review round 1 replaced the loose `now: &str` parameter with a context struct:

```rust
struct MutationContext<'a> {
    now: &'a str,                            // ADR-0019: replay's clock
    replay_claim: Option<ReplayedClaimEvidence<'a>>, // ADR-0019: foreign-version loop evidence
    // ADR-0018 folds in here rather than becoming a fifth parameter:
    // operator_authentication: OperatorAuthentication,
}

fn apply_mutation(task, mutation, event_revision, context: MutationContext<'_>) -> Result<()>
```

`mutate_governed_task` builds it with `MutationContext::live(&now)` plus the connection's
authentication; `validate_task_history` builds it with `&event.created_at`, the replayed claim
evidence, and `replay_operator_authentication` — which the ADR-0018 lane already sets before the
match, so it is in scope exactly where `&event.created_at` goes. The additions are orthogonal: one
supplies replay's clock, one supplies replay's loop evidence, one supplies replay's actor
provenance. Import lists and the two `mod tests` blocks are a plain union.

Three further facts from ADR-0018's own review round, confirmed by that lane:

- `authorize_governed_mutation` no longer has a `_ => Ok(())` catch-all; it is an exhaustive match.
  Adding `RecordPromotion` is therefore **compiler-enforced** rather than something the daemon lane
  has to remember — the function will not build until the new variant is classified. That is the
  better outcome for this ADR's threat model.
- `OperatorDecisionInput` gained `#[serde(deny_unknown_fields)]`. This lane's fixtures build that
  payload as a struct literal, never as JSON with extra keys, so nothing here is affected.
- `OperatorCapability::generate()` is fallible and now lives in a `OnceLock` set in `start()`, and
  `impulse-ops` gains an `operator_capability.rs`. This lane touches neither `Daemon`'s fields nor
  the accept loop, so there is no overlap.

**PR #45 (producer reservation journal):** the reviewer confirmed no serde tag collision between
this lane's new event kinds and #45's `producer_reservation_interrupted` /
`note_producer_reservation_interrupted`. The piece #45 must satisfy after rebase is this lane's
whole-enum replay assertion — `replayed_kind != event.kind` bails, so #45's new event arm has to
replay to its own kind, not fall through to a neighbouring one.

**After whichever lane rebases second:** re-run `cargo test --workspace`, and specifically the
ADR-0018 superseded-derivation reload test, which asserts a re-derived candidate equals the
pre-bump one field for field. This lane's change feeds `operator.based_on_revision + 1` into the
source digest rather than `task.revision`, so a promotion cannot move the digested value — that is
the property that test is checking, and it should hold, but it must be re-run rather than assumed.

## Review round 1

Adversarial review returned "needs changes": claims a, b, e-determinism, f and g held; three P1s
and three P2s were each reproduced in a real repository. All six are fixed on this branch, with a
regression test per finding. ADR-0019's rules are rewritten as amended and carry their own
"Review round 1" section.

| # | Finding | Fix | Regression |
|---|---|---|---|
| P1-1 | Promotion "succeeded" on a detached canonical HEAD: `git merge --ff-only` moves whatever HEAD is, the post-check passed, the ledger recorded `Promoted`, the worktree was discarded, and the next `git switch` orphaned the work | Require `git symbolic-ref --quiet HEAD` to resolve to `refs/heads/*`; a detached checkout returns a blocked outcome, not an `Err`. Move the branch with `git update-ref <ref> <accepted> <initial>` — a real compare-and-swap — and sync the working tree afterwards. Descendant check kept | `test_promotion_blocks_on_a_detached_canonical_head_without_moving_anything`, `test_promotion_advances_the_branch_ref_not_only_head`, `test_canonical_branch_ref_reports_none_on_a_detached_head`, `test_compare_and_swap_refuses_a_stale_expected_revision`, `test_compare_and_swap_succeeds_against_the_current_revision` |
| P1-2 | `Escalated` is terminal with no transition out, so the loop-trip path this ADR's own contract produces leaked `.impulse/worktrees/<task_id>` forever | `staged_worktree_is_discardable` now allows `Escalated`, `execution_state: LaunchFailed`, and an `Accepted` run with any recorded promotion (promoted **or** blocked) | `test_escalated_task_can_discard_its_staged_worktree`, `test_launch_failed_task_can_discard_its_staged_worktree`, `test_blocked_promotion_allows_discarding_the_staged_worktree`, `test_a_running_task_still_cannot_discard_its_staged_worktree` |
| P1-3 | `loop_report_digest` had no version pin and replay recomputed it with the running build's constants, so revising the budget — which the ADR says will happen — made `GOVERNED_TASKS.json` unloadable | `GOVERNED_BUILDER_LOOP_VERSION` stored on the claim beside the digest; digest is over `canonical_json` plus the version; replay reuses stored evidence **and the stored outcome** for any other version, checking only structural coherence | `test_a_ledger_written_under_an_older_loop_version_still_loads`, `test_an_older_version_loop_trip_replays_as_a_trip`, `test_a_claim_with_a_digest_but_no_version_fails_the_ledger_closed` |
| P2-1 | `derive_claim` read `workspace_root`, so a real staged Builder's claim would carry `subject_revision == initial_oid` and every promotion would bail | Observe `task.launch_working_directory()` — unchanged for every non-staged task | `test_derive_claim_observes_the_staged_worktree_for_a_staged_task` |
| P2-2 | `.git/hooks` is shared across linked worktrees, so a staged Builder could plant a hook that runs inside a producer at materialization or promotion time | Every producer Git invocation goes through `hook_free_git`, which sets `core.hooksPath=/dev/null` | `test_planted_git_hooks_never_execute_during_staging_or_promotion` (plants `post-checkout`, `post-merge`, `reference-transaction`) |
| P2-3 | A staged directory surviving a crash blocked re-materialization with no stated recovery | Still fails closed; the error now names the recovery (delete the directory, then `git worktree prune`), and ADR-0019's Consequences documents it | `test_occupied_staged_path_error_names_the_recovery` |

Nits, all applied: the digest uses `loop_contract::canonical_json` rather than `serde_json::to_vec`;
ADR-0019 rule 8 now states that "never structural" bounds *the scope's own contribution* and that a
runtime declaring `structural` still reports `structural` through the `max`; the lane card says why
`pub mod governed_producers` is kept.

Two things the fixes changed beyond the literal asks, both worth flagging:

- **P1-3 needed the trip outcome replayed, not just the digest skipped.** Skipping only the digest
  comparison would still have re-run the verdict under the new constants, so an old claim could
  replay as `LoopTripped` when it was stored as `ClaimSubmitted` and fail the whole-enum event-kind
  assertion. Foreign-version claims now replay their stored outcome verbatim.
- **The compare-and-swap does not close the working-tree window.** `update-ref` writes the ref
  atomically; syncing the working tree is a second step, and a file written into the canonical
  checkout between the cleanliness observation and that sync would be replaced. Documented in
  ADR-0019's Consequences rather than papered over, and the sync failure message names the branch
  that already advanced so an operator is never left guessing.

## Review round 2

Not from the reviewer: the ADR-0018 lane, reading round 1's `core.hooksPath` fix, asked whether
`.git/config` and `.git/info/attributes` — also worktree-shared — were reachable the same way, and
said plainly it was not going to guess at code it does not own. They are reachable. I confirmed it
against a real repository before writing any code: with `core.hooksPath` disabled exactly as the
producers set it, a `filter.<name>.smudge` in the shared `.git/config`, assigned by an in-tree
`.gitattributes`, still executes on checkout. And a staged Builder can write that config from
inside its own worktree, because `git config --local` resolves to the shared file for every linked
worktree.

That made ADR-0019's "the promotion path is sound" false as written, so it is fixed here rather
than deferred:

| # | Finding | Fix | Regression |
|---|---|---|---|
| R2-1 | **Round 1's hook fix was incomplete.** It covered the two obviously mutating commands but not `run_git`, which backs every observation the module makes — including the `git status` promotion runs twice. A `post-index-change` hook executed there | Every Git invocation in the module is built by one hook-free constructor; there is no "read-only enough to skip it" category | `test_planted_git_hooks_never_execute_during_staging_or_promotion`, now planting `post-index-change`, `pre-auto-gc`, and `fsmonitor-watchman` alongside the original three |
| R2-2 | A `filter`/`diff` driver written to shared `.git/config`, `.git/info/attributes`, or `.git/config.worktree` by the Builder executes during promotion's working-tree sync, in the canonical workspace, under operator authority, after review passed. Hooks were only one door, and `info/attributes` never appears in a work-tree diff | `StagedWorktree` records a per-component digest of that shared state at materialization; promotion refuses to check anything out unless the digests still hold, returning `PromotionBlocked { reason: repository_config_changed { component } }`. Config predating the run is the operator's and is honored; config the Builder introduced blocks | `test_a_filter_driver_planted_by_the_builder_never_executes_during_promotion`, `test_a_benign_shared_config_change_blocks_and_names_the_file`, `test_a_shared_info_attributes_change_blocks_and_names_that_file` |

Two corrections taken from the reviewer on the digest gate, both applied: `.git/config.worktree` is
pinned as well (worktree-scoped config *not* accepting filter keys is convention, not a Git
guarantee — the file takes arbitrary keys, and while reaching it needs `extensions.worktreeConfig`,
which flipping would itself be caught, pinning removes the argument); and the refusal names **which**
component changed, because benign churn hard-blocks promotion too and an operator must not be left
guessing which file to open.

Both fixes were checked for vacuity rather than assumed:

- The filter test carries a **negative control** — it fires the planted driver inside the staged
  worktree and asserts the marker appears, then clears it, then promotes. Without that, a passing
  assertion would not distinguish "the fix works" from "the filter was never armed".
- The hook test was verified by **reverting** `run_git` to its raw form and re-running: it fails at
  materialization with `a planted hook executed during materialization: post-index-change`. It
  genuinely covers the gap round 1 left.

Also recorded in ADR-0019 at the reviewer's request: after a **blocked** promotion the accepted
commit exists only in the staged worktree, so discarding it drops the only ref and leaves the
commit reflog-only until expiry — the discard surface must say what it costs and show the OID
(desktop track owns the wording). And none of these vectors is reachable in a running system today:
these producers still have no IPC endpoint and no CLI handler, which bounds live exploitability to
zero and is exactly why they were worth fixing before something calls them.

Honest limits, both stated in ADR-0019: the pin is a change detector, not a sandbox — it does not
stop a Builder committing a `.gitattributes` that routes new paths through a filter the operator
had *already* configured, which would need the OS-level isolation this ADR defers. And it fails
closed on benign drift: an operator who legitimately edits `.git/config` mid-run gets a blocked
promotion. That bias is intended.

Two of the repository's own guard rules fired during this investigation, and I complied rather
than working around them: the recursive-delete guard on a scratch probe (switched to `mktemp -d`),
and the hard-reset guard on a probe script. The second is why the promotion half of the vector is
proven through the product's own code in a regression test rather than by hand-running the
command — the better outcome anyway, since the test now ships.

## Handoff Notes

### For the daemon / socket-provenance lane (`src/daemon/**`)

Three endpoints are needed; all producer and state work behind them is done and tested.

1. **`MaterializeStagedWorktree`** — or, better, fold it into `RegisterGovernedTask` when
   `registration.world_scope.requires_staged_worktree()`, since the worktree must exist before the
   PTY starts. Call `governed_producers::materialize_staged_worktree_async(task)`, then submit
   `GovernedTaskMutation::MaterializeStagedWorktree { staged }` with the returned input at
   `expected_revision = task.revision`. Same idempotency discipline as the other producers
   (`governed_producer_request_is_replay` before the side effect).
2. **`PromoteGovernedOutcome`** — request shape mirroring `GovernedVerificationRequest`
   (`request_id`, `project_id`, `task_id`, `expected_revision`, `deny_unknown_fields`). Call
   `governed_producers::promote_governed_outcome_async(task)`, then submit
   `GovernedTaskMutation::RecordPromotion { promotion }`. **This must be operator-class**, not
   Builder-reachable — it is the step that makes work canonical. Per the ADR-0018 lane, the whole
   enforcement is one match arm in
   `src/daemon/actor_provenance.rs::authorize_governed_mutation`: add
   `GovernedTaskMutation::RecordPromotion { .. } => "RecordPromotion"` to the gated set and
   promotion becomes Builder-unreachable with no other file changes. If promote arrives as its own
   `DaemonRequest` variant instead of through `MutateGovernedTask`, gate it the same way in
   `handle_governed_producer_request` — `connection_class.is_operator()` checked **before** the side
   effect, with `connection_class` threaded in from `ProcessRequestContext`. A `PromotionBlocked`
   result is a successful response with a blocked outcome, not an error response.
   Gate `DiscardStagedWorktree` too — it destroys work. `MaterializeStagedWorktree` can stay
   ungated if it has to run inside pre-PTY registration.

   Protocol arithmetic: ADR-0018 moves `PROTOCOL_VERSION` to 7, so the promote bump is **8**, and
   `docs/validate_docs.py` carries three version-keyed required markers
   (`**Protocol version: N**`, `"protocol_version": N`, `### vN — ...`) plus the matching headings
   in `docs/IPC-PROTOCOL.md` that must move with it. Also note that after ADR-0018 a client sending
   `MutateGovernedTask` presents the operator capability first on the same connection
   (`DaemonClient` reads `<socket>.operator-cap` automatically); any raw-JSON socket test or CLI
   path expecting an approval to succeed will get a typed refusal instead.
3. **`DiscardStagedWorktree`** — call `governed_producers::discard_staged_worktree_async(task)`,
   then submit `GovernedTaskMutation::DiscardStagedWorktree { actor, reason }`. Reachable after a
   rejection or after a successful promotion; the state layer enforces both.

Also needed: a protocol version bump and a matching CLI subcommand for promote, following the
`governed-verify` / `governed-review` shape. Note that `--daemon` is a global flag and must precede
the subcommand.

### For the desktop track (`impulse-desktop/**`)

- The profiled Builder launch should register with
  `.world_scope(WorldScope::StagedAuthoritative)`. Registration validates that a staged scope
  carries a verification profile and an attested initial OID, both of which that path already
  supplies.
- The PTY working directory must come from `task.launch_working_directory()`, never from
  `task.workspace_root` directly. It returns the staged root while the worktree is active and the
  canonical root otherwise, so the change is safe for non-staged launches too.
- The role-compatibility preview should call
  `role_assignment::evaluate_role_compatibility_in_world(platform, declared, assignment, scope)`
  instead of `evaluate_role_compatibility`, and surface `filesystem.scoped: mediated` for a staged
  launch. The launch stays *allowed but degraded* — do not present it as satisfied.
- The task view should show the staged root, the staged status, and any promotion outcome. A
  `PromotionBlocked { canonical_head }` needs an operator-readable explanation: the canonical
  branch moved off the OID the task was registered at.

### For the producer-reservation-journal lane

`fast_forward_canonical_branch` in `governed_producers.rs` is the entire canonical-branch side
effect, isolated in one function with observation before it and recording after it. Wrap that call
and the promote producer's crash window is covered. No reservation logic was implemented here.

## Unfinished and out of scope

- `ReadOnlySnapshot` and `DisposableScratch` are declared but not materializable; registration
  refuses them. Making them real is future work.
- The loop budget constants (5 claim cycles, 4 hours, 3 same failures) have no production data
  behind them. They are constants in `loop_contract.rs`, meant to be revised once real governed
  Builder runs exist.
- ADR-0017's advisory heartbeat through `PublishTerminalOps` was **not** implemented. It is a
  daemon-side concern and `src/daemon/**` is blocked this session; the state layer already
  guarantees the property that mattered — nothing in the loop binding touches `review_state`
  except the trip itself.
- OS-level sandboxing and egress allowlists remain explicitly deferred. This is a Git-level scope,
  and ADR-0019 says so in as many words.
