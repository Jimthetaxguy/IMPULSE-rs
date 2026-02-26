//! Sessions view — active sessions and history timeline.
//!
//! Left panel: scrollable session list (active + history).
//! Right panel: detail view for the selected item.

use chrono::{DateTime, Utc};
use eframe::egui;

use super::{View, ViewId};
use crate::state::{ConnectionStatus, SharedState};
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Active,
    History,
}

pub struct SessionsView {
    tab: Tab,
    selected_id: Option<String>,
    filter: String,
}

impl SessionsView {
    pub fn new() -> Self {
        Self {
            tab: Tab::Active,
            selected_id: None,
            filter: String::new(),
        }
    }
}

impl View for SessionsView {
    fn id(&self) -> ViewId {
        ViewId::Sessions
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, _ctx: &egui::Context) {
        if state.connection == ConnectionStatus::Disconnected {
            empty_state(ui, "Sessions require a running daemon.");
            return;
        }

        // --- Tab selector + filter ---
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.tab == Tab::Active, "Active")
                .clicked()
            {
                self.tab = Tab::Active;
                self.selected_id = None;
            }
            if ui
                .selectable_label(self.tab == Tab::History, "History")
                .clicked()
            {
                self.tab = Tab::History;
                self.selected_id = None;
            }

            ui.separator();
            ui.label("Filter:");
            ui.text_edit_singleline(&mut self.filter);
        });

        ui.separator();

        // --- Split: list (left) + detail (right) ---
        let available = ui.available_size();
        let list_width = (available.x * 0.4).min(300.0);

        ui.horizontal(|ui| {
            // Left: session list.
            ui.allocate_ui(egui::vec2(list_width, available.y), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("session_list")
                    .show(ui, |ui| {
                        match self.tab {
                            Tab::Active => {
                                let filter_lower = self.filter.to_lowercase();
                                for session in &state.sessions {
                                    if !self.filter.is_empty()
                                        && !session.name.to_lowercase().contains(&filter_lower)
                                        && !session.platform.to_lowercase().contains(&filter_lower)
                                    {
                                        continue;
                                    }

                                    let selected = self.selected_id.as_deref() == Some(&session.id);
                                    let color = platform_color(&session.platform);

                                    ui.horizontal(|ui| {
                                        // Active dot.
                                        let dot = ui.allocate_space(egui::vec2(8.0, 8.0));
                                        ui.painter().circle_filled(
                                            dot.1.center(),
                                            3.5,
                                            if session.status == "active" {
                                                egui::Color32::from_rgb(0x3f, 0xb9, 0x50)
                                            } else {
                                                egui::Color32::from_rgb(0x6e, 0x76, 0x81)
                                            },
                                        );

                                        let text =
                                            egui::RichText::new(&session.name).color(if selected {
                                                color
                                            } else {
                                                egui::Color32::from_rgb(0xc9, 0xd1, 0xd9)
                                            });
                                        if ui.selectable_label(selected, text).clicked() {
                                            self.selected_id = Some(session.id.clone());
                                        }
                                    });

                                    // Subtitle.
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "  {} \u{00b7} {} files \u{00b7} {}",
                                            session.platform,
                                            session.active_files.len(),
                                            format_relative_time(&session.created_at)
                                        ))
                                        .small()
                                        .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                                    );
                                    ui.add_space(4.0);
                                }

                                if state.sessions.is_empty() {
                                    ui.label(
                                        egui::RichText::new("No active sessions")
                                            .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                                    );
                                }
                            }
                            Tab::History => {
                                let filter_lower = self.filter.to_lowercase();
                                for entry in &state.history {
                                    if !self.filter.is_empty()
                                        && !entry
                                            .session_name
                                            .to_lowercase()
                                            .contains(&filter_lower)
                                        && !entry.platform.to_lowercase().contains(&filter_lower)
                                    {
                                        continue;
                                    }

                                    let selected =
                                        self.selected_id.as_deref() == Some(&entry.session_id);

                                    let text = egui::RichText::new(&entry.session_name).color(
                                        if selected {
                                            platform_color(&entry.platform)
                                        } else {
                                            egui::Color32::from_rgb(0xc9, 0xd1, 0xd9)
                                        },
                                    );
                                    if ui.selectable_label(selected, text).clicked() {
                                        self.selected_id = Some(entry.session_id.clone());
                                    }

                                    ui.label(
                                        egui::RichText::new(format!(
                                            "  {} \u{00b7} {} files \u{00b7} {}",
                                            entry.platform,
                                            entry.files_touched.len(),
                                            format_relative_time(&entry.ended_at)
                                        ))
                                        .small()
                                        .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                                    );
                                    ui.add_space(4.0);
                                }

                                if state.history.is_empty() {
                                    ui.label(
                                        egui::RichText::new("No history entries")
                                            .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                                    );
                                }
                            }
                        }
                    });
            });

            ui.separator();

            // Right: detail panel.
            ui.vertical(|ui| {
                if let Some(ref sel_id) = self.selected_id {
                    match self.tab {
                        Tab::Active => {
                            if let Some(session) = state.sessions.iter().find(|s| &s.id == sel_id) {
                                detail_session(ui, session);
                            } else {
                                ui.label("Session not found.");
                            }
                        }
                        Tab::History => {
                            if let Some(entry) =
                                state.history.iter().find(|h| &h.session_id == sel_id)
                            {
                                detail_history(ui, entry);
                            } else {
                                ui.label("History entry not found.");
                            }
                        }
                    }
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 3.0);
                        ui.label(
                            egui::RichText::new("Select a session to view details")
                                .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                        );
                    });
                }
            });
        });
    }
}

fn detail_session(ui: &mut egui::Ui, s: &crate::ipc::Session) {
    ui.heading(&s.name);
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Platform:");
        ui.colored_label(platform_color(&s.platform), &s.platform);
    });
    ui.horizontal(|ui| {
        ui.label("Status:");
        ui.label(&s.status);
    });
    if !s.created_at.is_empty() {
        ui.horizontal(|ui| {
            ui.label("Started:");
            ui.label(&s.created_at);
        });
    }

    ui.add_space(8.0);
    if !s.active_files.is_empty() {
        ui.label(egui::RichText::new("Files:").strong());
        for f in &s.active_files {
            ui.label(format!("  {}", f));
        }
    }

    ui.add_space(4.0);
    if !s.recent_tools.is_empty() {
        ui.label(egui::RichText::new("Tools:").strong());
        for t in &s.recent_tools {
            ui.label(format!("  {}", t));
        }
    }
}

fn detail_history(ui: &mut egui::Ui, h: &crate::ipc::HistoryEntry) {
    ui.heading(&h.session_name);
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Platform:");
        ui.colored_label(platform_color(&h.platform), &h.platform);
    });
    if !h.started_at.is_empty() {
        ui.horizontal(|ui| {
            ui.label("Started:");
            ui.label(&h.started_at);
        });
    }
    if !h.ended_at.is_empty() {
        ui.horizontal(|ui| {
            ui.label("Ended:");
            ui.label(&h.ended_at);
        });
    }
    if !h.summary.is_empty() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Summary:").strong());
        ui.label(&h.summary);
    }

    ui.add_space(8.0);
    if !h.files_touched.is_empty() {
        ui.label(egui::RichText::new("Files touched:").strong());
        for f in &h.files_touched {
            ui.label(format!("  {}", f));
        }
    }

    ui.add_space(4.0);
    if !h.tools_used.is_empty() {
        ui.label(egui::RichText::new("Tools used:").strong());
        for t in &h.tools_used {
            ui.label(format!("  {}", t));
        }
    }
}

fn platform_color(platform: &str) -> egui::Color32 {
    match platform {
        "claude-code" => theme::agent_color("Claude Code"),
        "opencode" => theme::agent_color("OpenCode"),
        "codex" => theme::agent_color("Codex"),
        _ => egui::Color32::from_rgb(0xc9, 0xd1, 0xd9),
    }
}

fn empty_state(ui: &mut egui::Ui, message: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.label(egui::RichText::new(message).color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)));
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Run `impulse daemon` to start the background service.")
                .small()
                .color(egui::Color32::from_rgb(0x48, 0x4f, 0x58)),
        );
    });
}

fn format_relative_time(timestamp: &str) -> String {
    if timestamp.is_empty() {
        return "unknown".to_string();
    }
    // Try to parse ISO 8601 / RFC 3339
    if let Ok(dt) = DateTime::parse_from_rfc3339(timestamp) {
        let now = Utc::now();
        let duration = now.signed_duration_since(dt.with_timezone(&Utc));

        if duration.num_seconds() < 60 {
            return "just now".to_string();
        } else if duration.num_minutes() < 60 {
            return format!("{}m ago", duration.num_minutes());
        } else if duration.num_hours() < 24 {
            return format!("{}h ago", duration.num_hours());
        } else if duration.num_days() < 7 {
            return format!("{}d ago", duration.num_days());
        } else {
            return dt.format("%Y-%m-%d").to_string();
        }
    }
    timestamp.to_string()
}
