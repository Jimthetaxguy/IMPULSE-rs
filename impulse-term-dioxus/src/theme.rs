//! `TermColor` → CSS color mapping for the Dioxus renderer.
//!
//! Mirrors `impulse-term/src/theme.rs` (egui adapter) but emits CSS color
//! strings instead of `egui::Color32`. Using CSS strings (rather than e.g.
//! a `(u8, u8, u8)` tuple) lets the renderer interpolate them directly into
//! `style="color:..; background:.."` attributes with zero per-frame
//! conversion cost.

use impulse_term_core::TermColor;

/// CSS color resolver for terminal output.
///
/// The default fg/bg are configurable so the supervisor shell can theme
/// terminals by role (e.g. supervisor pane vs worker pane).
#[derive(Clone, PartialEq, Eq)]
pub struct CssTheme {
    pub fg_default: &'static str,
    pub bg_default: &'static str,
    /// 16-color ANSI palette (0–7 standard, 8–15 bright).
    pub palette_16: [&'static str; 16],
}

impl Default for CssTheme {
    fn default() -> Self {
        // VS Code dark+ palette — high readability, matches what most users
        // see in their other terminals so colors don't surprise.
        Self {
            fg_default: "#cccccc",
            bg_default: "#1e1e1e",
            palette_16: [
                "#000000", // 0 black
                "#cd3131", // 1 red
                "#0dbc79", // 2 green
                "#e5e510", // 3 yellow
                "#2472c8", // 4 blue
                "#bc3fbc", // 5 magenta
                "#11a8cd", // 6 cyan
                "#e5e5e5", // 7 white
                "#666666", // 8 bright black (gray)
                "#f14c4c", // 9 bright red
                "#23d18b", // 10 bright green
                "#f5f543", // 11 bright yellow
                "#3b8eea", // 12 bright blue
                "#d670d6", // 13 bright magenta
                "#29b8db", // 14 bright cyan
                "#e5e5e5", // 15 bright white
            ],
        }
    }
}

impl CssTheme {
    /// Resolve a foreground `TermColor` to a CSS color string.
    pub fn resolve_fg(&self, color: TermColor) -> String {
        self.resolve(color, self.fg_default)
    }

    /// Resolve a background `TermColor` to a CSS color string.
    pub fn resolve_bg(&self, color: TermColor) -> String {
        self.resolve(color, self.bg_default)
    }

    fn resolve(&self, color: TermColor, default: &str) -> String {
        match color {
            TermColor::Default => default.to_string(),
            TermColor::Indexed(i) if (i as usize) < self.palette_16.len() => {
                self.palette_16[i as usize].to_string()
            }
            TermColor::Indexed(i) => indexed_256_to_css(i),
            TermColor::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// Map 256-color palette indices 16–255 to CSS hex.
///
/// 16–231: 6×6×6 RGB cube (216 colors). Index = 16 + 36r + 6g + b, each
/// channel value mapped via the standard xterm cube: 0 → 0x00, 1 → 0x5f,
/// 2 → 0x87, 3 → 0xaf, 4 → 0xd7, 5 → 0xff.
///
/// 232–255: 24-step grayscale ramp from 0x08 to 0xee.
fn indexed_256_to_css(idx: u8) -> String {
    if idx < 16 {
        // Should be handled by the 16-palette caller; emit black as fallback.
        return "#000000".to_string();
    }
    if (16..=231).contains(&idx) {
        let n = idx - 16;
        let r = n / 36;
        let g = (n % 36) / 6;
        let b = n % 6;
        let to_chan = |c: u8| -> u8 {
            const STEPS: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
            STEPS[c as usize]
        };
        format!("#{:02x}{:02x}{:02x}", to_chan(r), to_chan(g), to_chan(b))
    } else {
        // 232–255 grayscale.
        let n = idx - 232;
        let level = 0x08 + n * 10;
        format!("#{level:02x}{level:02x}{level:02x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_resolves_to_theme_default() {
        let theme = CssTheme::default();
        assert_eq!(theme.resolve_fg(TermColor::Default), theme.fg_default);
        assert_eq!(theme.resolve_bg(TermColor::Default), theme.bg_default);
    }

    #[test]
    fn test_indexed_0_to_15_uses_palette() {
        let theme = CssTheme::default();
        assert_eq!(theme.resolve_fg(TermColor::Indexed(0)), "#000000");
        assert_eq!(theme.resolve_fg(TermColor::Indexed(1)), "#cd3131");
        assert_eq!(theme.resolve_fg(TermColor::Indexed(15)), "#e5e5e5");
    }

    #[test]
    fn test_indexed_16_to_231_uses_color_cube() {
        // Index 16 = (0,0,0) in the cube.
        let theme = CssTheme::default();
        assert_eq!(theme.resolve_fg(TermColor::Indexed(16)), "#000000");
        // Index 196 = pure red corner: (5, 0, 0) → 0xff, 0x00, 0x00.
        assert_eq!(theme.resolve_fg(TermColor::Indexed(196)), "#ff0000");
        // Index 231 = (5,5,5) → white.
        assert_eq!(theme.resolve_fg(TermColor::Indexed(231)), "#ffffff");
    }

    #[test]
    fn test_indexed_232_to_255_grayscale_monotonic() {
        let theme = CssTheme::default();
        let lightest = theme.resolve_fg(TermColor::Indexed(255));
        let darkest = theme.resolve_fg(TermColor::Indexed(232));
        // Lightest > darkest — compare hex strings (all chans equal).
        assert!(lightest > darkest, "{lightest} should be > {darkest}");
    }

    #[test]
    fn test_rgb_emits_lowercase_hex() {
        let theme = CssTheme::default();
        assert_eq!(
            theme.resolve_fg(TermColor::Rgb(0xab, 0xcd, 0xef)),
            "#abcdef"
        );
        assert_eq!(theme.resolve_fg(TermColor::Rgb(0, 0, 0)), "#000000");
    }
}
