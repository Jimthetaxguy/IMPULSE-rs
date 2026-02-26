//! Search view — query the daemon's retrieval system.
//!
//! Text input with Enter-to-submit, results list sorted by relevance score.

use eframe::egui;

use super::{View, ViewId};
use crate::state::{ConnectionStatus, PollerCommand, SharedState};

pub struct SearchView {
    query: String,
    cmd_tx: std::sync::mpsc::Sender<PollerCommand>,
}

impl SearchView {
    pub fn new(cmd_tx: std::sync::mpsc::Sender<PollerCommand>) -> Self {
        Self {
            query: String::new(),
            cmd_tx,
        }
    }

    fn submit(&self) {
        if !self.query.trim().is_empty() {
            let _ = self.cmd_tx.send(PollerCommand::Search(self.query.clone()));
        }
    }
}

impl View for SearchView {
    fn id(&self) -> ViewId {
        ViewId::Search
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, _ctx: &egui::Context) {
        if state.connection == ConnectionStatus::Disconnected {
            empty_state(ui);
            return;
        }

        // --- Search bar ---
        ui.horizontal(|ui| {
            ui.label("\u{1f50d}"); // 🔍
            let resp = ui.text_edit_singleline(&mut self.query);
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.submit();
            }
            if ui.button("Search").clicked() {
                self.submit();
            }
        });

        ui.add_space(4.0);

        // Loading indicator.
        if state.search_in_progress {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    egui::RichText::new("Searching...")
                        .small()
                        .color(egui::Color32::from_rgb(0x8b, 0x94, 0x9e)),
                );
            });
            return;
        }

        // Error display.
        if let Some(ref err) = state.error {
            ui.colored_label(egui::Color32::from_rgb(0xff, 0x7b, 0x72), err);
            ui.add_space(4.0);
        }

        ui.separator();

        // --- Results ---
        let results = &state.search_results;

        if results.is_empty() && !state.search_query.is_empty() {
            ui.add_space(32.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("No results found.")
                        .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                );
            });
            return;
        }

        if results.is_empty() {
            ui.add_space(32.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(
                        "Enter a query to search session history, genome, and more.",
                    )
                    .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                );
            });
            return;
        }

        ui.label(
            egui::RichText::new(format!("{} results", results.len()))
                .small()
                .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
        );

        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .id_salt("search_results")
            .show(ui, |ui| {
                for result in results {
                    ui.add_space(4.0);

                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(0x16, 0x1b, 0x22))
                        .corner_radius(4.0)
                        .inner_margin(8.0)
                        .stroke(egui::Stroke::new(
                            0.5,
                            egui::Color32::from_rgb(0x30, 0x36, 0x3d),
                        ))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&result.title)
                                        .strong()
                                        .color(egui::Color32::from_rgb(0xc9, 0xd1, 0xd9)),
                                );

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{:.0}%",
                                                result.score * 100.0
                                            ))
                                            .small()
                                            .color(egui::Color32::from_rgb(0x8b, 0x5c, 0xf6)),
                                        );
                                        ui.label(
                                            egui::RichText::new(&result.source_type)
                                                .small()
                                                .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                                        );
                                    },
                                );
                            });

                            if !result.snippet.is_empty() {
                                ui.add_space(2.0);
                                ui.label(
                                    egui::RichText::new(&result.snippet)
                                        .small()
                                        .color(egui::Color32::from_rgb(0x8b, 0x94, 0x9e)),
                                );
                            }
                        });
                }
            });
    }
}

fn empty_state(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.label(
            egui::RichText::new("Search requires a running daemon.")
                .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
        );
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Run `impulse daemon` to start the background service.")
                .small()
                .color(egui::Color32::from_rgb(0x48, 0x4f, 0x58)),
        );
    });
}
