//! Bottom status bar — daemon connection, workbench counts, and operator hints.

use std::time::Instant;

use eframe::egui;

use crate::state::{ConnectionStatus, DaemonAutoStartState, SharedState};
use crate::theme::colors;

pub fn show(ctx: &egui::Context, state: &SharedState) {
    egui::TopBottomPanel::bottom("status_bar")
        .exact_height(24.0)
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                // Connection status — enriched with auto-start state.
                let (dot, label): (egui::Color32, &str) = match state.connection {
                    ConnectionStatus::Connected => (colors::GREEN, "Daemon: connected"),
                    ConnectionStatus::Connecting => (colors::YELLOW, "Daemon: connecting..."),
                    ConnectionStatus::Disconnected => match &state.daemon_auto_start {
                        DaemonAutoStartState::Starting => {
                            (colors::YELLOW, "Starting daemon...")
                        }
                        DaemonAutoStartState::BinaryNotFound => {
                            (colors::RED, "impulse-rs not on PATH")
                        }
                        DaemonAutoStartState::Failed(_) => {
                            (colors::RED, "Daemon: failed to start")
                        }
                        DaemonAutoStartState::Running => {
                            (colors::YELLOW, "Daemon: reconnecting...")
                        }
                        DaemonAutoStartState::NotAttempted => {
                            (colors::TEXT_DIM, "Daemon: offline")
                        }
                    },
                };

                let dot_rect = ui.allocate_space(egui::vec2(8.0, 8.0));
                ui.painter().circle_filled(dot_rect.1.center(), 3.0, dot);
                ui.label(egui::RichText::new(label).small().color(colors::TEXT_DIM));

                // Health details when connected
                if state.connection == ConnectionStatus::Connected {
                    if let Some(ref status) = state.daemon_status {
                        let version_label = status
                            .protocol_version
                            .map(|v| format!("v{}", v))
                            .unwrap_or_else(|| "v?".to_string());
                        ui.label(
                            egui::RichText::new(format!(
                                "({}  {} sessions)",
                                version_label, status.sessions
                            ))
                            .small()
                            .color(colors::TEXT_FAINT),
                        );
                    }
                    if let Some(rtt) = state.last_ping_rtt {
                        let rtt_ms = rtt.as_millis();
                        let rtt_color = if rtt_ms < 10 {
                            colors::GREEN
                        } else if rtt_ms < 100 {
                            colors::YELLOW
                        } else {
                            colors::RED
                        };
                        ui.label(
                            egui::RichText::new(format!("{}ms", rtt_ms))
                                .small()
                                .color(rtt_color),
                        );
                    }
                    if let Some(last_poll) = state.last_poll {
                        let age = Instant::now().duration_since(last_poll);
                        let age_label = if age.as_secs() < 2 {
                            "just now".to_string()
                        } else {
                            format!("{}s ago", age.as_secs())
                        };
                        ui.label(
                            egui::RichText::new(age_label)
                                .small()
                                .color(colors::TEXT_FAINT),
                        );
                    }
                }

                ui.separator();

                // Workbench counts.
                if let Some(ref snapshot) = state.ops_snapshot {
                    let blocked = snapshot
                        .interventions
                        .iter()
                        .filter(|item| item.severity == "urgent")
                        .count();
                    ui.label(
                        egui::RichText::new(format!(
                            "Ops: {} agents  {} artifacts  {} reviews  {} blocked",
                            snapshot.agents.len(),
                            snapshot.artifacts.len(),
                            snapshot.context.pending_review_count,
                            blocked
                        ))
                        .small()
                        .color(colors::TEXT_DIM),
                    );
                    ui.separator();
                }

                if let Some(ref snapshot) = state.ops_snapshot {
                    ui.label(
                        egui::RichText::new(format!(
                            "Context: {}  Alerts: {}",
                            snapshot.context.tier,
                            snapshot.interventions.len()
                        ))
                        .small()
                        .color(colors::TEXT_DIM),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("Workbench snapshot pending")
                            .small()
                            .color(colors::TEXT_DIM),
                    );
                }

                // Right-aligned hint.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if state.connection == ConnectionStatus::Disconnected {
                        let hint = match &state.daemon_auto_start {
                            DaemonAutoStartState::BinaryNotFound => {
                                "Offline Mode \u{2014} terminal multiplexing available. Install impulse-rs for full features."
                            }
                            DaemonAutoStartState::Starting => {
                                "Starting daemon \u{2014} session tracking will activate shortly."
                            }
                            DaemonAutoStartState::Failed(_) => {
                                "Offline Mode \u{2014} panes still work, but memory/history are reduced."
                            }
                            _ => {
                                "Offline Mode \u{2014} panes still work, but memory/history/ops are reduced."
                            }
                        };
                        ui.label(
                            egui::RichText::new(hint)
                                .small()
                                .color(colors::YELLOW),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new(
                                "Ctrl+1-7: views  Ctrl+N: agent  Ctrl+L: panel  Ctrl+B: sidebar",
                            )
                            .small()
                            .color(colors::TEXT_FAINT),
                        );
                    }
                });
            });
        });
}
