//! Dark theme for Impulse GUI.
//!
//! Terminal-specific theming is now in `impulse_term::TerminalTheme`.

use eframe::egui;

/// Apply the Impulse dark theme to the egui context.
pub fn apply_dark_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    // GitHub dark palette.
    let bg = egui::Color32::from_rgb(0x0d, 0x11, 0x17);
    let surface = egui::Color32::from_rgb(0x16, 0x1b, 0x22);
    let border = egui::Color32::from_rgb(0x30, 0x36, 0x3d);
    let text = egui::Color32::from_rgb(0xc9, 0xd1, 0xd9);

    visuals.panel_fill = bg;
    visuals.window_fill = surface;
    visuals.extreme_bg_color = bg;
    visuals.faint_bg_color = surface;

    visuals.widgets.noninteractive.bg_fill = surface;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, text);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, border);

    visuals.widgets.inactive.bg_fill = surface;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, text);

    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(0x21, 0x26, 0x2d);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0x30, 0x36, 0x3d);

    visuals.selection.bg_fill = egui::Color32::from_rgba_premultiplied(0x58, 0xa6, 0xff, 0x40);

    ctx.set_visuals(visuals);
}

/// Return a color associated with an agent name for tab/button rendering.
pub fn agent_color(name: &str) -> egui::Color32 {
    impulse_term::theme::agent_color(name)
}
