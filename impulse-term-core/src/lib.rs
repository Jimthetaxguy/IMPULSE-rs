//! impulse-term-core — toolkit-neutral terminal core.
//!
//! Provides the PTY backend, vt100 parser ownership, grid snapshots, and the
//! context lifecycle bridge. **No GUI dependency.** Renderers (egui, Dioxus,
//! ratatui, future toolkits) consume `GridSnapshot` and `CellRun` and map
//! `TermColor` to whatever native color type they use.
//!
//! # Architecture
//!
//! ```text
//! Agent Process (claude, opencode, shell)
//!     │
//!     │ PTY (pseudoterminal pair)
//!     ▼
//! TerminalBackend
//!     ├── reader thread → vt100::Parser (background, continuous)
//!     ├── writer handle → Box<dyn Write> (for keyboard + injection)
//!     └── alive: AtomicBool (process status)
//!     │
//!     ├─── snapshot() → GridSnapshot — toolkit-neutral, run-based
//!     ├─── ContextBridge::extract() — agent insight extraction
//!     └─── ContextBridge::inject() — context block injection via PTY writer
//! ```
//!
//! # Status
//!
//! Phase 1 of the egui→Dioxus migration (Ralph Plan 7 L156). Initially this
//! crate re-exports modules that still live in `impulse-term`; subsequent
//! loops (L157+) move source files into this crate and the egui crate becomes
//! a pure adapter.

#![deny(clippy::all)]

// Modules will be populated in L157 (file moves from impulse-term).
// Keeping the lib buildable at L156 with empty module surface.
