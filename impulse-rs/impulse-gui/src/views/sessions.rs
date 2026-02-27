//! Sessions view — active sessions and history timeline.
//!
//! Left panel: scrollable session list with cards (active + history).
//! Right panel: detail view for the selected item.

use chrono::{DateTime, Utc};
use eframe::egui;

use super::{View, ViewId};
use crate::state::{ConnectionStatus, SharedState};
use crate::theme;
use crate::theme::colors;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Active,
    History,
}

pub struct SessionsView {
    tab: Tab,
    selected_id: Option<String>,
    filter: String,
    /// New session dialog state.
    show_new_dialog: bool,
    new_name: String,
    new_platform: String,
    /// End session dialog state (for selected active session).
    show_end_dialog: bool,
    end_summary: String,
    /// Status message after create/end action.
    action_status: Option<(String, bool)>,
}

impl SessionsView {
    pub fn new() -> Self {
        Self {
            tab: Tab::Active,
            selected_id: None,
            filter: String::new(),
            show_new_dialog: false,
            new_name: String::new(),
            new_platform: "claude-code".to_string(),
            show_end_dialog: false,
            end_summary: String::new(),
            action_status: None,
        }
    }

    /// Dispatch session creation on a background thread.
    fn create_session(&mut self, name: String, platform: String) {
        self.show_new_dialog = false;
        self.action_status = Some(("Creating session...".to_string(), true));
        std::thread::spawn(move || {
            let mut client = crate::ipc::DaemonClient::discover();
            match client.create_session(&name, Some(&platform)) {
                Ok(session) => {
                    log::info!("Created session: {} ({})", session.name, session.id);
                }
                Err(e) => {
                    log::warn!("Failed to create session: {}", e);
                }
            }
        });
    }

    /// Dispatch session end on a background thread.
    fn end_session(&mut self, session_id: String, summary: String) {
        self.show_end_dialog = false;
        self.action_status = Some(("Ending session...".to_string(), true));
        std::thread::spawn(move || {
            let mut client = crate::ipc::DaemonClient::discover();
            match client.end_session(&session_id, &summary) {
                Ok(()) => {
                    log::info!("Ended session: {}", session_id);
                }
                Err(e) => {
                    log::warn!("Failed to end session {}: {}", session_id, e);
                }
            }
        });
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
            let active_count = state.sessions.len();
            let history_count = state.history.len();

            let active_label = format!("Active ({})", active_count);
            let history_label = format!("History ({})", history_count);

            if ui
                .selectable_label(
                    self.tab == Tab::Active,
                    egui::RichText::new(active_label).color(if self.tab == Tab::Active {
                        colors::ACCENT
                    } else {
                        colors::TEXT_MUTED
                    }),
                )
                .clicked()
            {
                self.tab = Tab::Active;
                self.selected_id = None;
            }
            if ui
                .selectable_label(
                    self.tab == Tab::History,
                    egui::RichText::new(history_label).color(if self.tab == Tab::History {
                        colors::ACCENT
                    } else {
                        colors::TEXT_MUTED
                    }),
                )
                .clicked()
            {
                self.tab = Tab::History;
                self.selected_id = None;
            }

            ui.separator();

            let filter_edit = egui::TextEdit::singleline(&mut self.filter)
                .hint_text("Filter sessions...")
                .desired_width(160.0);
            ui.add(filter_edit);

            // Action buttons (right-aligned).
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("+ New Session").color(colors::GREEN),
                    ))
                    .clicked()
                {
                    self.show_new_dialog = !self.show_new_dialog;
                    if self.show_new_dialog {
                        self.new_name.clear();
                        self.new_platform = "claude-code".to_string();
                    }
                }

                if let Some((ref msg, ok)) = self.action_status {
                    let color = if ok { colors::GREEN } else { colors::RED };
                    ui.label(egui::RichText::new(msg).small().color(color));
                }
            });
        });

        // --- New Session dialog (inline) ---
        if self.show_new_dialog {
            egui::Frame::new()
                .fill(colors::SURFACE)
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .stroke(egui::Stroke::new(1.0, colors::ACCENT))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("New Session")
                                .strong()
                                .color(colors::ACCENT),
                        );
                    });
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_name)
                                .hint_text("session-name")
                                .desired_width(200.0),
                        );
                    });

                    ui.horizontal(|ui| {
                        ui.label("Platform:");
                        egui::ComboBox::from_id_salt("new_session_platform")
                            .selected_text(&self.new_platform)
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.new_platform,
                                    "claude-code".to_string(),
                                    "Claude Code",
                                );
                                ui.selectable_value(
                                    &mut self.new_platform,
                                    "opencode".to_string(),
                                    "OpenCode",
                                );
                                ui.selectable_value(
                                    &mut self.new_platform,
                                    "codex".to_string(),
                                    "Codex",
                                );
                                ui.selectable_value(
                                    &mut self.new_platform,
                                    "other".to_string(),
                                    "Other",
                                );
                            });
                    });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let can_create = !self.new_name.trim().is_empty();
                        if ui
                            .add_enabled(
                                can_create,
                                egui::Button::new(
                                    egui::RichText::new("Create").color(colors::GREEN),
                                ),
                            )
                            .clicked()
                        {
                            let name = self.new_name.trim().to_string();
                            let platform = self.new_platform.clone();
                            self.create_session(name, platform);
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_new_dialog = false;
                        }
                    });
                });
        }

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
                                    session_card(ui, selected, |ui| {
                                        ui.horizontal(|ui| {
                                            // Active dot.
                                            let dot = ui.allocate_space(egui::vec2(8.0, 8.0));
                                            ui.painter().circle_filled(
                                                dot.1.center(),
                                                3.5,
                                                if session.status == "active" {
                                                    colors::GREEN
                                                } else {
                                                    colors::TEXT_DIM
                                                },
                                            );

                                            let color = platform_color(&session.platform);
                                            let text = egui::RichText::new(&session.name)
                                                .color(if selected { color } else { colors::TEXT });
                                            if ui.selectable_label(selected, text).clicked() {
                                                self.selected_id = Some(session.id.clone());
                                            }
                                        });

                                        // Platform badge + metadata.
                                        ui.horizontal(|ui| {
                                            ui.add_space(16.0);
                                            platform_badge(ui, &session.platform);
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "{} files \u{00b7} {}",
                                                    session.active_files.len(),
                                                    format_relative_time(&session.created_at)
                                                ))
                                                .small()
                                                .color(colors::TEXT_DIM),
                                            );
                                        });
                                    });
                                    ui.add_space(4.0);
                                }

                                if state.sessions.is_empty() {
                                    ui.label(
                                        egui::RichText::new("No active sessions")
                                            .color(colors::TEXT_DIM),
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
                                    session_card(ui, selected, |ui| {
                                        let text = egui::RichText::new(&entry.session_name).color(
                                            if selected {
                                                platform_color(&entry.platform)
                                            } else {
                                                colors::TEXT
                                            },
                                        );
                                        if ui.selectable_label(selected, text).clicked() {
                                            self.selected_id = Some(entry.session_id.clone());
                                        }

                                        ui.horizontal(|ui| {
                                            ui.add_space(4.0);
                                            platform_badge(ui, &entry.platform);
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "{} files \u{00b7} {}",
                                                    entry.files_touched.len(),
                                                    format_relative_time(&entry.ended_at)
                                                ))
                                                .small()
                                                .color(colors::TEXT_DIM),
                                            );
                                        });
                                    });
                                    ui.add_space(4.0);
                                }

                                if state.history.is_empty() {
                                    ui.label(
                                        egui::RichText::new("No history entries")
                                            .color(colors::TEXT_DIM),
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

                                // End session action.
                                ui.add_space(12.0);
                                ui.separator();
                                ui.add_space(8.0);

                                if self.show_end_dialog {
                                    ui.label(
                                        egui::RichText::new("End Session")
                                            .strong()
                                            .color(colors::YELLOW),
                                    );
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.label("Summary:");
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.end_summary)
                                                .hint_text("Brief session summary...")
                                                .desired_width(250.0),
                                        );
                                    });
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add(egui::Button::new(
                                                egui::RichText::new("End Session")
                                                    .color(colors::RED),
                                            ))
                                            .clicked()
                                        {
                                            let id = session.id.clone();
                                            let summary = if self.end_summary.trim().is_empty() {
                                                "Session ended via GUI".to_string()
                                            } else {
                                                self.end_summary.trim().to_string()
                                            };
                                            self.end_session(id, summary);
                                            self.selected_id = None;
                                        }
                                        if ui.button("Cancel").clicked() {
                                            self.show_end_dialog = false;
                                        }
                                    });
                                } else if session.status == "active"
                                    && ui
                                        .add(egui::Button::new(
                                            egui::RichText::new("End Session...")
                                                .color(colors::YELLOW),
                                        ))
                                        .clicked()
                                {
                                    self.show_end_dialog = true;
                                    self.end_summary.clear();
                                }
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
                                .color(colors::TEXT_DIM),
                        );
                    });
                }
            });
        });
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Render a session card with a subtle background frame.
fn session_card(ui: &mut egui::Ui, selected: bool, content: impl FnOnce(&mut egui::Ui)) {
    let fill = if selected {
        colors::ACTIVE_BG
    } else {
        colors::SURFACE
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .stroke(egui::Stroke::new(
            if selected { 1.0 } else { 0.5 },
            if selected {
                colors::ACCENT
            } else {
                colors::BORDER
            },
        ))
        .show(ui, |ui| {
            content(ui);
        });
}

/// Render a small colored platform badge.
fn platform_badge(ui: &mut egui::Ui, platform: &str) {
    let color = platform_color(platform);
    let short = match platform {
        "claude-code" => "Claude",
        "opencode" => "OC",
        "codex" => "Codex",
        _ => platform,
    };
    egui::Frame::new()
        .fill(color.gamma_multiply(0.15))
        .corner_radius(egui::CornerRadius::same(3))
        .inner_margin(egui::Margin::symmetric(4, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(short).small().color(color));
        });
}

fn detail_session(ui: &mut egui::Ui, s: &crate::ipc::Session) {
    ui.heading(egui::RichText::new(&s.name).color(colors::TEXT));
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Platform:").color(colors::TEXT_MUTED));
        platform_badge(ui, &s.platform);
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Status:").color(colors::TEXT_MUTED));
        let status_color = if s.status == "active" {
            colors::GREEN
        } else {
            colors::TEXT_DIM
        };
        ui.colored_label(status_color, &s.status);
    });
    if !s.created_at.is_empty() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Started:").color(colors::TEXT_MUTED));
            ui.label(
                egui::RichText::new(format!(
                    "{} ({})",
                    &s.created_at,
                    format_relative_time(&s.created_at)
                ))
                .color(colors::TEXT_DIM),
            );
        });
    }

    ui.add_space(8.0);
    if !s.active_files.is_empty() {
        ui.label(egui::RichText::new("Files:").strong().color(colors::TEXT));
        for f in &s.active_files {
            ui.label(egui::RichText::new(format!("  {}", f)).color(colors::TEXT_MUTED));
        }
    }

    ui.add_space(4.0);
    if !s.recent_tools.is_empty() {
        ui.label(egui::RichText::new("Tools:").strong().color(colors::TEXT));
        for t in &s.recent_tools {
            ui.label(egui::RichText::new(format!("  {}", t)).color(colors::TEXT_MUTED));
        }
    }
}

fn detail_history(ui: &mut egui::Ui, h: &crate::ipc::HistoryEntry) {
    ui.heading(egui::RichText::new(&h.session_name).color(colors::TEXT));
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Platform:").color(colors::TEXT_MUTED));
        platform_badge(ui, &h.platform);
    });
    if !h.started_at.is_empty() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Started:").color(colors::TEXT_MUTED));
            ui.label(egui::RichText::new(&h.started_at).color(colors::TEXT_DIM));
        });
    }
    if !h.ended_at.is_empty() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Ended:").color(colors::TEXT_MUTED));
            ui.label(egui::RichText::new(&h.ended_at).color(colors::TEXT_DIM));
        });
    }
    if !h.summary.is_empty() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Summary:").strong().color(colors::TEXT));
        egui::Frame::new()
            .fill(colors::SURFACE)
            .corner_radius(egui::CornerRadius::same(4))
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(&h.summary).color(colors::TEXT_MUTED));
            });
    }

    ui.add_space(8.0);
    if !h.files_touched.is_empty() {
        ui.label(
            egui::RichText::new(format!("Files touched ({}):", h.files_touched.len()))
                .strong()
                .color(colors::TEXT),
        );
        for f in &h.files_touched {
            ui.label(egui::RichText::new(format!("  {}", f)).color(colors::TEXT_MUTED));
        }
    }

    ui.add_space(4.0);
    if !h.tools_used.is_empty() {
        ui.label(
            egui::RichText::new(format!("Tools used ({}):", h.tools_used.len()))
                .strong()
                .color(colors::TEXT),
        );
        for t in &h.tools_used {
            ui.label(egui::RichText::new(format!("  {}", t)).color(colors::TEXT_MUTED));
        }
    }
}

fn platform_color(platform: &str) -> egui::Color32 {
    match platform {
        "claude-code" => theme::agent_color("Claude Code"),
        "opencode" => theme::agent_color("OpenCode"),
        "codex" => theme::agent_color("Codex"),
        _ => colors::TEXT,
    }
}

fn empty_state(ui: &mut egui::Ui, message: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.label(egui::RichText::new(message).color(colors::TEXT_DIM));
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Run `impulse daemon` to start the background service.")
                .small()
                .color(colors::TEXT_FAINT),
        );
    });
}

fn format_relative_time(timestamp: &str) -> String {
    if timestamp.is_empty() {
        return "unknown".to_string();
    }
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
