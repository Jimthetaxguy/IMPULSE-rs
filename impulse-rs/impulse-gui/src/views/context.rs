use eframe::egui;

use super::{View, ViewId};
use crate::state::SharedState;
use crate::theme::colors;

pub struct ContextView;

impl ContextView {
    pub fn new() -> Self {
        Self
    }
}

impl View for ContextView {
    fn id(&self) -> ViewId {
        ViewId::Context
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, _ctx: &egui::Context) {
        let Some(snapshot) = state.ops_snapshot.as_ref() else {
            ui.label(
                egui::RichText::new("No daemon-backed context telemetry is available yet.")
                    .color(colors::TEXT_DIM),
            );
            return;
        };

        ui.heading(egui::RichText::new("Experimental Context Telemetry").color(colors::TEXT));
        ui.label(
            egui::RichText::new(
                "Read-only telemetry for context pressure, recent insights, compaction, and staged review work.",
            )
            .small()
            .color(colors::TEXT_DIM),
        );
        ui.label(
            egui::RichText::new(
                "This view does not prove injection or coordination is working end-to-end. Hook validation remains the source of truth.",
            )
            .small()
            .color(colors::YELLOW),
        );
        ui.add_space(10.0);

        let workspace_context = &snapshot.context;

        ui.horizontal_wrapped(|ui| {
            context_metric(ui, "Tier", workspace_context.tier.clone(), colors::ACCENT);
            context_metric(
                ui,
                "Compactions",
                workspace_context.compaction_count.to_string(),
                colors::YELLOW,
            );
            context_metric(
                ui,
                "Injections",
                workspace_context.injection_count.to_string(),
                colors::GREEN,
            );
            context_metric(
                ui,
                "Pending Review",
                workspace_context.pending_review_count.to_string(),
                if workspace_context.pending_review_count > 0 {
                    colors::YELLOW
                } else {
                    colors::TEXT
                },
            );
        });

        ui.add_space(12.0);
        ui.columns(2, |columns| {
            columns[0].group(|ui| {
                ui.label(
                    egui::RichText::new("Active Agent Context")
                        .strong()
                        .color(colors::ACCENT),
                );
                ui.add_space(6.0);
                if snapshot.agents.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "Open agent terminals to see per-agent context health.",
                        )
                        .small()
                        .color(colors::TEXT_DIM),
                    );
                } else {
                    for agent in &snapshot.agents {
                        egui::Frame::new()
                            .fill(colors::BG)
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(&agent.label)
                                            .strong()
                                            .color(colors::TEXT),
                                    );
                                    ui.label(
                                        egui::RichText::new(&agent.backend_kind)
                                            .small()
                                            .color(colors::TEXT_DIM),
                                    );
                                });
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Tier: {}  Tokens: {}/{}  Review: {}",
                                        agent.context.tier,
                                        agent.context.estimated_tokens,
                                        agent.context.window_tokens,
                                        agent.context.pending_review_count
                                    ))
                                    .small()
                                    .color(colors::TEXT_MUTED),
                                );
                                for warning in &agent.warnings {
                                    ui.label(
                                        egui::RichText::new(warning).small().color(colors::YELLOW),
                                    );
                                }
                            });
                        ui.add_space(6.0);
                    }
                }
            });

            columns[1].group(|ui| {
                ui.label(
                    egui::RichText::new("Recent Insights")
                        .strong()
                        .color(colors::ACCENT),
                );
                ui.add_space(6.0);
                if workspace_context.recent_insights.is_empty() {
                    ui.label(
                        egui::RichText::new("No insights captured yet.")
                            .small()
                            .color(colors::TEXT_DIM),
                    );
                } else {
                    for insight in workspace_context.recent_insights.iter().take(12) {
                        egui::Frame::new()
                            .fill(colors::SURFACE)
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "[{}] {}",
                                        insight.agent_label, insight.kind
                                    ))
                                    .small()
                                    .color(colors::ACCENT),
                                );
                                ui.label(
                                    egui::RichText::new(&insight.content)
                                        .small()
                                        .color(colors::TEXT),
                                );
                                if let Some(timestamp) = &insight.timestamp {
                                    ui.label(
                                        egui::RichText::new(timestamp)
                                            .small()
                                            .color(colors::TEXT_DIM),
                                    );
                                }
                            });
                        ui.add_space(6.0);
                    }
                }
            });
        });
    }
}

fn context_metric(ui: &mut egui::Ui, label: &str, value: String, accent: egui::Color32) {
    egui::Frame::new()
        .fill(colors::SURFACE)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_min_width(160.0);
            ui.label(egui::RichText::new(label).small().color(colors::TEXT_DIM));
            ui.label(egui::RichText::new(value).strong().color(accent));
        });
}
