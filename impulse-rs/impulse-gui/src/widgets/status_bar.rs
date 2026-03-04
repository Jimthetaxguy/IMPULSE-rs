//! Bottom status bar — daemon connection, session counts, active agents, hints.

use eframe::egui;

use crate::state::{ConnectionStatus, SharedState};
use crate::theme;
use crate::theme::colors;
use crate::widgets::signal_bus::SignalSummary;

/// Info about an active terminal for status bar display.
pub struct ActiveAgent {
    pub name: &'static str,
    pub alive: bool,
}

pub fn show(
    ctx: &egui::Context,
    state: &SharedState,
    terminal_tabs: usize,
    active_agents: &[ActiveAgent],
    signal_summary: &SignalSummary,
) {
    egui::TopBottomPanel::bottom("status_bar")
        .exact_height(24.0)
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                // Connection status.
                let (dot, label) = match state.connection {
                    ConnectionStatus::Connected => (colors::GREEN, "Daemon: connected"),
                    ConnectionStatus::Connecting => (colors::YELLOW, "Daemon: connecting..."),
                    ConnectionStatus::Disconnected => (colors::TEXT_DIM, "Daemon: offline"),
                };

                let dot_rect = ui.allocate_space(egui::vec2(8.0, 8.0));
                ui.painter().circle_filled(dot_rect.1.center(), 3.0, dot);
                ui.label(egui::RichText::new(label).small().color(colors::TEXT_DIM));

                ui.separator();

                // Session counts.
                if let Some(ref status) = state.daemon_status {
                    ui.label(
                        egui::RichText::new(format!(
                            "Sessions: {} / {}",
                            status.active, status.sessions
                        ))
                        .small()
                        .color(colors::TEXT_DIM),
                    );
                    ui.separator();
                }

                // Terminal tabs with active agent badges.
                if !active_agents.is_empty() {
                    for agent in active_agents {
                        let agent_color = if agent.alive {
                            theme::agent_color(agent.name)
                        } else {
                            colors::TEXT_FAINT
                        };
                        let dot_rect = ui.allocate_space(egui::vec2(6.0, 6.0));
                        ui.painter()
                            .circle_filled(dot_rect.1.center(), 2.5, agent_color);
                        ui.label(egui::RichText::new(agent.name).small().color(agent_color));
                        ui.add_space(2.0);
                    }
                } else {
                    ui.label(
                        egui::RichText::new(format!("Terminals: {}", terminal_tabs))
                            .small()
                            .color(colors::TEXT_DIM),
                    );
                }

                // Signal counters.
                if signal_summary.active_conflicts > 0 || signal_summary.unread_errors > 0 {
                    ui.separator();
                }
                if signal_summary.active_conflicts > 0 {
                    ui.label(
                        egui::RichText::new(format!("\u{26A0}{}", signal_summary.active_conflicts))
                            .small()
                            .color(colors::RED),
                    )
                    .on_hover_text("Active file conflicts between panes");
                }
                if signal_summary.unread_errors > 0 {
                    ui.label(
                        egui::RichText::new(format!("\u{2718}{}", signal_summary.unread_errors))
                            .small()
                            .color(colors::RED),
                    )
                    .on_hover_text("Unread agent errors");
                }
                if signal_summary.tasks_completed > 0 {
                    ui.label(
                        egui::RichText::new(format!("\u{2713}{}", signal_summary.tasks_completed))
                            .small()
                            .color(colors::GREEN),
                    )
                    .on_hover_text("Tasks completed this session");
                }

                // Right-aligned hint.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if state.connection == ConnectionStatus::Disconnected {
                        ui.label(
                            egui::RichText::new("Run `impulse daemon` to connect")
                                .small()
                                .color(colors::YELLOW),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("Ctrl+N: new tab  Ctrl+L: agent  Ctrl+B: sidebar")
                                .small()
                                .color(colors::TEXT_FAINT),
                        );
                    }
                });
            });
        });
}
