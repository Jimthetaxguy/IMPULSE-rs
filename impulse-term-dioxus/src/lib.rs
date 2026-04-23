//! Dioxus renderer for `impulse-term-core`.
//!
//! # Status (L161 / Phase 2 scaffold)
//!
//! Skeleton crate. Renders nothing useful yet — the goal of L161 is to land
//! a clean, buildable crate with the public API shape committed so subsequent
//! loops (L162–L167) can fill in the implementation without API churn:
//!
//! - L162: `TerminalView` component renders a `GridSnapshot` as `rsx!` runs
//! - L163: per-row `Signal<RowSnapshot>` damage tracking
//! - L164: PTY wiring (consume `impulse_term_core::TerminalBackend`)
//! - L165: bounded retained scrollback (frozen blocks)
//! - L166: `dx_key_to_term` event-handler shim
//! - L167: end-to-end smoke test in `impulse-supervisor`
//!
//! # Why webview-via-Dioxus, not native pixels
//!
//! The system webview (WKWebView on macOS, WebView2 on Windows, WebKitGTK on
//! Linux) is a retained-mode compositor. For terminal workloads — high churn
//! in a small visible window backed by deep scrollback — retained-mode is the
//! right shape: idle DOM = zero work, streaming output diffs only the active
//! rows. The system compositor handles virtualization of off-screen content
//! automatically.
//!
//! This is the structural fix for the egui immediate-mode memory bug:
//! immediate mode allocated ~3,200 galleys per frame for a typical visible
//! grid; the retained DOM allocates per *change*, not per frame.

#![deny(clippy::all)]

pub mod live;
pub mod source;
pub mod theme;

#[cfg(feature = "desktop")]
pub mod pty_view;
#[cfg(feature = "desktop")]
pub mod view;

pub use live::{LiveGrid, RowSnapshot, UpdateReport};
pub use source::{PtySource, PtySourceError, PtySpec};

#[cfg(feature = "desktop")]
pub use pty_view::{PtyTerminalView, PtyTerminalViewProps};
#[cfg(feature = "desktop")]
pub use view::{TerminalView, TerminalViewProps, ThemeProp};

pub use impulse_term_core::{
    CellAttrs, CellRun, ContextBridge, GridSnapshot, PaneRole, TermColor, TermKey, TermModifiers,
    TerminalBackend,
};

/// Crate version (matches Cargo.toml).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_matches_cargo_toml() {
        assert_eq!(VERSION, "0.1.0");
    }

    #[test]
    fn test_re_exports_present() {
        // Compile-time assertion that the re-exports resolve. If any of these
        // change in impulse-term-core without updating this crate, this test
        // fails to compile and we catch the breakage at L161 instead of L167.
        let _: Option<TermKey> = None;
        let _: Option<TermColor> = None;
        let _: Option<PaneRole> = None;
    }
}
