---
title: Desktop Stack Tradeoffs
status: active
version: 1.0.0
created: 2026-04-15
updated: 2026-04-15
---

# Desktop Stack Tradeoffs

This document records the full evaluation of desktop UI stack options for Impulse. It exists so the decision is not buried in chat history and so future contributors understand what was considered, what was rejected, and why.

The chosen stack is **Tauri + Dioxus + terminal bridge (xterm.js)**. See `docs/decisions/0007-desktop-shell-stack.md` for the ADR.

---

## Evaluation Matrix

| Criterion | egui (current) | Tauri + Dioxus + xterm.js | Pure Dioxus desktop | Tauri + TS frontend | SwiftUI | Wrap ratatui in Tauri |
|---|---|---|---|---|---|---|
| UI language | Rust (immediate-mode) | Rust (declarative rsx!) | Rust (declarative rsx!) | TypeScript/JS | Swift | N/A |
| Backend language | Rust | Rust | Rust | Rust | Swift | Rust |
| Terminal rendering | Custom egui widget | xterm.js (mature) | xterm.js or custom | xterm.js (mature) | Custom/SwiftTerm | PTY-in-PTY |
| Rendering model | Immediate-mode GPU | WebKit webview | WebKit webview | WebKit webview | Metal native | WebKit + PTY |
| Cross-platform | macOS/Win/Linux | macOS/Win/Linux + mobile | macOS/Win/Linux | macOS/Win/Linux + mobile | Apple only | macOS/Win/Linux |
| Memory at idle | High (repaints every frame) | Moderate (virtual DOM) | Moderate | Moderate | Low (native) | Low + overhead |
| Layout flexibility | Limited | Full CSS + Dioxus | Full CSS + Dioxus | Full CSS | SwiftUI layout | Constrained |
| Ecosystem maturity | Stable | Dioxus 0.6 (stable) | Dioxus 0.6 (stable) | Large (web ecosystem) | Mature (Apple only) | Not viable |
| Custom terminal widget required | Yes (already built) | No (xterm.js) | Likely yes | No (xterm.js) | Yes (SwiftTerm) | No |
| Web tech required | No | ~10 lines JS glue | ~10 lines JS glue | Full JS/CSS/TS | No | No |
| Mobile path | No | Yes (Tauri 2) | No | Yes (Tauri 2) | iOS/iPadOS only | No |
| Chosen | Deprecated | **CHOSEN** | Valid fallback | Valid alternative | Rejected | Rejected |

---

## Option Analysis

### Current: egui / impulse-gui

**Why it was used:** Fast to prototype, pure Rust, no webview overhead, the custom `TerminalBackend` could be integrated directly.

**Why it is being deprecated:**
- Immediate-mode renders every frame - idle CPU and memory do not scale as session count grows
- Layout is constrained: sidebars, overlays, inspector panels, and resize handles all require fighting against egui's layout model
- The egui terminal widget in `impulse-term` requires `eframe` as a dependency even though `backend.rs` has no rendering code
- egui is not suitable as a long-term polished desktop product shell

**Disposition:** Freeze. No new features. Remove after Tauri shell reaches parity on terminal/session/context/artifact/supervisor flows.

---

### Chosen: Tauri + Dioxus + xterm.js terminal bridge

**Why it is chosen:**
- Keeps the entire application logic (backend, IPC, session management, daemon integration) in Rust
- xterm.js is a battle-tested, feature-complete terminal renderer - Unicode, sixel, ligatures, WebGL acceleration, selection, copy/paste, scroll, accessibility. Writing a competing terminal renderer is not a good use of time.
- Dioxus's `rsx!` syntax provides React-style declarative component composition without writing JavaScript for application logic
- The JS surface area is minimal: ~10 lines of `eval()` glue per terminal pane to mount xterm.js and wire the event listener
- The existing `TerminalBackend` and `WriteQueue` map directly to the Tauri command/event bridge with no structural changes
- Tauri 2's capability system provides a security-first, minimal-privilege model
- Mobile path (iOS/Android) is available via Tauri 2 if ever needed

**Known tradeoffs:**
- WebKit rendering on macOS means UI chrome goes through the browser engine, not Metal directly
- Dioxus 0.6 is stable but smaller ecosystem than React/Svelte
- The `eval()` JS bridge for xterm.js initialization is a small but real seam. It must be contained to the terminal pane component.

---

### Alternative: Pure Dioxus desktop (without Tauri)

**Why considered:** Simpler project setup - no `src-tauri` directory, no Tauri capability configuration.

**Why not chosen as primary:**
- Loses Tauri's IPC model, plugin system (clipboard, notifications, file dialogs, tray), and future mobile path
- Tauri's capability-based security model is meaningful for a tool that runs arbitrary code

**Disposition:** Valid fallback if Tauri integration proves too complex. Dioxus `rsx!` components would be identical.

---

### Alternative: Tauri + TypeScript/JS frontend

**Why considered:** More mature UI component libraries. Terminal integration path (xterm.js) is identical.

**Why not chosen:** Requires writing TypeScript/JS for all application logic in the frontend. Contradicts the Rust-first stance.

**Disposition:** Valid alternative if Dioxus proves too limiting for complex UI requirements.

---

### Rejected: SwiftUI

**Why rejected:**
- Apple-ecosystem only - no Windows, Linux, or Android path
- Backend is in Rust; mixing Swift and Rust requires FFI bridges
- Does not align with the project's Rust-first stance

---

### Rejected: Wrap ratatui process in Tauri (Option 2)

**Why rejected:**
- Creates a terminal-inside-terminal architecture
- Resize events become a two-layer problem
- Copy/paste, focus management, mouse events, and overlay integration all hit both layers
- The result is not a polished desktop shell; it is a terminal app in a box

**Disposition:** Permanently rejected as the primary product path.

---

### Rejected: Iced / Slint / GPUI

| Framework | Why Rejected |
|---|---|
| **Iced** | No mature terminal widget. Building a full PTY + cell grid renderer is significant scope. |
| **Slint** | Terminal pane requires custom OpenGL/Metal renderer. Mature for forms/controls, not terminal emulation. |
| **GPUI (Zed)** | Unstable public API, tied to Zed's development cadence. Not viable for a project that needs to ship. |
