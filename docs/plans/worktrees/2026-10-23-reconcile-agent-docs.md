---
title: Reconcile agent docs
description: Work card for reconcile-agent-docs
updated: 2026-10-23
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff]
---

# Reconcile agent docs

## Lane Facts
- Owner: cloud agent
- Role: docs-only reconciliation of AGENTS.md and CLAUDE.md
- Branch: cursor/reconcile-agent-docs-e0c4
- Worktree: /workspace
- Owned paths: `AGENTS.md`, `CLAUDE.md`, `docs/guides/COLLABORATIVE-AGENTIC-CODING.md` (gate pointer), `docs/spec/RUST-CANONICAL-CONTRACT.md` (ownership line and the retired density table), `.claude/skills/impulse-development/SKILL.md` (one broken section reference), this work card
- Blocked/shared paths: application code, CI, deploy config, secrets, `decide_step_model`, provider implementation
- Plan/spec: agent-docs audit 2026-10-23, re-verified against this checkout
- Verification: `python3 docs/validate_docs.py --contract` and `--all`; marker check; source recount of `#[test]` attributes. Cargo workspace gate not run here because the image rustc is 1.83.0 and `impulse-rs` `rust-version` is 1.96.
- Latest status: docs updated on this branch. Draft PR only.

## Decisions
- 2026-10-23: `AGENTS.md` is the only project-context body. `CLAUDE.md` is `@AGENTS.md` plus Claude Code notes and the identity lines `docs/validate_docs.py` still requires in both files.
- 2026-10-23: One test-density table, source-counted on 2026-10-23. The contract points at it.
- 2026-10-23: egui wording follows `docs/plans/EGUI-DECOMMISSION.md`: frozen, compile-maintenance only, deletion waits on Track A / R1, not on Dioxus parity.
- 2026-10-23: Default features are `office-support` and `photon-subagent`. `monty-support` and `datafusion-support` are opt-in.

## Changes
- Moved unique CLAUDE.md guidance into AGENTS.md and corrected it against the tree.
- Shrunk CLAUDE.md to the pointer, the validator identity lines, and Claude-only notes.
- Pointed the collaboration guide and the canonical contract at the AGENTS.md gate, including an isolated `CARGO_TARGET_DIR`.
- Repointed the skill's guardrail row from a CLAUDE.md section that does not exist to AGENTS.md (Guardrails).

## Tests
- Docs validator and the source recount recorded in the PR. No application tests added.
