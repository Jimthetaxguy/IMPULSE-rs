---
title: Architecture Decision Records
description: Decision index separating current control-plane authority from accepted historical phase-era scope
version: '1.1'
status: active
phase: all
type: reference
category: architecture
audience: builder
tags: [decisions, adr, architecture]
updated: 2026-07-13
last_updated: 2026-07-13
---

# Architecture Decision Records

> Decision index for Impulse. `Accepted` means the decision is preserved, not that every phase-era
> assumption remains current whole-product authority. Use [`VISION.md`](../../VISION.md),
> [`RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md), and the most recent
> applicable ADR for current direction.

ADRs 0001-0005 were written for the early hook/memory and TypeScript/npm product shape. They remain
accepted records within that historical scope, but they do not override the Rust control plane,
registry-backed runtime identity, daemon truth, or the current Now/Next/Later roadmap.

## Decision Log

| # | Title | Decision status | Current authority | Date | Supersedes |
|---|-------|-----------------|-------------------|------|------------|
| [0001](0001-claude-code-primary.md) | Claude Code as primary agent | **Accepted** | Historical integration scope | 2026-02-20 | historical `0001-opencode-first` |
| [0002](0002-file-first-memory.md) | File-first memory (no DB in Phase 1) | **Accepted** | Historical Phase 1 memory scope | 2026-02-20 | historical `0002-unified-steward` |
| [0003](0003-progressive-search.md) | Progressive search (FTS5 then vectors) | **Accepted** | Historical phase framing; Rust contract owns current retrieval truth | 2026-02-20 | historical `0003-split-schema` |
| [0004](0004-extraction-strategy.md) | LLM extraction strategy | **Accepted** | Historical extraction scope | 2026-02-20 | -- |
| [0005](0005-distribution-model.md) | npm distribution model | **Accepted** | Historical distribution scope; superseded in practice by Rust | 2026-02-20 | -- |
| [0006](0006-hook-enhancement-conflict-resolution.md) | Hook enhancement conflict resolution | **Accepted** | Narrow hook/guardrail policy scope | 2026-03-31 | -- |
| [0007](0007-desktop-shell-stack.md) | Desktop shell stack | **Superseded** | Historical only | 2026-04-15 | -- |
| [0008](0008-dioxus-desktop-host.md) | Dioxus Desktop host | **Accepted** | Current desktop host authority | 2026-06-14 | 0007 |
| [0009](0009-reconcile-impulse-copies.md) | Reconcile duplicate Impulse codebases | **Accepted** | Current canonical-tree authority | 2026-06-25 | -- |
| [0010](0010-product-role-launch-contract.md) | Product role launch contract | **Accepted** | Current explicit role/task launch-preflight authority | 2026-07-13 | -- |

## ADR Format

Each ADR follows the template:

```markdown
# ADR-NNNN: Title

## Status
Accepted | Superseded | Proposed

## Context
What problem does this solve?

## Decision
What did we decide?

## Consequences
What follows from this decision?
```

## Adding New ADRs

1. Use the next sequential number
2. Create in `docs/decisions/`
3. Update this table
4. If superseding an existing ADR, preserve the old file only if the archive path actually exists in the workspace; otherwise record the historical filename in plain text
