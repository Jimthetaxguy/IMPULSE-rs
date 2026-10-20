---
title: ROSA and IMPULSE mode taxonomy cleanup lane
date: 2026-07-15
status: complete
owner: codex
role: documentation integrator
branch: codex/mode-taxonomy-cleanup
worktree: /Users/jamespustorino/code/IMPULSE-rs
---

# ROSA and IMPULSE Mode Taxonomy Cleanup

## Goal

Make IMPULSE's current vocabulary explicit without adding a global autonomy
mode or claiming that a shared ROSA/IMPULSE authorization type already exists.

## Owned Paths

- `CONTEXT.md`
- `docs/plans/worktrees/2026-07-15-mode-taxonomy-cleanup.md`

## Blocked And Shared Paths

- Product Rust and Dioxus source are out of scope.
- `AGENTS.md.bak-2026-07-15` and `CLAUDE.md.bak-2026-07-15` are unrelated
  concurrent artifacts and remain untouched.
- Risk classification, operator authentication, scheduling, and authority-lease
  implementation require separate decision-complete work.

## Decision

- `ExecutionPosture` describes approval cadence.
- `RunKind` describes hosting and lifecycle.
- A future `AuthorityEnvelope` would carry bounded delegated authority.
- Existing stewardship, injection, provider-route, host-surface, and Ion
  confirmation “modes” remain domain-scoped and do not grant global autonomy.
- `YOLO`, `Semi-YOLO`, and `Danger Mode` are not persisted policy terms.

## Verification

- `git diff --check`
- conflict-marker scan over the owned paths
- documentation-reference validation

Result: PASS. `docs/validate_docs.py` reported 141 valid and 0 invalid metadata
records; diff and conflict-marker checks were clean.

## Handoff

The cross-project rationale and cleanup queue live in ROSA's proposed
`architecture docs/ROSA-impulse-mode-contract.md`. IMPULSE implementation must
wait for the hierarchy/enforcement and authority-envelope decisions rather than
branching on user-facing labels.
