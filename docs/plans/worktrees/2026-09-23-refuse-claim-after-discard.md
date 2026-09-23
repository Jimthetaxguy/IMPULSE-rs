---
title: Refuse SubmitClaim after a discarded staged worktree
description: Work card for refuse-claim-after-discard
updated: 2026-09-23
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff]
---

# Refuse SubmitClaim after a discarded staged worktree

## Lane Facts
- Owner: cloud agent
- Role: implementer
- Branch: `cursor/refuse-claim-after-discard-5fa1`
- Worktree: `/workspace`
- Owned paths: `impulse-rs/src/state/governed_task.rs`, `impulse-rs/src/daemon/handlers.rs`, this card
- Blocked/shared paths: `decide_step_model`, Ion expand, NanoMachine, discard predicates, review-state transitions
- Plan/spec: ADR-0019 follow-on after #67 (`fbb9c10`)
- Verification: `cargo +stable test --locked` for the governed-task ledger test and the claim preflight
- Latest status: implementing the thin gate

## Decisions
- 2026-09-23: staged-scope `SubmitClaim` requires an active staged worktree. Review stays `AwaitingClaim` after a mid-run cancel. Accepted-without-promotion discard stays as it is.

## Changes
- Ledger `SubmitClaim` refuses when `world_scope` requires a staged worktree and none is active.
- `preflight_claim` refuses with the same condition before Git runs.

## Tests
- `test_submit_claim_refuses_after_discarded_staged_worktree`
- `test_preflight_claim_refuses_staged_scope_without_an_active_worktree`

## Handoff
- Open item: Supervisor review may still be requested while review is `AwaitingClaim` after cancel. That is a separate, stronger follow-on.
