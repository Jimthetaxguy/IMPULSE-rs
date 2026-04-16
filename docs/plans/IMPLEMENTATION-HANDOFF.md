---
title: Implementation Handoff
description: Current implementation sequence — Tauri desktop shell migration
version: '2.0'
updated: 2026-04-15
type: doc
category: handoff
phase: all
status: active
audience: builder
tags: [handoff, implementation, tauri, dioxus, desktop, egui-deprecation]
authors:
  - name: James Pustorino
    role: Creator
---

# Implementation Handoff Document

> **Updated:** 2026-04-15
> **Purpose:** Capture the actual next implementation sequence for Impulse.
> **Risk register:** [`../HONEST-ROADMAP.md`](../HONEST-ROADMAP.md)
> **Roadmap anchor:** [`../ROADMAP-PLAN.md`](../ROADMAP-PLAN.md)
> **Desktop architecture:** [`../spec/DESKTOP-SHELL-ARCHITECTURE.md`](../spec/DESKTOP-SHELL-ARCHITECTURE.md)
> **Migration build sequence:** [`TAURI-DIOXUS-MIGRATION-HANDOFF.md`](TAURI-DIOXUS-MIGRATION-HANDOFF.md)

---

## Executive Summary

The desktop stack has been formally reset. The previous EGUI-workbench-as-destination direction is superseded. The new desktop contract is:

- **Desktop shell:** Tauri 2.x
- **Desktop UI layer:** Dioxus (inside the Tauri webview)
- **Terminal rendering:** xterm.js terminal bridge
- **PTY/session/daemon ownership:** existing Rust backend (unchanged)
- **Terminal-native operator surface:** ratatui (preserved, first-class)
- **Legacy desktop surface:** egui / impulse-gui (frozen, sunset after parity)

The current phase is **Phase 0 — Documentation Contract Reset**. Implementation begins only after docs, spec, roadmap, and migration handoff all describe the same product.

Full migration build sequence: [`TAURI-DIOXUS-MIGRATION-HANDOFF.md`](TAURI-DIOXUS-MIGRATION-HANDOFF.md)

---

## Why The Sequence Changed

egui's immediate-mode rendering, constrained layout model, and the deep coupling between `impulse-term`'s PTY backend and `eframe` make it unsuitable as the long-term desktop shell. The PTY backend (`backend.rs`, `WriteQueue`, `context.rs`) is already framework-neutral in its logic — the `eframe` dependency is mechanical coupling that predates the current product direction.

Tauri + Dioxus + xterm.js gives us:
- All application logic stays in Rust
- xterm.js handles terminal rendering without building a custom cell-grid renderer
- Dioxus `rsx!` gives declarative component composition without a JS frontend
- Tauri 2's capability system is the right security model for a tool that spawns arbitrary subprocesses
- macOS-first delivery, with mobile path available via Tauri 2 when needed

Full tradeoff analysis: [`../spec/DESKTOP-STACK-TRADEOFFS.md`](../spec/DESKTOP-STACK-TRADEOFFS.md)

---

## Current Phase: Phase 0 — Documentation Reset

### In Scope

- All canonical contract docs updated to reflect Tauri+Dioxus as desktop target
- egui explicitly marked as legacy/freeze in all docs
- New spec files: architecture, tradeoffs, ADR, migration handoff, benchmark methodology
- Doc validation passes

### Out of Scope (for Phase 0)

- Any code changes
- Any Cargo.toml changes
- Any new crates

---

## Phase 0 Checklist

- [x] `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md`
- [x] `docs/spec/DESKTOP-STACK-TRADEOFFS.md`
- [x] `docs/decisions/0007-desktop-shell-stack.md`
- [x] `docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md`
- [x] `docs/guides/DESKTOP-BENCHMARK-METHODOLOGY.md`
- [x] `AGENTS.md` — egui removed as active target
- [x] `docs/plans/IMPLEMENTATION-HANDOFF.md` — this document
- [ ] `docs/spec/RUST-CANONICAL-CONTRACT.md` — egui section updated
- [ ] `docs/ROADMAP-PLAN.md` — phases updated
- [ ] `docs/INDEX.md` — new docs indexed
- [ ] `docs/SUMMARY.md` / `docs/SUMMARY.yaml` — updated
- [ ] `CLAUDE.md` — product description updated
- [ ] `impulse-rs/docs/IMPULSE_TERM_STATUS.md` — egui deprecation noted
- [ ] `impulse-rs/README.md` — workspace crate descriptions updated
- [ ] `impulse-rs/impulse-gui/README.md` — marked as legacy/freeze
- [ ] `python3 docs/validate_docs.py --contract` — passes

---

## Preserved Daemon Contracts

The following daemon IPC contracts are unchanged and remain authoritative. The desktop shell adapts to them — they are not changed to serve the desktop:

- `ProjectOpsSnapshot`
- `TerminalOpsReport`
- `GetOpsSnapshot`
- `SubscribeOps`
- `PublishTerminalOps`

The desktop shell adds the terminal bridge command/event surfaces as **additive** interfaces. See `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md` for the full terminal bridge API.

---

## Implementation Phases (Summary)

Full detail in [`TAURI-DIOXUS-MIGRATION-HANDOFF.md`](TAURI-DIOXUS-MIGRATION-HANDOFF.md).

| Phase | Goal | Entry Criteria |
|---|---|---|
| 0 | Documentation reset | — |
| 1 | Remove eframe from impulse-term | Phase 0 complete |
| 2 | Static Tauri+Dioxus shell skeleton | Phase 1 complete |
| 3 | Live terminal bridge (PTY → xterm.js) | Phase 2 complete |
| 4 | Daemon integration + parity + egui freeze | Phase 3 complete |

---

## Verification Checklist

### Documentation

```bash
python3 docs/validate_docs.py --contract
```

### Rust

```bash
cd impulse-rs
cargo fmt --check
cargo check --all-features
cargo test
cargo clippy --all-features --all-targets -- -D warnings
```

### Phase 0 Complete When

- No doc refers to egui as the active or target desktop surface
- Tauri+Dioxus is the desktop contract across all top-level docs
- `validate_docs.py --contract` passes
- ratatui is explicitly preserved as first-class standalone operator surface
