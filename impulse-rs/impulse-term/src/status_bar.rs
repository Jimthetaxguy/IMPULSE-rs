//! Status bar widget for `TerminalPanel`.
//!
//! Displays: alive indicator dot, title + dimensions, context tier + usage %,
//! compaction/injection counts, and a Copy screen-text button.

use std::sync::Arc;

use parking_lot::Mutex;

use eframe::egui;

use crate::backend::TerminalBackend;
use crate::context::{ContextBridge, ContextHealth, ContextTier};
use crate::theme::TerminalTheme;

/// Assembled status bar for a terminal panel.
pub struct StatusBar {
    backend: Arc<TerminalBackend>,
    context: Arc<Mutex<ContextBridge>>,
    title: String,
    theme: TerminalTheme,
}

impl StatusBar {
    /// Construct a new status bar.
    pub fn new(
        backend: Arc<TerminalBackend>,
        context: Arc<Mutex<ContextBridge>>,
        title: String,
        theme: TerminalTheme,
    ) -> Self {
        Self {
            backend,
            context,
            title,
            theme,
        }
    }

    /// Render the status bar. Copy button is wired directly — no return value needed.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let health = self.context.lock().health();

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            // Alive dot.
            let alive_color = if self.backend.is_alive() {
                egui::Color32::from_rgb(0x3f, 0xb9, 0x50) // green
            } else {
                egui::Color32::from_rgb(0x6e, 0x76, 0x81) // muted
            };
            let dot_rect = ui.allocate_space(egui::vec2(8.0, 8.0));
            ui.painter()
                .circle_filled(dot_rect.1.center(), 3.0, alive_color);

            // Title + dimensions.
            let (cols, rows) = self.backend.size();
            ui.label(
                egui::RichText::new(format!("{} {}x{}", self.title, cols, rows))
                    .small()
                    .color(egui::Color32::from_rgb(0x8b, 0x94, 0x9e)),
            );

            ui.separator();

            // Context health: tier icon + usage %.
            let (tier_icon, tier_color) = context_tier_indicator(&health, &self.theme);
            let usage_pct = (health.usage_fraction * 100.0) as u8;
            ui.label(
                egui::RichText::new(format!(
                    "{} {}% {}",
                    tier_icon,
                    usage_pct,
                    health.tier.as_str()
                ))
                .small()
                .color(tier_color),
            );

            ui.separator();

            // Compaction / injection counters.
            ui.label(
                egui::RichText::new(format!(
                    "\u{2193}{} \u{2191}{}",
                    health.compaction_count, health.injection_count
                ))
                .small()
                .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
            );

            ui.separator();

            // Copy button — copies visible screen text to clipboard.
            // Note: we call copy_text directly inside the closure where ui is mutable.
            // The old return-value approach was broken because ui.horizontal() returns ().
            if ui
                .small_button(
                    egui::RichText::new("Copy")
                        .small()
                        .color(egui::Color32::from_rgb(0x8b, 0x94, 0x9e)),
                )
                .on_hover_text("Copy screen text (Ctrl+Shift+X)")
                .clicked()
            {
                let text = self.backend.screen_text();
                ui.ctx().copy_text(text);
            }
        });
    }
}

/// Map context tier to a status indicator (icon, color).
fn context_tier_indicator(
    health: &ContextHealth,
    theme: &TerminalTheme,
) -> (&'static str, egui::Color32) {
    match health.tier {
        ContextTier::None | ContextTier::Full => {
            ("\u{25CF}", theme.context_health.comfortable) // ●
        }
        ContextTier::Essential => ("\u{25D0}", theme.context_health.essential), // ◐
        ContextTier::Critical => ("\u{25D1}", theme.context_health.critical),   // ◑
        ContextTier::Minimal => ("\u{25CB}", theme.context_health.minimal),     // ○
        ContextTier::PostCompaction => {
            ("\u{25CE}", theme.context_health.essential) // ◎
        }
    }
}
