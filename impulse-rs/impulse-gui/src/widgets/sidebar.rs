//! Collapsible navigation sidebar.
//!
//! 48px when collapsed (icons only), 160px when expanded (icons + labels).
//! Toggle with `Ctrl+B`.

use eframe::egui;

use crate::state::{ConnectionStatus, SharedState};
use crate::views::ViewId;

const COLLAPSED_WIDTH: f32 = 48.0;
const EXPANDED_WIDTH: f32 = 160.0;

/// Render the sidebar and return the newly-selected view (if changed).
pub fn show(
    ctx: &egui::Context,
    active: ViewId,
    expanded: bool,
    state: &SharedState,
) -> Option<ViewId> {
    let mut selected = None;
    let width = if expanded {
        EXPANDED_WIDTH
    } else {
        COLLAPSED_WIDTH
    };

    egui::SidePanel::left("sidebar")
        .resizable(false)
        .exact_width(width)
        .show(ctx, |ui| {
            ui.add_space(8.0);

            // Logo / brand.
            if expanded {
                ui.horizontal(|ui| {
                    ui.strong(
                        egui::RichText::new("IMPULSE")
                            .color(egui::Color32::from_rgb(0x8b, 0x5c, 0xf6)),
                    );
                });
            } else {
                ui.vertical_centered(|ui| {
                    ui.strong(
                        egui::RichText::new("I")
                            .color(egui::Color32::from_rgb(0x8b, 0x5c, 0xf6))
                            .size(18.0),
                    );
                });
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // Navigation items.
            for &view_id in ViewId::all() {
                let is_active = view_id == active;

                let text = if expanded {
                    format!("{}  {}", view_id.icon(), view_id.title())
                } else {
                    view_id.icon().to_string()
                };

                let color = if is_active {
                    egui::Color32::from_rgb(0x8b, 0x5c, 0xf6)
                } else {
                    egui::Color32::from_rgb(0x8b, 0x94, 0x9e)
                };

                let btn = egui::Button::new(egui::RichText::new(&text).color(color))
                    .fill(if is_active {
                        egui::Color32::from_rgb(0x1c, 0x1c, 0x2e)
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .min_size(egui::vec2(width - 16.0, 32.0));

                let resp = ui.add(btn);
                if resp.clicked() {
                    selected = Some(view_id);
                }
                if !expanded {
                    resp.on_hover_text(format!(
                        "{} ({})",
                        view_id.title(),
                        view_id.shortcut_label()
                    ));
                }
            }

            // Push remaining content to bottom.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.add_space(8.0);

                // Connection status indicator.
                let (dot_color, label) = match state.connection {
                    ConnectionStatus::Connected => {
                        (egui::Color32::from_rgb(0x3f, 0xb9, 0x50), "Online")
                    }
                    ConnectionStatus::Connecting => {
                        (egui::Color32::from_rgb(0xd2, 0x99, 0x22), "Connecting")
                    }
                    ConnectionStatus::Disconnected => {
                        (egui::Color32::from_rgb(0x6e, 0x76, 0x81), "Offline")
                    }
                };

                ui.horizontal(|ui| {
                    let dot = ui.allocate_space(egui::vec2(10.0, 10.0));
                    ui.painter().circle_filled(dot.1.center(), 4.0, dot_color);
                    if expanded {
                        ui.label(
                            egui::RichText::new(label)
                                .small()
                                .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                        );
                    }
                });
            });
        });

    selected
}
