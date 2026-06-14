---
title: Collaborative Agentic Coding Guide
description: Repo-local rules for parallel coding agents, worktree lanes, handoffs, and verification.
version: '1.0'
updated: 2026-05-21
type: guide
category: development
phase: all
status: active
audience: builders
tags: [agents, collaboration, worktrees, handoff, verification]
authors:
  - name: Impulse Maintainers
    role: Maintainer
---

# Collaborative Agentic Coding Guide

This guide defines how humans and AI coding agents work on Impulse without losing context, overwriting each other, or treating speculative plans as completed work.

The canonical product contract still wins all product disagreements: [`../spec/RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md). This guide controls collaboration mechanics.

---

## Operating Model

Impulse uses lane-scoped parallel work:

- A **lane** is one bounded workstream with a named owner, role, branch, worktree, owned paths, plan, and verification gate.
- A **lane orchestrator** decomposes and coordinates work inside that lane.
- An **integration lane** resolves cross-lane sequencing, shared files, and final verification when multiple lanes touch related behavior.
- Coding agents may run in parallel only when their owned paths do not overlap or when a handoff records the sequencing rule.

Parallel work is encouraged, but undocumented shared writes are not.

---

## Start Of Session Checklist

Before mutating code or docs, every agent must:

1. Read the current contract and relevant guide:
   - [`../spec/RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md)
   - this guide
   - the feature/spec/handoff doc for the assigned lane
2. Inspect repository state:
   - `git status --short`
   - `git branch --show-current`
   - `git worktree list`
3. Identify lane facts:
   - owner
   - role
   - branch
   - worktree path
   - owned files/directories
   - blocked or shared files/directories
   - verification commands
4. Create or update the lane work card under `docs/plans/worktrees/<date>-<lane-slug>.md`.
5. State any dirty files that are unrelated to the lane and leave them alone.

If the lane cannot be described with those facts, planning is incomplete.

---

## Spec-First Rule

Every non-trivial feature, refactor, migration, or multi-file documentation change must start from a decision-complete spec or work card.

The spec must include:

- goal and user-visible outcome
- owned paths and explicitly blocked paths
- non-goals
- public interfaces or docs that change
- acceptance criteria
- verification commands
- rollback or handoff notes when shared files are involved

Small typo fixes and single-line documentation corrections do not need a new spec, but they still require normal git and verification discipline.

---

## Worktree And Branch Rules

Use these conventions for parallel lanes:

| Concept | Rule |
|---|---|
| Worktree path | `.worktrees/<lane-slug>` |
| Branch name | `<platform>/<lane-slug>` |
| Platform prefix | `codex`, `claude`, or `human`; use `opencode` only for historical/legacy compatibility lanes |
| Work card | `docs/plans/worktrees/<date>-<lane-slug>.md` |

Shared files require explicit ownership before edits:

- `Cargo.toml`
- `Cargo.lock`
- `AGENTS.md`
- `CLAUDE.md`
- `CONTRIBUTING.md`
- `docs/INDEX.md`
- `docs/SUMMARY.md`
- `docs/SUMMARY.yaml`
- `docs/validate_docs.py`
- protocol, canonical contract, roadmap, and user-story specs

If two lanes need the same file, one lane pauses and records a handoff, or the work moves into a designated integration lane.

---

## Lane Work Card Template

Create one work card per lane:

```markdown
---
title: <Lane Title>
description: Work card for <lane-slug>
updated: YYYY-MM-DD
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff]
---

# <Lane Title>

## Lane Facts
- Owner:
- Role:
- Branch:
- Worktree:
- Owned paths:
- Blocked/shared paths:
- Plan/spec:
- Verification:
- Latest status:

## Decisions
- YYYY-MM-DD:

## Changes
- Pending:

## Tests
- Pending:

## Handoff Notes
- None yet.
```

Keep this file current while work is in progress. A stale work card is treated as missing coordination state.

---

## Documentation-As-You-Go

Agents must document meaningful work as it happens:

- Update the lane work card when scope, ownership, blockers, or verification changes.
- Add or update the relevant spec when behavior, public interfaces, roadmap status, or acceptance criteria change.
- Mark stale docs as `superseded`, `deprecated`, or `archive` before adding contradictory active docs.
- Prefer pointers to canonical docs over copying long sections.
- Record final verification results in the handoff notes.

Do not leave future agents to reconstruct intent from a diff alone.

---

## Handoff Rules

A handoff is required when:

- another lane must continue the work
- a lane needs a shared file already owned by another lane
- verification is blocked
- the task spans multiple sessions
- implementation changes the roadmap, protocol, public interface, or persistence format

Every handoff must include:

- current state
- changed files
- decisions made
- tests run and results
- tests not run and why
- known blockers
- next concrete step

---

## Verification Rules

Before claiming a lane is complete, run the verification commands named in the spec or work card.

For Rust workspace changes, the default gate is:

```bash
cd impulse-rs
cargo check --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

For documentation contract changes, run:

```bash
python3 docs/validate_docs.py --contract
python3 docs/validate_docs.py --all
```

If verification fails, record the failure and either fix it within the owned scope or hand it off with exact commands and output summary.

---

## Non-Negotiables

- Do not run `git add .` or `git add -A`; stage explicit paths only.
- Do not overwrite or revert another lane's work without explicit instruction.
- Do not delete files as cleanup; archive first and confirm destructive operations.
- Do not expand scope silently.
- Do not claim completion without verification.
- Do not add new egui features; egui is legacy/frozen until the Dioxus Desktop host reaches parity.
