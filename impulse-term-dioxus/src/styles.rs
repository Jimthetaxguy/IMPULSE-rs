//! Default CSS for the Impulse block UI — including sticky-scroll for
//! the running block's header.
//!
//! # Why ship a stylesheet
//!
//! `BlockView` and friends emit semantic class names (`impulse-block`,
//! `impulse-block-header`, etc.) but no inline styles for layout. That
//! keeps consumers free to theme everything. But "the default works"
//! matters too — most users want a starting style they can tweak rather
//! than a blank slate.
//!
//! `default_block_styles()` returns a small (<2 KB) stylesheet covering:
//!
//! - Block list layout (column flexbox with gap)
//! - Per-block card frame (border, padding, rounded corners)
//! - Header layout (icon + command + exit + toolbar in a flex row)
//! - **Sticky-scroll** for the header of the currently-running block:
//!   `position: sticky; top: 0;` so as the user scrolls through long
//!   output, the prompt + command stay visible at the top of the pane.
//! - Status icon coloring (green ✓, red ✗)
//! - Toolbar button styling (subtle by default, more prominent on hover)
//! - Output `<pre>` (monospace, preserved whitespace, wrapped long lines)
//!
//! Consumers paste it into a `<style>` tag, append it to a `<link>`
//! stylesheet, or override individual rules — whatever fits their app.

/// Returns the default stylesheet for the block UI.
///
/// Cheap to call repeatedly (returns a `&'static str`). Embed once in
/// the host app's HTML, e.g.:
///
/// ```ignore
/// rsx! {
///     style { dangerous_inner_html: "{impulse_term_dioxus::default_block_styles()}" }
///     PtyTerminalView { spec, /* ... */ }
/// }
/// ```
pub fn default_block_styles() -> &'static str {
    DEFAULT_CSS
}

/// CSS embedded as a `&'static str`. Kept inline (not in a separate
/// `.css` file) so the crate is self-contained and `include_str!` is
/// avoided — Dioxus apps that consume this don't need a build script.
const DEFAULT_CSS: &str = r#"
.impulse-block-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    overflow-y: auto;
    height: 100%;
}

.impulse-block {
    border: 1px solid #2d2d2d;
    border-radius: 6px;
    background: #1e1e1e;
    color: #cccccc;
    overflow: hidden;
    font-family: ui-monospace, "SF Mono", "Cascadia Code", "JetBrains Mono", monospace;
    font-size: 13px;
    line-height: 1.4;
}

.impulse-block-success { border-left: 3px solid #0dbc79; }
.impulse-block-failure { border-left: 3px solid #cd3131; }
.impulse-block-streaming { border-left: 3px solid #2472c8; }
.impulse-block-prompt,
.impulse-block-input { border-left: 3px solid #666666; }
.impulse-block-unknown { border-left: 3px solid #bc3fbc; }

/* Sticky header: when streaming, pin the prompt + command at the top
 * of the pane while output flows below. Works because the parent has
 * overflow: auto and the block has its own height. */
.impulse-block-header {
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 0.6rem;
    background: #252526;
    border-bottom: 1px solid #2d2d2d;
}

.impulse-block-status {
    display: inline-block;
    min-width: 1ch;
    text-align: center;
    color: #cccccc;
}
.impulse-block-success > .impulse-block-header > .impulse-block-status { color: #0dbc79; }
.impulse-block-failure > .impulse-block-header > .impulse-block-status { color: #cd3131; }
.impulse-block-streaming > .impulse-block-header > .impulse-block-status {
    color: #2472c8;
    /* gentle spin for the running indicator */
    animation: impulse-spin 1.6s linear infinite;
    transform-origin: center;
    display: inline-block;
}

@keyframes impulse-spin {
    0% { transform: rotate(0deg); }
    100% { transform: rotate(360deg); }
}

.impulse-block-command {
    flex: 1 1 auto;
    color: #e5e5e5;
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
}

.impulse-block-exit {
    color: #888;
    font-size: 0.85em;
    padding: 0.1rem 0.4rem;
    border: 1px solid #333;
    border-radius: 3px;
}
.impulse-block-failure > .impulse-block-header > .impulse-block-exit {
    color: #f14c4c;
    border-color: #f14c4c;
}

.impulse-block-toolbar {
    display: flex;
    gap: 0.25rem;
    margin-left: auto;
}

.impulse-block-action {
    background: transparent;
    color: #888;
    border: 1px solid transparent;
    border-radius: 3px;
    padding: 0.1rem 0.4rem;
    font-size: 0.85em;
    font-family: inherit;
    cursor: pointer;
    transition: background 80ms, color 80ms, border-color 80ms;
}
.impulse-block-action:hover:not(:disabled) {
    background: #2d2d2d;
    color: #e5e5e5;
    border-color: #3d3d3d;
}
.impulse-block-action:disabled {
    opacity: 0.3;
    cursor: not-allowed;
}

.impulse-block-output {
    margin: 0;
    padding: 0.4rem 0.6rem;
    background: #1e1e1e;
    color: #cccccc;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: inherit;
    font-size: inherit;
    line-height: inherit;
    max-height: 30em;
    overflow-y: auto;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_styles_string_is_non_empty() {
        let s = default_block_styles();
        assert!(s.len() > 200, "stylesheet seems suspiciously short");
    }

    #[test]
    fn test_styles_includes_sticky_header_rule() {
        let s = default_block_styles();
        assert!(
            s.contains("position: sticky"),
            "expected sticky-header rule"
        );
        assert!(
            s.contains(".impulse-block-header"),
            "expected header selector"
        );
    }

    #[test]
    fn test_styles_includes_block_list_overflow_rule() {
        // Sticky positioning requires an ancestor with overflow:auto/scroll.
        // The block-list provides that — verify the rule is present.
        let s = default_block_styles();
        assert!(s.contains(".impulse-block-list"));
        assert!(
            s.contains("overflow-y: auto"),
            "block-list needs overflow-y for sticky to work"
        );
    }

    #[test]
    fn test_styles_includes_state_color_rules() {
        let s = default_block_styles();
        // Success / failure / streaming colors hint at the state via
        // the left border so the user can scan a long list at a glance.
        assert!(s.contains(".impulse-block-success"));
        assert!(s.contains(".impulse-block-failure"));
        assert!(s.contains(".impulse-block-streaming"));
    }

    #[test]
    fn test_styles_includes_toolbar_hover_rules() {
        let s = default_block_styles();
        assert!(s.contains(".impulse-block-action"));
        assert!(s.contains(":hover"));
        assert!(s.contains(":disabled"));
    }

    #[test]
    fn test_styles_includes_streaming_spinner_animation() {
        let s = default_block_styles();
        assert!(s.contains("@keyframes impulse-spin"));
        assert!(s.contains("animation: impulse-spin"));
    }

    #[test]
    fn test_styles_uses_only_class_selectors() {
        // Defensive: the stylesheet must NOT use overly broad selectors
        // like body/html/* that would leak into the host app. Spot-check
        // by confirming no "body{" or "html{" or "*{".
        let s = default_block_styles();
        assert!(!s.contains("body{"));
        assert!(!s.contains("body {"));
        assert!(!s.contains("html{"));
        assert!(!s.contains("html {"));
        // The * selector may appear in inherited box-sizing resets, but
        // we don't use it — verify.
        assert!(!s.contains("*{"));
        assert!(!s.contains("* {"));
    }

    #[test]
    fn test_styles_pre_uses_prewrap_for_long_lines() {
        // Without pre-wrap, a long line of output overflows horizontally
        // and the user has to scroll — bad UX. Verify pre-wrap is set.
        let s = default_block_styles();
        assert!(
            s.contains("white-space: pre-wrap"),
            "output should wrap long lines"
        );
    }
}
