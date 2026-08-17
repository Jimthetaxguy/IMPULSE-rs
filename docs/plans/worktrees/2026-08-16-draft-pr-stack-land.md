---
title: Draft PR stack land (#24 → #25 → #26)
description: Work card for landing the kernel/LLM draft stack off stowaway commits
updated: 2026-08-17
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, base-url, stale-basis, settlement, adr-0014]
---

# Draft PR stack land (#24 → #25 → #26)

## Lane Facts
- Owner: Cursor
- Role: implementer
- Branch: `agent/base-url-override-20260809` (stack base); `agent/stale-basis-20260810`; `agent/settlement-record-20260810`
- Worktree: `.worktrees/base-url-override` (this card); `.worktrees/stale-basis`; `.worktrees/settlement-record`
- Owned paths: `impulse-rs/src/llm_backends/anthropic.rs`; `impulse-rs/src/basis.rs`; `impulse-rs/src/settlement.rs`; `impulse-rs/src/lib.rs` (`pub mod basis` / `pub mod settlement` only); `docs/decisions/0014-work-item-and-comparative-settlement.md`; `docs/decisions/README.md` (insert 0014 beside 0015); env-table rows in `CLAUDE.md`; this work card
- Blocked/shared paths: `Cargo.toml`; `Cargo.lock`; `AGENTS.md`; `CONTEXT.md`; `docs/validate_docs.py`; protocol/spec docs; `GovernedTaskRun` / daemon IPC; `.github/workflows/openwiki-update.yml`; `codex/mode-taxonomy-cleanup`; PR #17
- Plan/spec: land drafts #24 → #25 → #26 onto `main` `12f957c` (ADR-0015 already merged). Drop stowaway commits. Keep ADR-0014 numbering.
- Verification: isolated `CARGO_TARGET_DIR`; `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
- Latest status: #24 rebased onto `origin/main` from cherry-pick `96455f2`; override origin is logged; OpenWiki/mode-taxonomy stowaways dropped

## Decisions
- 2026-08-17: Do not merge #24 as-is. Cherry-pick only `96455f2`; drop `01a4eb1` (mode-taxonomy) and `a024166` (OpenWiki cron).
- 2026-08-17: ADR-0014 stays 0014. Insert its README row above the existing ADR-0015 row. Do not retitle to 0016.
- 2026-08-17: Never take the old stack `lib.rs` wholesale (it deletes `pub mod voice`). Add `basis` then `settlement` onto current main.
- 2026-08-17: PR #17 is a later docs lane. Do not force-push or merge it in this train.
- 2026-08-17: Hash whole ledger files for basis freshness; do not treat a 20-line tail as the version handle.

## Changes
- Pending: rebase + fail-closed fixes, then merge #24 → #25 → #26.

## Tests
- Pending: isolated workspace gate on the stack head.

## Handoff Notes
- Primary checkout on `main` is not this lane's edit tree.
- OpenWiki workflow and mode-taxonomy docs stay on `codex/mode-taxonomy-cleanup` for a later review (pin package, least-privilege).
