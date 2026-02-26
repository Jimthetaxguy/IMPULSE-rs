//! Dark theme for Impulse GUI.
//!
//! All palette colors are centralized here. Import `theme::colors::*` instead
//! of hardcoding `Color32::from_rgb(...)` in view/widget code.
//! Terminal-specific theming is in `impulse_term::TerminalTheme`.

use eframe::egui;

// ---------------------------------------------------------------------------
// Centralized color palette
// ---------------------------------------------------------------------------

/// Semantic color constants used across the entire GUI.
///
/// Usage: `use crate::theme::colors;` then `colors::BG`, `colors::ACCENT`, etc.
pub mod colors {
    use eframe::egui;

    // -- Backgrounds --
    /// Primary background (#0d1117).
    pub const BG: egui::Color32 = egui::Color32::from_rgb(0x0d, 0x11, 0x17);
    /// Elevated surface (#161b22).
    pub const SURFACE: egui::Color32 = egui::Color32::from_rgb(0x16, 0x1b, 0x22);
    /// Hover highlight (#21262d).
    pub const HOVER: egui::Color32 = egui::Color32::from_rgb(0x21, 0x26, 0x2d);
    /// Active/pressed highlight and border (#30363d).
    pub const BORDER: egui::Color32 = egui::Color32::from_rgb(0x30, 0x36, 0x3d);
    /// Active item background tint (sidebar, selected states).
    pub const ACTIVE_BG: egui::Color32 = egui::Color32::from_rgb(0x1c, 0x1c, 0x2e);
    /// Active agent-panel background tint.
    pub const ACTIVE_AGENT_BG: egui::Color32 = egui::Color32::from_rgb(0x1c, 0x2e, 0x1c);

    // -- Text --
    /// Primary text (#c9d1d9).
    pub const TEXT: egui::Color32 = egui::Color32::from_rgb(0xc9, 0xd1, 0xd9);
    /// Secondary / muted text (#8b949e).
    pub const TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(0x8b, 0x94, 0x9e);
    /// Dimmed / disabled text (#6e7681).
    pub const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(0x6e, 0x76, 0x81);
    /// Very dim / placeholder text (#484f58).
    pub const TEXT_FAINT: egui::Color32 = egui::Color32::from_rgb(0x48, 0x4f, 0x58);

    // -- Accent --
    /// Brand purple / primary accent (#8b5cf6).
    pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x8b, 0x5c, 0xf6);

    // -- Status --
    /// Success / online / idle green (#3fb950).
    pub const GREEN: egui::Color32 = egui::Color32::from_rgb(0x3f, 0xb9, 0x50);
    /// Warning / connecting / thinking yellow (#d29922).
    pub const YELLOW: egui::Color32 = egui::Color32::from_rgb(0xd2, 0x99, 0x22);
    /// Error / danger red (#ff7b72).
    pub const RED: egui::Color32 = egui::Color32::from_rgb(0xff, 0x7b, 0x72);
    /// Blue info / link (#58a6ff).
    #[allow(dead_code)]
    pub const BLUE: egui::Color32 = egui::Color32::from_rgb(0x58, 0xa6, 0xff);
}

// ---------------------------------------------------------------------------
// Theme setup
// ---------------------------------------------------------------------------

/// Apply the Impulse dark theme to the egui context.
pub fn apply_dark_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = colors::BG;
    visuals.window_fill = colors::SURFACE;
    visuals.extreme_bg_color = colors::BG;
    visuals.faint_bg_color = colors::SURFACE;

    visuals.widgets.noninteractive.bg_fill = colors::SURFACE;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, colors::TEXT);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, colors::BORDER);

    visuals.widgets.inactive.bg_fill = colors::SURFACE;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, colors::TEXT);

    visuals.widgets.hovered.bg_fill = colors::HOVER;
    visuals.widgets.active.bg_fill = colors::BORDER;

    visuals.selection.bg_fill = egui::Color32::from_rgba_premultiplied(0x58, 0xa6, 0xff, 0x40);

    ctx.set_visuals(visuals);
}

/// Return a color associated with an agent name for tab/button rendering.
pub fn agent_color(name: &str) -> egui::Color32 {
    impulse_term::theme::agent_color(name)
}
