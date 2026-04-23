//! `TerminalView` Dioxus component — renders a `GridSnapshot` as run-based
//! `<span>` cells inside one `<div>` per row.
//!
//! # Render shape
//!
//! ```html
//! <div class="impulse-term-view" data-rows="N" data-cols="M">
//!   <div class="impulse-term-row" data-row="0">
//!     <span style="color:#cd3131; background:#1e1e1e; font-weight:bold;">runText</span>
//!     <span style="color:#cccccc; background:#1e1e1e;">moreText</span>
//!     ...
//!   </div>
//!   <div class="impulse-term-row" data-row="1">...</div>
//!   ...
//! </div>
//! ```
//!
//! One `<span>` per `CellRun`, not per cell — a 4,800-cell grid (80×60)
//! typically collapses to ~100–300 runs, a ~10–40× DOM-node reduction.
//!
//! # Status (L162)
//!
//! Stateless render of an immutable `GridSnapshot`. L163 adds per-row
//! `Signal` damage tracking so updates only diff the rows that actually
//! changed. L164 wires the `TerminalBackend` PTY source.

#![cfg(feature = "desktop")]

use dioxus::prelude::*;
use impulse_term_core::{CellRun, GridSnapshot};

use crate::theme::CssTheme;

/// Props for the terminal view.
///
/// Holds an immutable snapshot for L162. L163 replaces this with a
/// `Signal<GridSnapshot>` (or per-row signals) for damage tracking.
#[derive(Props, Clone, PartialEq)]
pub struct TerminalViewProps {
    pub snapshot: GridSnapshot,
    /// Theme used to resolve `TermColor` to CSS color strings. Defaults to
    /// `CssTheme::default()` (VS Code dark+ palette) if `None`.
    #[props(default)]
    pub theme: Option<ThemeProp>,
    /// Font size in pixels. Default: 13.
    #[props(default = 13)]
    pub font_size_px: u32,
    /// Line height multiplier. Default: 1.2.
    #[props(default = 1.2)]
    pub line_height: f32,
}

/// Wrapper around `CssTheme` so it can derive `PartialEq` (which `&'static str`
/// arrays don't satisfy through their equality but `CssTheme` does directly —
/// however `Option<CssTheme>` would require `CssTheme: PartialEq`, which it
/// is not. This wrapper makes the prop type usable.)
#[derive(Clone, PartialEq)]
pub struct ThemeProp(pub std::rc::Rc<CssTheme>);

impl ThemeProp {
    pub fn new(theme: CssTheme) -> Self {
        Self(std::rc::Rc::new(theme))
    }
}

impl Default for ThemeProp {
    fn default() -> Self {
        Self::new(CssTheme::default())
    }
}

/// Render a terminal grid as Dioxus rsx.
///
/// Walks `snapshot.row_runs`, emitting one `<div>` per row and one `<span>`
/// per run. Style strings are inline (not classes) because each run's color
/// combination is effectively unique — a class-per-color cache would grow
/// unboundedly under truecolor output.
#[component]
pub fn TerminalView(props: TerminalViewProps) -> Element {
    let theme = props.theme.unwrap_or_default();
    let font_size = props.font_size_px;
    let line_height = props.line_height;

    let container_style = format!(
        "font-family: ui-monospace, 'SF Mono', 'Cascadia Code', 'JetBrains Mono', monospace; \
         font-size: {font_size}px; \
         line-height: {line_height}; \
         background: {bg}; \
         color: {fg}; \
         white-space: pre; \
         overflow: hidden;",
        bg = theme.0.bg_default,
        fg = theme.0.fg_default,
    );

    rsx! {
        div {
            class: "impulse-term-view",
            "data-rows": "{props.snapshot.rows}",
            "data-cols": "{props.snapshot.cols}",
            style: "{container_style}",
            for (row_idx, runs) in props.snapshot.row_runs.iter().enumerate() {
                TerminalRow {
                    key: "{row_idx}",
                    row_idx: row_idx,
                    runs: runs.clone(),
                    theme: theme.clone(),
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct TerminalRowProps {
    row_idx: usize,
    runs: Vec<CellRun>,
    theme: ThemeProp,
}

/// One terminal row. Extracted as its own component so L163's per-row
/// `Signal` damage tracking can swap this for a `Signal<RowSnapshot>`-aware
/// version without touching the parent.
#[component]
fn TerminalRow(props: TerminalRowProps) -> Element {
    rsx! {
        div {
            class: "impulse-term-row",
            "data-row": "{props.row_idx}",
            for run in props.runs.iter() {
                {render_run(run, &props.theme.0)}
            }
        }
    }
}

/// Render one `CellRun` as a styled `<span>`.
///
/// Inline styles only — no class attributes. See the `TerminalView`
/// docstring for why classes don't fit here (truecolor output explodes the
/// class space).
fn render_run(run: &CellRun, theme: &CssTheme) -> Element {
    let fg = theme.resolve_fg(run.fg);
    let bg = theme.resolve_bg(run.bg);

    let mut style = format!("color:{fg};background:{bg};");
    if run.attrs.bold {
        style.push_str("font-weight:bold;");
    }
    if run.attrs.italic {
        style.push_str("font-style:italic;");
    }
    if run.attrs.underline {
        style.push_str("text-decoration:underline;");
    }
    if run.attrs.strikethrough {
        // Combine with underline if both present.
        if run.attrs.underline {
            style = style.replace(
                "text-decoration:underline;",
                "text-decoration:underline line-through;",
            );
        } else {
            style.push_str("text-decoration:line-through;");
        }
    }

    let text = run.text.clone();
    rsx! {
        span {
            style: "{style}",
            "{text}"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use impulse_term_core::GridSnapshot;

    fn snapshot_from_bytes(rows: u16, cols: u16, bytes: &[u8]) -> GridSnapshot {
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(bytes);
        GridSnapshot::from_screen(parser.screen())
    }

    fn render_to_string(snapshot: GridSnapshot) -> String {
        let mut vdom = VirtualDom::new_with_props(
            TerminalView,
            TerminalViewProps {
                snapshot,
                theme: None,
                font_size_px: 13,
                line_height: 1.2,
            },
        );
        vdom.rebuild_in_place();
        dioxus_ssr::render(&vdom)
    }

    #[test]
    fn test_props_round_trip() {
        let snapshot = snapshot_from_bytes(2, 5, b"");
        let props = TerminalViewProps {
            snapshot: snapshot.clone(),
            theme: None,
            font_size_px: 13,
            line_height: 1.2,
        };
        assert_eq!(props.snapshot.rows, 2);
        assert_eq!(props.snapshot.cols, 5);
    }

    #[test]
    fn test_render_emits_view_container() {
        let snapshot = snapshot_from_bytes(1, 5, b"hi");
        let html = render_to_string(snapshot);
        assert!(
            html.contains(r#"class="impulse-term-view""#),
            "missing view container class: {html}"
        );
        assert!(
            html.contains(r#"data-rows="1""#),
            "missing data-rows attr: {html}"
        );
        assert!(
            html.contains(r#"data-cols="5""#),
            "missing data-cols attr: {html}"
        );
    }

    #[test]
    fn test_render_emits_one_row_per_grid_row() {
        let snapshot = snapshot_from_bytes(3, 5, b"");
        let html = render_to_string(snapshot);
        let row_count = html.matches(r#"class="impulse-term-row""#).count();
        assert_eq!(row_count, 3, "expected 3 rows in HTML, got {html}");
    }

    #[test]
    fn test_render_emits_runs_as_spans_with_inline_style() {
        let snapshot = snapshot_from_bytes(1, 5, b"hi");
        let html = render_to_string(snapshot);
        // The default-color "hi   " should render as a single span with
        // color:#cccccc and background:#1e1e1e (VS Code dark+ defaults).
        assert!(html.contains("<span"), "expected at least one span: {html}");
        assert!(
            html.contains("color:#cccccc"),
            "expected default fg in style: {html}"
        );
        assert!(
            html.contains("background:#1e1e1e"),
            "expected default bg in style: {html}"
        );
    }

    #[test]
    fn test_render_emits_text_content() {
        let snapshot = snapshot_from_bytes(1, 10, b"hello");
        let html = render_to_string(snapshot);
        assert!(
            html.contains("hello"),
            "expected 'hello' in rendered output: {html}"
        );
    }

    #[test]
    fn test_render_color_change_produces_separate_spans() {
        // "AB" default + "CD" red + "EF" default + spaces → at least 3 spans
        let snapshot = snapshot_from_bytes(1, 20, b"AB\x1b[31mCD\x1b[0mEF");
        let html = render_to_string(snapshot);
        let span_count = html.matches("<span").count();
        assert!(
            span_count >= 3,
            "expected at least 3 spans for 3 color regions, got {span_count}: {html}"
        );
        // Red is index 1 → palette #cd3131.
        assert!(
            html.contains("color:#cd3131"),
            "expected red color in HTML: {html}"
        );
    }

    #[test]
    fn test_render_bold_emits_font_weight() {
        let snapshot = snapshot_from_bytes(1, 5, b"\x1b[1mBOLD");
        let html = render_to_string(snapshot);
        assert!(
            html.contains("font-weight:bold"),
            "expected bold style: {html}"
        );
    }

    #[test]
    fn test_render_italic_and_underline_styles() {
        let snapshot = snapshot_from_bytes(1, 5, b"\x1b[3mIT");
        let html = render_to_string(snapshot);
        assert!(
            html.contains("font-style:italic"),
            "expected italic style: {html}"
        );

        let snapshot = snapshot_from_bytes(1, 5, b"\x1b[4mUL");
        let html = render_to_string(snapshot);
        assert!(
            html.contains("text-decoration:underline"),
            "expected underline style: {html}"
        );
    }

    #[test]
    fn test_render_truecolor_rgb_emits_hex() {
        // \x1b[38;2;r;g;b m sets fg to RGB.
        let snapshot = snapshot_from_bytes(1, 5, b"\x1b[38;2;171;205;239mX");
        let html = render_to_string(snapshot);
        // 171=0xab, 205=0xcd, 239=0xef → #abcdef.
        assert!(
            html.contains("color:#abcdef"),
            "expected RGB hex color: {html}"
        );
    }

    #[test]
    fn test_render_uses_monospace_font_family() {
        let snapshot = snapshot_from_bytes(1, 5, b"x");
        let html = render_to_string(snapshot);
        assert!(
            html.contains("monospace"),
            "expected monospace font family: {html}"
        );
    }

    #[test]
    fn test_render_minimal_grid_still_emits_container() {
        // vt100::Parser doesn't accept 0×0 grids (panics on subtract overflow),
        // so the smallest meaningful case is 1×1.
        let snapshot = snapshot_from_bytes(1, 1, b"");
        let html = render_to_string(snapshot);
        assert!(
            html.contains("impulse-term-view"),
            "expected container for minimal grid: {html}"
        );
    }
}
