//! Terminal theme — ANSI color resolution and Impulse dark palette.
//!
//! Resolves vt100 `Color` values (named 0-15, 216-cube, grayscale, RGB)
//! into `egui::Color32` for rendering. The default palette matches
//! Impulse's GitHub-dark aesthetic.
//!
//! # Agent Themes
//!
//! Per-agent color themes are configurable via [`AgentThemeConfig`].  Users can
//! override the built-in palette through a `HashMap<String, AgentTheme>` (stored
//! in config JSON with `#[serde(default)]`).  Two built-in presets ship out of
//! the box: **github-dark** (the original palette) and **terminal-native**
//! (cyan-on-black aesthetic).

use std::collections::HashMap;

use eframe::egui;
use serde::{Deserialize, Serialize};

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
    /// Create a terminal theme that matches a GUI accent color.
    ///
    /// Uses `bg` as the terminal background (from the GUI palette's `bg_deep`),
    /// `accent` for cursor and selection, and keeps the standard ANSI color set.
    pub fn from_accent(bg: egui::Color32, accent: egui::Color32) -> Self {
        Self {
            bg,
            cursor: accent,
            selection_bg: egui::Color32::from_rgba_premultiplied(
                accent.r(),
                accent.g(),
                accent.b(),
                0x40,
            ),
            ..Self::default()
        }
    }

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

// ---------------------------------------------------------------------------
// Agent color themes
// ---------------------------------------------------------------------------

/// RGB triplet for serialization (egui::Color32 is not Serialize/Deserialize).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTheme {
    /// Primary foreground color for the agent's UI elements.
    pub foreground: [u8; 3],
    /// Accent color used for highlights, borders, or active indicators.
    pub accent: [u8; 3],
    /// Optional background tint applied behind the agent's panel.
    #[serde(default)]
    pub bg_tint: Option<[u8; 3]>,
}

impl AgentTheme {
    /// Convert the foreground RGB to an egui `Color32`.
    pub fn foreground_color(&self) -> egui::Color32 {
        egui::Color32::from_rgb(self.foreground[0], self.foreground[1], self.foreground[2])
    }

    /// Convert the accent RGB to an egui `Color32`.
    pub fn accent_color(&self) -> egui::Color32 {
        egui::Color32::from_rgb(self.accent[0], self.accent[1], self.accent[2])
    }

    /// Convert the optional background tint to an egui `Color32`.
    pub fn bg_tint_color(&self) -> Option<egui::Color32> {
        self.bg_tint
            .map(|rgb| egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]))
    }
}

/// Configurable per-agent color themes.
///
/// Holds a map of agent name -> theme.  When looking up a color, the config
/// checks user overrides first, then falls back to the built-in defaults.
///
/// # Serde
///
/// The `agent_themes` field defaults to an empty map so existing configs
/// without it deserialize cleanly.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentThemeConfig {
    /// User-provided per-agent theme overrides.
    #[serde(default)]
    pub agent_themes: HashMap<String, AgentTheme>,
}

impl AgentThemeConfig {
    /// Resolve the foreground color for `agent_name`.
    ///
    /// Lookup order:
    /// 1. User override in `agent_themes`
    /// 2. Built-in default from `default_agent_theme`
    /// 3. Hardcoded fallback text color
    pub fn resolve_color(&self, agent_name: &str) -> egui::Color32 {
        if let Some(theme) = self.agent_themes.get(agent_name) {
            return theme.foreground_color();
        }
        // Fall back to hardcoded defaults (same as the original agent_color)
        agent_color(agent_name)
    }

    /// Resolve the full `AgentTheme` for `agent_name`.
    ///
    /// Returns the user override if present, otherwise the built-in default.
    pub fn resolve_theme(&self, agent_name: &str) -> AgentTheme {
        if let Some(theme) = self.agent_themes.get(agent_name) {
            return *theme;
        }
        default_agent_theme(agent_name)
    }

    /// Return the **github-dark** preset — matches the original hardcoded palette.
    pub fn preset_github_dark() -> Self {
        let mut themes = HashMap::new();
        themes.insert(
            "Claude Code".to_string(),
            AgentTheme {
                foreground: [0x8b, 0x5c, 0xf6],
                accent: [0xd2, 0xa8, 0xff],
                bg_tint: Some([0x1c, 0x1c, 0x2e]),
            },
        );
        themes.insert(
            "OpenCode".to_string(),
            AgentTheme {
                foreground: [0x3f, 0xb9, 0x50],
                accent: [0x56, 0xd3, 0x64],
                bg_tint: Some([0x1c, 0x2e, 0x1c]),
            },
        );
        themes.insert(
            "Codex".to_string(),
            AgentTheme {
                foreground: [0xd2, 0x99, 0x22],
                accent: [0xe3, 0xb3, 0x41],
                bg_tint: Some([0x2e, 0x2a, 0x1c]),
            },
        );
        themes.insert(
            "Shell".to_string(),
            AgentTheme {
                foreground: [0x58, 0xa6, 0xff],
                accent: [0x79, 0xc0, 0xff],
                bg_tint: Some([0x1c, 0x22, 0x2e]),
            },
        );
        Self {
            agent_themes: themes,
        }
    }

    /// Return the **terminal-native** preset — cyan-on-black aesthetic.
    pub fn preset_terminal_native() -> Self {
        let mut themes = HashMap::new();
        themes.insert(
            "Claude Code".to_string(),
            AgentTheme {
                foreground: [0x00, 0xff, 0xff],
                accent: [0x00, 0xcc, 0xcc],
                bg_tint: None,
            },
        );
        themes.insert(
            "OpenCode".to_string(),
            AgentTheme {
                foreground: [0x00, 0xff, 0x00],
                accent: [0x00, 0xcc, 0x00],
                bg_tint: None,
            },
        );
        themes.insert(
            "Codex".to_string(),
            AgentTheme {
                foreground: [0xff, 0xff, 0x00],
                accent: [0xcc, 0xcc, 0x00],
                bg_tint: None,
            },
        );
        themes.insert(
            "Shell".to_string(),
            AgentTheme {
                foreground: [0xff, 0xff, 0xff],
                accent: [0xcc, 0xcc, 0xcc],
                bg_tint: None,
            },
        );
        Self {
            agent_themes: themes,
        }
    }
}

/// Built-in default theme for a given agent name (github-dark palette).
fn default_agent_theme(name: &str) -> AgentTheme {
    match name {
        "Claude Code" => AgentTheme {
            foreground: [0x8b, 0x5c, 0xf6],
            accent: [0xd2, 0xa8, 0xff],
            bg_tint: Some([0x1c, 0x1c, 0x2e]),
        },
        "OpenCode" => AgentTheme {
            foreground: [0x3f, 0xb9, 0x50],
            accent: [0x56, 0xd3, 0x64],
            bg_tint: Some([0x1c, 0x2e, 0x1c]),
        },
        "Codex" => AgentTheme {
            foreground: [0xd2, 0x99, 0x22],
            accent: [0xe3, 0xb3, 0x41],
            bg_tint: Some([0x2e, 0x2a, 0x1c]),
        },
        "Shell" => AgentTheme {
            foreground: [0x58, 0xa6, 0xff],
            accent: [0x79, 0xc0, 0xff],
            bg_tint: Some([0x1c, 0x22, 0x2e]),
        },
        _ => AgentTheme {
            foreground: [0xc9, 0xd1, 0xd9],
            accent: [0xc9, 0xd1, 0xd9],
            bg_tint: None,
        },
    }
}

/// Return a color associated with an agent name for UI elements.
///
/// This is the original hardcoded lookup, preserved as a fallback for callers
/// that do not (yet) have access to an [`AgentThemeConfig`].
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
    fn test_from_accent_uses_custom_bg_and_cursor() {
        let bg = egui::Color32::from_rgb(0x02, 0x06, 0x17);
        let accent = egui::Color32::from_rgb(0x3b, 0x82, 0xf6);
        let theme = TerminalTheme::from_accent(bg, accent);
        assert_eq!(theme.bg, bg);
        assert_eq!(theme.cursor, accent);
        // Selection should use accent with alpha
        assert_eq!(theme.selection_bg.r(), accent.r());
        assert_eq!(theme.selection_bg.a(), 0x40);
        // ANSI colors should be unchanged from default
        assert_eq!(theme.ansi_colors, TerminalTheme::default().ansi_colors);
    }

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

    // -----------------------------------------------------------------------
    // AgentTheme tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_agent_theme_foreground_color() {
        let theme = AgentTheme {
            foreground: [0xAA, 0xBB, 0xCC],
            accent: [0x11, 0x22, 0x33],
            bg_tint: None,
        };
        assert_eq!(
            theme.foreground_color(),
            egui::Color32::from_rgb(0xAA, 0xBB, 0xCC)
        );
    }

    #[test]
    fn test_agent_theme_accent_color() {
        let theme = AgentTheme {
            foreground: [0xAA, 0xBB, 0xCC],
            accent: [0x11, 0x22, 0x33],
            bg_tint: None,
        };
        assert_eq!(
            theme.accent_color(),
            egui::Color32::from_rgb(0x11, 0x22, 0x33)
        );
    }

    #[test]
    fn test_agent_theme_bg_tint_some() {
        let theme = AgentTheme {
            foreground: [0, 0, 0],
            accent: [0, 0, 0],
            bg_tint: Some([0x1c, 0x2e, 0x1c]),
        };
        assert_eq!(
            theme.bg_tint_color(),
            Some(egui::Color32::from_rgb(0x1c, 0x2e, 0x1c))
        );
    }

    #[test]
    fn test_agent_theme_bg_tint_none() {
        let theme = AgentTheme {
            foreground: [0, 0, 0],
            accent: [0, 0, 0],
            bg_tint: None,
        };
        assert!(theme.bg_tint_color().is_none());
    }

    #[test]
    fn test_agent_theme_serde_round_trip() {
        let original = AgentTheme {
            foreground: [0x8b, 0x5c, 0xf6],
            accent: [0xd2, 0xa8, 0xff],
            bg_tint: Some([0x1c, 0x1c, 0x2e]),
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: AgentTheme = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_agent_theme_serde_round_trip_no_bg_tint() {
        let original = AgentTheme {
            foreground: [0x00, 0xff, 0x00],
            accent: [0x00, 0xcc, 0x00],
            bg_tint: None,
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: AgentTheme = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_agent_theme_deserialize_without_bg_tint_field() {
        // bg_tint is optional with serde(default) — missing field should default to None
        let json = r#"{"foreground":[255,0,0],"accent":[0,255,0]}"#;
        let theme: AgentTheme = serde_json::from_str(json).unwrap();
        assert_eq!(theme.foreground, [255, 0, 0]);
        assert_eq!(theme.accent, [0, 255, 0]);
        assert!(theme.bg_tint.is_none());
    }

    // -----------------------------------------------------------------------
    // AgentThemeConfig tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_agent_theme_config_default_is_empty() {
        let config = AgentThemeConfig::default();
        assert!(config.agent_themes.is_empty());
    }

    #[test]
    fn test_agent_theme_config_resolve_falls_back_to_hardcoded() {
        let config = AgentThemeConfig::default();
        // No overrides — should match the original agent_color() values
        assert_eq!(
            config.resolve_color("Claude Code"),
            egui::Color32::from_rgb(0x8b, 0x5c, 0xf6)
        );
        assert_eq!(
            config.resolve_color("Shell"),
            egui::Color32::from_rgb(0x58, 0xa6, 0xff)
        );
        assert_eq!(
            config.resolve_color("unknown"),
            egui::Color32::from_rgb(0xc9, 0xd1, 0xd9)
        );
    }

    #[test]
    fn test_agent_theme_config_resolve_uses_override() {
        let mut config = AgentThemeConfig::default();
        config.agent_themes.insert(
            "Claude Code".to_string(),
            AgentTheme {
                foreground: [0xFF, 0x00, 0x00],
                accent: [0x00, 0xFF, 0x00],
                bg_tint: None,
            },
        );
        // Override should take precedence
        assert_eq!(
            config.resolve_color("Claude Code"),
            egui::Color32::from_rgb(0xFF, 0x00, 0x00)
        );
        // Non-overridden agents still use defaults
        assert_eq!(
            config.resolve_color("Shell"),
            egui::Color32::from_rgb(0x58, 0xa6, 0xff)
        );
    }

    #[test]
    fn test_agent_theme_config_resolve_theme_with_override() {
        let custom = AgentTheme {
            foreground: [0xFF, 0x00, 0x00],
            accent: [0x00, 0xFF, 0x00],
            bg_tint: Some([0x10, 0x10, 0x10]),
        };
        let mut config = AgentThemeConfig::default();
        config.agent_themes.insert("Shell".to_string(), custom);
        let resolved = config.resolve_theme("Shell");
        assert_eq!(resolved, custom);
    }

    #[test]
    fn test_agent_theme_config_resolve_theme_default_fallback() {
        let config = AgentThemeConfig::default();
        let theme = config.resolve_theme("Claude Code");
        assert_eq!(theme.foreground, [0x8b, 0x5c, 0xf6]);
        assert_eq!(theme.accent, [0xd2, 0xa8, 0xff]);
        assert!(theme.bg_tint.is_some());
    }

    #[test]
    fn test_agent_theme_config_resolve_theme_unknown_agent() {
        let config = AgentThemeConfig::default();
        let theme = config.resolve_theme("SomeNewAgent");
        assert_eq!(theme.foreground, [0xc9, 0xd1, 0xd9]);
        assert!(theme.bg_tint.is_none());
    }

    #[test]
    fn test_agent_theme_config_serde_round_trip_empty() {
        let original = AgentThemeConfig::default();
        let json = serde_json::to_string(&original).unwrap();
        let recovered: AgentThemeConfig = serde_json::from_str(&json).unwrap();
        assert!(recovered.agent_themes.is_empty());
    }

    #[test]
    fn test_agent_theme_config_serde_round_trip_with_overrides() {
        let mut original = AgentThemeConfig::default();
        original.agent_themes.insert(
            "Claude Code".to_string(),
            AgentTheme {
                foreground: [0xFF, 0x00, 0x00],
                accent: [0x00, 0xFF, 0x00],
                bg_tint: Some([0x10, 0x10, 0x10]),
            },
        );
        let json = serde_json::to_string(&original).unwrap();
        let recovered: AgentThemeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            recovered.agent_themes.get("Claude Code"),
            original.agent_themes.get("Claude Code")
        );
    }

    #[test]
    fn test_agent_theme_config_deserialize_missing_agent_themes_field() {
        // Backwards compat: config JSON with no agent_themes field
        let json = "{}";
        let config: AgentThemeConfig = serde_json::from_str(json).unwrap();
        assert!(config.agent_themes.is_empty());
    }

    // -----------------------------------------------------------------------
    // Preset tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_preset_github_dark_has_four_agents() {
        let preset = AgentThemeConfig::preset_github_dark();
        assert_eq!(preset.agent_themes.len(), 4);
        assert!(preset.agent_themes.contains_key("Claude Code"));
        assert!(preset.agent_themes.contains_key("OpenCode"));
        assert!(preset.agent_themes.contains_key("Codex"));
        assert!(preset.agent_themes.contains_key("Shell"));
    }

    #[test]
    fn test_preset_github_dark_matches_original_colors() {
        let preset = AgentThemeConfig::preset_github_dark();
        // The github-dark foreground should match the original agent_color() values
        assert_eq!(
            preset.resolve_color("Claude Code"),
            agent_color("Claude Code")
        );
        assert_eq!(preset.resolve_color("OpenCode"), agent_color("OpenCode"));
        assert_eq!(preset.resolve_color("Codex"), agent_color("Codex"));
        assert_eq!(preset.resolve_color("Shell"), agent_color("Shell"));
    }

    #[test]
    fn test_preset_terminal_native_has_four_agents() {
        let preset = AgentThemeConfig::preset_terminal_native();
        assert_eq!(preset.agent_themes.len(), 4);
        assert!(preset.agent_themes.contains_key("Claude Code"));
        assert!(preset.agent_themes.contains_key("OpenCode"));
        assert!(preset.agent_themes.contains_key("Codex"));
        assert!(preset.agent_themes.contains_key("Shell"));
    }

    #[test]
    fn test_preset_terminal_native_uses_cyan_for_claude() {
        let preset = AgentThemeConfig::preset_terminal_native();
        assert_eq!(
            preset.resolve_color("Claude Code"),
            egui::Color32::from_rgb(0x00, 0xff, 0xff) // cyan
        );
    }

    #[test]
    fn test_preset_terminal_native_no_bg_tints() {
        let preset = AgentThemeConfig::preset_terminal_native();
        for theme in preset.agent_themes.values() {
            assert!(
                theme.bg_tint.is_none(),
                "terminal-native preset should have no background tints"
            );
        }
    }
}
