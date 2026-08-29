//! impulse-term — framework-neutral PTY and terminal-context core for Impulse.
//!
//! Provides process lifecycle, VT100 parsing, serialized writes, and context
//! extraction/injection for operator surfaces. Optional egui rendering remains
//! behind the `egui` feature for legacy compatibility; Dioxus/xterm.js owns
//! the active desktop cockpit.
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
//!     ├─── optional legacy TerminalRenderer → egui paint calls
//!     ├─── ContextBridge reads screen_text() for extraction
//!     └─── ContextBridge writes inject_context() via PTY writer
//! ```

pub mod backend;
pub mod context;
#[cfg(feature = "egui")]
pub mod input;
#[cfg(feature = "egui")]
pub mod panel;
pub mod paste;
#[cfg(feature = "egui")]
pub mod renderer;
#[cfg(feature = "egui")]
pub mod status_bar;
#[cfg(feature = "egui")]
pub mod theme;

// Re-export public API.
pub use backend::TerminalBackend;
pub use context::{
    AgentKind, ContextBridge, ContextHealth, ContextTier, ExtractedInsight, InsightType,
};
#[cfg(feature = "egui")]
pub use input::key_to_pty_bytes;
#[cfg(feature = "egui")]
pub use panel::TerminalPanel;
pub use paste::bracketed_paste;
#[cfg(feature = "egui")]
pub use renderer::TerminalRenderer;
#[cfg(feature = "egui")]
pub use theme::{AgentTheme, AgentThemeConfig, TerminalTheme};
