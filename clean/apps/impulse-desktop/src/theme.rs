//! Visual theme — terminal-futurist minimal palette.

/// The brand palette. The hex values match the `aura-dark` theme
/// used elsewhere in the user's projects.
pub mod palette {
    /// Background.
    pub const BG: &str = "#0b0b0f";
    /// Surface (cards).
    pub const SURFACE: &str = "#15151b";
    /// Accent (selection, links).
    pub const ACCENT: &str = "#7df9ff";
    /// Foreground text.
    pub const FG: &str = "#e6e6f0";
    /// Dim text.
    pub const DIM: &str = "#8a8a9a";
    /// Border.
    pub const BORDER: &str = "#2a2a35";
    /// Warning.
    pub const WARN: &str = "#ffb86b";
    /// Error.
    pub const ERROR: &str = "#ff6b9d";
}

/// Inline CSS the desktop shell injects at startup. Kept as a single
/// `&str` so the binary stays one artifact.
pub const STYLE: &str = r#"
:root {
  --bg: #0b0b0f;
  --surface: #15151b;
  --accent: #7df9ff;
  --fg: #e6e6f0;
  --dim: #8a8a9a;
  --border: #2a2a35;
  --warn: #ffb86b;
  --error: #ff6b9d;
}
* { box-sizing: border-box; }
body, #app {
  margin: 0;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  background: var(--bg);
  color: var(--fg);
  min-height: 100vh;
}
.view-switcher { display: flex; flex-direction: column; min-height: 100vh; }
.tabs { display: flex; gap: 4px; padding: 8px; background: var(--surface); border-bottom: 1px solid var(--border); }
.tab { background: transparent; color: var(--dim); border: 1px solid transparent; padding: 6px 14px; border-radius: 6px; cursor: pointer; font-family: inherit; }
.tab:hover { color: var(--fg); }
.tab--active { color: var(--accent); border-color: var(--accent); }
.view-body { flex: 1; padding: 16px; overflow: auto; }
.terminal-view { display: flex; flex-direction: column; height: 100%; }
.terminal-header { color: var(--dim); font-size: 0.85em; margin-bottom: 8px; }
.terminal-body { flex: 1; background: #000; color: var(--fg); padding: 12px; border: 1px solid var(--border); border-radius: 6px; font-size: 0.9em; overflow: auto; }
h2 { margin-top: 0; color: var(--accent); }
.empty-state { color: var(--dim); font-style: italic; }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_has_brand_colors() {
        assert_eq!(palette::ACCENT, "#7df9ff");
        assert_eq!(palette::BG, "#0b0b0f");
    }

    #[test]
    fn style_includes_all_classes() {
        for class in [
            ".view-switcher",
            ".tabs",
            ".tab",
            ".tab--active",
            ".view-body",
            ".terminal-view",
            ".terminal-header",
            ".terminal-body",
            ".empty-state",
        ] {
            assert!(STYLE.contains(class), "missing class {class}");
        }
    }
}
