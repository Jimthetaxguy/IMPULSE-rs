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
| `cargo test --workspace` | **2378 passed, 0 failed, 9 ignored** (base `36bda00`: 2310 passed; +68) |
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
| `impulse-ops` lib | 94 | 0 | 0 |
| `impulse-ops` `governed_producer_contract` | 8 | 0 | 0 |
| `impulse-ops` `governed_task_contract` | 5 | 0 | 0 |
| `impulse-ops` `memory_candidate_contract` | 5 | 0 | 0 |
| `impulse-rs` lib | 1819 | 0 | 5 |
| `impulse-rs` `governed_process_flow` | 2 | 0 | 0 |
| `impulse-rs` `governed_staged_worktree` (new) | 12 | 0 | 0 |
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
   Builder-reachable — it is the step that makes work canonical, so it belongs behind whatever
   actor authorization ADR-0018 lands. A `PromotionBlocked` result is a successful response with a
   blocked outcome, not an error response.
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
