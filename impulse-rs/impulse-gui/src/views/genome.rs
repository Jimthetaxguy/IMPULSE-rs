//! Genome view — displays decisions, preferences, and constraints.
//!
//! Shows a timeline of decisions with cards. Includes filter search,
//! decision cards with date headers, tags, and rationale.

use eframe::egui;

use super::{View, ViewId};
use crate::state::{ConnectionStatus, SharedState};
use crate::theme::colors;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Decisions,
    Raw,
}

pub struct GenomeView {
    tab: Tab,
    filter: String,
}

impl GenomeView {
    pub fn new() -> Self {
        Self {
            tab: Tab::Decisions,
            filter: String::new(),
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
                    ui.label(egui::RichText::new("Loading genome data...").color(colors::TEXT_DIM));
                });
                return;
            }
        };

        // --- Tab selector + filter ---
        ui.horizontal(|ui| {
            let dec_count = genome.decisions.len();
            if ui
                .selectable_label(
                    self.tab == Tab::Decisions,
                    egui::RichText::new(format!("Decisions ({})", dec_count)).color(
                        if self.tab == Tab::Decisions {
                            colors::ACCENT
                        } else {
                            colors::TEXT_MUTED
                        },
                    ),
                )
                .clicked()
            {
                self.tab = Tab::Decisions;
            }
            if ui
                .selectable_label(
                    self.tab == Tab::Raw,
                    egui::RichText::new("Raw").color(if self.tab == Tab::Raw {
                        colors::ACCENT
                    } else {
                        colors::TEXT_MUTED
                    }),
                )
                .clicked()
            {
                self.tab = Tab::Raw;
            }

            ui.separator();

            if self.tab == Tab::Decisions {
                let filter_edit = egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Filter decisions...")
                    .desired_width(160.0);
                ui.add(filter_edit);
            }

            if !genome.last_updated.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("Updated: {}", genome.last_updated))
                            .small()
                            .color(colors::TEXT_DIM),
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
                                        .color(colors::TEXT_DIM),
                                );
                            });
                            return;
                        }

                        let filter_lower = self.filter.to_lowercase();
                        let mut shown = 0;

                        for decision in &genome.decisions {
                            // Filter by description, rationale, or tags.
                            if !self.filter.is_empty()
                                && !decision.description.to_lowercase().contains(&filter_lower)
                                && !decision.rationale.to_lowercase().contains(&filter_lower)
                                && !decision
                                    .tags
                                    .iter()
                                    .any(|t| t.to_lowercase().contains(&filter_lower))
                            {
                                continue;
                            }

                            shown += 1;
                            ui.add_space(6.0);

                            // Decision card with accent border.
                            egui::Frame::new()
                                .fill(colors::SURFACE)
                                .corner_radius(egui::CornerRadius::same(6))
                                .inner_margin(egui::Margin::symmetric(12, 10))
                                .stroke(egui::Stroke::new(0.5, colors::BORDER))
                                .show(ui, |ui| {
                                    // Date header with accent color.
                                    if !decision.date.is_empty() {
                                        ui.label(
                                            egui::RichText::new(&decision.date)
                                                .small()
                                                .color(colors::ACCENT),
                                        );
                                        ui.add_space(4.0);
                                    }

                                    // Description.
                                    ui.label(
                                        egui::RichText::new(&decision.description)
                                            .color(colors::TEXT),
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
                                            .color(colors::TEXT_MUTED),
                                        );
                                    }

                                    // Tags as inline badges.
                                    if !decision.tags.is_empty() {
                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            for tag in &decision.tags {
                                                egui::Frame::new()
                                                    .fill(colors::BLUE.gamma_multiply(0.12))
                                                    .corner_radius(egui::CornerRadius::same(3))
                                                    .inner_margin(egui::Margin::symmetric(4, 1))
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            egui::RichText::new(tag)
                                                                .small()
                                                                .color(colors::BLUE),
                                                        );
                                                    });
                                            }
                                        });
                                    }
                                });
                        }

                        if shown == 0 && !self.filter.is_empty() {
                            ui.add_space(32.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "No decisions match \"{}\"",
                                        self.filter
                                    ))
                                    .color(colors::TEXT_DIM),
                                );
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
                                    .color(colors::TEXT_DIM),
                            );
                        } else {
                            egui::Frame::new()
                                .fill(colors::SURFACE)
                                .corner_radius(egui::CornerRadius::same(4))
                                .inner_margin(egui::Margin::symmetric(10, 8))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(&genome.raw_text)
                                            .monospace()
                                            .color(colors::TEXT),
                                    );
                                });
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
            egui::RichText::new("Genome data requires a running daemon.").color(colors::TEXT_DIM),
        );
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Run `impulse daemon` to start the background service.")
                .small()
                .color(colors::TEXT_FAINT),
        );
    });
}
