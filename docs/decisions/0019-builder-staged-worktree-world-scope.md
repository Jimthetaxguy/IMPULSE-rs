---
title: "ADR-0019: Builder Staged-Worktree World Scope"
description: A declared world scope, a disposable staged worktree for the Builder, and promotion as a separate step after operator acceptance
status: review
created: 2026-09-02
updated: 2026-09-02
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
   worktree is active and the canonical workspace root otherwise, so a runtime that has not been
   taught about world scopes still lands somewhere correct.

5. **Promotion is a separate step after acceptance.** `promote_governed_outcome` is allowed only
   on an `accepted` task with an active staged worktree. It requires the staged worktree to be
   clean and sitting exactly on the accepted claim's subject revision, then fast-forwards the
   canonical branch — `git merge --ff-only` — but only while the canonical head still equals the
   registered initial OID. The entire canonical-branch side effect is isolated in one function
   (`fast_forward_canonical_branch`) so a durable producer reservation can wrap it without
   re-reading this ADR.

6. **A moved canonical head is an execution fact, not an error.** Promotion returns
   `PromotionBlocked { canonical_head }`, recorded against the already-accepted run. Review state
   stays `Accepted`; the staged worktree stays active; an operator who reconciles the canonical
   branch can retry. A run is promoted at most once: a second promotion after a successful one is
   refused, and a promoted outcome must land exactly the accepted revision.

7. **A finished staged worktree is discarded.** `discard_staged_worktree` removes the checkout and
   its administrative entry, and the matching mutation marks the record `discarded`. It is allowed
   only after a rejection or after a completed promotion, so a live Builder's work cannot be
   deleted out from under it.

8. **The compatibility preview reports `filesystem.scoped` as `mediated`, never `structural`.**
   `world_scope_filesystem_enforcement` returns `Mediated` for `StagedAuthoritative` and
   `Unsupported` for every other scope, and `evaluate_role_compatibility_in_world` merges that
   into the runtime's own declared support, taking the stronger of the two per capability. A
   staged Builder launch therefore reads as *allowed but degraded*: the canonical Builder role
   asks for structural containment, and a Git worktree honestly supplies only Git-level mediation.
   Nothing stops the Builder's process from writing outside the checkout.

9. **A staged claim is admitted under `LoopContract::governed_builder()`.** Five claim cycles, a
   four-hour wall clock measured from registration, and three consecutive verification failures
   with the same signature. The verdict is computed by a new pure
   `LoopBudget::evaluate_observed`, which reads no clock: every input comes from stored counts and
   stored timestamps. A trip records the claim as evidence, emits a `loop_tripped` event, and moves
   review state to `escalated`, which accepts no further claims. Every staged claim also carries a
   `loop_report_digest`, the SHA-256 of the canonical ADR-0017 `LoopReport` the verdict came from.
   Tasks outside the staged scope are not evaluated at all, which is what keeps every pre-ADR-0019
   ledger replaying identically.

10. **Replay is verified, not assumed.** `apply_mutation` now takes the clock as a parameter.
    Normal operation passes the current time; ledger validation passes each event's own recorded
    timestamp. Reload therefore reproduces every clock-derived decision exactly, and validation was
    tightened to prove it: the replayed event *kind* must match the stored kind, the replayed
    claim's `loop_report_digest` must match the stored digest, and the replayed staged worktree and
    promotion records must match theirs. A ledger whose loop evidence or staged root was rewritten
    fails closed on load.

11. **The accepted-run memory candidate is pinned to the accepting revision.** ADR-0013's
    projection previously digested `task.revision`, which was equivalent to the accepting revision
    only because nothing could mutate a task after acceptance. Promotion can. The projection now
    derives its `accepted_task_revision` from the operator decision's `based_on_revision + 1`. For
    every record written before this ADR the two numbers are identical, so no stored candidate
    digest changes.

## Consequences

**What this buys.** A failed verification or a declined review is now recoverable by deleting a
directory. The canonical tree is byte-identical from registration through `awaiting_operator`, and
the integration tests prove it against real repositories. A Builder that cannot make the gate green
escalates instead of churning. Operators see an honest scope claim rather than silence.

**What it does not buy.** This is a Git-level boundary and nothing more. There is no OS sandbox, no
mount namespace, no egress allowlist. A Builder process can `cd` out of its staged worktree and
write anywhere the user can. `filesystem.scoped` is reported as `mediated` precisely because
promising more would be false. OS-level sandboxing and egress control remain explicitly deferred.

**Promotion is fast-forward only.** No merge, no rebase, no force. A canonical head that moved
blocks, and a human decides. This is deliberate: an automated merge is a semantic decision, and the
governed pipeline's whole premise is that semantic decisions need an operator.

**A crash between the Git side effect and its receipt is still not covered.** Promotion inherits
the same window every other producer has. The side effect is isolated in one function so the
sibling reservation lane can wrap it; this lane does not implement reservations.

**The loop budget is a guess informed by nothing yet.** Five cycles and four hours are starting
values with no production data behind them. They are constants in `loop_contract.rs` and are meant
to be revised once real governed Builder runs exist.

**Untracked `.impulse/worktrees/` no longer marks the canonical tree dirty.** The staged worktree
lives inside the project's own runtime namespace, so the producers' cleanliness check ignores
untracked paths under it, exactly as it already ignores `GOVERNED_TASKS.json`. Tracked or staged
mutations under that path are still subject changes.

**`governed_producers` is now a public module.** The three staged producers are `pub` so an
integration test can drive them against real repositories before any daemon endpoint exists. Every
other item in the module stays `pub(crate)`.

## Not delivered by this ADR's lane

| Deferred | Owner |
|---|---|
| `PromoteGovernedOutcome` daemon endpoint, protocol bump, and CLI subcommand | the daemon / socket-provenance lane (`src/daemon/**`) |
| Desktop launch wiring: registering with a staged scope, materializing before the PTY starts, and using `launch_working_directory` as the pane cwd | the desktop track (`impulse-desktop/**`) |
| Durable producer reservation around `fast_forward_canonical_branch` | the producer-reservation-journal lane |
| Materializing `ReadOnlySnapshot` and `DisposableScratch` | future work; registration refuses them today |
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
