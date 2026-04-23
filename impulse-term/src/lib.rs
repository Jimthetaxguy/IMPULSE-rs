//! impulse-term — egui adapter for `impulse-term-core`.
//!
//! Wraps the toolkit-neutral terminal core (`impulse-term-core`) with an
//! egui-specific renderer, theme, status bar, key shim, and composite panel
//! widget. **For new code, prefer importing the renderer-specific crate
//! directly** (`impulse-term-egui`, `impulse-term-dioxus`) and the core
//! types from `impulse-term-core`.
//!
//! # Re-exports for back-compat
//!
//! Existing consumers (`impulse-gui`, `impulse-rs`) can keep using
//! `impulse_term::TerminalBackend` etc. — these are re-exported from
//! `impulse-term-core`. This shim layer disappears in L161 once consumers
//! repoint to `impulse-term-egui` directly.
//!
//! # Architecture (post-L157)
//!
//! ```text
//! impulse-term-core     ← PTY, parser, context bridge (no GUI dep)
//!     ▲
//!     │ consumed by
//!     │
//! impulse-term (this)   ← egui renderer, theme, panel, status bar, keys
//! ```

pub mod input;
pub mod panel;
pub mod renderer;
pub mod status_bar;
pub mod theme;

// Re-export core types AND modules for back-compat. The module re-exports
// allow internal panel.rs / status_bar.rs to keep using `crate::backend::*`
// etc. without a sweeping import rewrite.
pub use impulse_term_core::{backend, context, role};
pub use impulse_term_core::{
    AgentKind, ContextBridge, ContextHealth, ContextTier, ExtractedInsight, InsightType, PaneRole,
    TerminalBackend,
};

pub use input::key_to_pty_bytes;
pub use panel::TerminalPanel;
pub use renderer::TerminalRenderer;
pub use theme::{AgentTheme, AgentThemeConfig, TerminalTheme};
