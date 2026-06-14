---
title: Implementation Handoff
description: Current implementation sequence — Dioxus Desktop host migration
version: '2.0'
updated: 2026-06-14
type: doc
category: handoff
phase: all
status: active
audience: builder
tags: [handoff, implementation, dioxus, desktop, egui-deprecation]
authors:
  - name: Impulse Maintainers
    role: Maintainer
---

# Implementation Handoff Document

> **Updated:** 2026-06-14
> **Purpose:** Capture the actual next implementation sequence for Impulse.
> **Risk register:** [`../HONEST-ROADMAP.md`](../HONEST-ROADMAP.md)
> **Roadmap anchor:** [`../ROADMAP-PLAN.md`](../ROADMAP-PLAN.md)
> **Desktop architecture:** [`../spec/DESKTOP-SHELL-ARCHITECTURE.md`](../spec/DESKTOP-SHELL-ARCHITECTURE.md)
> **Historical migration context:** [`TAURI-DIOXUS-MIGRATION-HANDOFF.md`](TAURI-DIOXUS-MIGRATION-HANDOFF.md)

---

## Executive Summary

The desktop stack has been formally reset again after the Dioxus host decision. The previous EGUI-workbench-as-destination direction and the Tauri-as-target direction are superseded. The new desktop contract is:

- **Desktop host:** Dioxus Desktop
- **Desktop UI layer:** Dioxus
- **Terminal rendering:** xterm.js terminal bridge
- **PTY/session/daemon ownership:** existing Rust backend (unchanged)
- **Terminal-native operator surface:** ratatui (preserved, first-class)
- **Legacy desktop surface:** egui / impulse-gui (frozen, sunset after parity)
- **Legacy host adapter:** Tauri-shaped command/event bridge (compatibility only)

Phase 0 established the earlier desktop contract, but the current implementation stance is: Dioxus Desktop is active, `impulse-gui` is already frozen, Tauri-shaped code is compatibility-only, and remaining work should build the Dioxus Desktop launch scaffold plus daemon-backed desktop parity without reviving egui or new Tauri scaffolding as product paths.

Historical migration build sequence: [`TAURI-DIOXUS-MIGRATION-HANDOFF.md`](TAURI-DIOXUS-MIGRATION-HANDOFF.md)

---

## Why The Sequence Changed

egui's immediate-mode rendering, constrained layout model, and the deep coupling between `impulse-term`'s PTY backend and `eframe` make it unsuitable as the long-term desktop shell. The PTY backend (`backend.rs`, `WriteQueue`, `context.rs`) is already framework-neutral in its logic — the `eframe` dependency is mechanical coupling that predates the current product direction.

Dioxus Desktop + xterm.js gives us:
- All application logic stays in Rust
- xterm.js handles terminal rendering without building a custom cell-grid renderer
- Dioxus `rsx!` gives declarative component composition without a JS frontend
- A Dioxus-owned host adapter instead of Tauri IPC as the product center
- macOS-first delivery with native islands added only where needed

Full tradeoff analysis: [`../spec/DESKTOP-STACK-TRADEOFFS.md`](../spec/DESKTOP-STACK-TRADEOFFS.md)

---

## Current Plan 6 Focus — Platform Truth Stabilization

### In Scope

- All canonical contract docs updated to reflect Dioxus Desktop as desktop target
- egui explicitly marked as legacy/freeze in all docs
- Active docs route users to Claude Code/Codex primary platform truth, with OpenCode marked as legacy compatibility
- Doc validation passes

### Out of Scope

- Any code changes
- Any Cargo.toml changes
- Any new crates

---

## Platform Truth Checklist

- [x] `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md`
- [x] `docs/spec/DESKTOP-STACK-TRADEOFFS.md`
- [x] `docs/decisions/0007-desktop-shell-stack.md`
- [x] `docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md`
- [x] `docs/guides/DESKTOP-BENCHMARK-METHODOLOGY.md`
- [x] `AGENTS.md` — egui removed as active target
- [x] `docs/plans/IMPLEMENTATION-HANDOFF.md` — this document
- [x] `docs/spec/RUST-CANONICAL-CONTRACT.md` — egui section updated
- [ ] `docs/ROADMAP-PLAN.md` — phases updated
- [ ] `docs/INDEX.md` — new docs indexed
- [ ] `docs/SUMMARY.md` / `docs/SUMMARY.yaml` — updated
- [x] `CLAUDE.md` — product description updated
- [ ] `impulse-rs/docs/IMPULSE_TERM_STATUS.md` — egui deprecation noted
- [ ] `impulse-rs/README.md` — workspace crate descriptions updated
- [x] `impulse-rs/impulse-gui/README.md` — marked as legacy/freeze
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

Historical detail in [`TAURI-DIOXUS-MIGRATION-HANDOFF.md`](TAURI-DIOXUS-MIGRATION-HANDOFF.md); current implementation follows ADR-0008.

| Phase | Goal | Entry Criteria |
|---|---|---|
| 0 | Documentation reset | — |
| 1 | Remove eframe from impulse-term | Active-doc drift resolved |
| 2 | Static Dioxus shell skeleton | Phase 1 complete |
| 3 | Dioxus Desktop launch scaffold + live terminal bridge (PTY → xterm.js) | Phase 2 complete |
| 4 | Daemon integration + parity | Phase 3 complete |

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

### Platform Truth Cleanup Complete When

- No doc refers to egui as the active or target desktop surface
- Dioxus Desktop is the desktop host contract across all top-level docs
- Tauri-shaped command/event code is described only as a temporary compatibility adapter or historical context
- `validate_docs.py --contract` passes
- ratatui is explicitly preserved as first-class standalone operator surface
