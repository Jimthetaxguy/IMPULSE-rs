//! Terminal theme — ANSI color resolution and Impulse dark palette.
//!
//! Resolves vt100 `Color` values (named 0-15, 216-cube, grayscale, RGB)
//! into `egui::Color32` for rendering. The default palette matches
//! Impulse's GitHub-dark aesthetic.

use eframe::egui;

/// Terminal color theme with the full 16-color ANSI palette plus context health colors.
#[derive(Clone)]
pub struct TerminalTheme {
    pub bg: egui::Color32,
    pub fg: egui::Color32,
    pub cursor: egui::Color32,
    pub selection_bg: egui::Color32,
    /// Standard 0-7 + bright 8-15 ANSI colors.
    pub ansi_colors: [egui::Color32; 16],
    pub context_health: ContextHealthColors,
}

/// Colors for the context health indicator in the status bar.
#[derive(Clone)]
pub struct ContextHealthColors {
    /// Green — below 45%.
    pub comfortable: egui::Color32,
    /// Yellow — 45-60%.
    pub essential: egui::Color32,
    /// Orange — 60-80%.
    pub critical: egui::Color32,
    /// Red — above 80%.
    pub minimal: egui::Color32,
}

impl Default for TerminalTheme {
    /// Impulse dark palette (GitHub dark).
    fn default() -> Self {
        Self {
            bg: egui::Color32::from_rgb(0x0d, 0x11, 0x17),
            fg: egui::Color32::from_rgb(0xc9, 0xd1, 0xd9),
            cursor: egui::Color32::from_rgb(0xc9, 0xd1, 0xd9),
            selection_bg: egui::Color32::from_rgba_premultiplied(0x58, 0xa6, 0xff, 0x40),
            ansi_colors: [
                // Standard (0-7)
                egui::Color32::from_rgb(0x48, 0x4f, 0x58), // black
                egui::Color32::from_rgb(0xff, 0x7b, 0x72), // red
                egui::Color32::from_rgb(0x3f, 0xb9, 0x50), // green
                egui::Color32::from_rgb(0xd2, 0x99, 0x22), // yellow
                egui::Color32::from_rgb(0x58, 0xa6, 0xff), // blue
                egui::Color32::from_rgb(0xbc, 0x8c, 0xff), // magenta
                egui::Color32::from_rgb(0x39, 0xd2, 0xc0), // cyan
                egui::Color32::from_rgb(0xb1, 0xba, 0xc4), // white
                // Bright (8-15)
                egui::Color32::from_rgb(0x6e, 0x76, 0x81), // bright black
                egui::Color32::from_rgb(0xff, 0xa1, 0x98), // bright red
                egui::Color32::from_rgb(0x56, 0xd3, 0x64), // bright green
                egui::Color32::from_rgb(0xe3, 0xb3, 0x41), // bright yellow
                egui::Color32::from_rgb(0x79, 0xc0, 0xff), // bright blue
                egui::Color32::from_rgb(0xd2, 0xa8, 0xff), // bright magenta
                egui::Color32::from_rgb(0x56, 0xd4, 0xdd), // bright cyan
                egui::Color32::from_rgb(0xf0, 0xf6, 0xfc), // bright white
            ],
            context_health: ContextHealthColors {
                comfortable: egui::Color32::from_rgb(0x3f, 0xb9, 0x50),
                essential: egui::Color32::from_rgb(0xd2, 0x99, 0x22),
                critical: egui::Color32::from_rgb(0xff, 0x7b, 0x72),
                minimal: egui::Color32::from_rgb(0xff, 0x45, 0x45),
            },
        }
    }
}

impl TerminalTheme {
    /// Resolve a vt100 `Color` to an egui `Color32`.
    pub fn resolve_fg(&self, color: vt100::Color) -> egui::Color32 {
        self.resolve_color(color, self.fg)
    }

    /// Resolve a vt100 `Color` to an egui `Color32` for backgrounds.
    pub fn resolve_bg(&self, color: vt100::Color) -> egui::Color32 {
        self.resolve_color(color, self.bg)
    }

    fn resolve_color(&self, color: vt100::Color, default: egui::Color32) -> egui::Color32 {
        match color {
            vt100::Color::Default => default,
            vt100::Color::Idx(idx) => self.resolve_indexed(idx),
            vt100::Color::Rgb(r, g, b) => egui::Color32::from_rgb(r, g, b),
        }
    }

    /// Resolve an indexed color (0-255).
    fn resolve_indexed(&self, idx: u8) -> egui::Color32 {
        match idx {
            // Named colors (0-15): use theme palette.
            0..=15 => self.ansi_colors[idx as usize],
            // 216-color cube (16-231).
            16..=231 => {
                let i = idx - 16;
                let r = i / 36;
                let g = (i / 6) % 6;
                let b = i % 6;
                let to_byte = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
                egui::Color32::from_rgb(to_byte(r), to_byte(g), to_byte(b))
            }
            // Grayscale ramp (232-255).
            232..=255 => {
                let v = 8 + 10 * (idx - 232);
                egui::Color32::from_rgb(v, v, v)
            }
        }
    }
}

/// Return a color associated with an agent name for UI elements.
pub fn agent_color(name: &str) -> egui::Color32 {
    match name {
        "Claude Code" => egui::Color32::from_rgb(0x8b, 0x5c, 0xf6), // purple
        "OpenCode" => egui::Color32::from_rgb(0x3f, 0xb9, 0x50),    // green
        "Codex" => egui::Color32::from_rgb(0xd2, 0x99, 0x22),       // yellow
        "Shell" => egui::Color32::from_rgb(0x58, 0xa6, 0xff),       // blue
        _ => egui::Color32::from_rgb(0xc9, 0xd1, 0xd9),             // default text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_theme_has_16_ansi_colors() {
        let theme = TerminalTheme::default();
        assert_eq!(theme.ansi_colors.len(), 16);
    }

    #[test]
    fn test_resolve_default_fg() {
        let theme = TerminalTheme::default();
        assert_eq!(theme.resolve_fg(vt100::Color::Default), theme.fg);
    }

    #[test]
    fn test_resolve_default_bg() {
        let theme = TerminalTheme::default();
        assert_eq!(theme.resolve_bg(vt100::Color::Default), theme.bg);
    }

    #[test]
    fn test_resolve_named_color() {
        let theme = TerminalTheme::default();
        // Index 1 = red
        assert_eq!(
            theme.resolve_fg(vt100::Color::Idx(1)),
            egui::Color32::from_rgb(0xff, 0x7b, 0x72)
        );
    }

    #[test]
    fn test_resolve_rgb_passthrough() {
        let theme = TerminalTheme::default();
        assert_eq!(
            theme.resolve_fg(vt100::Color::Rgb(0xAA, 0xBB, 0xCC)),
            egui::Color32::from_rgb(0xAA, 0xBB, 0xCC)
        );
    }

    #[test]
    fn test_resolve_216_cube() {
        let theme = TerminalTheme::default();
        // Index 16 = (0,0,0) → black
        let c = theme.resolve_fg(vt100::Color::Idx(16));
        assert_eq!(c, egui::Color32::from_rgb(0, 0, 0));
        // Index 196 = (4,0,0) → 55+40*4=215 red
        let c = theme.resolve_fg(vt100::Color::Idx(196));
        // 196 - 16 = 180; r = 180/36 = 5, g = (180/6)%6 = 0, b = 180%6 = 0
        // r=5: to_byte(5) = 55+40*5 = 255
        assert_eq!(c, egui::Color32::from_rgb(255, 0, 0));
    }

    #[test]
    fn test_resolve_grayscale() {
        let theme = TerminalTheme::default();
        // Index 232 = darkest gray (8)
        let c = theme.resolve_fg(vt100::Color::Idx(232));
        assert_eq!(c, egui::Color32::from_rgb(8, 8, 8));
        // Index 255 = lightest gray (238)
        let c = theme.resolve_fg(vt100::Color::Idx(255));
        assert_eq!(c, egui::Color32::from_rgb(238, 238, 238));
    }

    #[test]
    fn test_agent_colors() {
        assert_eq!(
            agent_color("Claude Code"),
            egui::Color32::from_rgb(0x8b, 0x5c, 0xf6)
        );
        assert_eq!(
            agent_color("Shell"),
            egui::Color32::from_rgb(0x58, 0xa6, 0xff)
        );
        assert_eq!(
            agent_color("unknown"),
            egui::Color32::from_rgb(0xc9, 0xd1, 0xd9)
        );
    }
}
