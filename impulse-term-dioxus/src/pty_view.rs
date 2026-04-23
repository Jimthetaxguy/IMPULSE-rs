//! `PtyTerminalView` — the live, PTY-driven Dioxus component.
//!
//! Owns a `PtySource` and per-row `Signal<RowSnapshot>`s. A background
//! `use_future` polls `PtySource::tick()` at ~16ms and writes ONLY changed
//! rows into their signals. Dioxus's component memoization then re-renders
//! only the affected `LivePtyRow` children — idle rows produce zero diffs,
//! streaming output diffs only the active row.
//!
//! # Composition
//!
//! ```text
//!  PTY child process
//!     │ bytes
//!     ▼
//!  TerminalBackend (reader thread + parser)
//!     │
//!     ▼
//!  PtySource::tick()  ── 16ms polling via use_future
//!     │ UpdateReport { changed_rows }
//!     ▼
//!  per-row Signal<RowSnapshot>s   ← write only changed rows
//!     │
//!     ▼
//!  LivePtyRow components (memoized on RowSnapshot.version u64)
//! ```
//!
//! # Why per-row signals, not one Signal<Vec<RowSnapshot>>
//!
//! A single `Signal<Vec<...>>` invalidates every consumer on any write.
//! With one Signal per row, only the consumers reading the changed row's
//! signal re-run. This is the structural fix that makes "60fps streaming
//! cargo build output" cheap — a 60-row screen with output appended to
//! the bottom row triggers exactly one row re-render per tick instead of
//! all 60.
//!
//! # Polling vs notification
//!
//! Polling at 16ms is the simplest correct design. A notification path
//! (reader thread signals when bytes arrive) is faster on the wakeup side
//! but harder to integrate with Dioxus's reactive runtime, and the parser
//! thread already runs continuously — so the *worst-case* polling latency
//! is one 16ms frame, well within human perception. If profiling shows
//! the 16ms tick is the bottleneck, swap for `tokio::sync::Notify` later.

use std::time::Duration;

use dioxus::prelude::*;
use impulse_term_core::input::key_to_pty_bytes;
use impulse_term_core::CellRun;

use crate::blocks_view::BlockListView;
use crate::key_shim::{dx_key_to_term, dx_mods_to_term};
use crate::live::RowSnapshot;
use crate::source::{PtySource, PtySpec};
use crate::theme::CssTheme;
use crate::view::ThemeProp;
use impulse_term_core::Block;

/// Polling cadence in milliseconds. Defaults to 16ms (~60 Hz). Lower this
/// (5–10ms) for ultra-responsive typing latency at the cost of idle CPU;
/// raise it (50–100ms) for low-power background panes.
const TICK_INTERVAL_MS: u64 = 16;

/// Props for the live PTY-driven terminal view.
#[derive(Props, Clone, PartialEq)]
pub struct PtyTerminalViewProps {
    pub spec: PtySpec,
    /// Theme override. Defaults to `CssTheme::default()` (VS Code dark+).
    #[props(default)]
    pub theme: Option<ThemeProp>,
    /// Font size in pixels. Default: 13.
    #[props(default = 13)]
    pub font_size_px: u32,
    /// Line height multiplier. Default: 1.2.
    #[props(default = 1.2)]
    pub line_height: f32,
    /// When true, render a `BlockListView` of OSC 133-derived command
    /// blocks beneath the live grid. The supervisor uses this to expose
    /// the Warp-style block history alongside the live terminal output.
    /// Default: `false` (preserves the original L165 behavior).
    #[props(default = false)]
    pub show_blocks: bool,
}

/// Live, PTY-driven terminal component.
#[component]
pub fn PtyTerminalView(props: PtyTerminalViewProps) -> Element {
    let theme = props.theme.unwrap_or_default();
    let font_size = props.font_size_px;
    let line_height = props.line_height;
    let rows = props.spec.rows;
    let cols = props.spec.cols;
    let spec = props.spec.clone();
    let show_blocks = props.show_blocks;

    // The PtySource lives for the lifetime of this component. use_signal
    // ensures it's created exactly once and gives us a Copy handle for
    // sharing with the polling task.
    let mut source = use_signal(|| match PtySource::spawn(&spec) {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::error!("PtySource::spawn failed: {e}");
            None
        }
    });

    // Per-row signals — sized once at component init based on spec.rows.
    // If the PTY is later resized to more rows, we'll need to handle that
    // (deferred to a future loop; for L165 the spec.rows is authoritative).
    let row_signals = use_signal(|| {
        (0..rows)
            .map(|_| Signal::new(RowSnapshot::default()))
            .collect::<Vec<_>>()
    });

    // OSC 133 block history. Empty until the shell emits OSC 133 markers
    // (most modern shells: zsh w/ p10k, fish, bash w/ vte integration).
    // Refreshed inside the same tick loop that updates row signals so the
    // block list stays consistent with the live grid.
    let mut blocks_signal = use_signal(Vec::<Block>::new);

    // Background poll. use_future runs once per component lifecycle.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_millis(TICK_INTERVAL_MS)).await;

            // Skip if PtySource failed to spawn.
            let report = {
                let mut src = source.write();
                let Some(s) = src.as_mut() else { return };
                s.tick()
            };

            if report.is_clean() {
                continue;
            }

            let src = source.read();
            let Some(s) = src.as_ref() else { return };
            let sigs = row_signals.read();
            for &row_idx in &report.changed_rows {
                let Some(snapshot) = s.live_grid().row(row_idx as usize) else {
                    continue;
                };
                if let Some(sig) = sigs.get(row_idx as usize) {
                    let mut sig = *sig;
                    sig.set(snapshot.clone());
                }
            }
            // Refresh the block snapshot once per dirty tick. Cheap when
            // the store has 0..N blocks; we clone so the rendering side
            // gets a stable Vec<Block>. If show_blocks=false, the signal
            // is never read by any consumer, so updating it is essentially
            // free (Dioxus only re-renders subscribed components).
            let snapshot = s.block_store().blocks.clone();
            if blocks_signal.peek().as_slice() != snapshot.as_slice() {
                blocks_signal.set(snapshot);
            }
        }
    });

    // Container CSS — same shape as TerminalView but parameterized.
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

    // Snapshot the row signals into props at render time. The actual
    // memoization of "did this row change" lives inside LivePtyRow which
    // reads its specific signal.
    let signals_snapshot: Vec<Signal<RowSnapshot>> = row_signals.read().clone();

    rsx! {
        div {
            class: "impulse-pty-view",
            tabindex: "0",
            "data-rows": "{rows}",
            "data-cols": "{cols}",
            style: "{container_style}",
            onkeydown: move |evt| {
                handle_key_event(&evt, source);
            },
            for (idx, row_sig) in signals_snapshot.into_iter().enumerate() {
                LivePtyRow {
                    key: "{idx}",
                    row_idx: idx,
                    row_signal: row_sig,
                    theme: theme.clone(),
                }
            }
        }
        if show_blocks {
            BlockListView {
                blocks: blocks_signal.read().clone(),
                on_action: None,
            }
        }
    }
}

/// Translate a Dioxus `KeyboardEvent` into PTY bytes and write them to the
/// `PtySource`. Returns silently on:
/// - keys that have no terminal mapping (modifier keys, function-only keys)
/// - source write errors (logged via `tracing` so they surface in dev tools)
/// - missing source (spawn failed at component mount)
///
/// Application-cursor mode is currently hard-coded false — when the
/// supervisor learns to track DECCKM state per pane (Phase 3+), pass it in
/// via a `Signal<bool>`.
fn handle_key_event(evt: &Event<KeyboardData>, source: Signal<Option<PtySource>>) {
    let key = evt.key();
    let modifiers = evt.modifiers();

    let Some(term_key) = dx_key_to_term(&key) else {
        return;
    };
    let term_mods = dx_mods_to_term(&modifiers);

    let Some(bytes) = key_to_pty_bytes(&term_key, &term_mods, false) else {
        return;
    };

    let src = source.read();
    let Some(s) = src.as_ref() else { return };
    if let Err(e) = s.write_input(&bytes) {
        tracing::warn!("write_input failed: {e}");
    }
    // Prevent the browser/webview from also handling the event (e.g. Tab
    // moving focus, Backspace navigating back).
    evt.prevent_default();
}

/// One row of the live terminal. Reads from a `Signal<RowSnapshot>` so it
/// re-renders only when its signal updates. `RowSnapshot::version` (u64) is
/// the equality check for memoization — see `live.rs` module docs.
#[derive(Props, Clone, PartialEq)]
struct LivePtyRowProps {
    row_idx: usize,
    row_signal: Signal<RowSnapshot>,
    theme: ThemeProp,
}

#[component]
fn LivePtyRow(props: LivePtyRowProps) -> Element {
    let snapshot = props.row_signal.read();
    let row_idx = props.row_idx;
    let theme = props.theme.clone();

    rsx! {
        div {
            class: "impulse-term-row",
            "data-row": "{row_idx}",
            "data-version": "{snapshot.version}",
            for run in snapshot.runs.iter() {
                {render_run_styled(run, &theme.0)}
            }
        }
    }
}

/// Local copy of the run renderer (mirrors `view::render_run` for now).
/// Kept as a local function so changes to one renderer don't accidentally
/// alter the other. If a third renderer ever shows up, extract.
fn render_run_styled(run: &CellRun, theme: &CssTheme) -> Element {
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

    /// Smoke test: the component compiles and TICK_INTERVAL_MS is sane.
    /// Rendering tests for PtyTerminalView require a Dioxus runtime with
    /// a Tokio executor (use_future spawns); covered by the integration
    /// smoke test in impulse-supervisor at L167.
    // ~60fps is 16ms. Anything above ~33ms (30fps) becomes perceptibly
    // laggy for typing feedback. Anything below 5ms wastes CPU when
    // idle since we'd poll a parser that hasn't changed. Asserted at
    // compile time via const block.
    const _: () = {
        assert!(TICK_INTERVAL_MS >= 5);
        assert!(TICK_INTERVAL_MS <= 33);
    };

    #[test]
    fn test_props_construct_with_defaults() {
        let props = PtyTerminalViewProps {
            spec: PtySpec::shell("/bin/sh"),
            theme: None,
            font_size_px: 13,
            line_height: 1.2,
            show_blocks: false,
        };
        assert_eq!(props.spec.command, "/bin/sh");
        assert_eq!(props.font_size_px, 13);
        assert!(!props.show_blocks);
    }

    #[test]
    fn test_props_show_blocks_can_be_enabled() {
        // L181: supervisor opts in via show_blocks=true to render the
        // OSC 133 block history beneath the live grid in the same card.
        let props = PtyTerminalViewProps {
            spec: PtySpec::shell("/bin/sh"),
            theme: None,
            font_size_px: 12,
            line_height: 1.25,
            show_blocks: true,
        };
        assert!(props.show_blocks);
    }
}
