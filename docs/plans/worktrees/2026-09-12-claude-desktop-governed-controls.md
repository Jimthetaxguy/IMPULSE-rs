---
title: Desktop Governed Controls Lane
description: Work card for the lane that gives the Dioxus cockpit ADR-0019's Promote and Discard controls, their typed banners, and a hardened governed Git preflight
updated: 2026-09-12
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, governed, desktop, dioxus, adr-0019]
---

# Desktop Governed Controls Lane

## Lane Facts

- Owner: Claude (Fable 5.1)
- Role: implementation lane
- Branch: `claude/desktop-governed-controls-20260912`, based on
  `claude/daemon-governed-wiring-20260912` at `bb63a7e` — **not** on `main`. The protocol-v9
  request/ack types this lane calls (`PromoteGovernedOutcome`,
  `DiscardGovernedStagedWorktree`, `GovernedProducerAck`,
  `GovernedStagedWorktreeDiscardAck`) live on PR #52's branch and are not on `main` yet.
- Worktree: `.worktrees/desktop-governed-controls-20260912`
- Owned paths: `impulse-rs/impulse-desktop/**`, this card, the CONTEXT.md glossary entry, one
  dated ADR-0019 line, and one **additive** function in
  `impulse-rs/impulse-ops/src/governed_wiring.rs` (see "The one shared-crate addition").
- Blocked/shared paths: `impulse-rs/src/daemon/**`, `impulse-rs/src/state/**`,
  `impulse-rs/src/governed_producers.rs`, `Cargo.toml`/`Cargo.lock`, `.github/**`,
  `impulse-rs/scripts/**`, `CLAUDE.md`, `AGENTS.md`. None was touched.
- Plan/spec: ADR-0019 ("Not delivered by this ADR's lane", Consequences), ADR-0018 (operator-class
  provenance), PR #52's lane card ("Handoff Notes → For the desktop track"), PR #53's lane card
  ("Handoff Notes → Desktop track").
- Verification: isolated `CARGO_TARGET_DIR`; `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `python3 ../docs/validate_docs.py --all`.

## What Landed

### 1. Promote and Discard, through the acknowledged operator-class path

The cockpit now drives both protocol-v9 staged-worktree endpoints:

- `GovernedTaskGateway` (`impulse-desktop/src/runtime.rs`) gained `promote_outcome` and
  `discard_staged_worktree`. Both are **defaulted to an `Err`** so every existing gateway double in
  the workspace keeps compiling and fails closed rather than silently doing nothing.
- `UnixDaemonOpsClient` implements them over `send_acknowledged_with_read_timeout`, so the exact
  serialized request — idempotency key and expected revision included — is reused across transport
  retries: a daemon commit followed by a lost response is replayed off the stored receipt, never
  fast-forwarded twice.
- `requires_operator_class` now names `PromoteGovernedOutcome` and
  `DiscardGovernedStagedWorktree`, so the ADR-0018 capability is presented on the same connection
  the gated request uses. `RegisterGovernedTask` is deliberately **not** added even though
  `DaemonClient::requires_operator_class` lists it — the daemon gates registration only *with a
  staged world scope*, and desktop staged-scope registration is the other, still-open ADR-0019
  desktop row. Adding it here would be an unrequested behavior change on a path this lane does not
  otherwise touch. Recorded so the staged-launch lane picks it up.
- `DesktopRuntime::{promote_governed_outcome, discard_governed_staged_worktree}` run through a new
  shared `adopt_acknowledged_governed_task`, extracted from `mutate_governed_task`. A promotion or a
  discard therefore cannot bypass the identity checks the mutation path always applied: same
  project, same task, strictly newer revision, immutable identity unchanged. The daemon guarantees
  the strictly-newer part on both the fresh and the replay path
  (`handlers::require_producer_request_state`), so the check is the same invariant, not a guess.
- `GOVERNED_REGISTRATION_READ_TIMEOUT` was renamed `GOVERNED_PRODUCER_READ_TIMEOUT` and both new
  requests use it. Under the ordinary two-second IPC bound every real promotion — which runs Git in
  the canonical checkout — would read as a transport failure and be retried three times.

### 2. A blocked promotion is rendered as an execution fact

`blocked_promotion_notice` is a pure function over the task's latest promotion. Each
`PromotionBlockedReason` gets its own headline and remedy; `RepositoryConfigChanged` names the
component's actual file so the operator is not guessing which one to inspect. The banner carries
`data-promotion-blocked-reason="<PromotionBlockedReason::as_str>"` (the stable machine value, not
`Display`, which for a config change appends the component), shows the canonical head, and says in
so many words that the run stays accepted and the staged worktree stays active. It is deliberately
**not** `role="alert"` and not an error class, and Promote stays live for the retry the remedy
describes — proven per reason by
`test_a_blocked_promotion_renders_as_an_execution_fact_with_its_remedy`.

### 3. The discard confirmation states its cost and shows the OID

ADR-0019's Consequences require the surface offering a discard to say when it drops the only ref to
an accepted commit **and to show the OID**. The confirmation is armed by the first click and
computes the notice from the task through
`impulse_ops::governed_wiring::unreferenced_accepted_commit_on_discard` — the same predicate the
daemon uses to fill the acknowledgement's `unreferenced_accepted_commit` — so the cost is stated
*before* anything is sent, and the acknowledgement then confirms it. The wording is modelled on the
CLI's `unreferenced_commit_warning` and ends in the recovery command.

The confirmation also collects the discard `reason`, which is the one caller-authored field on the
request. `UnixDaemonOpsClient::discard_staged_worktree` validates it before opening a socket, so a
destructive request never travels to be told it was blank.

### 4. `StagedConfigRefusal`, matched on text, with a named replacement point

`staged_config_refusal_notice` classifies a daemon error message as a staged-configuration refusal
(`Changed`, `Unpinned`, `UnsupportedSubmodules`) and renders it as a refusal with a remedy rather
than as a failed host call — `BridgeStatusUpdate::headline` gives it its own headline and the banner
renders the remedy.

**This is text matching on purpose, and it is temporary.** `governed_producers::StagedConfigRefusal`
is a `pub`, downcastable error introduced by PR #53 (`claude/adr0019-p1-fixes-20260912`, round-1 fix
`9d9a8c6`); #52's post-merge work item 5 turns it into a typed daemon response variant. Neither is
on this lane's base, so the only thing that crosses the socket today is the `Display` string. The
classifier matches the **substrings that are the refusal's identity** (`no comparable
shared-repository-configuration pin`, `refusing to run Git in that worktree`, `the staged world
scope does not support submodules`) rather than whole sentences, so an upstream wording tweak does
not silently reclassify a refusal as a generic error.

**Replacement point when the typed variant lands:** construct `StagedConfigRefusalNotice` from the
typed variant and replace the body of `staged_config_refusal_notice` — that function is the only
thing that changes. Its callers take a `StagedConfigRefusalNotice`, and its tests
(`test_a_staged_config_refusal_reads_as_a_refusal_not_a_failed_host_call`) assert behavior — that a
refusal reads as a refusal, that an ordinary transport failure does not — not the matching strategy.
No TODO comment is left behind; this paragraph is the record.

### 5. The governed Git preflight is hook- and global-config-free

PR #53's handoff recorded `impulse-desktop/src/runtime.rs`'s `run_bounded_governed_git` as a Git
invocation on a governed path with none of the producers' hardening: no `core.hooksPath`, no
`core.fsmonitor`, and no `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`. It could not fix it —
`impulse-desktop/**` was blocked for that lane. Fixed here:

- `HOOK_FREE_GIT_OVERRIDES` (`-c core.hooksPath=/dev/null -c core.fsmonitor=false`) is prepended to
  every invocation, mirroring `governed_producers::hook_free_git` and extending it with the
  `core.fsmonitor` suppression that module's callers get from running in a daemon-materialized
  worktree. `-c` beats repository configuration, which is the level that matters: this preflight runs
  in the operator's own checkout, whose `.git/config` is writable by anything that has already run in
  the project.
- `scrub_governed_git_environment` now sets `GIT_CONFIG_GLOBAL=/dev/null` and
  `GIT_CONFIG_SYSTEM=/dev/null`. `HOME` stays on the allowlist — Git needs it to resolve `~` in paths
  it is *given* — but the two files it would otherwise read from there and from the system prefix are
  neutralized.

**No shared helper was extracted.** `governed_producers::hook_free_git` is `fn`-private in a blocked
crate and builds a `std::process::Command` through `tooling::env_scrub`, which `impulse-desktop` does
not depend on; the desktop preflight has its own bounded-spawn, process-group, and drain machinery.
Lifting it to `impulse-ops` would mean moving process-spawn code into a crate that is pure
protocol/model types today. The two are kept textually parallel instead, and both carry a doc comment
pointing at the other.

**Proof, both directions.** `test_governed_git_preflight_runs_neither_repository_hooks_nor_fsmonitor`
plants a real `post-index-change` hook and a real `core.fsmonitor` script in a real repository, runs
the preflight, and asserts neither marker file appears and the preflight still resolves a commit OID.
Removing `HOOK_FREE_GIT_OVERRIDES` makes it fail on the hook assertion — verified by deleting the
line and re-running, against Git 2.50.1. The pre-existing bound-and-reap test could no longer use
`core.fsmonitor` as its hang vector (that is now suppressed), so it was rewritten to hang through a
repository **alias**, which `-c` overrides do not disable and which therefore remains a real way for
repository configuration to hang the preflight. It still proves the deadline and the process-group
reap of a backgrounded descendant.

## The one shared-crate addition

`impulse-ops/src/governed_wiring.rs` gained exactly one public function,
`governed_outcome_is_promotable`, plus its tests. Nothing was modified.

Judged unavoidable: the discard control already had `staged_worktree_is_discardable` in that module,
written for exactly this purpose ("so the daemon can refuse *before* running the destructive side
effect"), and promotion had no equivalent. The alternative was re-deriving the daemon's four-clause
promotion preflight (`staged_authoritative`, accepted, active checkout, not already promoted) inside
the desktop crate — a governance rule expressed twice in two crates, which is the drift
`staged_worktree_is_discardable`'s own doc comment argues against. The function deliberately excludes
whether the canonical branch can actually be advanced: that is observable only by running Git, and a
head that moved is an execution fact the daemon reports, never a reason to withhold the attempt.

`ui.rs`'s `promote_control_state`/`discard_control_state` use the shared predicates for the
enable/disable decision and only *explain* a refusal the predicate already made.
`test_control_states_track_the_shared_impulse_ops_predicates` walks a matrix of tasks and asserts
`is_enabled()` equals the predicate for every one, so the cockpit cannot offer a control the daemon
would refuse or withhold one it would accept.

## Conflict Surface — the stale Codex branch

`agent/codex-dioxus-packaged-acceptance-20260830` (2026-08-30, unmerged) against `36bda00`:

```
$ git diff --stat 36bda00..origin/agent/codex-dioxus-packaged-acceptance-20260830 \
    -- impulse-rs/impulse-desktop/src/
 .../impulse-desktop/src/bin/impulse_desktop.rs     |   16 +-
 impulse-rs/impulse-desktop/src/daemon_ops.rs       |  783 +++++++-
 impulse-rs/impulse-desktop/src/host_bridge.rs      |  361 +++-
 impulse-rs/impulse-desktop/src/lib.rs              |    1 +
 .../impulse-desktop/src/packaged_acceptance.rs     | 2025 ++++++++++++++++++++
 impulse-rs/impulse-desktop/src/runtime.rs          |   14 +
 impulse-rs/impulse-desktop/src/ui.rs               |  169 +-
 impulse-rs/impulse-desktop/src/views.rs            |    3 +-
 8 files changed, 3264 insertions(+), 108 deletions(-)
```

Overlap with this lane, file by file:

- **`daemon_ops.rs` (+783)** — the real conflict. That branch rewrites large parts of the file; this
  lane adds two trait-method implementations next to `routing_metadata`, extends
  `requires_operator_class` by two match arms, renames one private field/constant, and appends a
  block of tests. All five edits are localized and none is inside the transport core that branch
  rewrites, but a textual conflict on the `impl GovernedTaskGateway` block is likely.
- **`ui.rs` (+169)** — that branch also edits this file; this lane adds two bridge-JS blocks next to
  `mutateGovernedTask`, two script builders next to
  `governed_task_mutation_bridge_script`, a helper block before `OperatorLane`, two props on
  `GovernedTaskCard`/`OperatorBoard`, and one `section` at the end of the card. Conflicts, if any,
  are adjacency conflicts; take both sides.
- **`runtime.rs` (+14)** — small on both sides; this lane's edits are the Git-preflight overrides,
  two defaulted trait methods, and the `adopt_acknowledged_governed_task` extraction.
- **`host_bridge.rs` (+361)** — this lane adds one import line and two dispatch arms.
- **`packaged_acceptance.rs`, `bin/impulse_desktop.rs`, `views.rs`, `lib.rs`** — not touched here.

That branch also claims protocol v7, which #52's stacking note already says must be rebased to take
9. Whoever lands it should rebase onto the merged v9 line rather than resolving these files by hand.

## Handoff Notes

### For the still-open desktop staged-launch row (ADR-0019)

- Register with `.world_scope(WorldScope::StagedAuthoritative)`; the returned record already carries
  an active staged worktree (the daemon materializes during registration since v9).
- Take the pane cwd from `task.launch_working_directory()`. **Its signature changed** on
  `claude/adr0019-p1-fixes-20260912`: it now returns
  `Result<&str, LaunchWorkingDirectoryError>`, not `&str`.
- Add `RegisterGovernedTask` to `UnixDaemonOpsClient::requires_operator_class` at the same time — the
  daemon gates staged-scope registration as operator-class, and this lane left it out on purpose (see
  above).

### For whoever lands #52's post-merge work item 5 (typed staged-config refusal)

`staged_config_refusal_notice` in `impulse-desktop/src/ui.rs` is the single replacement point;
see section 4.

### For the accepted-run / promote UX follow-up

`GovernedProducerAck::pending_rerun_reason` reaches the desktop intact (proven by
`a_blocked_promotion_is_an_operator_class_success_not_a_client_error`) but is not yet rendered. It
means a crashed producer's work is being redone, which is worth showing next to a promotion the
operator just triggered. Not done here: the cockpit's governed bridge is fire-and-forget by design
— it waits for the authoritative `ops_update` rather than rendering an invoke's return value — so
surfacing it means deciding where per-request producer feedback lives, which is a UX decision beyond
this lane's scope.

## Verification

Isolated `CARGO_TARGET_DIR`, run from `impulse-rs/`. Package-level totals are recorded in the PR
body from the run on the final commit, per CLAUDE.md's final-gate evidence rule (no aggregate is
copied into this card).

The desktop crate's `desktop-app` feature was **not** exercised: it pulls `dioxus-desktop`, which
needs a windowing/WebView host. The default gate covers every line this lane touched — all of it is
outside `#[cfg(feature = "desktop-app")]`, which gates only `desktop_host.rs` and the binary.

`python3 ../docs/validate_docs.py --all` reports only pre-existing failures: `0014`'s
`status: proposed` and three stale documents (`LONG-RANGE-ENHANCEMENTS.md`,
`RUST-MULTI-AGENT-PATTERNS.md`, `RUST-MULTI-AGENT-PROGRAMMING.md`). None is a file this lane owns.

**Environment note for whoever re-runs this.** The first full `cargo test --workspace` on this
checkout wedged in `tests/integration_enhancements.rs`: three `impulse-rs` child processes it spawns
sat for 29 minutes at `_dyld_start` with a 112K footprint — stuck in the dynamic loader before
reaching any application code — while a *second* lane's `cargo test --workspace` had been stuck the
same way for thirteen hours. It is machine-level contention between concurrent lanes, not a code
fault: nothing that suite exercises is touched here, and a clean re-run with the machine quieter
passed the whole suite. If it wedges again, re-run rather than bisecting.

## Review round 1

Adversarial review of PR #58. 417 desktop + ops tests green; claims a, b, c, f and the host-bridge
envelope route confirmed. The brief's XDG premise was **refuted** — `GIT_CONFIG_GLOBAL=/dev/null`
already covers it and `XDG_CONFIG_HOME` is scrubbed by the env allowlist, so nothing was changed for
it. What follows is what round 1 found and what changed.

### P1 — the confirmation told an operator a discard cost nothing when it cost a commit

`unreferenced_accepted_commit_on_discard` answered `None` for an **accepted run with no promotion
attempt**, because it read the question as "was a promotion blocked". That population is reachable:
`staged_worktree_is_discardable` short-circuits to `true` for a worktree whose pin is `Unknown` —
the pre-pin records `PromotionBlockedReason::RepositoryConfigUnpinned` exists for — so an accepted,
never-promoted run is discardable with zero promotions recorded. The confirmation then rendered
"No accepted commit loses its only reference: nothing here was accepted and left unpromoted."
immediately before an irreversible action, with no OID.

Fixed in `impulse_ops::governed_wiring::unreferenced_accepted_commit_on_discard`: an accepted run
with no `Promoted` outcome now answers with the accepted claim's `subject_revision` — the same value
`governed_producers::promote_governed_outcome` uses for `accepted_revision` — preferring a blocked
promotion's already-recorded `accepted_revision` when one exists. **This widens the daemon's
acknowledgement too**: `GovernedStagedWorktreeDiscardAck::unreferenced_accepted_commit` is filled
from this function, so the CLI's `unreferenced_commit_warning` and the daemon's discard endpoint now
also report the unpinned-accepted case. That is the intended consequence, not a side effect — the
field's contract is "the commit this discard strands", and it was under-reporting.

On the UI side, `discard_reassurance_notice` replaces the old unconditional `else` branch. An
accepted run can never reach a "nothing was accepted" sentence: it gets the cost notice with its
OID, or — for the claimless record the state layer should not produce — an explicit admission that
the commit cannot be named and the staged HEAD should be checked before discarding.

The round-0 test that asserted the buggy behavior
(`test_only_an_accepted_but_blocked_promotion_names_an_unreferenced_commit`) was renamed and its
assertion corrected rather than deleted, so the diff shows the belief that changed.

### P2 — "hook-free" overstated the guarantee

A repository-level `filter.*.clean` still executes during the preflight's `git status`, reproduced
by the reviewer with a `.gitattributes` line of `* filter=probe`, and **no Git switch disables it**:
`-c filter.x.clean=` breaks one name, and the set of names is whatever the repository defines.

Wording is now "hook- and global-config-free" in the ADR line, the lane card, and the PR body. More
than wording: `refuse_executable_git_drivers` runs before the first Git process and refuses the
preflight, by filesystem read only, if `.git/config`, `.git/config.worktree`, or
`.git/info/attributes` defines a `filter.*.clean`/`.smudge` or `diff.*.textconv`/`.command`/`.process`
key. The daemon's producers answer this with a materialization-time pin; this preflight owns no pin
(it runs in the operator's own checkout before any staged worktree exists), so refusing is the
honest equivalent. Proven with the reviewer's reproduction — the filter's marker file is never
written — plus a negative control (removing the refusal makes the marker appear) and an
acceptance case so it is not a blanket block.

**Residuals, recorded rather than implied:**

- An `include.path` / `includeIf` directive in `.git/config` can pull a driver in from a file this
  check does not read. Following includes means implementing Git's include resolution, which is the
  producers' pin machinery by another name.
- `.git`-as-a-file is followed one level (`gitdir:`); a deeper chain, and `commondir` indirection,
  are not resolved.
- The parser is line-level and case-insensitive on keys. It accepts both the section-header and
  fully-qualified spellings and ignores comments; it does not implement Git's full config grammar
  (line continuations, quoted values containing `=`).

### P2 — `staged_config_refusal_notice` matched loosely and hid the daemon's own words

Three changes. The classifier **matches nothing on this base** — the strings it targets are
introduced by PR #53 — which is now stated in its doc comment as a fact rather than left to be
discovered. The changed-pin arm no longer accepts "refusing to run Git in that worktree" on its own:
that trailing clause is a generic consequence a future unrelated refusal could reuse, and matching
it would attach a discard-and-re-materialize remedy that does not apply. A near-miss test
("...refusing to run Git in that worktree because it is dirty") pins that. And `BridgeStatusBanner`
now renders the raw daemon text **alongside** any interpretation instead of replacing it, because
the classifier reads a message it does not own. The single replacement point for the typed variant
is unchanged.

### P2 — promote-side matrix cross-check

`test_promotability_is_between_the_daemon_and_state_layer_rules_over_the_matrix` walks
9 reviews × 4 executions × 4 scopes × 3 staged statuses × 3 promotion outcomes (432 cases) and
asserts `governed_outcome_is_promotable` is a superset of the daemon endpoint's inline checks and a
subset of the state layer's `RecordPromotion` preconditions, plus that the promotable set is
non-empty so the upper bound is not vacuous.

**Boundary, stated:** both authorities live in `impulse-rs` (`src/daemon/governed_wiring.rs`,
`src/state/governed_task.rs`), which `impulse-ops` cannot depend on and which this lane does not
own, so the two rules are **restated** in the test from those exact sites rather than imported. The
version that imports the real functions belongs directly beside
`both_discardability_rules_agree_over_the_whole_state_matrix` in `src/daemon/governed_wiring.rs` —
a one-test handoff for whoever owns that file next.

### P2 — ack-only fields no longer dropped

`pending_rerun_reason` (promotion) and `unreferenced_accepted_commit` + `discarded_root` (discard)
exist only on the acknowledgement; the `ops_update` the card waits for carries none of them. The JS
bridge now forwards each into the existing bridge-status banner channel under its own status
(`governed_promotion_rerun_pending`, `governed_discard_unreferenced_commit`), each with a headline
that does not read as a failed host call.

### Nits

- **Discard drafts survive an unrelated revision bump.** The card stays keyed `id:revision` —
  deliberately, so a new daemon revision clears the *decision rationale* — but the discard draft and
  armed flag moved up into `OperatorBoard`, keyed by task id alone. A half-typed discard reason is no
  longer collateral damage of an unrelated `ops_update`.
- **World scope pinned by assertion.** `test_every_desktop_registration_is_authoritative_scoped`
  asserts against the source that `impulse-desktop/src/runtime.rs` contains no `.world_scope(` call,
  so adding a staged launch trips the test rather than silently shipping a staged registration on a
  connection that never presents the operator capability.
- **Stale revisions read as a stale board.** A `revision conflict` reason now headlines as "Board is
  out of date: this task changed on the daemon — refresh and retry" instead of "Host call failed".
