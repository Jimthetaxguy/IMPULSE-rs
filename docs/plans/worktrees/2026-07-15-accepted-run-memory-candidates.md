---
title: Accepted Run Memory Candidates
description: Work card for accepted-run-memory-candidates
updated: 2026-07-15
type: doc
category: planning
phase: all
status: complete
audience: builders
tags: [worktree, lane, memory, governed-task, verification]
---

# Accepted Run Memory Candidates

## Lane Facts
- Owner: Codex root
- Role: implementation and integration owner; bounded documentation and audit subagents may edit
  only their explicitly assigned paths
- Branch: `codex/accepted-run-memory-candidates`
- Worktree: `/Users/jamespustorino/code/IMPULSE-rs/.worktrees/accepted-run-memory-candidates`
- Owned paths: accepted-run memory-candidate contract/state/daemon tests; this work card; narrowly related contract documentation
- Blocked/shared paths: root checkout dirty docs (`AGENTS.md`, `CLAUDE.md`, `CONTEXT.md`), OpenWiki
  backup/workflow files, legacy-UI PR #17, and every other existing worktree. This isolated lane
  updates its own `CONTEXT.md` contract while leaving the root checkout bytes untouched.
- Plan/spec: derive a review-only durable candidate from an accepted governed task without mutating `GENOME.md`
- Verification: focused candidate/state/daemon tests, `cargo fmt --all -- --check`, workspace check, workspace tests, strict workspace Clippy, docs contract validation, diff hygiene, and staged leak scan
- Latest status: candidate contract/state/UI implementation and aligned documentation are verified
  from `origin/main@305eee2`; the lane is ready for commit and PR review

## Decisions
- 2026-07-15: Candidate generation must be daemon-derived from accepted governed evidence and must not silently promote worker claims into curated memory.
- 2026-07-15: Reuse the repository's existing atomic state/persistence and review-artifact patterns before introducing another storage subsystem.
- 2026-07-15: Keep `GOVERNED_TASKS.json` authoritative and persist candidates separately in
  owner-only `MEMORY_CANDIDATES.json` as a deterministic materialized view. Acceptance replay and
  daemon startup repair absence; orphaned or source-mismatched candidates fail closed.
- 2026-07-15: V1 is `pending_review` only. It has no promotion/dismissal request and never mutates
  `GENOME.md` or `HISTORY.jsonl`.
- 2026-07-15: Accepted/rejected decisions are terminal. Candidate determinism hashes exact JSON
  bytes from a fixed ordered/versioned source struct; it does not semantically normalize Unicode.
  Separate private-file replacements plus reconciliation are not a cross-file transaction or a
  claim of parent-directory-fsync durability.

## Changes
- Added the versioned accepted-run candidate contract with deterministic IDs, source assurance,
  bounded provenance, and structural exclusion of worker/Supervisor/operator rationale text.
- Added the separate owner-only candidate ledger, acceptance-replay/startup reconciliation, and
  fail-closed source validation.
- Added protocol-v6 snapshot exposure and read-only Dioxus Memory rendering with an explicit
  pending/not-in-GENOME boundary.
- Added ADR-0013 and aligned README, vision, glossary, canonical contract, IPC protocol,
  architecture map, test traceability, decision/index navigation, and `impulse-ops` contract docs.

## Tests
- Candidate contract/state/replay/restart/rejection/non-mutation, snapshot compatibility, daemon,
  and Dioxus SSR coverage pass.
- `cargo fmt --all -- --check` passes.
- `CARGO_TARGET_DIR=/tmp/impulse-accepted-memory-target CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo check --workspace --locked` passes.
- `cargo test --workspace --locked` passes, including 1,595 core tests with 5 ignored plus the
  workspace integration, process, desktop, and terminal suites.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passes.
- `cargo check -p impulse-desktop --features desktop-app --locked` passes; the existing future-
  incompatibility advisory for transitive dependency `block v0.1.6` remains non-blocking.
- `python3 docs/validate_docs.py --all` passes with 142/142 metadata records and contract validation.
- `git diff --check` passes.
- Independent MiniMax adversarial review returns `PASS` after checking determinism, terminal
  decisions, repair, provenance boundaries, protocol compatibility, and read-only UI behavior.
- Staged diff hygiene passes, and Gitleaks 8.30.1 reports no leaks in the staged change.

## Handoff Notes
- The concurrent root documentation lane remains untouched and must be reconciled separately.
- The next product forcing function is one launched Builder plus Supervisor process proof through
  acceptance and exactly one staged candidate. Explicit candidate promotion/dismissal remains a
  later memory-writing contract.
