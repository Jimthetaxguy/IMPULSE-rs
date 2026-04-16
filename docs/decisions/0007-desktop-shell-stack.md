---
title: "ADR-0007: Desktop Shell Stack"
status: accepted
created: 2026-04-15
deciders: [James Pustorino]
---

# ADR-0007: Desktop Shell Stack

## Status

Accepted

## Context

Impulse has an existing `impulse-gui` crate built on `eframe` / `egui`. It provides a custom terminal widget (`impulse-term`) with a production-quality PTY backend (`TerminalBackend`, `WriteQueue`, vt100 parser) but couples that backend to an immediate-mode UI framework that repaints every frame, has constrained layout capabilities, and is not suitable as a long-term polished desktop product shell.

The product goal is a **desktop app that feels polished but is fundamentally terminal-centric**: real PTY-backed terminal panes in the center, session/project/agent rail on the left, context/artifact/supervisor inspector on the right, daemon status on top, event strip on the bottom.

Multiple desktop UI stack options were evaluated. The full tradeoff analysis is in `docs/spec/DESKTOP-STACK-TRADEOFFS.md`.

## Decision

Adopt **Tauri 2.x + Dioxus + xterm.js terminal bridge** as the desktop shell stack.

- **Tauri 2.x** is the native desktop container: window management, OS integration, IPC capability system, and future mobile path
- **Dioxus** is the UI framework: `rsx!` declarative Rust components for all non-terminal chrome
- **xterm.js** is the terminal renderer: mounted into Dioxus-created `<div>` elements, fed by PTY byte streams via Tauri events
- **Existing Rust backend** (`impulse-term` core, `impulse-ops`, daemon) is unchanged in responsibility

## Consequences

### Accepted

- `impulse-term` must have `eframe` removed from `Cargo.toml`. `backend.rs` already has no rendering code; this is a mechanical cleanup.
- A new `src-tauri` workspace member replaces the stale `src-tauri` build residue.
- `impulse-gui` enters freeze mode immediately: compile-only maintenance, no new features, removed from active roadmap after parity.
- The JS surface area for xterm.js is deliberately minimal (~10 lines of `eval()` glue per terminal pane). It must not grow.
- macOS-first delivery is acceptable for the initial desktop cut.
- Memory usage will exceed `ratatui`. Acceptance threshold: competitive with or bounded versus current `egui`, and PTY responsiveness must not regress by more than 10%.

### Rejected

- **egui as long-term desktop UI:** Immediate-mode rendering, constrained layout, unsuitable for a polished product shell.
- **Wrap ratatui process in Tauri (Option 2):** Creates terminal-inside-terminal architecture. Not a polished desktop shell.
- **SwiftUI:** Apple-ecosystem only. Incompatible with Rust-first stack.
- **Pure JS/TS Tauri frontend:** Contradicts the Rust-first stance.
- **Iced / Slint / GPUI:** No mature terminal widget exists in any of these frameworks.

## Validation

The decision is validated when:

1. `cargo check --manifest-path impulse-rs/Cargo.toml --workspace` passes with `eframe` removed from `impulse-term/Cargo.toml`
2. A static Tauri + Dioxus shell renders the five-panel layout on macOS
3. A live terminal prototype streams PTY output to xterm.js correctly
4. Benchmark thresholds in `docs/guides/DESKTOP-BENCHMARK-METHODOLOGY.md` are met
5. Standalone `ratatui` operator surface remains functional throughout migration

## Related Documents

- `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md`
- `docs/spec/DESKTOP-STACK-TRADEOFFS.md`
- `docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md`
- `docs/guides/DESKTOP-BENCHMARK-METHODOLOGY.md`
