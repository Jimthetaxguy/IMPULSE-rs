//! impulse-term-core — toolkit-neutral terminal core.
//!
//! Provides the PTY backend, vt100 parser ownership, and the context lifecycle
//! bridge. **No GUI dependency.** Renderers (egui, Dioxus, ratatui, future
//! toolkits) consume the parser's screen and map colors/keys to whatever
//! native types they use.
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
//!     ├─── with_parser() → renderer reads vt100::Screen
//!     ├─── ContextBridge::extract() — agent insight extraction
//!     └─── ContextBridge::inject() — context block injection via PTY writer
//! ```
//!
//! # Status
//!
//! Phase 1 of the egui→Dioxus migration (Ralph Plan 7 L157). The pure
//! PTY/parser/context modules now live here. Toolkit-neutral grid types
//! (TermColor, CellAttrs, GridSnapshot) and key types (TermKey) extracted
//! at L158–L159. Egui adapter consumes via `impulse-term-egui`.

#![deny(clippy::all)]

pub mod backend;
pub mod blocks;
pub mod context;
pub mod escape;
pub mod grid;
pub mod input;
pub mod osc133;
pub mod role;

pub use blocks::{Block, BlockState, BlockStore};
pub use grid::{CellAttrs, CellRun, GridSnapshot, TermColor};
pub use input::{TermKey, TermModifiers};
pub use osc133::{Osc133Event, Osc133Parser};

pub use backend::TerminalBackend;
pub use context::{
    AgentKind, ContextBridge, ContextHealth, ContextTier, ExtractedInsight, InsightType,
};
pub use role::PaneRole;
