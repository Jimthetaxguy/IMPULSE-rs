---
title: Promotion refusal surface parity
description: Close PR 62 CLI exit-status and desktop refusal review findings
updated: 2026-09-14
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, governed, desktop, cli, review]
---

# Promotion refusal surface parity

- Owner: Codex, IMPULSE PR integration lane.
- Branch/worktree: `codex/pr62-merge-20260914`, `.worktrees/pr62-merge-20260914`.
- Owned paths: CLI refusal helper, desktop runtime/gateway/host/UI, their tests,
  IPC protocol refusal wording, glossary world-scope entry, and this card.
- Shared ownership: protocol docs within this lane only; no other IMPULSE writer is assigned.
- Outcome: JSON/text CLI refusal output survives with nonzero exit. Desktop decodes the typed
  refusal before mutation adoption, validates task/project/revision, and displays the daemon
  reason/remedy in the existing durable per-task acknowledgement notice.
- Invariants: refusal records nothing and never advances cached task state; recorded promotion
  still requires a newer revision; recorded promotion-blocked remains a recorded outcome.
- Tests: CLI subprocess with socket fixture for text/JSON and claim/verify/promote; gateway socket
  shape and malformed discriminator; runtime binding and no-event behavior; UI notice parsing/rendering.
- Verification: workspace build/check/test/clippy/fmt; desktop feature gate; minimal-feature lib
  tests after integrating PR 60. Logs are in the external PR integration receipt directory.
- Non-goals: producer policy changes, live daemon/task mutation, alternate state stores.
- Rollback: retain original commits/branches; no force push or destructive cleanup.
- Verification results: workspace build and all-target check passed; full workspace tests
  passed (3,064 passed, zero failed, nine ignored); minimal-feature library tests passed
  (2,181 passed, zero failed, five ignored); strict all-target Clippy, formatting, and
  `impulse-desktop --features desktop-app --all-targets` check passed. The final gate
  confirmed the Rust source diff remained unchanged throughout verification.
- Documentation validators retain pre-existing failures: ADR 0014 uses `status: proposed`,
  and three March guide timestamps are stale. This change does not suppress those checks
  or manufacture freshness updates.
- Status: implementation and local verification complete; remote CI and merge pending.
