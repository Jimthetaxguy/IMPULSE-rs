---
title: "ADR-0014: WorkItem Identity and Comparative Settlement"
status: proposed
created: 2026-08-10
deciders: [Impulse Maintainers]
---

# ADR-0014: WorkItem Identity and Comparative Settlement

## Status

Proposed.

## Context

Three predicates landed in `impulse-rs` during 2026-08, each written as a self-contained plain type
with validation tests and each deferring the same dependency. The stale-basis predicate
(`impulse-rs/src/basis.rs`) says so directly at line 16: it "does not depend on the WorkItem/WorkGraph
types, whose ADR is not yet ratified." The reversibility taxonomy needs a unit to attach an effect
class to. The comparative settlement record needs a unit that acceptance checks belong to and that
candidates fan out from. All three name the same missing noun.

ADR-0011 gave the daemon a governed task-run record, and its shape is deliberately singular: one
registration, one execution, one claim, one verification, one supervisor verdict, one operator
decision. That shape is correct for what it governs. It has no place to put a check that several
runs share, no place to put an effect class that bounds whether fanning out is permitted at all, and
no way to express that three runs were alternatives to each other rather than three unrelated tasks.
Adding those concepts to `GovernedTaskRun` would change what a governed run means.

Meanwhile the N-candidate shape is already in use, informally. The judge-panel pattern in the
workflow playbook, the three-builder execution wave, and adversarial-verify votes are all fan-outs
whose settlement is a human or a synthesis agent reading diffs and writing a paragraph in a ledger.
The comparison happens; it is simply not a checkable artifact. The question this ADR answers is where
the missing noun lives and what it is allowed to touch.

## Decision

1. **A WorkItem is a planning identity, not a second kind of task.**
   It names a unit of intended work before any execution exists: what is to be done, what would count
   as done, and what class of effects doing it may have. It has no execution state, no PTY, no
   runtime, and no review queue. Nothing observes a WorkItem running, because a WorkItem does not run
   — its candidates do.

2. **Acceptance checks belong to the WorkItem; candidates inherit them unchanged.**
   A check defined once at the WorkItem is the same check for every candidate, which is what makes a
   check matrix a comparison rather than a collection of unrelated verdicts. Check outcomes use the
   three-way vocabulary already established by `GovernedVerificationOutcome`: passed, failed, or
   inconclusive. A check that could not reach a verdict is inconclusive and says so; it is never
   recorded as a pass because nothing objected.

3. **A WorkItem is a new planning-identity type; it does not modify `GovernedTaskRun`.**
   The WorkItem names the unit a basis is captured against, an effect class bounds, and candidates
   fan out from. A `GovernedTaskRegistration` MAY carry an optional `work_item` reference, which is
   additive and defaulted. `GovernedTaskRun` remains exactly one candidate's execution, and every
   guarantee ADR-0011 makes about it stands unrestated here. The rejected alternative was to add the
   WorkItem's fields — acceptance checks, effect class, candidate set — directly to
   `GovernedTaskRegistration`. That was considered and rejected because it couples planning identity
   to the wire type and forces a protocol bump before the concept has proven itself. A separate type
   can be revised, narrowed, or abandoned without a compatibility story; a registration field cannot.

4. **Fan-out is bounded to Pure and Reversible effect classes, and worse classes fail at planning
   time.**
   N candidates each performing Compensatable effects means N compensations for N-1 discarded
   results, and Irreversible effects cannot be discarded at all. Fan-out and Irreversible effects are
   mutually exclusive by construction, and the refusal happens when the fan-out is authorized — not
   at settlement, where the effects have already happened and a refusal would be a report.

5. **Promotion after selection is where worse-class effects happen, once.**
   The winning candidate's promotion is a single run that may carry Compensatable or Irreversible
   effects under the ordinary governed-run rules. This is the reason the fan-out bound in rule 4 is
   not a restriction on the WorkItem's ultimate capability: the WorkItem may do irreversible things,
   but only once, after the comparison has already been made against candidates that could not.

6. **Settlement over N candidates is comparative, and the comparison is recorded with four required
   parts.**
   A settlement record carries the per-candidate check matrix, a selection rationale naming the
   concrete difference between the top candidates, a graft record whenever the winner absorbs a piece
   of a loser, and preserved dissent for any losing candidate that passed every check. A record
   missing any part is not a weaker record; it is not a record. An unpopulated matrix counts as a
   missing part: a settlement whose candidates checked nothing compares nothing, and the record it
   would produce is a vote rather than a comparison. `impulse-rs/src/settlement.rs`
   enforces this in its constructor, so an invalid settlement has no value to be.

7. **A fatal check that did not pass is disqualifying, not weighable.**
   A fatal check exists to prove safety, so an inconclusive result disqualifies exactly as a failure
   does: a proof that did not conclude is not a proof, and treating it as one is the fail-open
   reading of the same evidence. A candidate that did not pass a fatal check cannot be selected
   regardless of how attractive its diff is,
   how many other checks it passed, or how strong the rationale for it would have been. Eligibility
   is evaluated before comparison and composes with basis freshness: a candidate whose basis moved
   under it is equally ineligible, because its work was planned against state that no longer holds.

8. **Losers are archived, never deleted, until the graft-bearing winner itself settles.**
   A graft names a source candidate and the piece taken; the piece is unreviewable if the source
   worktree is gone. The archive is therefore a precondition of the graft, checked against the
   filesystem when the settlement record is constructed, not a cleanup task deferred to whoever
   remembers.

9. **The three predicates compose in a fixed order.**
   Basis freshness gates eligibility per candidate. Effect class gates whether the fan-out was
   permitted at all. Settlement compares what remains after both. The order matters: comparing first
   and checking eligibility afterward produces a ranked list whose top entry may be disqualified,
   which is a comparison that has to be redone.

10. **The default is N=1, and fan-out is reserved for expensive-to-get-wrong work until cost
    attribution exists.**
    N candidates cost roughly N times the tokens and the disk of one. Whether a panel beat a single
    attempt is answerable with numbers only once per-run cost attribution lands. Until then fan-out
    is a judgment call reserved for security-adjacent changes, wide-blast-radius refactors, and
    decisions that are hard to reverse — which is the reversibility taxonomy again, now steering the
    spend rather than the safety.

11. **The schema lands before the wiring, in `impulse-rs` rather than `impulse-ops`.**
    These are kernel types with validation, not wire types: they belong beside `basis.rs` in
    `impulse-rs/src/`, self-contained and testable without a daemon. Promoting a settlement to a
    governed outcome needs daemon wiring and a `PROTOCOL_VERSION` bump — currently 6 — and that is a
    separate change with its own review. Nothing in this ADR is reachable from the IPC surface today.

## Consequences

- `basis.rs` gains the dependency it deferred: a WorkItem is the unit a basis is captured against, so
  the predicate can be wired into a real planning path instead of standing alone with its tests.
- N=1 tasks are unaffected in every respect. A single-candidate WorkItem has no comparative record to
  produce, no dissent to preserve, and no graft to check; `SettlementRecord::new` rejects N=1
  explicitly rather than accepting a degenerate comparison.
- `GovernedTaskRun` keeps its meaning and its ADR-0011 guarantees. The optional `work_item` reference
  on registration is additive; old payloads deserialize unchanged.
- Fan-out becomes a decision with a stated cost rather than a default, and the reversibility taxonomy
  acquires a second job: it steers the spend decision as well as the safety bound.

Explicitly **not** decided here:

- **IPC registration.** How a WorkItem is created, addressed, or listed over the daemon protocol,
  and what the `PROTOCOL_VERSION` bump contains, is future work.
- **Worktree lifecycle ownership.** Who creates candidate worktrees, who archives them, and when an
  archive may finally be reclaimed after the graft-bearing winner settles.
- **Cost attribution location.** Whether per-run cost lives on the governed run, the candidate
  result, or a separate telemetry record. Rule 10's default stands until it does.

## Related Documents

- [`0011-governed-task-run-lifecycle.md`](0011-governed-task-run-lifecycle.md)
- [`0012-daemon-owned-governed-runtime-producers.md`](0012-daemon-owned-governed-runtime-producers.md)
- [`../../impulse-rs/src/basis.rs`](../../impulse-rs/src/basis.rs)
- [`../../impulse-rs/src/settlement.rs`](../../impulse-rs/src/settlement.rs)
