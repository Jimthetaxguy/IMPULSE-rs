---
title: "ADR-0019: Builder Staged-Worktree World Scope"
description: A declared world scope, a disposable staged worktree for the Builder, and promotion as a separate step after operator acceptance
status: review
created: 2026-09-02
updated: 2026-09-12
type: decision
category: architecture
phase: all
audience: builders
deciders: [Impulse Maintainers]
tags: [adr, governance, sandbox, worktree, world-scope, loop, promotion]
---

# ADR-0019: Builder Staged-Worktree World Scope

## Status

Proposed and implemented at the state and producer layer on lane
`claude/staged-worktree-scope-20260902`; accepted on merge. The daemon endpoint, the desktop
launch wiring, and the durable producer reservation are separate lanes and are **not** delivered
here — see [Not delivered by this ADR's lane](#not-delivered-by-this-adrs-lane).

## Context

ADR-0011 through ADR-0013 govern a run's *evidence*: a revisioned task record, four separated
attestations, daemon-owned verification in a detached worktree, operator-required acceptance, and
a review-only memory candidate. What they do not govern is *where the Builder writes while it
works*.

Today a launched Builder is pointed at the canonical project tree and mutates it directly.
Verification alone is snapshot-scoped: `run_verification` materializes a detached worktree at the
claimed commit and runs the profile there, so the evidence is clean. The mutation is not. The
consequences are concrete:

- A verification that fails, or a Supervisor or operator that declines, leaves the canonical tree
  carrying the Builder's work with nothing that undoes it. The run's review state says "rejected";
  the filesystem says "applied".
- Nothing bounds how many times a Builder may re-claim after a failure, so a Builder that cannot
  make the gate green can churn the canonical tree indefinitely.
- The role-launch compatibility preview (ADR-0010) reports `filesystem.scoped` as `unsupported`
  for every runtime, which is honest, but it leaves the operator with no scope story at all.

The sandbox analysis for background agents ranks filesystem snapshot-at-mutation as the single
highest-value missing capability for exactly this pipeline, and names the implementation path
Impulse already owns: it creates detached worktrees for verification; create one for the Builder
too, and let acceptance promote the result. ROSA's capability algebra supplies the vocabulary — a
*world scope* contract with `read_only_snapshot`, `disposable_scratch`, `staged_authoritative`,
and `authoritative` tiers. E2B's filesystem forks and Codex's `workspace-write` mode are the same
idea at the OS layer; Git worktrees are the layer Impulse can actually enforce today.

ADR-0017 supplies the second half. It defined a typed `LoopContract` deliberately free of
provider, tool, and daemon types, and named governed Builder iterations as the intended next
consumer. A staged Builder is the first loop Impulse can bound end to end, because every input the
contract needs — claim count, elapsed time, repeated verification failures — is already persisted
in the task record.

## Decision

1. **Every governed task declares a `WorldScope`.** `ReadOnlySnapshot`, `DisposableScratch`,
   `StagedAuthoritative`, `Authoritative`, serialized `snake_case`, defaulting to `Authoritative`
   on both the registration and the durable record, so every `GOVERNED_TASKS.json` written before
   this ADR loads and replays unchanged. A scope the daemon cannot materialize fails registration
   closed rather than being silently downgraded: today only `StagedAuthoritative` and
   `Authoritative` are materializable.

2. **A staged scope requires a closed-loop verification profile.** The staged worktree is created
   at the registration's `initial_subject_revision`, so that OID must be daemon-attested, which is
   exactly what the profiled registration path already guarantees. A staged task id must also be
   one filesystem path segment of ASCII letters, digits, `-`, or `_`, because it becomes a
   directory name.

3. **The daemon materializes the staged worktree before launch.** `materialize_staged_worktree`
   observes a clean canonical tree still sitting on the attested OID, then creates
   `<workspace root>/.impulse/worktrees/<task id>` through the same detached `git worktree add`
   path verification uses — the helper is now shared, not duplicated, so both paths get the same
   bounded, env-scrubbed, closed-argv invocation and the same failure cleanup. The path is derived
   from the task record by `GovernedTaskRun::expected_staged_worktree_root`, defined once in
   `impulse-ops` so the producer that creates it and the ledger that validates it can never
   disagree. A caller cannot choose it. The materialization is its own revisioned mutation with
   its own `staged_worktree_materialized` event.

4. **`launch_working_directory` is the Builder's cwd.** It returns the staged root while the
   worktree is active and the canonical workspace root for every non-staged scope, so a runtime
   that has not been taught about world scopes still lands somewhere correct. A *staged* task with
   no active staged worktree has no launch directory at all and returns a typed error rather than
   the canonical workspace, and `MarkRunning` refuses such a task outright — see
   [Post-merge fixes (2026-09-12)](#post-merge-fixes-2026-09-12).

5. **Promotion is a separate step after acceptance.** `promote_governed_outcome` is allowed only
   on an `accepted` task with an active staged worktree. It requires the staged worktree to be
   clean and sitting exactly on the accepted claim's subject revision, and it requires the
   canonical checkout to be **on a branch**: `git symbolic-ref --quiet HEAD` must resolve to a
   `refs/heads/` reference. It also requires the worktree-shared repository configuration to be
   byte-identical to what was observed at materialization (rule 13). The move itself is a compare-and-swap —
   `git update-ref <branch> <accepted> <initial>` — which writes only if the ref is still exactly
   the registered initial OID at the instant it writes. `git merge --ff-only` cannot promise that:
   it re-reads HEAD and then writes, leaving a window a concurrent commit can win. The working
   tree is then synced to the moved ref, since `update-ref` writes the ref and nothing else. The
   entire canonical-branch side effect is isolated in `compare_and_swap_canonical_branch` and
   `sync_canonical_worktree` so a durable producer reservation can wrap it without re-reading this
   ADR.

6. **A canonical branch that cannot be advanced is an execution fact, not an error.** Promotion
   returns `PromotionBlocked { canonical_head, reason }`, recorded against the already-accepted
   run. The reason is one of `canonical_head_moved` (the head no longer equals the registered
   initial OID), `detached_head` (there is no branch to advance), `concurrent_branch_update`
   (the compare-and-swap lost), or `repository_config_changed` (rule 13). Review state stays `Accepted`; the staged worktree stays active;
   an operator who reconciles the canonical branch can retry. A run is promoted at most once: a
   second promotion after a successful one is refused, and a promoted outcome must land exactly
   the accepted revision.

7. **A finished staged worktree is discarded.** `discard_staged_worktree` removes the checkout and
   its administrative entry, and the matching mutation marks the record `discarded`. It is allowed
   from every state a staged worktree can legitimately be reclaimed from — `rejected`,
   `escalated` (the terminal state this ADR's own loop contract produces), an `accepted` run that
   has recorded any promotion outcome (promoted, so the checkout is spent; or blocked, so the
   operator may reclaim the space instead of retrying), and any task whose execution state is
   `launch_failed` — and from nowhere else, so a live Builder's work cannot be deleted out from
   under it.

8. **The world scope's own contribution to `filesystem.scoped` is `mediated`, never
   `structural`.** `world_scope_filesystem_enforcement` returns `Mediated` for
   `StagedAuthoritative` and `Unsupported` for every other scope, and
   `evaluate_role_compatibility_in_world` merges that into the runtime's own declared support,
   taking the stronger of the two per capability. Two consequences worth stating precisely: a
   staged launch on a runtime that declares nothing reads as *allowed but degraded*, because the
   canonical Builder role asks for structural containment and a Git worktree honestly supplies
   only Git-level mediation; and a runtime that itself declares `structural` still reports
   `structural`, because the `max` takes the runtime's stronger claim. The "never structural" rule
   bounds what **the scope** may claim, not what a genuinely sandboxed runtime may. Nothing about
   the staged worktree stops the Builder's process from writing outside the checkout.

9. **A staged claim is admitted under `LoopContract::governed_builder()`.** Five claim cycles, a
   four-hour wall clock measured from registration, and three consecutive verification failures
   with the same signature. The verdict is computed by a new pure
   `LoopBudget::evaluate_observed`, which reads no clock: every input comes from stored counts and
   stored timestamps. A trip records the claim as evidence, emits a `loop_tripped` event, and moves
   review state to `escalated`, which accepts no further claims. Tasks outside the staged scope
   are not evaluated at all, which is what keeps every pre-ADR-0019 ledger replaying identically.

10. **Loop evidence is versioned.** Every staged claim carries a `loop_report_digest` — the
    SHA-256 of the version tag plus the key-order-independent canonical JSON of the ADR-0017
    `LoopReport` the verdict came from — *and* the `loop_report_version` it was computed under
    (`GOVERNED_BUILDER_LOOP_VERSION`). Replay only recomputes digests written under the version it
    is running; a claim from any other version is replayed verbatim, reusing its stored digest and
    its stored outcome, and is checked only for structural coherence (a digest and a version are
    present together, and the digest is well formed). This is what makes rule 9's budget constants
    genuinely revisable: bumping them, or adding a field to `LoopReport`, changes future digests
    without making a single existing `GOVERNED_TASKS.json` unloadable. The version must be bumped
    whenever anything feeding a persisted digest changes.

11. **Replay is verified, not assumed.** `apply_mutation` now takes the clock as a parameter.
    Normal operation passes the current time; ledger validation passes each event's own recorded
    timestamp. Reload therefore reproduces every clock-derived decision exactly, and validation was
    tightened to prove it: the replayed event *kind* must match the stored kind, the replayed
    claim's `loop_report_digest` must match the stored digest, and the replayed staged worktree and
    promotion records must match theirs. A ledger whose loop evidence or staged root was rewritten
    fails closed on load.

12. **The accepted-run memory candidate is pinned to the accepting revision.** ADR-0013's
    projection previously digested `task.revision`, which was equivalent to the accepting revision
    only because nothing could mutate a task after acceptance. Promotion can. The projection now
    derives its `accepted_task_revision` from the operator decision's `based_on_revision + 1`. For
    every record written before this ADR the two numbers are identical, so no stored candidate
    digest changes.

13. **Every repository-level configuration file Git reads is pinned across the run — in a
    repository without submodules, which is the only kind this scope runs in.**
    `.git/config` and `.git/info/attributes` are shared by the main worktree and every linked one,
    so a staged Builder can write them from inside its own checkout — `info/attributes` never
    appears in a diff of the work tree, so it is invisible to code review. `config.worktree` is
    pinned too, and **per worktree**: it is not shared, and the copy that matters most is the
    *staged* worktree's own `<common>/worktrees/<task id>/config.worktree`, which Git reads for
    every command a producer runs in that checkout. The original draft of this rule argued that
    `config.worktree` was covered because enabling `extensions.worktreeConfig` rewrites
    `.git/config` and would be caught. That argument is wrong whenever the extension is *already*
    on — `git sparse-checkout` in a linked worktree turns it on for entirely ordinary reasons — and
    the review of the post-merge fixes reproduced the consequence end to end: a `filter.<name>.clean`
    plus a `core.attributesFile` written there leaves `.git/config` byte-identical and still
    executes inside a daemon-owned producer. The pin covers the canonical worktree's copy, the
    common-directory copy, and the staged worktree's own, and it is recorded at materialization
    *after* the staged worktree exists so all three are in it. A `filter.<name>.smudge` or
    `diff.<name>.textconv` defined there executes whenever Git materializes a file — including the
    working-tree sync promotion performs, in the canonical workspace, under operator authority,
    after review has already passed. Disabling hooks (rule 5's `core.hooksPath`) does not touch
    that path. Materialization therefore records a
    digest of that shared state on the `StagedWorktree` record — the **raw bytes** of each pinned
    file, plus the raw bytes of every file reachable through the `include`/`includeIf` paths those
    files name — as a
    `SharedRepositoryConfigPin` covering the canonical worktree, the common directory, and the
    staged worktree, so a worktree recorded before the pin existed loads as `Unknown`
    rather than failing the whole ledger on a missing field, and promotion refuses it with its own
    `repository_config_unpinned` reason telling the operator to discard and re-materialize. The pin
    carries the `scheme_version` it was computed under, and a pin from any other scheme is refused
    the same way rather than compared across schemes. The comparison, and the digest itself, read
    files and run no Git process, so a producer can refuse **before** any Git invocation touches the
    repository whose configuration is in question. **Every** producer that runs Git in the staged
    worktree compares it first, not only promotion: the claim and verification producers run
    `git status` there, which applies a `filter.<name>.clean` to any file the attributes route
    through it, so a Builder that planted one executed its own script at claim time — before an
    operator ever saw the run — until they were gated too. They refuse with a typed
    `StagedConfigRefusal`, which is an error rather than a recorded outcome because, unlike
    promotion, there is no accepted run to record it against. An
    empty digest would have been the wrong default: it would either compare equal to nothing
    (silently unsafe) or to everything (blocked with no explanation). Promotion refuses to check
    anything out unless the same digests still hold, reporting
    `repository_config_changed { component }` instead — the component names which file changed,
    because benign churn (a new remote, a credential helper) hard-blocks promotion too and the
    operator must not be left guessing which file to inspect. The trust boundary is deliberate: configuration that predates the run is the
    operator's own and is honored, while configuration the Builder introduced blocks until a human
    looks at it. An in-tree `.gitattributes` the Builder commits stays legitimate work, because a
    driver it names cannot be *defined* without changing the pinned digest.

    Global and system configuration are outside the pin and cannot be brought inside it — they are
    not repository state — so every daemon-owned producer Git invocation in `governed_producers.rs`
    suppresses them with `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_SYSTEM=/dev/null`. The env
    scrub keeps `HOME`, so without that a `filter.<name>.clean` in the operator's own `~/.gitconfig`
    would execute inside a daemon-owned producer.

    **Submodules are refused, not pinned.** Each submodule is a repository with a configuration file
    of its own — `<common>/modules/<name>/config` for the main worktree,
    `<common>/worktrees/<id>/modules/<name>/config` for a linked one, each with its own
    `info/attributes` — and a `filter.<name>.clean` defined there fires on the *enclosing*
    worktree's `git status`, which is the first thing every producer runs. Verified against Git
    2.50.1. Pinning that set means walking a directory tree whose shape is Git's to change, and this
    rule's whole claim is that the file list is complete; a list that has to be re-derived from a
    directory walk is not one this ADR can stand behind. So the staged scope refuses a repository
    that carries submodule configuration — a `.gitmodules` in either tree, or a `modules/` directory
    in either Git directory — at materialization, and again on every producer path so a
    `.gitmodules` introduced mid-run cannot slip past. That is a real capability gap, stated as one
    rather than papered over.

    **The pin is recorded after the worktree exists**, so it covers that worktree's own per-worktree
    files. The window between `git worktree add` and the recording mutation is synchronous, with no
    `.await` and no launch path: launching requires the active staged record the caller has not
    recorded yet, `MarkRunning` refuses a staged task without one, and materialization is
    operator-only. Nothing runs in the staged worktree before its pin exists.

    **The load-bearing argument is detection, not enumeration — within the files it covers.** This
    gate is safe because it
    compares bytes and refuses on any difference, so it never has to know which Git keys are
    dangerous. What it is *not* immune to is being wrong about **which files Git reads**: a
    configuration source outside the list is outside the gate, which is exactly how the staged
    worktree's own `config.worktree` was missed in the first draft and how global configuration was
    reachable until it was suppressed. The file list is the security-relevant surface and must be
    re-derived whenever Git grows a new source. That distinction matters for how this ADR should be read: an enumeration claim
    ("no Git switch suppresses repository config", "`-c` cannot mask an attacker-named key") is a
    statement about a large surface that is right until one version or one key makes it not, and
    it appears here only as supporting evidence. If a reviewer disproves it, or a future Git adds
    a suppression switch, rule 13 still holds — a filter driver cannot be *defined* without
    changing the bytes being compared. Do not rewrite this rule to lean on the enumeration.

## Consequences

**What this buys.** A failed verification or a declined review is now recoverable by deleting a
directory. The canonical tree is byte-identical from registration through `awaiting_operator` — the
daemon-owned verifier observes and snapshots the *staged* tree, never the canonical one — and the
integration tests prove it against real repositories. A Builder that cannot make the gate green
escalates instead of churning. Operators see an honest scope claim rather than silence.

**What it does not buy.** This is a Git-level boundary and nothing more. There is no OS sandbox, no
mount namespace, no egress allowlist. A Builder process can `cd` out of its staged worktree and
write anywhere the user can. `filesystem.scoped` is reported as `mediated` precisely because
promising more would be false. OS-level sandboxing and egress control remain explicitly deferred.

**Promotion is fast-forward only.** No merge, no rebase, no force. A canonical head that moved
blocks, and a human decides. This is deliberate: an automated merge is a semantic decision, and the
governed pipeline's whole premise is that semantic decisions need an operator.

**The compare-and-swap closes the ref race, not the working-tree race.** `update-ref` guarantees
the branch only moves from exactly the initial OID, so a concurrent commit loses and is reported as
`concurrent_branch_update`. Syncing the working tree afterwards is a second step, and a file
written into the canonical checkout between the cleanliness observation and that sync would be
replaced. The window is small and the tree was verified clean immediately before, but it is real
and is not closed by this ADR. If the sync fails, the error names the branch that already advanced
so the operator can finish it by hand rather than guessing what state the repository is in.

**After a blocked promotion, discarding the worktree drops the only ref to the accepted commit.**
A blocked run's accepted commit exists only in the staged worktree; the canonical branch never
advanced. Discarding removes the checkout and its administrative entry, leaving that commit
reachable only through the reflog until it expires. Discard is still the right escape hatch, but
the surface offering it must say what it costs, and should show the commit OID so an operator can
recover it deliberately. The desktop track owns that wording.

**None of this is reachable in a running system yet.** These producers have no IPC endpoint and no
CLI handler; the daemon lane owns both. Live exploitability of the vectors above is zero today,
which is exactly why they were worth fixing now rather than after something calls them.

**A surviving staged directory fails closed, and names its own recovery.** If a run is interrupted
between `git worktree add` and the recording mutation, the directory outlives the task and blocks
re-materialization. Reusing it is not safe — it could be someone else's half-finished tree — so the
producer refuses, and the error tells the operator exactly how to recover: delete the leftover
directory, then run `git -C <workspace> worktree prune` to drop its administrative entry.

**The configuration pin compares raw bytes, and it compares them first.** The digest is the bytes
of `.git/config`, `.git/config.worktree`, and `.git/info/attributes`, plus the bytes of every file
their `include`/`includeIf` directives reach. It is not a rendering of `git config --list`: a
rendering loses order, and order is semantic, because Git resolves a repeated scalar key to the
last one. And it is computed without running Git — the Git directory and the common directory are
resolved by reading `.git` and `commondir` the way Git does — so promotion compares the pin
*before* it spawns any Git process against that repository. Both properties were forced by the
post-merge review; both are load-bearing.

**The configuration pin is a change detector, not a sandbox.** It catches a driver the Builder
*introduced*, which is the reachable attack. It does not stop a Builder from committing a
`.gitattributes` that routes new paths through a filter the operator had already configured — a
pre-existing `git-lfs` or `crypt` driver, say. That is inherent to honoring the operator's own
configuration, and closing it would need the OS-level isolation this ADR explicitly defers. The
pin also fails closed on benign drift: an operator who legitimately edits `.git/config` mid-run
gets a blocked promotion and has to look at it, which is the intended bias.

**Git hooks never run inside a producer.** `.git/hooks` is shared across every linked worktree, so
a staged Builder could otherwise plant a hook that executes inside a daemon-owned producer — at
materialization, at promotion *after* review has already passed, or during plain observation:
`post-index-change` and `fsmonitor-watchman` fire on a bare `git status`, which the promotion path
runs twice. **Every** Git invocation in the producer module is therefore built by one constructor
that sets `core.hooksPath` to a non-directory; there is no "read-only enough to skip it" category,
and the first version of this fix was wrong precisely because it assumed one. That constructor also
disables `core.fsmonitor`, which is a second hook mechanism `core.hooksPath` does not cover — but
that flag is supporting defense, not the guarantee. The guarantee for promotion is the ordering
above: the configuration comparison happens before any Git process exists to be protected.

**A crash between the Git side effect and its receipt is still not covered.** Promotion inherits
the same window every other producer has. The side effect is isolated in one function so the
sibling reservation lane can wrap it; this lane does not implement reservations.

**The loop budget is a guess informed by nothing yet.** Five cycles and four hours are starting
values with no production data behind them. They are constants in `loop_contract.rs` and are meant
to be revised once real governed Builder runs exist — which is exactly why rule 10 versions the
evidence: revising them must not strand existing ledgers, and without the version pin it would.

**Untracked `.impulse/worktrees/` no longer marks the canonical tree dirty.** The staged worktree
lives inside the project's own runtime namespace, so the producers' cleanliness check ignores
untracked paths under it, exactly as it already ignores `GOVERNED_TASKS.json`. Tracked or staged
mutations under that path are still subject changes.

**`governed_producers` is now a public module.** The three staged producers are `pub` so an
integration test can drive them against real repositories before any daemon endpoint exists. Every
other item in the module stays `pub(crate)`.

**A derivation-version bump must migrate candidate status, not drop it.** ADR-0018 adds
`prune_superseded_derivations()` to the memory-candidate load path so a derivation-version bump
cannot fail an existing ledger closed. That is lossless only while `MemoryCandidateStatus` has a
single variant. The moment ADR-0020 adds `Promoted` or `Dismissed`, pruning a superseded record
would silently revert an operator's review decision to `PendingReview` — a governance regression,
not a cache miss. Whoever adds the second status variant must migrate status forward on load
instead of dropping the record. Recorded here because this ADR is what put a post-acceptance
mutation into the picture, and it lands before ADR-0020.

## Not delivered by this ADR's lane

| Deferred | Owner |
|---|---|
| `PromoteGovernedOutcome` daemon endpoint, protocol bump, and CLI subcommand | the daemon / socket-provenance lane (`src/daemon/**`) |

## Stacking on ADR-0018 (recorded 2026-09-03 during the merge train)

This lane is stacked on ADR-0018's socket actor provenance, so:

- **`DAEMON_PROTOCOL_VERSION` is 8 here.** `main` is 6; ADR-0018 takes 7 (it adds
  `PresentOperatorCapability` and the operator-class requirement on `RecordOperatorDecision`);
  this ADR takes 8 (the staged-worktree mutations and the promotion outcome they carry). Any
  further lane claiming 7 -- the Codex packaged-acceptance branch does -- must rebase and take 9.
- **`apply_mutation` takes a single `MutationContext`.** ADR-0018 added an
  `operator_authentication: OperatorAuthentication` parameter; this lane replaced the loose `now:
  &str` with a `MutationContext { now, replay_claim }` so replay reproduces clock-derived decisions
  exactly. The merged shape keeps `MutationContext` and carries `operator_authentication` as a
  field on it: the live path builds `MutationContext::live(&now, operator_authentication)`, the
  replay path builds one carrying the provenance read back off the stored decision. The struct is
  private to `state/governed_task.rs`, so a request handler still cannot assert its own provenance.
- **All three of this ADR's mutations are operator-only.** `MaterializeStagedWorktree`,
  `DiscardStagedWorktree`, and `RecordPromotion` are classified as requiring an operator-class
  connection in `authorize_governed_mutation`, regardless of verification profile. The staged
  worktree is daemon-owned end to end; a launched Builder that could drive these would be able to
  materialize a scope it was never registered for, discard the evidence of its own run, or
  fast-forward the canonical branch without acceptance. ADR-0018's deliberately exhaustive,
  no-catch-all match is what forced this classification at compile time -- `RecordPromotion` is the
  exact variant that rule was written to catch.

| Desktop launch wiring: registering with a staged scope, materializing before the PTY starts, and using `launch_working_directory` as the pane cwd | the desktop track (`impulse-desktop/**`) |
| Durable producer reservation around `fast_forward_canonical_branch` | the producer-reservation-journal lane |
| Materializing `ReadOnlySnapshot` and `DisposableScratch` | future work; registration refuses them today |
| A promotion reservation covering the crash window between the compare-and-swap and its receipt | producer-reservation-journal lane |
| OS-level sandboxing, egress allowlists, container runtimes | explicitly deferred by the staging plan |

## Research

- The 2026-08-22 sandbox analysis for background agents: filesystem snapshot as the highest-value
  missing capability, and the ROSA world-scope vocabulary this ADR adopts.
- ADR-0017 rule 6 and the `LoopContract` types, which were written to bound governed Builder
  iterations next.
- E2B's Firecracker/reflink filesystem forks and Codex CLI's `workspace-write` mode as precedent
  for staging mutation away from the authoritative tree.
- ADR-0011's revisioned ledger and ADR-0012's detached verification worktree, whose `git worktree
  add --detach` path this ADR shares rather than duplicates.

## Post-merge fixes (2026-09-12)

PR #50 merged with four unresolved P1 review threads, all four confirmed against a real repository
before being fixed (Git 2.50.1 here; the review used 2.43, and both reproduce). The rules above are
written as amended. What was wrong, and what closed it:

- **Staged claims could never be verified.** Review round 1 moved `derive_claim` onto the staged
  worktree but left `run_verification` observing `task.workspace_root` and requiring that canonical
  HEAD to equal the claim. The canonical checkout deliberately stays on the initial OID until
  promotion, so every staged run failed that check *before* its detached verifier was created, and
  no staged run could reach Supervisor review or operator acceptance. Both producers now take the
  source tree from `launch_working_directory`, and the detached verifier is created from the staged
  worktree's own commit — still detached, still hook-free. The canonical tree is neither read nor
  written by a staged verification, which the integration-grade test asserts by comparing the
  canonical head and source bytes before and after a real `rust_workspace_v1` run.

- **Promotion ran Git before it compared the pin.** `promote_governed_outcome` observed the staged
  worktree and the canonical tree first and compared shared configuration afterwards. Both
  observations run `git status`, and a `core.fsmonitor` set to a pathname is a hook Git executes
  there — `core.hooksPath` does not disable it. A Builder writes that key from inside its own
  checkout, because `.git/config` is shared with every linked worktree, so the Builder's program
  ran under operator authority before the gate that was supposed to refuse the run said anything.
  The comparison now happens first, and it is implementable as "first" only because it no longer
  needs Git at all (below). `core.fsmonitor=false` was also added to the one hook-free Git
  constructor, for the materialization path where pre-existing operator configuration is legitimately
  in force and there is no pin to compare yet; it is belt and braces, and this ADR should not be
  rewritten to lean on it.

- **The digest erased ordering.** It hashed a *sorted* `git config --local --list` rendering. Git
  resolves a repeated scalar key to the **last** value, so a Builder could swap two existing
  `filter.<name>.smudge` lines — adding nothing, editing nothing — and arm a different driver while
  producing an identical digest. The same was true of ordered includes. The digest is now the raw
  bytes of each pinned file plus the raw bytes of every file reachable through its
  `include`/`includeIf` paths, which is what rule 13 always claimed. Because nothing is rendered,
  nothing runs: the pin is computed and compared with filesystem reads, which is also what makes
  the ordering fix above possible. A pin recorded under the superseded scheme is not comparable
  with a raw-bytes one, so `SharedRepositoryConfigDigest` carries a `scheme_version` and promotion
  refuses a foreign one with `repository_config_unpinned` — discardable, per rule 7 — rather than
  comparing two different questions. The version field defaults to the legacy value and is omitted
  from serialization when legacy, so an existing record re-serializes to the same bytes and its
  receipt fingerprint still matches: the round-3 lesson about failing a whole ledger closed applies
  to this field too.

- **`MarkRunning` did not require staging.** It checked only `execution_state`, so a
  `staged_authoritative` task could be marked running with no staged worktree — and
  `launch_working_directory` then returned the canonical workspace, handing the Builder
  authoritative filesystem access under a record that says the opposite. The mutation now refuses
  that transition, and `launch_working_directory` returns a typed error for a staged task with no
  active worktree instead of falling back. Only the staged scope is affected: `authoritative`
  tasks, which every pre-ADR-0019 ledger replays as, are untouched, and a regression test replays
  one to prove it.

Two things worth carrying forward. First, the third and second findings are the same mistake seen
twice: asking Git a question is not a neutral act inside a repository whose configuration is the
thing under suspicion. Reading bytes is. Second, every one of these was a gap between what the ADR
*said* and what the code did — rule 13 promised raw bytes, rule 4 promised the Builder's cwd — which
is an argument for reviewing prose against implementation, not only implementation against itself.

### Review round 1 on the fixes (PR #53)

An adversarial review of the fixes above confirmed all five claims and the non-vacuity of every
test, and found four more defects plus two residuals. All four are fixed; the rules above are
written as amended.

- **The staged worktree's own `config.worktree` was unpinned.** The pin was computed against the
  canonical workspace only, so it covered `<repo>/.git/config.worktree` and the common-directory
  copy but never `<common>/worktrees/<task id>/config.worktree` — which is the file Git actually
  reads for commands run in the staged checkout, and the first thing every producer does there is
  `git status`. Reachable whenever `extensions.worktreeConfig` is already on, which
  `git sparse-checkout` in a linked worktree does for ordinary reasons; `.git/config` then stays
  byte-identical. Reproduced end to end. Rule 13 is rewritten: the pin is recorded after the staged
  worktree exists and covers all three copies, and the old "enabling the extension would be caught"
  argument is retracted in place.

- **Only promotion compared the pin.** `derive_claim` and `run_verification` spawned Git against the
  staged tree with no comparison at all, so a `filter.<name>.clean` in the shared `.git/config`
  plus a committed `.gitattributes` executed during `governed-claim` — the earliest producer, before
  an operator sees anything. Both now compare first and refuse with a typed `StagedConfigRefusal`.
  Note the failure mode the regression had to be written around: without the gate the producer
  *succeeds*, because a passthrough `clean` filter leaves the tree reading clean. The marker file,
  not the error, is what proves nothing ran.

- **Global and system configuration were in force.** The env scrub keeps `HOME` and nothing set
  `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`, so a driver in the operator's own `~/.gitconfig` would
  execute inside a producer — a file the repository pin does not and cannot cover. Both are now
  suppressed for every producer invocation. The test harness had been setting them all along, which
  is how the gap stayed invisible: a harness that is safer than production hides the thing it is
  meant to test.

- **The `MarkRunning` precondition was enforced during replay.** A #50-era ledger can legitimately
  hold a staged task that reached `running` with no materialized worktree — that *is* the bug — and
  re-judging it under the new rule made `GovernedTaskLedger::load` fail on that task, taking every
  unrelated task in the file down with it. `MutationContext` now carries `is_replay`, and
  transition *rejections* are live-only. This is the general rule, not a special case: replay
  reproduces decisions, it does not re-authorize them. Clock-derived *computations* (rule 11) are
  the opposite case and are still recomputed.

**Residuals, accepted for now and named so they are not rediscovered as findings.** The include walk
does not resolve `~user/` — Git does, but it needs a passwd lookup the standard library does not
offer; `~/` and backslash line continuation are both handled. And a repository using Git's
`reftable` backend cannot have its HEAD read without Git, so a promotion blocked on configuration
returns an error instead of the typed `PromotionBlocked` record; it still fails closed and still
names the component that changed, but the operator does not get a recorded outcome. Both are
narrow, both fail safe, and both are cheaper to fix once something needs them than to guess at now.

### Review round 2 on the fixes (PR #53)

Three P3s and two wording corrections; the branch freezes after them.

- **Submodule configuration was outside the pinned set** while rule 13 asserted that every
  repository-level configuration file Git reads is pinned. Verified reachable against Git 2.50.1.
  Resolved by refusing submodule repositories outright rather than by extending the walk — see rule
  13. Refusing is the smaller and the more honest of the two, because the alternative replaces a
  fixed file list with a directory walk and quietly weakens what the rule can promise.

- **The pin's scheme version was not bumped when the covered file set changed.** Round 1 added the
  staged worktree's files and changed the attributes digest's domain separator but left the version
  at 2, so a pin written by the immediately preceding commit was *compared* — producing a
  `repository_config_changed` naming a component nothing had touched — instead of being refused as
  uncomparable. Now 3, with the constant's own documentation saying to bump it whenever the covered
  file set changes, not only when the algorithm does. A misleading difference is worse than an
  honest refusal: it sends an operator looking for a change that never happened.

- **A comment line ending in a backslash swallowed the line after it.** Git's continuation lives
  inside value parsing, not at the line level, so `# hidden \` leaves a following `[include]`
  header in force — and the line-level join hid that header from the include walk, leaving its
  target unpinned. Comment lines now never continue, and never absorb a continuation in progress.

- **Scope of the Git-invocation hardening, stated precisely.** "Every producer Git invocation"
  means every daemon-owned producer in `governed_producers.rs`. The Dioxus governed-launch preflight
  (`impulse-desktop/src/runtime.rs`, `run_bounded_governed_git`) is a separate code path and is
  **not** hook-free: it sets no `core.hooksPath`, no `core.fsmonitor`, and no `GIT_CONFIG_*`
  overrides, and its env scrub keeps `HOME`. It runs before a staged worktree exists, in the
  operator's own checkout, so it is not this ADR's trust boundary — but it is a Git invocation on a
  governed path with none of these protections, and it belongs to the desktop track. Recorded here
  so it is not mistaken for covered.

## Review round 1

An adversarial review of the implementing PR confirmed the ADR's claims about world scope,
determinism, honest capability reporting, and the canonical tree staying byte-identical through
`awaiting_operator`, and found six defects. All six are fixed in the same PR; the rules above are
written as amended. What changed, and why each mattered:

- **Promotion could "succeed" on a detached canonical HEAD.** `git merge --ff-only` moves whatever
  HEAD is, so on a detached checkout the post-check passed, the ledger recorded `Promoted`, the
  staged worktree was then discarded, and the next `git switch` orphaned the work — a silent loss
  of an accepted outcome. Rule 5 now requires HEAD to resolve to a branch, and rule 6 reports
  `detached_head` as a blocked outcome rather than an error.
- **The move was not a compare-and-swap.** `--ff-only` re-reads HEAD and then writes. Rule 5 now
  uses `git update-ref <branch> <accepted> <initial>`, which writes only against the expected old
  value, and reports a lost swap as `concurrent_branch_update`.
- **The loop-trip path leaked its own worktree.** `escalated` is terminal and was not a discardable
  state, so the one terminal state this ADR's loop contract produces could never reclaim its
  checkout. Rule 7 now enumerates every legitimate reclaim state, including `launch_failed` and a
  blocked promotion.
- **The loop digest had no version pin.** Replay recomputed it with the running build's constants
  and failed the ledger closed on mismatch, so revising the budget — which this ADR says will
  happen — would have made existing ledgers unloadable. Rule 10 versions the evidence and replays
  foreign versions verbatim.
- **`derive_claim` read the canonical workspace, not the staged root**, so a real staged Builder's
  claim would have carried the initial OID and every promotion would have bailed. It now observes
  `launch_working_directory()`, which is unchanged for every non-staged task.
- **Producer Git invocations ran the project's hooks.** See the Consequences note above.

The digest is now composed over `loop_contract::canonical_json` rather than `serde_json::to_vec`,
so key ordering can never affect it.

## Review round 3

One HIGH finding, on a field round 2 itself introduced: `shared_config_digest` was non-`Option`,
non-defaulted, and its type had no `Default`, so a staged worktree recorded before the pin existed
would fail to deserialize with `missing field` — and `GovernedTaskLedger::load` propagates that,
taking **every** task in the ledger down with it. The round-2 legacy test only stripped
`staged_worktree` wholesale, so the case was uncovered.

The fix is a typed `SharedRepositoryConfigPin::{Recorded, Unknown}` defaulting to `Unknown`, not an
empty digest. `Unknown` states what is true — the comparison cannot be made — and carries its own
consequences: promotion blocks with `repository_config_unpinned`, and
`staged_worktree_is_discardable` always allows discard in that state, so the operator has a way
forward instead of a worktree that can neither promote nor be reclaimed.

The regression fixture is worth noting: stripping the field from the stored record alone made the
ledger fail on a *receipt fingerprint* mismatch instead, which would have proven nothing about the
pin. A genuine pre-pin ledger has both a record without the field and a receipt fingerprint
computed without it, so the test recomputes the fingerprint the old shape would have had.

## Review round 2

The ADR-0018 lane, reading round 1's `core.hooksPath` fix, pointed out that hooks are only one
instance of worktree-shared state and asked whether `.git/config` and `.git/info/attributes` were
reachable the same way. They are. Verified against a real repository: with `core.hooksPath`
disabled exactly as the producers set it, a `filter.<name>.smudge` defined in the shared
`.git/config` and assigned by an in-tree `.gitattributes` still executes on checkout — and a staged
Builder can write that config from inside its own worktree, because `git config --local` resolves
to the shared file for every linked worktree. Rule 13 and its Consequences note are the fix. The
regression test carries a negative control that fires the planted driver in the staged worktree
first, so the assertion that promotion does not run it cannot pass vacuously.

The ADR-0018 lane also pushed back on how rule 13 was argued, and was right to: an early draft
leaned on "no Git switch suppresses repository config" as though the enumeration were the reason
the gate is safe. It is not, and the paragraph above now says so explicitly. The gate detects
change; the enumeration is supporting evidence that can be disproved without touching the result.
The claim was also imprecise — `-c` *can* override a key you are able to name; what no one can do
is enumerate names the attacker chooses.

Round 2 verification also found round 1's hook fix **incomplete**: it covered the two obviously
mutating commands but not `run_git`, which backs every observation the module makes, including the
`git status` that promotion runs twice. A `post-index-change` hook executed there. Every Git
invocation in the module now goes through one hook-free constructor, and the regression plants
`post-index-change`, `pre-auto-gc`, and `fsmonitor-watchman` alongside the original three. Reverting
the constructor makes that test fail at materialization, so it genuinely covers the gap.

The two findings are one boundary seen through two doors, and ADR-0018 states the other half: a
capability proves who opened a connection and nothing about the integrity of the work that
connection's request then acts on. ADR-0018's promotion gate establishes the first half; this rule
establishes the second.
