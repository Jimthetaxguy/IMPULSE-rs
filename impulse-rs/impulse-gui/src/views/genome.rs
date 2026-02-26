//! Genome view — displays decisions, preferences, and constraints.
//!
//! Shows a timeline of decisions with cards. Read-only in the first pass;
//! editing planned as a future enhancement.

use eframe::egui;

use super::{View, ViewId};
use crate::state::{ConnectionStatus, SharedState};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Decisions,
    Raw,
}

pub struct GenomeView {
    tab: Tab,
}

impl GenomeView {
    pub fn new() -> Self {
        Self {
            tab: Tab::Decisions,
        }
    }
}

impl View for GenomeView {
    fn id(&self) -> ViewId {
        ViewId::Genome
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, _ctx: &egui::Context) {
        if state.connection == ConnectionStatus::Disconnected {
            empty_state(ui);
            return;
        }

        let genome = match state.genome.as_ref() {
            Some(g) => g,
            None => {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.label(
                        egui::RichText::new("Loading genome data...")
                            .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                    );
                });
                return;
            }
        };

        // --- Tab selector ---
        ui.horizontal(|ui| {
            if ui
                .selectable_label(
                    self.tab == Tab::Decisions,
                    format!("Decisions ({})", genome.decisions.len()),
                )
                .clicked()
            {
                self.tab = Tab::Decisions;
            }
            if ui.selectable_label(self.tab == Tab::Raw, "Raw").clicked() {
                self.tab = Tab::Raw;
            }

            if !genome.last_updated.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("Updated: {}", genome.last_updated))
                            .small()
                            .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                    );
                });
            }
        });

        ui.separator();

        match self.tab {
            Tab::Decisions => {
                egui::ScrollArea::vertical()
                    .id_salt("genome_decisions")
                    .show(ui, |ui| {
                        if genome.decisions.is_empty() {
                            ui.add_space(32.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new("No decisions recorded yet.")
                                        .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                                );
                            });
                            return;
                        }

                        for decision in &genome.decisions {
                            ui.add_space(8.0);

                            // Decision card.
                            egui::Frame::new()
                                .fill(egui::Color32::from_rgb(0x16, 0x1b, 0x22))
                                .corner_radius(6.0)
                                .inner_margin(12.0)
                                .stroke(egui::Stroke::new(
                                    0.5,
                                    egui::Color32::from_rgb(0x30, 0x36, 0x3d),
                                ))
                                .show(ui, |ui| {
                                    // Date header.
                                    if !decision.date.is_empty() {
                                        ui.label(
                                            egui::RichText::new(&decision.date)
                                                .small()
                                                .color(egui::Color32::from_rgb(0x8b, 0x5c, 0xf6)),
                                        );
                                        ui.add_space(4.0);
                                    }

                                    // Description.
                                    ui.label(
                                        egui::RichText::new(&decision.description)
                                            .color(egui::Color32::from_rgb(0xc9, 0xd1, 0xd9)),
                                    );

                                    // Rationale.
                                    if !decision.rationale.is_empty() {
                                        ui.add_space(4.0);
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "Rationale: {}",
                                                decision.rationale
                                            ))
                                            .small()
                                            .color(egui::Color32::from_rgb(0x8b, 0x94, 0x9e)),
                                        );
                                    }

                                    // Tags.
                                    if !decision.tags.is_empty() {
                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            for tag in &decision.tags {
                                                ui.label(
                                                    egui::RichText::new(format!("[{}]", tag))
                                                        .small()
                                                        .color(egui::Color32::from_rgb(
                                                            0x58, 0xa6, 0xff,
                                                        )),
                                                );
                                            }
                                        });
                                    }
                                });
                        }
                    });
            }
            Tab::Raw => {
                egui::ScrollArea::vertical()
                    .id_salt("genome_raw")
                    .show(ui, |ui| {
                        if genome.raw_text.is_empty() {
                            ui.label(
                                egui::RichText::new("No raw genome content available.")
                                    .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new(&genome.raw_text)
                                    .monospace()
                                    .color(egui::Color32::from_rgb(0xc9, 0xd1, 0xd9)),
                            );
                        }
                    });
            }
        }
    }
}

fn empty_state(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.label(
            egui::RichText::new("Genome data requires a running daemon.")
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
