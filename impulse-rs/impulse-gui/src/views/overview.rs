use eframe::egui;

use super::{View, ViewId};
use crate::state::{ConnectionStatus, SharedState};
use crate::theme::colors;
use crate::widgets::signal_bus::SignalUrgency;

pub struct OverviewView;

impl OverviewView {
    pub fn new() -> Self {
        Self
    }
}

impl View for OverviewView {
    fn id(&self) -> ViewId {
        ViewId::Overview
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, _ctx: &egui::Context) {
        let Some(snapshot) = state.ops_snapshot.as_ref() else {
            ui.heading(egui::RichText::new("Impulse Overview").color(colors::TEXT));
            ui.separator();

            match state.connection {
                ConnectionStatus::Disconnected => {
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(
                            "Offline Mode \u{2014} terminal multiplexing is available. \
                             Session tracking and memory features require the daemon.",
                        )
                        .color(colors::YELLOW),
                    );
                    ui.add_space(12.0);

                    // Show local-only info: session count from shared state.
                    ui.horizontal_wrapped(|ui| {
                        summary_card(
                            ui,
                            "Sessions",
                            format!("{}", state.sessions.len()),
                            "Daemon-tracked sessions (available when connected).",
                            colors::TEXT_DIM,
                        );
                        summary_card(
                            ui,
                            "History",
                            format!("{} entries", state.history.len()),
                            "Past session entries.",
                            colors::TEXT_DIM,
                        );
                    });
                }
                _ => {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(
                                "Waiting for daemon snapshot from the thin workbench runtime...",
                            )
                            .color(colors::TEXT_DIM),
                        );
                    });
                }
            }
            return;
        };

        let agent_count = snapshot.agents.len();
        let pending_reviews = snapshot.context.pending_review_count;

        ui.heading(
            egui::RichText::new(format!("{} Thin Workbench", snapshot.project.name))
                .color(colors::TEXT),
        );
        ui.label(
            egui::RichText::new(snapshot.project.root_path.clone())
                .small()
                .color(colors::TEXT_DIM),
        );
        ui.label(
            egui::RichText::new(
                "Hooks and the daemon are the source of truth. Coordination telemetry shown here is advisory until validated against real Claude sessions.",
            )
            .small()
            .color(colors::YELLOW),
        );
        ui.add_space(10.0);

        ui.horizontal_wrapped(|ui| {
            summary_card(
                ui,
                "Agents",
                format!("{}", agent_count),
                "Live terminals plus daemon-tracked sessions.",
                colors::ACCENT,
            );
            summary_card(
                ui,
                "Memory",
                format!(
                    "{} active / {} history",
                    snapshot.memory.active_sessions, snapshot.memory.history_entries
                ),
                "Session continuity plus genome-backed decisions.",
                colors::GREEN,
            );
            summary_card(
                ui,
                "Context",
                if pending_reviews > 0 {
                    format!("{} review pending", pending_reviews)
                } else {
                    snapshot.context.tier.clone()
                },
                "Experimental telemetry for pressure, compaction risk, and staged review artifacts.",
                if pending_reviews > 0 {
                    colors::YELLOW
                } else {
                    colors::BLUE
                },
            );
            summary_card(
                ui,
                "Artifacts",
                format!("{}", snapshot.artifacts.len()),
                "Experimental daemon review material, not proof of recall or coordination.",
                colors::TEXT,
            );
        });

        ui.add_space(12.0);
        ui.columns(2, |columns| {
            columns[0].group(|ui| {
                ui.label(
                    egui::RichText::new("Active Interventions")
                        .strong()
                        .color(colors::ACCENT),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Experimental only. These items do not prove automated coordination is working.")
                        .small()
                        .color(colors::TEXT_DIM),
                );
                ui.add_space(6.0);
                if snapshot.interventions.is_empty() {
                    ui.label(
                        egui::RichText::new("No active daemon interventions.")
                            .small()
                            .color(colors::TEXT_DIM),
                    );
                } else {
                    for intervention in snapshot.interventions.iter().take(8) {
                        intervention_row(ui, intervention);
                    }
                }
            });

            columns[1].group(|ui| {
                ui.label(
                    egui::RichText::new("Recent Ops Events")
                        .strong()
                        .color(colors::ACCENT),
                );
                ui.add_space(6.0);
                if state.ops_events.is_empty() {
                    ui.label(
                        egui::RichText::new("No recent operator events.")
                            .small()
                            .color(colors::TEXT_DIM),
                    );
                } else {
                    for event in state.ops_events.iter().take(8) {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(egui::RichText::new(&event.title).small().color(
                                match event.severity.as_str() {
                                    "urgent" => colors::RED,
                                    "warning" => colors::YELLOW,
                                    _ => colors::TEXT,
                                },
                            ));
                            if !event.detail.is_empty() {
                                ui.label(
                                    egui::RichText::new(&event.detail)
                                        .small()
                                        .color(colors::TEXT_DIM),
                                );
                            }
                        });
                        ui.add_space(4.0);
                    }
                }
            });
        });

        // Signal history log — recent GUI signals from the signal bus.
        if !state.signal_log.is_empty() {
            ui.add_space(12.0);
            ui.group(|ui| {
                ui.label(
                    egui::RichText::new("Signal History")
                        .strong()
                        .color(colors::ACCENT),
                );
                ui.add_space(6.0);
                // Show most recent first, capped at 10 for the overview.
                for entry in state.signal_log.iter().rev().take(10) {
                    ui.horizontal_wrapped(|ui| {
                        let urgency_color = match entry.urgency {
                            SignalUrgency::Urgent => colors::RED,
                            SignalUrgency::Important => colors::YELLOW,
                            SignalUrgency::Ambient => colors::TEXT_DIM,
                        };
                        ui.label(
                            egui::RichText::new(&entry.kind_label)
                                .small()
                                .strong()
                                .color(urgency_color),
                        );
                        ui.label(
                            egui::RichText::new(&entry.message)
                                .small()
                                .color(colors::TEXT_MUTED),
                        );
                        let age_label = if entry.age_secs < 2 {
                            "just now".to_string()
                        } else if entry.age_secs < 60 {
                            format!("{}s ago", entry.age_secs)
                        } else {
                            format!("{}m ago", entry.age_secs / 60)
                        };
                        ui.label(
                            egui::RichText::new(age_label)
                                .small()
                                .color(colors::TEXT_FAINT),
                        );
                    });
                    ui.add_space(2.0);
                }
            });
        }
    }
}

fn summary_card(
    ui: &mut egui::Ui,
    title: &str,
    value: String,
    description: &str,
    accent: egui::Color32,
) {
    egui::Frame::new()
        .fill(colors::SURFACE)
        .corner_radius(egui::CornerRadius::same(8))
        .stroke(egui::Stroke::new(1.0, colors::BORDER))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_min_width(220.0);
            ui.label(egui::RichText::new(title).small().color(colors::TEXT_DIM));
            ui.label(egui::RichText::new(value).heading().color(accent));
            ui.label(
                egui::RichText::new(description)
                    .small()
                    .color(colors::TEXT_MUTED),
            );
        });
}

fn intervention_row(ui: &mut egui::Ui, recommendation: &impulse_ops::InterventionRecommendation) {
    egui::Frame::new()
        .fill(colors::BG)
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(&recommendation.title).strong().color(
                match recommendation.severity.as_str() {
                    "urgent" => colors::RED,
                    "warning" => colors::YELLOW,
                    _ => colors::TEXT,
                },
            ));
            ui.label(
                egui::RichText::new(&recommendation.description)
                    .small()
                    .color(colors::TEXT_MUTED),
            );
            ui.label(
                egui::RichText::new(&recommendation.action_label)
                    .small()
                    .color(colors::ACCENT),
            );
        });
    ui.add_space(6.0);
}
