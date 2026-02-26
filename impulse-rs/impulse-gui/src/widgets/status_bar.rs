//! Bottom status bar — daemon connection, session counts, hints.

use eframe::egui;

use crate::state::{ConnectionStatus, SharedState};

pub fn show(ctx: &egui::Context, state: &SharedState, terminal_tabs: usize) {
    egui::TopBottomPanel::bottom("status_bar")
        .exact_height(24.0)
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                // Connection status.
                let (dot, label) = match state.connection {
                    ConnectionStatus::Connected => (
                        egui::Color32::from_rgb(0x3f, 0xb9, 0x50),
                        "Daemon: connected",
                    ),
                    ConnectionStatus::Connecting => (
                        egui::Color32::from_rgb(0xd2, 0x99, 0x22),
                        "Daemon: connecting...",
                    ),
                    ConnectionStatus::Disconnected => {
                        (egui::Color32::from_rgb(0x6e, 0x76, 0x81), "Daemon: offline")
                    }
                };

                let dot_rect = ui.allocate_space(egui::vec2(8.0, 8.0));
                ui.painter().circle_filled(dot_rect.1.center(), 3.0, dot);
                ui.label(
                    egui::RichText::new(label)
                        .small()
                        .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                );

                ui.separator();

                // Session counts.
                if let Some(ref status) = state.daemon_status {
                    ui.label(
                        egui::RichText::new(format!(
                            "Sessions: {} active / {} total",
                            status.active, status.sessions
                        ))
                        .small()
                        .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                    );
                    ui.separator();
                }

                // Terminal tabs.
                ui.label(
                    egui::RichText::new(format!("Terminals: {}", terminal_tabs))
                        .small()
                        .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                );

                // Right-aligned hint.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if state.connection == ConnectionStatus::Disconnected {
                        ui.label(
                            egui::RichText::new("Run `impulse daemon` to connect")
                                .small()
                                .color(egui::Color32::from_rgb(0xd2, 0x99, 0x22)),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("Ctrl+B: toggle sidebar")
                                .small()
                                .color(egui::Color32::from_rgb(0x48, 0x4f, 0x58)),
                        );
                    }
                });
            });
        });
}
