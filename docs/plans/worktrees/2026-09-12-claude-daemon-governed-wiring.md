---
title: Daemon Governed Wiring Lane
description: Work card for the lane that makes ADR-0019's staged producers and ADR-0012's reservation journal reachable over the daemon socket (protocol v9)
updated: 2026-09-12
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, governed, adr-0012, adr-0019]
---

# Daemon Governed Wiring Lane

## Lane Facts
- Owner: Claude (Fable 5.1)
- Role: implementation lane
- Branch: `claude/daemon-governed-wiring-20260912` (based on `origin/main` `8dfd2ab`)
- Worktree: `.worktrees/daemon-governed-wiring-20260912`
- Owned paths: `impulse-rs/src/daemon/**`, `impulse-rs/src/handlers/**` (governed CLI),
  `impulse-rs/src/cli.rs` (subcommand registration), `impulse-rs/src/client/**`,
  `impulse-rs/impulse-ops/src/{lib,governed_wiring}.rs`, `impulse-rs/tests/**`,
  `docs/IPC-PROTOCOL.md`, `docs/validate_docs.py` (markers only), the ADR-0012 and ADR-0019
  appendices, `CONTEXT.md` (glossary entries), two named `CLAUDE.md` lines, this card.
- Blocked/shared paths: `impulse-rs/impulse-desktop/**`, `.github/**`, `impulse-rs/scripts/**`,
  `Cargo.toml`/`Cargo.lock`, `AGENTS.md`, `impulse-rs/src/state/**`, and — per the 2026-09-12
  coordination note — `impulse-rs/src/governed_producers.rs`,
  `impulse-rs/impulse-ops/src/governed_task.rs`, `impulse-rs/src/state/governed_task.rs`, all three
  owned by the sibling `claude/adr0019-p1-fixes-20260912` lane.
  **Three hunks taken by necessity; see "Blocked-path hunks" below.**
- Plan/spec: ADR-0012 (2026-09-02 amendment + its handler-wiring note),
  ADR-0019 ("Not delivered by this ADR's lane"),
  `docs/superpowers/specs/2026-09-02-producer-reservation-journal.md` ("Handoff: handler wiring").
- Verification: isolated `CARGO_TARGET_DIR`; `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `python3 ../docs/validate_docs.py --all`.
- Latest status: complete; draft PR open.

## Decisions

- **2026-09-12: the new request/ack types live in a new `impulse-ops/src/governed_wiring.rs`, not in
  `governed_task.rs`.** The original brief put them in `governed_task.rs`; a mid-lane coordination
  note assigned that file to the ADR-0019 P1 lane. The types were moved to a sibling module rather
  than dropped, so this lane touches none of the three files that lane owns. The split is also
  honest on its own terms: `governed_task.rs` is the durable ledger contract, `governed_wiring.rs`
  is request/response shape for one protocol version.
- **2026-09-12: staged materialization happens inside `RegisterGovernedTask`, before the record
  exists.** ADR-0019 requires the checkout before any PTY launch, and the brief requires "a failed
  materialization leaves no task record". Those two together force the order: build a provisional
  `GovernedTaskRun` from the registration (carrying exactly the four fields the producer reads),
  materialize, *then* register and record the mutation. The provisional record canonicalizes its
  workspace root the same way the state layer does, and the registered record's derived staged root
  is re-compared against the materialized one rather than assumed equal.
- **2026-09-12: registration with a staged scope is operator-class.** A staged launch is
  operator-initiated, and a Builder that could stage its own scope could materialize a world scope
  it was never registered for. Non-staged registration keeps its pre-ADR-0019 behavior exactly, so
  no existing caller changes.
- **2026-09-12: producer acknowledgements flatten the governed task.** `#[serde(flatten)]` makes
  `GovernedProducerAck` a strict superset of the bare `GovernedTaskRun` earlier versions returned,
  so `replayed` and `pending_rerun_reason` could be added without breaking a pre-v9 client. Two
  tests pin that compatibility rather than leaving it to inspection.
- **2026-09-12: the replay path deliberately takes no reservation.** When the existing receipt check
  recognizes a request id, no side effect runs, so there is nothing to protect; reserving there
  would only create a duplicate-conflict surface.
- **2026-09-12: discard is preflighted against a copy of the state layer's discardability rule.**
  The enforcing authority is still `state/governed_task.rs`'s private
  `staged_worktree_is_discardable`, which refuses the mutation. But the destructive side effect runs
  *before* that mutation, so refusing only at the mutation would delete the checkout and then
  record nothing. `impulse_ops::governed_wiring::staged_worktree_is_discardable` mirrors the rule so
  the daemon can refuse first. **Unifying them is a one-line change in a blocked file — see
  handoff.**
- **2026-09-12: no reservation around discard — corrected in review round 1.** The original
  rationale here said discard "records no evidence", which is **wrong**: the mutation flips the
  staged record's `status` to `discarded` and appends an event, so the crash window is real. A
  daemon that exits between removing the checkout and persisting that receipt leaves a record whose
  `status` is still `active` pointing at a path that no longer exists. What actually bounds it is
  narrower and worth stating precisely: the side effect is **idempotent on retry** (removing an
  already-removed checkout is a no-op, and the endpoint's replay branch answers from the receipt),
  and the stale record is inert because nothing re-launches a task that has already reached a
  discardable state. The correct fix is a fourth `ProducerKind::Discard` and a `with_reservation`
  wrap, which needs a variant added to `src/state/producer_reservation.rs` — a file this lane does
  not own. It is on the handoff list below and recorded in the ADR-0012 appendix.

## Changes

- `impulse-rs/impulse-ops/src/governed_wiring.rs` (**new**): `GovernedPromotionRequest`,
  `GovernedStagedWorktreeDiscardRequest`, `GovernedProducerAck`,
  `GovernedStagedWorktreeDiscardAck`, `staged_worktree_is_discardable`,
  `unreferenced_accepted_commit_on_discard`, 17 tests.
- `impulse-rs/impulse-ops/src/lib.rs`: `DAEMON_PROTOCOL_VERSION` 8 -> 9; two new
  `WorkbenchDaemonRequest` variants; `pub mod governed_wiring`.
- `impulse-rs/src/daemon/governed_wiring.rs` (**new**): staged materialization at registration with
  rollback, the promote and discard endpoints, the `reserved_producer` wrapper, and the
  reservation-wrapped `RunGovernedVerification` / `RunGovernedSupervisorReview` handlers. 15 tests
  against real Git repositories and real project state.
- `impulse-rs/src/daemon/protocol.rs`: `PromoteGovernedOutcome` and
  `DiscardGovernedStagedWorktree` variants, `request_type_name` arms, shared-request compatibility
  assertions, protocol-version marker test 8 -> 9.
- `impulse-rs/src/daemon/handlers.rs`: routing for the two new requests; registration delegates to
  `governed_wiring::register_governed_task`; `project_id` control-character validation extended;
  the verification and Supervisor-review arms moved into `governed_wiring` so their side effect and
  receipt share one reservation closure; nine helpers widened to `pub(super)`.
- `impulse-rs/src/daemon/mod.rs`: one `pub mod governed_wiring;` line.
- `impulse-rs/src/client/mod.rs`: `promote_governed_outcome` and
  `discard_governed_staged_worktree`; `governed_task_response` generalized to `governed_response<T>`;
  `requires_operator_class` extended to the two new requests plus `RegisterGovernedTask`.
- `impulse-rs/src/cli.rs`, `src/handlers/daemon_dispatch.rs`, `src/handlers/direct_dispatch.rs`:
  `governed-promote` and `governed-discard` under the global `--daemon` flag, with a printer that
  names the blocked reason and the commit a discard leaves unreferenced.
- `impulse-rs/src/handlers/config.rs`: `PRODUCER_RESERVATIONS.json` added to the ignore list
  `impulse init` writes, plus `repo_runtime_gitignore_entries()` so governed test fixtures can
  reproduce a real project's `.gitignore` instead of hand-maintaining a second copy.
- `impulse-rs/tests/daemon_governed_wiring.rs` (**new**): three real-daemon socket proofs.
- Docs: `docs/IPC-PROTOCOL.md` (v9 + the missing v8 changelog entry), `docs/validate_docs.py`
  markers, ADR-0012 "Handler wiring landed (2026-09-12)", ADR-0019 "Not delivered" table and two
  superseded Consequences paragraphs, `CONTEXT.md` (producer reservation, world scope, governed
  task wire), two `CLAUDE.md` lines.

## Blocked-path hunks taken by necessity

Each is isolated so it can be dropped or re-applied independently.

1. **`impulse-rs/src/governed_producers.rs`** (sibling P1 lane's file) — one additive entry in
   `is_untracked_impulse_runtime_artifact`: `.impulse/PRODUCER_RESERVATIONS.json` plus its
   `.tmp.` prefix. **Caused by this lane and not optional.** Adopting `with_reservation` made the
   daemon write that ledger on a governed path for the first time; in a project whose `.impulse`
   namespace is not gitignored, the file made the canonical worktree read as dirty the moment a
   verification reserved, which broke the existing
   `daemon_owned_producers_complete_one_persistent_governed_run` test and would break every real
   claim, registration, and promotion in the same workspace. The hunk is three lines inside a
   `matches!` list plus one `starts_with`; the P1 lane's four fixes are elsewhere in the file.
2. **`impulse-rs/impulse-term/src/renderer.rs`** — `1.0` -> `1.0_f32` in one `egui::Stroke::new`
   call. Pre-existing `clippy -D warnings` failure on the current toolchain
   (`float_literal_f32_fallback`), unrelated to this lane, blocking the gate. No behavior change:
   the literal was already `f32` by inference fallback.
3. **`impulse-rs/impulse-desktop/tests/desktop_contract.rs`** — `drain(..).collect()` ->
   `std::mem::take`. Same situation: a pre-existing `clippy::drain_collect` failure in a blocked
   crate, blocking the gate. One line, no behavior change.

## Tests

New, all passing:

- `impulse-ops::governed_wiring` (17): serde round trips for every new type;
  `deny_unknown_fields` refusals proving a caller cannot author the promotion outcome or the
  discard actor; `validate()` `Err` paths (blank, NUL, oversize) and the accept path; the
  discardability matrix (live / rejected / escalated / accepted-with-and-without-promotion /
  launch-failed / unpinned); the unreferenced-commit matrix; and two tests proving each ack
  deserializes as a bare `GovernedTaskRun` for a pre-v9 client.
- `impulse-rs::daemon::governed_wiring` (15), against real Git repositories and real project state:
  staged registration materializes at the attested OID before any launch and sets
  `launch_working_directory`; a non-operator staged registration is refused and records nothing;
  an authoritative registration is unaffected; a materialization that fails leaves no task record;
  a non-operator promote is refused with the task byte-identical and the branch unmoved; an
  operator promote fast-forwards the branch *and* syncs the working tree; a canonical head that
  moved returns a successful blocked outcome with review state still `accepted`; discard is refused
  while the run is live (before anything is deleted) and succeeds after a blocked promotion,
  naming the unreferenced accepted commit; a non-operator discard is refused; a live same-revision
  reservation refuses a second verification with the journal's typed error; an interrupted
  reservation is reconciled, visible on the task's event chain, and the rerun proceeds carrying
  `pending_rerun_reason`; a fresh verification reports none; plus routing and scope refusals.
- `impulse-rs/tests/daemon_governed_wiring.rs` (5), real daemon over the socket: a raw
  non-operator connection can neither stage its own world scope nor promote nor discard, and the
  operator surface can; an unknown task id is refused before any side effect; a replayed staged
  registration returns the recorded task without disturbing a live Builder's checkout; and a
  materialization failure reaches the operator with its recovery text intact.
- `impulse-rs::handlers::daemon_dispatch::governed_message_tests` (3): the two operator-facing
  messages ADR-0019 requires are one line each with no run of spaces, and say what they must.

### Gate

Run on this checkout with `CARGO_TARGET_DIR` isolated to
`/private/tmp/.../scratchpad/target-wiring`.

| Command | Result |
|---|---|
| `cargo build --workspace` | clean |
| `cargo test --workspace` | **2905 passed / 0 failed / 9 ignored** after the final verification round, against `origin/main` `22f9630` (2612 initial, 2624 after review round 1, 2762 after the #53/#55 merge; the rest is #56's and #59's own tests arriving with main) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `python3 ../docs/validate_docs.py --all` | 4 pre-existing failures only (unchanged) |

Per-target totals below are from the **pre-merge** run and are kept as the record of what this
lane's own work contributed; after three `origin/main` merges the workspace total is dominated by
other lanes' tests. The final run's own figures: `impulse-ops` lib **157**, `impulse-rs`
tests/daemon_governed_wiring **5**, tests/governed_staged_worktree **30**,
tests/socket_actor_provenance **5**.

| Target | passed | failed | ignored |
|---|---|---|---|
| `impulse-rs` lib | 2004 | 0 | 5 |
| `impulse-rs` tests/daemon_governed_wiring (**new**) | 5 | 0 | 0 |
| `impulse-rs` tests/governed_process_flow | 2 | 0 | 0 |
| `impulse-rs` tests/governed_staged_worktree | 21 | 0 | 0 |
| `impulse-rs` tests/socket_actor_provenance | 5 | 0 | 0 |
| `impulse-rs` tests/integration_enhancements | 11 | 0 | 1 |
| `impulse-rs` tests/ion_binary | 4 | 0 | 0 |
| `impulse-rs` tests/ion_verify_cli | 2 | 0 | 0 |
| `impulse-rs` tests/hook_validation_extraction_benchmark | 5 | 0 | 0 |
| `impulse-rs` tests/hook_validation_precompact | 5 | 0 | 0 |
| `impulse-rs` tests/hook_validation_session_start | 5 | 0 | 0 |
| `impulse-rs` doc-tests | 3 | 0 | 1 |
| `impulse-rs` bins (`main.rs`, `bin/ion.rs`) | 0 | 0 | 0 |
| `impulse-ops` lib | 129 | 0 | 0 |
| `impulse-ops` tests/governed_producer_contract | 8 | 0 | 0 |
| `impulse-ops` tests/governed_task_contract | 5 | 0 | 0 |
| `impulse-ops` tests/memory_candidate_contract | 5 | 0 | 0 |
| `impulse-desktop` lib | 153 | 0 | 0 |
| `impulse-desktop` tests/desktop_contract | 64 | 0 | 0 |
| `impulse-desktop` tests/host_surface | 8 | 0 | 0 |
| `impulse-desktop` tests/runtime | 22 | 0 | 1 |
| `impulse-desktop` tests/views_ssr | 7 | 0 | 0 |
| `impulse-term` lib | 94 | 0 | 0 |
| `impulse-term` tests/backend_tests | 19 | 0 | 0 |
| `impulse-term` tests/boundary_tests | 3 | 0 | 0 |
| `impulse-ion` lib | 23 | 0 | 1 |
| `impulse-step-model` lib | 12 | 0 | 0 |
| all other doc-test targets | 0 | 0 | 0 |

The four pre-existing `validate_docs.py` failures, unchanged by this lane:

1. `decisions/0014-work-item-and-comparative-settlement.md` — invalid status `proposed`.
2. `docs/LONG-RANGE-ENHANCEMENTS.md` — stale (2026-03-17).
3. `docs/guides/RUST-MULTI-AGENT-PATTERNS.md` — stale (2026-03-31).
4. `docs/guides/RUST-MULTI-AGENT-PROGRAMMING.md` — stale (2026-03-31).

## Conflict surface

### With `origin/agent/codex-dioxus-packaged-acceptance-20260830` (stale, rebases after this lane)

`git diff --stat 36bda00..origin/agent/codex-dioxus-packaged-acceptance-20260830 -- impulse-rs/src/daemon/`
reports `handlers.rs +19/-7`, `mod.rs +40`, `protocol.rs +4/-2`, `tests.rs +40/-7`. Compared against
this lane's daemon diff (`handlers.rs`, `mod.rs +1`, `protocol.rs`), the surface is:

| File | Codex hunks | This lane's hunks | Expected outcome |
|---|---|---|---|
| `src/daemon/protocol.rs` | `test_protocol_version_is_seven` asserting `7` | `test_protocol_version_is_nine` asserting `9` | **Real conflict, one test.** Take this lane's version: the constant is 9 on `main` after this merges. Codex must not re-take 7. |
| `src/daemon/handlers.rs` | `ProcessRequestContext.daemon_identity` (~698), the destructuring (~716), the `Ping` arm (~783), a new `handle_ping` (~916), `handle_chat_request` model resolution (~1461) | governed routing arms later in the same `match` (~850), registration delegation (~988), helper visibilities (~1061-1081), the verification arm (~1220-1300), the Supervisor-review arm (~1942) | **No overlapping hunk.** The `Ping` arm and the governed arms are in the same `match` but far apart. Keep both. |
| `src/daemon/mod.rs` | hunks at 134, 155, 190, 208, 230, 320, 334, 369, 386, 444 | one line at ~14 (`pub mod governed_wiring;`) | **No overlap.** Keep both. |
| `src/daemon/tests.rs` | +40/-7 | untouched by this lane | No conflict. |
| `docs/validate_docs.py` | the three version-keyed markers | the same three markers, moved to 9 (and a `### v8` line added) | **Real conflict.** Take one version, not both; this lane's is correct against the merged constant. |

Note: that Codex branch's diff is measured against `36bda00`, an old base. Against current `main` it
also appears to delete `src/ion_repl/tool_document.rs` and `src/loop_contract.rs`, which landed
after its fork point. That is a rebase problem for that lane, not a conflict with this one.

### With `claude/adr0019-p1-fixes-20260912`

By construction this lane touches none of that lane's three files **except** the single additive
`is_untracked_impulse_runtime_artifact` entry described under "Blocked-path hunks". None of that
lane's four reported fixes (`run_verification`'s workspace observation, `git status` ordering in
promotion, digest sorting, `MarkRunning`'s staged fallback) is in that function.

## Handoff Notes

### For the desktop track (`impulse-desktop/**`)

- **Promote and discard buttons.** The endpoints exist now:
  `DaemonRequest::PromoteGovernedOutcome { request: GovernedPromotionRequest { request_id,
  project_id, task_id, expected_revision } }` and `DaemonRequest::DiscardGovernedStagedWorktree
  { request: GovernedStagedWorktreeDiscardRequest { .., reason } }`. The desktop client already
  presents the operator capability on its connection (#48), so no new auth work is needed — but
  `impulse-desktop/src/daemon_ops.rs` must add these two to whatever it treats as requiring the
  capability, exactly as `DaemonClient::requires_operator_class` does.
- **A blocked promotion is a success.** Do not render it as an error. Read
  `task.promotions.last().outcome`; a `promotion_blocked` carries `canonical_head` and a `reason`
  (`canonical_head_moved`, `detached_head`, `concurrent_branch_update`,
  `repository_config_changed { component }`, `repository_config_unpinned`). Review state stays
  `accepted` and the staged worktree stays active, so the correct affordance is "reconcile the
  canonical branch and retry", not "the run failed".
- **The discard confirmation must state what it costs.** The discard acknowledgement carries
  `unreferenced_accepted_commit` when the discard drops the only ref to an accepted-but-blocked
  commit. ADR-0019's Consequences require the surface to say so and to show the OID. The CLI's
  wording in `handlers/daemon_dispatch.rs::handle_governed_discard` is a usable starting point.
- **Registration no longer needs a separate materialize step.** Register with
  `.world_scope(WorldScope::StagedAuthoritative)` and the returned record already carries an active
  staged worktree; take the pane cwd from `task.launch_working_directory()`.
- **Switch `staged_config_refusal_notice` off string matching.** PR #58 recognizes #53's
  `StagedConfigRefusal` by text-matching its strings at one replacement point in
  `impulse-desktop/src/ui.rs`. Once this lane adds the typed response variant (post-#53 checklist
  step 5), that notice should read the variant and its `component` instead. Text matching on an
  error message is a contract nobody declared: it breaks silently the first time the wording is
  improved, and the wording of exactly these operator-facing strings has already been corrected
  once in this lane's review round 1.
- **Producer responses changed shape (additively).** `RunGovernedVerification`,
  `RunGovernedSupervisorReview`, and `PromoteGovernedOutcome` now answer with the task flattened
  plus `replayed` and `pending_rerun_reason`. Existing code that deserializes a `GovernedTaskRun`
  keeps working; surfacing `pending_rerun_reason` is the new opportunity (it means a crashed
  producer's work is being redone).

### For the ADR-0019 P1 lane (`impulse-rs/src/{governed_producers,state/governed_task}.rs`, `impulse-ops/src/governed_task.rs`)

Three exact changes this lane needs but did not make, in priority order:

1. ~~**`MEMORY_CANDIDATES.json` has the same exemption gap `PRODUCER_RESERVATIONS.json` had.**~~
   **Accepted and fixed by PR #53** (`af2f737`, "fix(governed): exempt the accepted-run memory
   candidate ledger"), with a regression test driving the real chain against a repository that has
   no `.gitignore` at all, plus a negative control proving Git actually sees the candidate ledger.
   Nothing left to do here. Kept for the record: the gap was that `MEMORY_CANDIDATES.json` was in
   `REPO_RUNTIME_GITIGNORE_ENTRIES` but not in
   `governed_producers::is_untracked_impulse_runtime_artifact`, so recording an operator approval
   dirtied the canonical worktree and the very next step for a staged run — promotion — failed on a
   tree the daemon had dirtied itself.
2. **Add a fourth `ProducerKind::Discard` and wrap the discard endpoint** (`src/state/producer_reservation.rs`,
   which this lane does not own). `DiscardGovernedStagedWorktree` removes a checkout and then
   records `DiscardStagedWorktree`, which flips `staged.status` and appends an event — so a crash
   between the two leaves an `active` record pointing at a missing path. It is bounded (the side
   effect is idempotent on retry and the stale record is inert, since nothing re-launches an
   already-discardable task) but it is the same window the other three producers now close. The
   wrap itself is four lines in `governed_wiring::discard_governed_staged_worktree`, identical in
   shape to the promote path; only the enum variant is out of reach.

3. **Unify the discardability rule.** `state/governed_task.rs`'s private
   `staged_worktree_is_discardable(task: &GovernedTaskRun) -> bool` is now duplicated as
   `impulse_ops::governed_wiring::staged_worktree_is_discardable`, because the daemon must refuse a
   discard *before* deleting the checkout, not after. The fix is one line: delete the private copy
   and call the `impulse_ops` one. The two are byte-for-byte the same logic today, and both have
   tests; leaving them split risks drift where the daemon allows what the ledger refuses.
4. **Staged verification end to end is still unproven on this branch.** Per the 2026-09-12
   coordination note, `run_verification` observes `task.workspace_root`, so a staged task cannot
   pass verification on `8dfd2ab`. This lane's reservation tests therefore run against
   **authoritative** profiled tasks, and its staged tests compose the accepted state through
   `state.mutate_governed_task` directly rather than through the producer chain. PR #53 fixes it;
   the end-to-end test is written and waiting — see "Post-#53 merge checklist" below.

## Review round 1 (2026-09-12)

Adversarial review returned "needs changes" with claims b, c, d, e, f and both clippy fixes
CONFIRMED, and a live SIGKILL-and-restart proving the reservation path. Every finding below is
fixed on this branch; each regression was reverted once to watch its test fail.

| Finding | Fix | Proof |
|---|---|---|
| **P1-1** `.impulse/MEMORY_CANDIDATES.json` is written by every approval, so the very next promotion sees a dirty canonical tree when `.impulse` is not gitignored. Every `governed_wiring` fixture wrote the full ignore list, so the suite could not reach it. | `git cherry-pick af2f737` from the P1 lane, so both PRs carry byte-identical hunks in the same `matches!` arm. | New `repo_state_without_runtime_ignores()` fixture plus `an_approval_does_not_dirty_the_canonical_tree_for_promotion`, with a negative control asserting Git actually reports the candidate ledger as untracked. Reverting the two exemption lines fails it. |
| **P1-2** Staged registration was not replay-idempotent: materialization ran before the ledger write, so a retry (a client that timed out on a slow `git worktree add`) hit "path already exists" and never reached the receipt — and the recovery text it produced was actively wrong advice, because the "leftover" directory was a live Builder's checkout. | The request id is checked against the governed ledger's receipts immediately after the class check and before the producer runs; a hit delegates to `register_governed_task`, which fingerprint-checks the replay and returns the recorded task. | `a_replayed_staged_registration_returns_the_recorded_task_and_touches_nothing` (in-process, asserts a Builder's scratch file survives) and `a_replayed_staged_registration_over_the_socket_returns_the_recorded_task` (real daemon, the path the retry actually comes from). Removing the probe fails both. |
| **P1-2b** `respond_err` rendered `anyhow` with `Display`, collapsing the `.context()` chain, so the producer's recovery instructions never reached the operator. The existing test asserted `{error:#}`, not the wire form. | `handle_governed_task_request` now renders `format!("{error:#}")`. | `a_materialization_failure_reaches_the_operator_with_its_recovery_text` asserts **over the socket** that the message carries both "already exists" and "worktree prune". |
| **P2-1** `handlers.rs` ran registry evaluation and `observe_clean_git_subject` — which spawns `git` inside a caller-supplied path — before the operator check, so a non-operator connection could probe arbitrary paths and start a subprocess pre-capability. | `require_staged_registration_class` is now the first statement in the `RegisterGovernedTask` arm. `register_governed_task` keeps its own check, so the function stays safe standalone; this is ordering, not relocation. | `a_non_operator_staged_registration_is_refused_before_git_runs` points the registration at a non-Git temp directory and asserts the class refusal, and asserts the message contains no "git root discovery"/"canonicalize" text. |
| **P2-5** Two operator-facing messages were wrapped string literals with no `\` continuation, which rustfmt joined into single strings carrying 22- and 14-space runs — in exactly the wording ADR-0019 requires the surface to get right. | Extracted to `blocked_promotion_line` and `unreferenced_commit_warning` and joined. | `governed_message_tests`: no run of spaces after the two-space indent, no embedded newline, plus content assertions (the blocked line must say "stays accepted"; the warning must show the OID and `git cat-file -p`). |
| **P2-3** The discard replay branch answered `unreferenced_accepted_commit: None`, and `DaemonClient` retries acknowledged requests, so a lost first response hid the orphaned-commit warning entirely. | The replay branch recomputes it from the recorded promotion. | `a_replayed_discard_still_names_the_unreferenced_commit`. |
| **P2-2** Discard has no reservation and this card's rationale was inaccurate. | Documentation option taken this round, as directed: the Decisions entry above is corrected and ADR-0012's appendix now states the crash window, what bounds it (idempotent-on-retry side effect, inert stale record), and that the fix is a fourth `ProducerKind::Discard`. | Handoff item below. |
| **P2-4** The two `staged_worktree_is_discardable` copies could drift silently. | `state/governed_task.rs`'s copy is now `pub(crate)` (one keyword, commented) and a cross-check test runs both over the same matrix. | `both_discardability_rules_agree_over_the_whole_state_matrix`: 9 review states x 4 execution states x 2 pin states x 3 promotion outcomes = 216 comparisons, with an assertion that the matrix is exhaustive rather than a sample. |
| **Nit** `roll_back_staged_checkout` had five call sites and no coverage. | — | `rolling_back_a_checkout_removes_its_administrative_entry_too` asserts the directory *and* the Git administrative entry are gone, then proves the path is reusable by materializing again. |
| **Nit** the dead `b".impulse/worktrees"` arm (no trailing slash). | Left in place. It is in `src/governed_producers.rs`, which the P1 lane owns, and the pair already carries an explanatory comment; touching it would add a hunk to their file for no behavior change. Named here so it is not re-discovered. | — |

### Blocked-path hunks after review round 1

The count is now **five**, and the post-#53 merge adds a sixth (the
`record_promotion_preconditions_hold` extraction, decided by the owner — see checklist step 6). Three are unchanged from the first round — the two clippy one-liners
(`impulse-term/src/renderer.rs`, `impulse-desktop/tests/desktop_contract.rs`) and the
`PRODUCER_RESERVATIONS.json` exemption in `src/governed_producers.rs`. Two are new:

4. **`impulse-rs/src/governed_producers.rs`, second hunk** — `git cherry-pick af2f737` from
   `claude/adr0019-p1-fixes-20260912`, taken verbatim rather than hand-written so both PRs carry
   byte-identical changes and the eventual merge is a no-op. That commit's own regression test and
   `staged_registration_at` fixture live in `src/state/governed_task.rs` and depend on its three
   earlier commits, so they are **not** carried; equivalent coverage lands in this lane's own
   fixture instead.
5. **`impulse-rs/src/state/governed_task.rs`** — one keyword, `fn` -> `pub(crate) fn` on
   `staged_worktree_is_discardable`, with a comment explaining why. Required by P2-4: the drift
   test cannot compare against a private function. No behavior change.

### Known narrow race, recorded rather than closed

The replay probe added for P1-2 is check-then-act: it consults the ledger's receipts, then
materializes, then registers. Two *concurrent* registrations carrying the same request id can both
miss the probe, and one of them then loses — either on the staged path ("already exists") or on the
ledger's own `AlreadyExists`. It is narrow (one daemon, one project, the same request id sent twice
at once rather than sequentially), it fails closed rather than corrupting anything, and the loser's
rollback removes only the checkout it created itself. Closing it properly means reserving the
registration the same way the three producers reserve, which is the same `ProducerKind` addition
P2-2 needs.

## Post-#53 merge-in (2026-09-12)

`origin/main` at `1f866d6` (#53 squashed as `ceb29a8`, plus #51 and #55) merged in with a merge
commit. Every checklist item below was executed; the checklist itself is kept underneath as the
record of what was planned.

### Merge resolution

One conflict, in `CONTEXT.md`'s **world scope** entry: #53 documented the three invariants its
fixes hardened, this lane documented v9 reachability. Both kept, joined into one paragraph. The two
cherry-picked commits (`8dca0a1`, `e533b09`) arrived from both sides and merged as no-ops, and
`is_untracked_impulse_runtime_artifact` auto-merged with **both** exemption entries present
(`PRODUCER_RESERVATIONS.json` from this lane, `MEMORY_CANDIDATES.json` from the cherry-pick) —
verified by inspection, not assumed.

### What #53 changed under this lane

- `launch_working_directory()` returns `Result`, so both call sites (`src/daemon/governed_wiring.rs`,
  `tests/daemon_governed_wiring.rs`) take `.expect("a materialized staged task has a launch working
  directory")`. `LaunchWorkingDirectoryError` losing `Copy` touched nothing: both sites use the
  `Ok` value.
- `SharedRepositoryConfigDigest` gained `scheme_version`, so two test fixtures switched to the
  `::current(..)` constructor rather than a struct literal — which is the right call anyway, since a
  literal would pin the legacy scheme silently.
- `MarkRunning` now refuses a staged task before materialization. Registration-time materialization
  already satisfies it; `staged_registration_materializes_the_worktree_before_any_launch` covers it.

### Delivered this round

| Item | What landed |
|---|---|
| **Staged end-to-end** | `one_staged_run_completes_through_every_endpoint`: register (materializes) -> launch -> Builder commit -> claim -> **real Cargo verification** -> Supervisor review -> operator approval -> promote -> discard, against a real repository. Asserts the canonical branch does not move until promotion, that promotion syncs the working tree and not just the ref, that a promoted run orphans no commit, and that every producer released its reservation. This is the chain that could not complete before #53. |
| **Typed `StagedConfigRefusal`** | New `StagedConfigRefusalReason` + `GovernedStagedConfigRefusalAck` in `impulse-ops`, recognized by `governed_wiring::staged_config_refusal` and answered as a successful-shape response by `SubmitGovernedClaim` and `RunGovernedVerification`. Covers all three producer variants, not just the two named in the handoff — `UnsupportedSubmodules` is a refusal too, and dropping it would have left one class rendering as a generic error. `DaemonClient` returns `GovernedProducerOutcome::{Recorded, StagedConfigRefused}`; the CLI and Ion's `governed_submit_claim` print the reason and the remedy. |
| **Refusal releases its reservation** | `a_refused_verification_releases_its_reservation` plants the driver *between* the claim and the verification, asserts the typed refusal, that no Git ran, that nothing was recorded, and that `open_reservations()` is empty — the property the remedy's retry depends on. |
| **`unreferenced_accepted_commit_on_discard` widened** | Falls back to the accepted claim's `subject_revision` when no promotion is recorded. The `impulse-ops` test was rewritten and renamed (`test_an_accepted_run_that_was_never_promoted_names_its_unreferenced_commit`), and the accepted/unpinned/zero-promotions case added at both fill sites. |
| **Promotion preconditions extracted** | `record_promotion_preconditions_hold` + `PromotionPreconditionFailure` in `src/state/governed_task.rs`, called from the `RecordPromotion` arm so predicate and mutation cannot drift; each variant's `Display` is the message the arm produced inline, so nothing an operator reads changed. |
| **Promotability sandwich test** | `promotability_sits_between_the_endpoint_and_the_ledger_over_the_whole_matrix`: 432 states x claim/no-claim = 864 comparisons, asserting the shared predicate is a superset of the endpoint's admission and a subset of the ledger's preconditions, plus an assertion that it admits something (a sandwich that admits nothing proves nothing). |

### What the sandwich test caught

The endpoint's inline promote preflight was **weaker than the ledger's**: it checked only "accepted"
and "has an active staged worktree", so an accepted staged task with no claim, or one already
promoted, was admitted — a durable reservation taken and the producer called — and then refused by
the ledger. Benign in outcome, wrong in shape. Fixed by making the endpoint call the ledger's own
predicate (`promote_preflight`), so all three now agree by construction rather than by coincidence;
the test calls that function rather than restating it, so changing the endpoint changes the test.
This is exactly the class of drift the P2-4 cross-check was raised about, found the first time the
same technique was pointed at promotion.

### Gate and merges taken during the checklist

`origin/main` was merged three times, because main moved under this lane twice while the checklist
was being executed: `1f866d6` (#53 + #51 + #55), `16ed8c1` (#56 scoped memory promotion), `22f9630`
(#59 property-test harnesses). Only the first conflicted, in `CONTEXT.md`.

Each merge was followed by a check that every `.impulse` cleanliness exemption survived — this
lane's `PRODUCER_RESERVATIONS.json`, the cherry-picked `MEMORY_CANDIDATES.json`, and #56's
`is_impulse_memory_evidence_artifact` — since they now sit in adjacent lines of the same `matches!`
arm and three lanes have edited it. Verified by inspection after each merge rather than inferred
from a clean auto-merge.

Final gate against `22f9630`: `cargo build --workspace` clean; `cargo test --workspace` **2905
passed / 0 failed / 9 ignored** (2896 before review round 2, 2898 after its guard tests, 2905 after
the final verification round's seven); `cargo clippy --workspace --all-targets -- -D warnings` clean;
`cargo fmt --all -- --check` clean; `python3 ../docs/validate_docs.py --all` still the same four
pre-existing failures.

Clippy caught one thing worth recording: `GovernedProducerOutcome::StagedConfigRefused` carries a
whole flattened `GovernedTaskRun` (~824 bytes) while `Recorded` is often far smaller, so the
refusal variant is boxed — the rare branch should not make every caller pay for it on the common
path.

### Deliberate deviations from the plan

- **`governed_outcome_is_promotable` is implemented here, not taken from #58.** #58 is stacked on
  this branch and is not on `main`, so the function did not exist to import. It is written in
  `impulse-ops/src/governed_wiring.rs` — this lane's file — with the sandwich relationship stated in
  its doc comment. **#58 should drop its copy and rebase onto this one**; if the two differ, the
  sandwich test is what will say so.
- **The accepted/unpinned/zero-promotions case is proven at the function, not at the endpoint.**
  That state is real but **not producible by this build**: `materialize_staged_worktree` always
  records a pin, so an unpinned worktree can only arrive on a ledger written before the pin existed.
  Synthesizing one by editing `GOVERNED_TASKS.json` fails closed on ADR-0019 rule 11's replay
  validation — correct behavior, and not worth defeating for a test. The behavior is covered in
  `impulse-ops`; `the_discard_ack_is_filled_from_the_shared_orphaned_commit_rule` covers the wiring
  that carries it to the operator.
- **The prepared e2e's negative branch became its own test.** `a_drifted_config_pin_refuses_the_claim_with_a_typed_reason`
  is separate from the happy path rather than a branch inside it: one test asserting a run completes
  and another asserting a run is refused read better than one test doing both.

## Review round 2 (2026-09-12)

One finding: `StagedConfigRefusalReason::remedy()` for `UnsupportedSubmodules` carried an 18-space
run mid-sentence. **The same defect review round 1 fixed, reintroduced by the same mechanism** — a
wrapped literal whose `\` continuation went missing — and it landed in the very message that exists
to tell an operator what to do.

The cause is worth recording, because it is not carelessness in the Rust: the literal was authored
through a shell heredoc, and the `\<newline>` was consumed by the heredoc before `rustc` ever saw
it. Round 1's guard could not catch it either, because that guard lives in
`handlers::daemon_dispatch` and only knows about the two CLI strings it was written for.

Fixed, and the guard moved to where the strings are defined rather than where they are rendered:
`test_operator_facing_strings_have_no_run_of_interior_spaces` in `impulse-ops` now covers **every**
`StagedConfigRefusalReason` variant's `remedy()`, `Display`, and `as_str()`, and **every**
`PromotionBlockedReason`'s `Display` and `as_str()` — 22 strings, asserted to have no interior
space run, no leading or trailing whitespace, and no newline or tab. Each enumeration ends in an
exhaustive `match`, so adding a variant fails to compile until it is listed. A second test pins the
regressed string by exact content, not only by shape.

Proven non-vacuous: reintroducing a three-space run fails the guard with the offending string in
the message.

**A lint would be better than a test.** `clippy` has no rule for this, and the defect is invisible
in review because the source looks correctly wrapped — the run only exists after the continuation
is lost. Enumerating the strings at their definition is the best available substitute; the standing
risk is a *new* operator-facing string defined somewhere neither guard enumerates.

## Final verification round (2026-09-12)

Five items from the pre-merge verification at `d8df6aa`. Four fixed, one recorded.

- **F2 — the refusal ack had no serde or wire-compat test**, unlike both sibling acks. Added:
  `test_refusal_ack_round_trips_through_serde`,
  `test_every_refusal_reason_round_trips_through_serde`, and
  `test_refusal_ack_is_wire_compatible_with_a_bare_governed_task`.

  Writing them surfaced a defect worth naming: the serde `kind` and
  `StagedConfigRefusalReason::as_str()` **disagreed** — `unpinned` on the wire, but
  `repository_config_unpinned` in logs and the CLI. Two names for one reason is a lookup table
  nobody maintains, so the variants now carry explicit `#[serde(rename = ...)]` matching `as_str()`,
  the round-trip test asserts `kind == as_str()` for every variant so they cannot drift again, and
  `docs/IPC-PROTOCOL.md` was corrected (it documented the short names).

- **F3 — no endpoint-level observation of the new promote preconditions.** Added three tests.
  `an_already_promoted_run_is_refused_before_the_producer_runs` and
  `a_promotion_without_an_active_worktree_is_refused_before_the_producer_runs` drive the endpoint
  and assert the ledger's own message, an empty `open_reservations()`, and no new promotion record.
  "The producer never ran" is *proven* rather than asserted: the staged checkout is deleted first,
  so a producer that did run would fail loudly on a missing worktree instead of returning the
  precondition message. The `NoClaim` case is checked at `promote_preflight` rather than through the
  ledger, because `Accepted` is only reachable via an operator decision on a verdict on a
  verification on a claim — a claimless accepted task cannot be built by any sequence of real
  mutations. That check is defense in depth against a future transition relaxing the chain, which is
  exactly the kind of thing the old inline preflight missed.

- **F4 — the "cannot be synthesized" comment was wrong.** It was, twice over:
  `state/governed_task.rs` synthesizes an unpinned ledger by rewriting the record *and* its receipt
  fingerprint, and more simply `require_shared_config_digest` accepts `Unknown`, so submitting the
  materialization mutation with an unpinned input produces a genuine pre-pin record with a naturally
  computed receipt and no ledger surgery at all.

  That route produced a real new endpoint test —
  `an_unpinned_staged_worktree_cannot_be_launched_but_can_be_reclaimed` — and, in writing it, a
  fact that changes the answer: **#53 made `MarkRunning` refuse an unpinned staged task**, so an
  unpinned worktree can no longer be driven forward into an accepted state by this build *at all*.
  The accepted-and-unpinned records that exist are ones a pre-pin build accepted and this build
  later loaded; reproducing that means rewriting the receipt fingerprint for every mutation in the
  history through the private `fingerprint_mutation`. So the comment is now correct and specific,
  the reachable half is covered end to end at the endpoint, and the unreachable half stays at the
  function plus its wiring. Making `fingerprint_mutation` `pub(crate)` would allow the full test and
  is on the handoff list rather than taken as a seventh blocked-path hunk.

- **F6 — discard compares no pin**, unlike the other three producers. Stated in ADR-0019's
  Consequences and in `CONTEXT.md`'s world-scope entry, with the reason: removing a checkout
  materializes no files, so no filter or textconv driver can fire, and an unpinned or drifted
  worktree is precisely the one an operator most needs to reclaim. Both notes say explicitly not to
  "fix" it by adding a pin check.

- **F5 — recorded, not changed.** Extracting `record_promotion_preconditions_hold` moved
  `require_actor` from *first* in the `RecordPromotion` arm to *after* the task-state checks. Every
  message is byte-identical, and a request violating exactly one precondition reports exactly what
  it did before. A request violating **two** — a non-System actor on a task that is also, say, not
  accepted — now reports the task-state failure where it previously reported the actor failure. No
  test depended on the old order, and the new order is the better one: the task-state checks are
  about whether the transition is possible at all, the actor check about whether this caller may
  make it. Worth knowing if a downstream test matches on the first message of a doubly-invalid
  request.

## Post-#53 merge checklist

PR #53 (`claude/adr0019-p1-fixes-20260912`) merges **before** this one. It contains verbatim
cherry-picks of this lane's `8dca0a1` (clippy unblock, as `ba25bac`) and `e533b09`
(`PRODUCER_RESERVATIONS.json` exemption, as `6c5fd6b`), so those two commits arrive on `main` from
both sides. When it lands:

1. `git merge origin/main` on this branch. **No rebase, no force** — a merge commit is expected.
2. **Expected resolutions.**
   - The two cherry-picked commits carry identical content on both sides and should merge with no
     conflict. If Git does flag the `impulse-term`/`impulse-desktop` one-liners, take either side;
     they are byte-identical.
   - `impulse-rs/src/governed_producers.rs`, the `is_untracked_impulse_runtime_artifact` region:
     **keep both entries** — this lane's `.impulse/PRODUCER_RESERVATIONS.json` and #53's
     `.impulse/MEMORY_CANDIDATES.json`, with both `starts_with` prefixes.
3. **Adapt to the one breaking signature.** `GovernedTaskRun::launch_working_directory()` becomes
   `Result<&str, LaunchWorkingDirectoryError>`, with no canonical fallback for a staged task. This
   lane has exactly **two** call sites, both test assertions:
   - `impulse-rs/src/daemon/governed_wiring.rs:1009`
   - `impulse-rs/tests/daemon_governed_wiring.rs:278`

     Both become
     `.expect("a materialized staged task has a launch directory")`. The other call sites
     (`src/governed_producers.rs`, `src/state/governed_task.rs`) belong to #53 and arrive adapted.
     `MarkRunning` on a staged task is refused until materialization is recorded, which this lane's
     registration-time materialization already satisfies — no change needed, and
     `staged_registration_materializes_the_worktree_before_any_launch` already asserts the record
     is present at `Registered`.
4. **Add the prepared staged end-to-end test**, including its negative branch. Written, reviewed, and deliberately *not* run
   before the merge because step 5 cannot pass on `8dfd2ab`. It lives at
   `<scratchpad>/prepared/staged_end_to_end.rs` and drops into the `mod tests` block of
   `src/daemon/governed_wiring.rs`. It drives one staged run through every endpoint this lane
   added — register (materializes) -> `MarkRunning` -> Builder commit in the staged checkout ->
   `SubmitGovernedClaim` -> `RunGovernedVerification` (real Cargo, under a reservation) ->
   `RunGovernedSupervisorReview` (a bound fake provider, the one non-real step, copied from the
   existing `BoundSupervisorProvider` pattern) -> operator approval -> `PromoteGovernedOutcome` ->
   `DiscardGovernedStagedWorktree` — and asserts the canonical branch does not move until
   promotion, that promotion syncs the working tree, that a promoted run orphans no commit, and
   that every producer released its reservation. It needs two small `*_from_response` helpers and
   five extra imports, both listed in the file's header comment.

   It also carries a **negative branch** added for #53's typed refusal: after materialization, plant
   a `filter.*.smudge` driver in the staged worktree's shared repository configuration, then assert
   the claim is refused with `StagedConfigRefusal::Changed { component }` naming the file that
   changed, that no claim record was written, and that the Builder's marker file is untouched --
   the refusal must not have run Git against that tree at all.
5. **Adopt #53's typed staged-config refusal.** Its round-1 fix (`9d9a8c6`) adds a `pub`,
   downcastable `StagedConfigRefusal::{Changed { component }, Unpinned}`, returned by `derive_claim`
   and `run_verification` **before any Git runs** when a staged task's pinned shared configuration
   drifted. It is a refusal to touch the tree, not a run failure, and the operator remedy is
   discard-and-re-materialize. Four things follow for this lane:

   - **Downcast it into a typed response** in `SubmitGovernedClaim` (which this lane left in
     `handlers::handle_governed_producer_request`) and in
     `governed_wiring::run_governed_verification`, plus the `governed-claim` CLI handler. A new
     response variant — `StagedConfigRefused { component }` — carrying the remedy text, not a
     generic error string. This is the same argument as the blocked promotion: a refusal that names
     what happened and what to do is not an error the operator has to decode. Model it on
     `blocked_promotion_line`, which already exists for exactly this purpose.
   - **The verification reservation must close on the refusal, and it already does.** Verified by
     reading `state/producer_reservation.rs` rather than assuming: `with_reservation`'s `Err` arm
     calls `release(&reservation_id, format!("failed: {error}"))`, so the refusal is recorded as a
     closed reservation, not left open. That matters here specifically because the remedy is
     discard-and-re-materialize followed by a **retry**, and an open reservation at the same
     revision would meet `DuplicateOpenReservation` and block it. Worth noting the refusal happens
     *inside* the closure (it is raised by `run_verification`, which the closure calls), so a
     reservation is taken and immediately released — harmless, but it means the journal will carry
     a `failed: staged repository configuration ...` entry per refusal, which is the desired audit
     trail rather than noise. **Add a test asserting `state.open_reservations()` is empty after a
     refused verification and that an immediate retry is not blocked.**
   - **`LaunchWorkingDirectoryError` gained a `task_id` field and lost `Copy`.** Both of this lane's
     call sites are `.expect(...)` on the `Ok` path, so neither moves or copies the error — no
     change beyond the `.expect(...)` already listed in step 3. Re-check after the merge rather
     than assuming, since a `Copy` removal can surface in unrelated places.
   - **Document it** in `docs/IPC-PROTOCOL.md` beside the blocked-promotion semantics, under the
     same principle: a typed refusal carried on a successful-shape response is part of the contract,
     not an error-string convention. Bump nothing — it is additive within v9 if it lands before
     this PR merges; if v9 has already shipped, it takes v10.

6. **Reconcile with PR #58 (desktop controls, stacked on this branch).** It edits
   `impulse-ops/src/governed_wiring.rs`, which is this lane's file, so the merge-in is where the two
   meet.

   - **A real hole in this lane's `unreferenced_accepted_commit_on_discard`, found by #58.** As
     delivered here it returns `None` whenever no promotion is recorded — it reads
     `task.latest_promotion()?` and gives up. That is wrong for exactly one reachable state, and it
     is a state this lane's own discardability rule creates: an **accepted** task whose staged
     worktree carries an `Unknown` configuration pin is discardable *without* any promotion
     attempt (`staged_worktree_is_discardable` short-circuits on an unpinned worktree, because such
     a worktree can never be promoted and discarding is the only way forward). Discarding it drops
     the only ref to the accepted commit, and the operator is told nothing. #58 widened the
     function to fall back to the accepted claim's `subject_revision`. **Take #58's version.** Then:
     - `impulse-ops`'s `test_only_an_accepted_but_blocked_promotion_names_an_unreferenced_commit`
       asserts the old semantics (`None` when no promotion has been attempted) and **will fail**
       after the merge. That failure is correct and expected; rewrite the assertion rather than
       reverting the widening, and rename the test, since "only an accepted but blocked promotion"
       stops being true.
     - Add the **accepted / unpinned / zero-promotions** case to both sides of the chain that fills
       from this function: `GovernedStagedWorktreeDiscardAck.unreferenced_accepted_commit` (the
       endpoint test in `src/daemon/governed_wiring.rs`) and
       `unreferenced_commit_warning` (`governed_message_tests`). Re-run both suites: neither
       currently exercises an unpinned worktree.

   - **Add the importing counterpart to #58's promotability matrix.** #58 added
     `impulse_ops::governed_wiring::governed_outcome_is_promotable` plus a 432-case matrix test that
     *restates* this lane's inline promote checks and the state layer's `RecordPromotion`
     preconditions, because from `impulse-ops` it cannot import either. A restatement is exactly the
     drift risk P2-4 was raised about, so the counterpart belongs here, beside
     `both_discardability_rules_agree_over_the_whole_state_matrix`, where both are importable: over
     the same matrix, assert `governed_outcome_is_promotable` is a **superset** of this lane's
     inline checks (`is_accepted` + `active_staged_worktree().is_some()` + staged scope — every task
     the endpoint would let through, the shared predicate must also allow) and a **subset** of the
     state layer's `RecordPromotion` preconditions (nothing the predicate promises is promotable may
     be refused by the ledger).

     **The subset half: decided.** Extract the task-state preconditions of the `RecordPromotion`
     arm in `src/state/governed_task.rs` into

     ```rust
     pub(crate) fn record_promotion_preconditions_hold(
         task: &GovernedTaskRun,
     ) -> Result<(), PromotionPreconditionFailure>
     ```

     where `PromotionPreconditionFailure` is a small enum with **one variant per check** and each
     variant's `Display` is the existing operator-facing message verbatim. The `RecordPromotion` arm
     calls it first and maps a failure straight back into `invalid_transition(&failure.to_string())`,
     so **no operator-visible message changes**; the matrix test uses `.is_ok()`. This is what makes
     the predicate the single source rather than a restatement of the arm — a predicate that merely
     re-listed the same conditions would be the exact failure mode this test exists to catch. A
     sixth blocked-path hunk of that shape is accepted; the state file's other lanes (#53, #56) are
     frozen or nearly so and the conflict is trivial. Driving 432 real-ledger mutations instead was
     rejected as certain to be deleted the first time the suite feels slow.

     **Only the task-state half moves.** The arm mixes preconditions on the *task* with validation
     of the promotion *input*, and a predicate taking `&GovernedTaskRun` can only carry the former.

     | Check | Existing message | Goes |
     |---|---|---|
     | promotion-record capacity | `governed promotions reached its limit of 256` (via `require_capacity`) | predicate |
     | `world_scope == StagedAuthoritative` | `only a staged_authoritative world scope promotes an outcome` | predicate |
     | `review_state == Accepted` | `promotion requires an accepted governed task` | predicate |
     | `active_staged_worktree().is_some()` | `promotion requires an active staged worktree` | predicate |
     | `latest_claim().is_some()` | `promotion requires an accepted worker claim` | predicate |
     | not already promoted | `this governed outcome was already promoted` | predicate |
     | `require_actor(&promotion.actor, System)` | — | arm (reads `promotion`) |
     | `initial_subject_revision` matches the staged worktree | `promotion must reference the staged worktree's initial Git OID` | arm (reads `promotion`) |
     | `accepted_revision` matches the accepted claim | `promotion must reference the accepted claim's subject revision` | arm (reads `promotion`) |
     | `validate_promotion_outcome` | — | arm (reads `promotion`) |

     All six predicate checks already terminate in `InvalidTransition` today — `require_capacity`
     routes through `invalid_transition` itself — so the mapping back is uniform and lossless. The
     arm keeps its own ordering: the predicate runs first, then the input-bound checks, because the
     input checks borrow `staged` and `claim` that the predicate has just proven exist.

   - **Record the desktop follow-up.** #58 text-matches `StagedConfigRefusal`'s strings at a single
     replacement point, `staged_config_refusal_notice` in `impulse-desktop/src/ui.rs`. When step 5's
     typed response variant lands, #58 should switch that notice to the typed variant instead of
     string matching. `impulse-desktop/**` is blocked for this lane, so this is a handoff, not a
     change to make here — it is listed under "For the desktop track" below.

7. Re-run the full gate, push, and report the new totals. Do not merge.

### Residual gaps this lane knowingly leaves

- **Registration can still strand a task record in one narrow case.** If materialization succeeds
  but `register_governed_task` or the `MaterializeStagedWorktree` mutation then fails, the checkout
  is rolled back through the real discard producer (so the administrative entry goes with it) — but
  if it was the *mutation* that failed, a registered task exists with no staged worktree and nothing
  re-materializes it. Recovery today is an operator-composed `MutateGovernedTask`. A dedicated
  `MaterializeGovernedStagedWorktree` endpoint would close it; it was left out because the brief
  scopes materialization to registration.
- **`unreferenced_accepted_commit_on_discard` misses one reachable state as delivered here.** It
  returns `None` whenever no promotion is recorded, but an accepted task with an `Unknown`
  configuration pin is discardable with no promotion attempt — this lane's own discardability rule
  says so — and discarding it drops the only ref to the accepted commit with no warning. Found by
  PR #58, which widened the function to fall back to the accepted claim's `subject_revision`. Fixed
  on merge-in, not here, because #58 is stacked on this branch and already owns the change; see the
  post-#53 checklist for the tests that must move with it.
- **`with_reservation` is not panic-safe and this wiring adds no `catch_unwind`.** An in-process
  panic inside a producer closure is treated exactly like a process crash: the reservation stays
  open until a restart's reconcile, or until the revision-scoped duplicate check closes it when the
  task's revision next advances. Two attempts at the *exact same* revision after a panic still
  conflict — the documented residual from the journal's own spec.
- **The CLI has no `governed-materialize`.** Registration is the only path that materializes.
