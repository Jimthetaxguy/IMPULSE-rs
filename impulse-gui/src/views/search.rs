//! Search view — query the daemon's retrieval system.
//!
//! Text input with Enter-to-submit, results list sorted by relevance score.

use eframe::egui;

use super::{View, ViewId};
use crate::state::{PollerCommand, SharedState};
use crate::theme::colors;

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

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
    }

    fn submit(&self) {
        if !self.query.trim().is_empty() {
            let _ = self.cmd_tx.send(PollerCommand::Search(self.query.clone()));
        }
    }

    pub fn submit_current(&self) {
        self.submit();
    }
}

impl View for SearchView {
    fn id(&self) -> ViewId {
        ViewId::Memory
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, _ctx: &egui::Context) {
        // --- Search bar (always shown, works with or without daemon) ---
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
                        .color(colors::TEXT_DIM),
                );
            });
            return;
        }

        // Error display.
        if let Some(ref err) = state.error {
            ui.colored_label(colors::RED, err);
            ui.add_space(4.0);
        }

        ui.separator();

        // --- Live insight results (current session) ---
        let live_results = &state.live_search_results;
        let daemon_results = &state.search_results;
        let total = live_results.len() + daemon_results.len();

        if total == 0 && !state.search_query.is_empty() {
            ui.add_space(32.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("No results found.").color(colors::TEXT_DIM));
            });
            return;
        }

        if total == 0 {
            ui.add_space(32.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(
                        "Enter a query to search session history, live insights, genome, and more.",
                    )
                    .color(colors::TEXT_DIM),
                );
            });
            return;
        }

        ui.label(
            egui::RichText::new(format!(
                "{} results{}",
                total,
                if !live_results.is_empty() {
                    format!(" ({} live)", live_results.len())
                } else {
                    String::new()
                }
            ))
            .small()
            .color(colors::TEXT_DIM),
        );

        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .id_salt("search_results")
            .show(ui, |ui| {
                // Live results first (most recent/relevant).
                for result in live_results {
                    ui.add_space(4.0);
                    render_live_result(ui, result);
                }

                // Daemon results.
                for result in daemon_results {
                    ui.add_space(4.0);
                    render_daemon_result(ui, result);
                }
            });
    }
}

use crate::ipc::SearchResult;
use crate::state::LiveSearchResult;

/// Render a live insight search result with a "Live" badge.
fn render_live_result(ui: &mut egui::Ui, result: &LiveSearchResult) {
    egui::Frame::new()
        .fill(colors::SURFACE)
        .corner_radius(4.0)
        .inner_margin(8.0)
        .stroke(egui::Stroke::new(0.5, colors::ACCENT.gamma_multiply(0.3)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&result.title)
                        .strong()
                        .color(colors::TEXT),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("Live")
                            .small()
                            .strong()
                            .color(colors::GREEN),
                    );
                    ui.label(
                        egui::RichText::new(&result.agent)
                            .small()
                            .color(colors::TEXT_DIM),
                    );
                });
            });
        });
}

/// Render a daemon search result.
fn render_daemon_result(ui: &mut egui::Ui, result: &SearchResult) {
    egui::Frame::new()
        .fill(colors::SURFACE)
        .corner_radius(4.0)
        .inner_margin(8.0)
        .stroke(egui::Stroke::new(0.5, colors::BORDER))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&result.title)
                        .strong()
                        .color(colors::TEXT),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{:.0}%", result.score * 100.0))
                            .small()
                            .color(colors::ACCENT),
                    );
                    ui.label(
                        egui::RichText::new(&result.source_type)
                            .small()
                            .color(colors::TEXT_DIM),
                    );
                });
            });

            if !result.snippet.is_empty() {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(&result.snippet)
                        .small()
                        .color(colors::TEXT_MUTED),
                );
            }
        });
}
