//! Theme system for Impulse GUI.
//!
//! Provides `ColorPalette` — a complete set of semantic color tokens.
//! Four named themes ship: Launch (default), Nebula, Solar, Aurora.
//! The active palette is stored in config.json and switchable at runtime.
//!
//! ## Migration
//!
//! The `colors` module provides backwards-compatible constants that resolve
//! from the Launch palette. New code should accept `&ColorPalette` instead.

use eframe::egui;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ColorPalette
// ---------------------------------------------------------------------------

/// Semantic color palette for the entire GUI.
#[allow(dead_code)]
// dead_code: semantic palette slots stay explicit even when a given release does not consume every accent/status color.
#[derive(Clone, Debug)]
pub struct ColorPalette {
    // Backgrounds
    pub bg_deep: egui::Color32,
    pub bg_surface: egui::Color32,
    pub bg_hover: egui::Color32,
    pub border: egui::Color32,

    // Accent
    pub accent: egui::Color32,
    pub accent_bright: egui::Color32,
    pub accent_dim: egui::Color32,

    // Text
    pub text: egui::Color32,
    pub text_muted: egui::Color32,
    pub text_dim: egui::Color32,
    pub text_faint: egui::Color32,

    // Status
    pub green: egui::Color32,
    pub yellow: egui::Color32,
    pub red: egui::Color32,
    pub blue: egui::Color32,
}

// ---------------------------------------------------------------------------
// ThemeName (persisted)
// ---------------------------------------------------------------------------

/// Theme name — persisted to config.json.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeName {
    #[default]
    Launch,
    Nebula,
    Solar,
    Aurora,
}

impl ThemeName {
    pub fn all() -> &'static [ThemeName] {
        &[
            ThemeName::Launch,
            ThemeName::Nebula,
            ThemeName::Solar,
            ThemeName::Aurora,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Launch => "Launch",
            Self::Nebula => "Nebula",
            Self::Solar => "Solar",
            Self::Aurora => "Aurora",
        }
    }

    pub fn palette(&self) -> ColorPalette {
        match self {
            Self::Launch => ColorPalette::launch(),
            Self::Nebula => ColorPalette::nebula(),
            Self::Solar => ColorPalette::solar(),
            Self::Aurora => ColorPalette::aurora(),
        }
    }
}

// ---------------------------------------------------------------------------
// Named palettes
// ---------------------------------------------------------------------------

impl ColorPalette {
    /// Launch — deep space navy, electric blue accents. Default theme.
    pub fn launch() -> Self {
        Self {
            bg_deep: egui::Color32::from_rgb(0x02, 0x06, 0x17),
            bg_surface: egui::Color32::from_rgb(0x0c, 0x1a, 0x3d),
            bg_hover: egui::Color32::from_rgb(0x1e, 0x3a, 0x5f),
            border: egui::Color32::from_rgba_premultiplied(0x3b, 0x82, 0xf6, 0x26),
            accent: egui::Color32::from_rgb(0x3b, 0x82, 0xf6),
            accent_bright: egui::Color32::from_rgb(0x60, 0xa5, 0xfa),
            accent_dim: egui::Color32::from_rgb(0x1d, 0x4e, 0xd8),
            text: egui::Color32::from_rgb(0xe2, 0xe8, 0xf0),
            text_muted: egui::Color32::from_rgb(0x94, 0xa3, 0xb8),
            text_dim: egui::Color32::from_rgb(0x64, 0x74, 0x8b),
            text_faint: egui::Color32::from_rgb(0x33, 0x41, 0x55),
            green: egui::Color32::from_rgb(0x4a, 0xde, 0x80),
            yellow: egui::Color32::from_rgb(0xfb, 0xbf, 0x24),
            red: egui::Color32::from_rgb(0xf8, 0x71, 0x71),
            blue: egui::Color32::from_rgb(0x60, 0xa5, 0xfa),
        }
    }

    /// Nebula — purple-violet on deep plum.
    pub fn nebula() -> Self {
        Self {
            bg_deep: egui::Color32::from_rgb(0x0a, 0x00, 0x15),
            bg_surface: egui::Color32::from_rgb(0x1a, 0x0a, 0x2e),
            bg_hover: egui::Color32::from_rgb(0x2e, 0x1a, 0x47),
            border: egui::Color32::from_rgba_premultiplied(0x8b, 0x5c, 0xf6, 0x26),
            accent: egui::Color32::from_rgb(0x7c, 0x3a, 0xed),
            accent_bright: egui::Color32::from_rgb(0xa7, 0x8b, 0xfa),
            accent_dim: egui::Color32::from_rgb(0x5b, 0x21, 0xb6),
            text: egui::Color32::from_rgb(0xe2, 0xe8, 0xf0),
            text_muted: egui::Color32::from_rgb(0x94, 0xa3, 0xb8),
            text_dim: egui::Color32::from_rgb(0x64, 0x74, 0x8b),
            text_faint: egui::Color32::from_rgb(0x33, 0x41, 0x55),
            green: egui::Color32::from_rgb(0x4a, 0xde, 0x80),
            yellow: egui::Color32::from_rgb(0xfb, 0xbf, 0x24),
            red: egui::Color32::from_rgb(0xf8, 0x71, 0x71),
            blue: egui::Color32::from_rgb(0x60, 0xa5, 0xfa),
        }
    }

    /// Solar — amber-gold on dark brown.
    pub fn solar() -> Self {
        Self {
            bg_deep: egui::Color32::from_rgb(0x0c, 0x0a, 0x00),
            bg_surface: egui::Color32::from_rgb(0x1a, 0x10, 0x00),
            bg_hover: egui::Color32::from_rgb(0x2e, 0x1f, 0x00),
            border: egui::Color32::from_rgba_premultiplied(0xf5, 0x9e, 0x0b, 0x26),
            accent: egui::Color32::from_rgb(0xd9, 0x77, 0x06),
            accent_bright: egui::Color32::from_rgb(0xfb, 0xbf, 0x24),
            accent_dim: egui::Color32::from_rgb(0x92, 0x40, 0x0e),
            text: egui::Color32::from_rgb(0xe2, 0xe8, 0xf0),
            text_muted: egui::Color32::from_rgb(0x94, 0xa3, 0xb8),
            text_dim: egui::Color32::from_rgb(0x64, 0x74, 0x8b),
            text_faint: egui::Color32::from_rgb(0x33, 0x41, 0x55),
            green: egui::Color32::from_rgb(0x4a, 0xde, 0x80),
            yellow: egui::Color32::from_rgb(0xfb, 0xbf, 0x24),
            red: egui::Color32::from_rgb(0xf8, 0x71, 0x71),
            blue: egui::Color32::from_rgb(0x60, 0xa5, 0xfa),
        }
    }

    /// Aurora — emerald-cyan on deep ocean.
    pub fn aurora() -> Self {
        Self {
            bg_deep: egui::Color32::from_rgb(0x00, 0x0a, 0x0a),
            bg_surface: egui::Color32::from_rgb(0x00, 0x1a, 0x1a),
            bg_hover: egui::Color32::from_rgb(0x00, 0x2e, 0x2e),
            border: egui::Color32::from_rgba_premultiplied(0x10, 0xb9, 0x81, 0x26),
            accent: egui::Color32::from_rgb(0x05, 0x96, 0x69),
            accent_bright: egui::Color32::from_rgb(0x6e, 0xe7, 0xb7),
            accent_dim: egui::Color32::from_rgb(0x04, 0x73, 0x57),
            text: egui::Color32::from_rgb(0xe2, 0xe8, 0xf0),
            text_muted: egui::Color32::from_rgb(0x94, 0xa3, 0xb8),
            text_dim: egui::Color32::from_rgb(0x64, 0x74, 0x8b),
            text_faint: egui::Color32::from_rgb(0x33, 0x41, 0x55),
            green: egui::Color32::from_rgb(0x4a, 0xde, 0x80),
            yellow: egui::Color32::from_rgb(0xfb, 0xbf, 0x24),
            red: egui::Color32::from_rgb(0xf8, 0x71, 0x71),
            blue: egui::Color32::from_rgb(0x60, 0xa5, 0xfa),
        }
    }
}

// ---------------------------------------------------------------------------
// Apply theme to egui
// ---------------------------------------------------------------------------

/// Apply a ColorPalette to the egui context visuals.
pub fn apply_theme(ctx: &egui::Context, palette: &ColorPalette) {
    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = palette.bg_deep;
    visuals.window_fill = palette.bg_surface;
    visuals.extreme_bg_color = palette.bg_deep;
    visuals.faint_bg_color = palette.bg_surface;

    visuals.widgets.noninteractive.bg_fill = palette.bg_surface;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, palette.text);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, palette.border);

    visuals.widgets.inactive.bg_fill = palette.bg_surface;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, palette.text);

    visuals.widgets.hovered.bg_fill = palette.bg_hover;
    visuals.widgets.active.bg_fill = palette.bg_hover;

    visuals.selection.bg_fill = egui::Color32::from_rgba_premultiplied(
        palette.accent.r(),
        palette.accent.g(),
        palette.accent.b(),
        0x40,
    );

    ctx.set_visuals(visuals);
}

/// Return a color associated with an agent name for tab/button rendering.
pub fn agent_color(name: &str) -> egui::Color32 {
    let (r, g, b) = impulse_term_core::theme::agent_color_rgb(name);
    egui::Color32::from_rgb(r, g, b)
}

// ---------------------------------------------------------------------------
// Backwards-compat shim — Launch palette as constants
// ---------------------------------------------------------------------------

/// Backwards-compatible color constants. New code should use `ColorPalette` directly.
pub mod colors {
    use eframe::egui;

    // -- Backgrounds (Launch palette) --
    pub const BG: egui::Color32 = egui::Color32::from_rgb(0x02, 0x06, 0x17);
    pub const SURFACE: egui::Color32 = egui::Color32::from_rgb(0x0c, 0x1a, 0x3d);
    pub const HOVER: egui::Color32 = egui::Color32::from_rgb(0x1e, 0x3a, 0x5f);
    pub const BORDER: egui::Color32 = egui::Color32::from_rgb(0x1e, 0x29, 0x3b);
    pub const ACTIVE_BG: egui::Color32 = egui::Color32::from_rgb(0x0f, 0x17, 0x2a);
    pub const ACTIVE_AGENT_BG: egui::Color32 = egui::Color32::from_rgb(0x0c, 0x1a, 0x2e);

    // -- Text --
    pub const TEXT: egui::Color32 = egui::Color32::from_rgb(0xe2, 0xe8, 0xf0);
    pub const TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(0x94, 0xa3, 0xb8);
    pub const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(0x64, 0x74, 0x8b);
    pub const TEXT_FAINT: egui::Color32 = egui::Color32::from_rgb(0x33, 0x41, 0x55);

    // -- Accent (blue — Launch identity) --
    pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x3b, 0x82, 0xf6);

    // -- Status --
    pub const GREEN: egui::Color32 = egui::Color32::from_rgb(0x4a, 0xde, 0x80);
    pub const YELLOW: egui::Color32 = egui::Color32::from_rgb(0xfb, 0xbf, 0x24);
    pub const RED: egui::Color32 = egui::Color32::from_rgb(0xf8, 0x71, 0x71);
    pub const BLUE: egui::Color32 = egui::Color32::from_rgb(0x60, 0xa5, 0xfa);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_name_all_has_four() {
        assert_eq!(ThemeName::all().len(), 4);
    }

    #[test]
    fn test_theme_name_default_is_launch() {
        assert_eq!(ThemeName::default(), ThemeName::Launch);
    }

    #[test]
    fn test_each_theme_builds_valid_palette() {
        for name in ThemeName::all() {
            let p = name.palette();
            assert_ne!(
                p.bg_deep, p.accent,
                "bg and accent should differ for {:?}",
                name
            );
            assert_ne!(
                p.text, p.bg_deep,
                "text and bg should differ for {:?}",
                name
            );
        }
    }

    #[test]
    fn test_theme_name_serde_round_trip() {
        for name in ThemeName::all() {
            let json = serde_json::to_string(name).unwrap();
            let recovered: ThemeName = serde_json::from_str(&json).unwrap();
            assert_eq!(*name, recovered);
        }
    }

    #[test]
    fn test_launch_palette_is_blue() {
        let p = ColorPalette::launch();
        assert!(
            p.accent.b() > p.accent.r(),
            "Launch accent should be blue-dominant"
        );
        assert!(p.accent.b() > p.accent.g());
    }

    #[test]
    fn test_nebula_palette_is_purple() {
        let p = ColorPalette::nebula();
        assert!(
            p.accent.b() > p.accent.g(),
            "Nebula accent should be purple (blue > green)"
        );
    }

    #[test]
    fn test_solar_palette_is_warm() {
        let p = ColorPalette::solar();
        assert!(
            p.accent.r() > p.accent.b(),
            "Solar accent should be warm (red > blue)"
        );
    }

    #[test]
    fn test_aurora_palette_is_green() {
        let p = ColorPalette::aurora();
        assert!(
            p.accent.g() > p.accent.r(),
            "Aurora accent should be green-dominant"
        );
    }

    #[test]
    fn test_colors_compat_matches_launch() {
        let p = ColorPalette::launch();
        assert_eq!(colors::ACCENT, p.accent);
        assert_eq!(colors::BG, p.bg_deep);
        assert_eq!(colors::TEXT, p.text);
    }

    #[test]
    fn test_theme_labels_nonempty() {
        for name in ThemeName::all() {
            assert!(!name.label().is_empty());
        }
    }
}
