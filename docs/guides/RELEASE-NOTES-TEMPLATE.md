---
title: Release Notes Template
description: Template fields required for Impulse contract-impacting releases
version: '1.0'
updated: 2026-02-23
type: guide
category: guides
phase: all
status: superseded
audience: builders
tags: [release, template, governance, contract]
---

# Release Notes Template

> **Historical template — superseded.** It predates the current workspace-wide Rust, Dioxus, and
> product-contract verification gates. Release evidence must be generated from the live commands
> in [`../../AGENTS.md`](../../AGENTS.md) and the canonical Rust contract.

Use this template for all releases that affect CLI interfaces, hooks, state files, or roadmap contract statements.

## Required Fields

- **Release ID:**
- **Date:**
- **Scope:**
- **Risk Level:** low | medium | high

## Contract Impact (Required)

- **Canonical contract updated:** yes | no
- **Contract file touched:** `docs/spec/RUST-CANONICAL-CONTRACT.md`
- **CLI surface changes:** list commands and flags
- **State artifact changes:** list `.impulse/*` paths affected
- **Hook behavior changes:** Claude/OpenCode parity notes

## Compatibility

- **Breaking changes:** yes | no
- **Migration required:** yes | no
- **Fallback/deprecation path:**

## Verification Evidence (Required)

- `cargo test` result:
- `python3 docs/validate_docs.py` result:
- `python3 docs/validate_docs.py --contract` result:

## Documentation Sync Checklist (Required)

- [ ] `AGENTS.md` updated if behavior changed
- [ ] `CLAUDE.md` updated if behavior changed
- [ ] `docs/INDEX.md` updated if navigation/source-of-truth changed
- [ ] `docs/SUMMARY.md` updated if roadmap/category changed

## Notes

- Additional context, known limitations, follow-up tasks.
