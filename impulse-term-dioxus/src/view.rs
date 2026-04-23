//! `TerminalView` Dioxus component (skeleton).
//!
//! The full implementation lands across L162–L167:
//!
//! - L162: render a static `GridSnapshot` as `rsx!` runs
//! - L163: per-row `Signal<RowSnapshot>` damage tracking
//! - L164: PTY wiring (consume `TerminalBackend` via a coroutine task)
//! - L165: bounded retained scrollback (frozen blocks)
//! - L166: `dx_key_to_term` shim
//! - L167: integrated supervisor smoke test
//!
//! L161 lands only the public surface and a compile-only smoke test, so
//! `impulse-supervisor` can already declare a dependency on this crate.

#![cfg(feature = "desktop")]

use dioxus::prelude::*;
use impulse_term_core::GridSnapshot;

/// Props for the terminal view.
///
/// Holds an immutable snapshot for the L161 skeleton. Subsequent loops
/// replace this with a `Signal<GridSnapshot>` (or a per-row signal vector)
/// so the renderer reacts to backend updates without recomputing the whole
/// `rsx!` tree.
#[derive(Props, Clone, PartialEq)]
pub struct TerminalViewProps {
    pub snapshot: GridSnapshot,
}

/// Render a terminal grid as Dioxus rsx.
///
/// **Skeleton only.** Returns an empty fragment that compiles against
/// Dioxus 0.6 — proves the crate's public component shape and forces a
/// Dioxus dependency-graph build at L161 so version conflicts surface
/// before the implementation work begins.
#[component]
pub fn TerminalView(props: TerminalViewProps) -> Element {
    let _ = props.snapshot.run_count(); // touch the field so it isn't dead code
    rsx! {
        div {
            class: "impulse-term-view",
            "data-rows": "{props.snapshot.rows}",
            "data-cols": "{props.snapshot.cols}",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_view_props_round_trip() {
        // Build a minimal snapshot; verify Props construction compiles.
        let parser = vt100::Parser::new(2, 5, 0);
        let snapshot = GridSnapshot::from_screen(parser.screen());
        let props = TerminalViewProps {
            snapshot: snapshot.clone(),
        };
        assert_eq!(props.snapshot.rows, 2);
        assert_eq!(props.snapshot.cols, 5);
    }
}
