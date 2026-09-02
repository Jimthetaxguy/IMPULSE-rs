---
title: "ADR-0017: Canonical Loop Contract"
description: Typed loop budgets, layered stop conditions, and termination evidence for Impulse-owned loops
status: review
created: 2026-09-01
updated: 2026-09-01
type: decision
category: architecture
phase: all
audience: builders
deciders: [Impulse Maintainers]
tags: [adr, loop, ion, governance, evidence]
---

# ADR-0017: Canonical Loop Contract

## Status

Proposed and implemented on lane `claude/loop-contract-20260901`; accepted on merge.
Provenance: the unfiled 2026-08-09 loop-discipline draft in `_working-files`, reduced to the
slice that can be proven in code today.

## Context

ADR-0011 through ADR-0013 govern *runs*: registration, four separated attestations, detached
verification, operator-required acceptance, and review-only memory candidates. None of them
govern the *loops* inside a run. Loop safety existed only as scattered per-call-site constants:
the Ion tool loop capped rounds (`DEFAULT_MAX_TOOL_ROUNDS`) and wall-clock time
(`DEFAULT_TOOL_LOOP_TIMEOUT`), and the harness subprocess query had its own timeout. Two failure
modes those constants do not catch are the ones that actually waste budget in practice:

- the model re-issues the exact same tool call round after round, and
- the same tool fails the same way round after round while the model keeps retrying.

Both burn every remaining round and the whole wall-clock budget before anything stops them, and
when they do stop, the only evidence is a bare error variant. Nothing records how many rounds ran,
how many tool calls executed, or why the loop ended. That evidence is exactly what a future
harness-diagnosis loop (the drafted ADR-0016, following the AutoSaddler finding that
trace-grounded diagnosis beats shallow reflection) needs as input, and what an operator surface
needs to explain a stalled agent.

Circuit-breaker termination with layered stop conditions (iteration cap, budget cap, no-progress
detection) is the established pattern for agent loops. Impulse adopts the pattern as a typed
primitive rather than as more constants.

## Decision

1. **Every Impulse-owned loop runs under a declared `LoopContract`.** A contract is a stable name
   plus a `LoopBudget`: `max_rounds` and `wall_clock` (hard caps, always enforced) and two
   optional no-progress detectors, `max_repeated_call_streak` and `max_same_error_streak`. A
   contract that could never run (zero rounds, zero wall clock, a zero streak limit) fails
   validation and is rejected before the loop starts. The stored contract is only replaceable
   through a validating setter, and the *effective* contract, after any per-call round or
   wall-clock override, is validated again at the execution boundary.
2. **A `LoopBreaker` evaluates every trip condition on every round.** The breaker admits each
   model round (`begin_round`) and observes each executed tool call (`observe_call`). A repeated
   call is the same tool name with structurally equal input, compared through key-order-independent
   canonical JSON. A same-error streak is consecutive error results from the same tool whose
   trimmed first line matches. A different call resets the first streak; a non-error result or a
   different error resets the second. An error signature is the first line of the result that
   carries a letter or digit, so a pretty-printed JSON failure payload (whose first line is `{`)
   is keyed by its first field rather than matching every other failure. The breaker also closes
   each round: the same set of calls requested in `max_repeated_call_streak` consecutive rounds
   trips `RepeatedRound`, which catches a batch such as `[read a, read b]` re-issued every round
   that the per-call comparison cannot see. A trip stops the loop immediately, including the
   remaining calls of a batched tool-use response; the caller must not continue.
3. **Termination is typed evidence.** Every run leaves a `LoopReport`: contract name,
   `LoopTermination` (`Completed`, `Tripped { trip }` with a `LoopTrip` of `RoundCap`,
   `WallClock` (milliseconds), `RepeatedCall`, `RepeatedRound`, or `SameError`, or
   `Failed { error }` when a model round itself failed), rounds used, tool calls that completed,
   tool calls that were dispatched but interrupted by the wall clock, tool errors, and elapsed
   time. A run clears the previous report when it starts, so a reader never sees stale evidence
   describing a later turn.
   Reports are serde round-trippable and carry no verdict: a trip is an execution fact, not
   a rejection, not a failed claim, and not a verification outcome. This extends ADR-0011's
   "process exit is not acceptance" with "a trip is not review".
4. **The contract owns the defaults.** `DEFAULT_MAX_TOOL_ROUNDS` and `DEFAULT_TOOL_LOOP_TIMEOUT`
   are defined from `LoopContract::ion_tool_loop()` so a constant and the contract can never
   disagree. Existing error variants stay for callers that match on them; the two new trips
   surface as `AgentError::ToolLoopStalled { trip }`.
5. **Wall-clock enforcement stays with the caller.** `tokio::time::timeout` still bounds the
   whole exchange; the breaker records the `WallClock` trip so the report is complete. Full
   mid-`.await` interruptibility remains out of scope, as before.
6. **The module depends on no provider, tool, or daemon type.** The same contract will bound
   governed Builder iterations and future scheduled or autonomous runs without a second breaker.

## Consequences

- Ion's REPL now stops a stalled model after three identical calls, three identical batches, or
  three identical failures instead of spending the full ten rounds, and
  `ChatState::last_loop_report` exposes why.
- A user who declines a gated tool three times in one turn ends that turn: declines are error
  results with one fixed signature, so the same-error detector trips and the REPL prints the
  reason. This is intended. A human saying no three times is a stronger stop signal than any
  budget, and the next prompt starts clean.
- A tool call cut off by the wall clock while running is reported as interrupted, not as never
  having happened. Whether that call's side effects landed is unknowable to the loop; the report
  says so instead of implying no tool ran.
- Callers of `chat_with_tools` that matched on `ToolLoopLimitExceeded` or `ToolLoopTimedOut` are
  unchanged; callers that render errors generically already show the new trip's `Display`.
- The report is the first typed loop-evidence record. Persisting it beside governed-task
  evidence, attaching it to `GovernedTaskEvent` chains, and feeding it to a harness-diagnosis
  producer are follow-on work, sequenced after ADR-0016 is accepted.
- Not adopted here: heartbeat liveness, checkpoint/replay of loop state, HALF_OPEN automatic
  probes, decision-trace records, and external-trigger intake. Each stays with the 2026-08-09
  draft until it can be proven in code; external triggers remain blocked behind socket
  peer-credential authorization.
- The harness subprocess timeout (`DEFAULT_HARNESS_TIMEOUT`) is not yet expressed as a contract.
  Moving it is mechanical and should happen when that path grows a second stop condition.

## Verification

This decision is represented when tests prove:

1. every contract type round-trips through serde;
2. invalid budgets are rejected with a typed error;
3. the breaker trips on round cap, repeated identical calls (key order ignored), and same-error
   streaks, and resets exactly as rule 2 states;
4. `Agent::chat_with_tools` surfaces each trip as the documented `AgentError`, leaves history
   untouched, stops a batched tool-use response at the tripping call, records a `LoopReport` on
   completion, cap, timeout, stall, and provider failure, and rejects an invalid effective
   contract before any model round runs;
5. `DEFAULT_MAX_TOOL_ROUNDS` and `DEFAULT_TOOL_LOOP_TIMEOUT` equal the Ion contract's budget.

Source of truth: `impulse-rs/src/loop_contract.rs`, `impulse-rs/src/llm_backends/mod.rs`,
`impulse-rs/src/error.rs`.

## Related Documents

- [`0011-governed-task-run-lifecycle.md`](0011-governed-task-run-lifecycle.md)
- [`0012-daemon-owned-governed-runtime-producers.md`](0012-daemon-owned-governed-runtime-producers.md)
- [`0015-harness-owned-step-model.md`](0015-harness-owned-step-model.md)
- `docs/superpowers/specs/2026-09-01-loop-contract-design.md`
- `docs/plans/worktrees/2026-09-01-claude-loop-contract.md`
