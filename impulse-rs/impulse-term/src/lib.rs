//! impulse-term — custom terminal widget with context lifecycle integration.
//!
//! Replaces `egui_term` with a terminal backend that gives zero-copy, in-process
//! access to both context extraction (reading agent output) and context injection
//! (writing context blocks into agent terminals).
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
//!     ├─── TerminalRenderer reads vt100::Screen → egui paint calls
//!     ├─── ContextBridge reads screen_text() for extraction
//!     └─── ContextBridge writes inject_context() via PTY writer
//! ```

pub mod backend;
pub mod context;
pub mod input;
pub mod panel;
pub mod renderer;
pub mod status_bar;
pub mod theme;

// Re-export public API.
pub use backend::TerminalBackend;
pub use context::{
    AgentKind, ContextBridge, ContextHealth, ContextTier, ExtractedInsight, InsightType,
};
pub use input::key_to_pty_bytes;
pub use panel::TerminalPanel;
pub use renderer::TerminalRenderer;
pub use theme::TerminalTheme;
