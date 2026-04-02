//! Agent Panel — interactive chat panel for the Impulse coordinator agent.
//!
//! Provides a left-side chat UI that uses subprocess-based agent queries
//! (claude --print) or direct API calls (ureq). Cross-pane insights from
//! worker terminals are included as context in each query.
//!
//! Layout:
//! ```text
//! ┌─ Agent Panel ─────────────────────┐
//! │ [Backend: Claude Code] [Idle]     │
//! ├───────────────────────────────────┤
//! │                                   │
//! │  (scrollable chat messages)       │
//! │                                   │
//! ├───────────────────────────────────┤
//! │ [input text field         ] [Send]│
//! └───────────────────────────────────┘
//! ```

pub mod actions;
pub mod backend;
pub mod chat;
pub mod persistence;

use std::path::PathBuf;
use std::sync::mpsc;

use eframe::egui;

use crate::state::{ConnectionStatus, StateHandle};
use crate::theme::colors;

use actions::{PanelAction, ProposalExecutionMode};
use backend::{AgentBackend, AgentResponse};
use chat::{ChatMessage, ChatRole};

// ---------------------------------------------------------------------------
// Agent state
// ---------------------------------------------------------------------------

/// Current state of the agent panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentState {
    /// Ready to accept input.
    Idle,
    /// Waiting for a response from the agent backend.
    Thinking,
    /// An error occurred (displayed until next query).
    Error(String),
}

impl AgentState {
    pub fn label(&self) -> &str {
        match self {
            Self::Idle => "Idle",
            Self::Thinking => "Thinking...",
            Self::Error(_) => "Error",
        }
    }
}

// ---------------------------------------------------------------------------
// AgentPanel
// ---------------------------------------------------------------------------

/// The agent chat panel — owns messages, input state, and backend.
pub struct AgentPanel {
    messages: Vec<ChatMessage>,
    input_buf: String,
    state: AgentState,
    backend: AgentBackend,
    /// Daemon shared state — used to resolve DaemonChat backend at query time.
    daemon_state: Option<StateHandle>,
    response_rx: Option<mpsc::Receiver<AgentResponse>>,
    pending_context: Option<String>,
    scroll_to_bottom: bool,
    /// Recent cross-pane activity for display in the activity feed.
    activity_items: Vec<String>,
    /// Whether the activity feed section is expanded.
    activity_expanded: bool,
    /// Whether the input field should receive focus on the next frame.
    focus_requested: bool,
    /// Path to the JSONL conversation history file.
    history_path: PathBuf,
    /// Actions queued for dispatch by app.rs (drained each frame).
    pending_actions: Vec<PanelAction>,
    /// Latest effective supervisor permission state.
    supervisor_permissions: Option<impulse_ops::SupervisorPermissionState>,
    /// Cached connection status — updated each frame by app.rs before ui() is called.
    /// Avoids re-entrant locking of SharedState from within the agent panel.
    connection_status: ConnectionStatus,
}

impl AgentPanel {
    /// Create a new agent panel, auto-detecting the best backend.
    ///
    /// If `daemon_state` is provided, the panel will prefer routing queries
    /// through the daemon's AgentAssist endpoint when connected — falling
    /// back to the statically-detected backend (Claude Code / API) otherwise.
    pub fn new(daemon_state: Option<StateHandle>) -> Self {
        let backend = AgentBackend::detect();
        let welcome = match &backend {
            AgentBackend::DaemonChat => {
                "Impulse supervisor ready (daemon). Ask me to monitor or control your agents."
            }
            AgentBackend::Harness { .. } => {
                "Impulse supervisor ready (Claude Code). Ask me to monitor or control your agents."
            }
            AgentBackend::Api { .. } => {
                "Impulse supervisor ready (API mode). Ask me to monitor or control your agents."
            }
            AgentBackend::Unavailable => {
                "No agent backend available. Install Claude Code or set ANTHROPIC_API_KEY."
            }
        };

        let history_path = persistence::history_path();
        let mut messages = persistence::load_history(&history_path);
        let loaded_count = messages.len();

        // Add a system separator if we loaded history.
        if loaded_count > 0 {
            messages.push(ChatMessage::system(&format!(
                "--- Loaded {} messages from previous sessions ---",
                loaded_count
            )));
        }
        messages.push(ChatMessage::system(welcome));

        log::info!(
            "Agent panel initialized: {} history messages from {}",
            loaded_count,
            history_path.display()
        );

        Self {
            messages,
            input_buf: String::new(),
            state: AgentState::Idle,
            backend,
            daemon_state,
            response_rx: None,
            pending_context: None,
            scroll_to_bottom: true,
            activity_items: Vec::new(),
            activity_expanded: true,
            focus_requested: false,
            history_path,
            pending_actions: Vec::new(),
            supervisor_permissions: None,
            connection_status: ConnectionStatus::Disconnected,
        }
    }

    /// Send a user message — intercept slash commands, otherwise dispatch to backend.
    ///
    /// At query time, resolves the effective backend: if the daemon is connected,
    /// routes through DaemonChat; otherwise falls back to the static backend.
    pub fn send_message(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        // Intercept slash commands.
        if trimmed.starts_with('/') {
            self.handle_slash_command(trimmed);
            return;
        }

        let msg = ChatMessage::user(trimmed);
        persistence::append_message(&self.history_path, &msg);
        self.messages.push(msg);
        self.state = AgentState::Thinking;
        self.scroll_to_bottom = true;

        // Resolve effective backend (DaemonChat when connected, else static).
        let mut effective = backend::resolve_backend(&self.backend, self.connection_status);
        let context = self.pending_context.take().unwrap_or_default();
        let rx = backend::dispatch_query(&mut effective, trimmed, &context);
        self.response_rx = Some(rx);
    }

    /// Execute a slash command locally without hitting the backend.
    fn handle_slash_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let command = parts[0].to_lowercase();

        match command.as_str() {
            "/clear" => {
                self.messages.clear();
                self.messages.push(ChatMessage::system(
                    "Chat cleared. History preserved in backend.",
                ));
                self.state = AgentState::Idle;
                self.scroll_to_bottom = true;
            }
            "/help" => {
                self.messages.push(ChatMessage::system(
                    "Commands:\n\
                     /clear — Clear chat messages\n\
                     /help — Show this help\n\
                     /status — Show backend and connection info\n\
                     /activity — Toggle activity feed\n\
                     /history — Show conversation history info\n\
                     /inject <tab_id> <text> — Inject context into a terminal pane\n\
                     /send <tab_id> <text> — Send raw input to a terminal pane\n\
                     /focus <tab_id> — Switch to a terminal tab\n\
                     /search <query> — Search terminal output\n\
                     /memory-search <query> — Search session memory\n\n\
                     Shortcuts:\n\
                     Ctrl+L — Focus agent input\n\
                     Ctrl+5 — Toggle agent panel\n\
                     Ctrl+N — New terminal tab\n\
                     Escape — Dismiss error state",
                ));
                self.scroll_to_bottom = true;
            }
            "/status" => {
                let static_label = self.backend.label();
                let effective = backend::resolve_backend(&self.backend, self.connection_status);
                let effective_label = effective.label();
                let msg_count = self.messages.len();
                let activity_count = self.activity_items.len();
                let has_context = self.pending_context.is_some();
                let daemon_connected =
                    self.daemon_state.is_some() && effective_label == "Daemon Chat";

                let backend_line = if daemon_connected {
                    format!(
                        "Backend: {} (daemon connected, fallback: {})",
                        effective_label, static_label
                    )
                } else {
                    format!("Backend: {}", static_label)
                };

                self.messages.push(ChatMessage::system(&format!(
                    "{}\n\
                     State: {}\n\
                     Messages: {}\n\
                     Activity items: {}\n\
                     Pending context: {}",
                    backend_line,
                    self.state.label(),
                    msg_count,
                    activity_count,
                    if has_context { "yes" } else { "no" }
                )));
                self.scroll_to_bottom = true;
            }
            "/activity" => {
                self.activity_expanded = !self.activity_expanded;
                let state = if self.activity_expanded {
                    "expanded"
                } else {
                    "collapsed"
                };
                self.messages
                    .push(ChatMessage::system(&format!("Activity feed {}.", state)));
                self.scroll_to_bottom = true;
            }
            "/history" => {
                let total = self
                    .messages
                    .iter()
                    .filter(|m| m.role != ChatRole::System)
                    .count();
                let user_count = self
                    .messages
                    .iter()
                    .filter(|m| m.role == ChatRole::User)
                    .count();
                let agent_count = self
                    .messages
                    .iter()
                    .filter(|m| m.role == ChatRole::Agent)
                    .count();
                let file_exists = self.history_path.exists();
                let file_size = if file_exists {
                    std::fs::metadata(&self.history_path)
                        .map(|m| format!("{:.1} KB", m.len() as f64 / 1024.0))
                        .unwrap_or_else(|_| "unknown".to_string())
                } else {
                    "not created yet".to_string()
                };

                self.messages.push(ChatMessage::system(&format!(
                    "Conversation history:\n\
                     Messages: {} ({} user, {} agent)\n\
                     File: {}\n\
                     Size: {}",
                    total,
                    user_count,
                    agent_count,
                    self.history_path.display(),
                    file_size,
                )));
                self.scroll_to_bottom = true;
            }
            "/inject" => {
                if let Some(args) = parts.get(1) {
                    if let Some((id_str, content)) = args.split_once(' ') {
                        if let Ok(tab_id) = id_str.parse::<u64>() {
                            self.pending_actions.push(PanelAction::InjectTo {
                                tab_id,
                                content: content.to_string(),
                            });
                            self.messages.push(ChatMessage::system(&format!(
                                "Injecting context into tab {}.",
                                tab_id
                            )));
                        } else {
                            self.messages.push(ChatMessage::system(
                                "Usage: /inject <tab_id> <text>  (tab_id must be a number)",
                            ));
                        }
                    } else {
                        self.messages
                            .push(ChatMessage::system("Usage: /inject <tab_id> <text>"));
                    }
                } else {
                    self.messages
                        .push(ChatMessage::system("Usage: /inject <tab_id> <text>"));
                }
                self.scroll_to_bottom = true;
            }
            "/send" => {
                if let Some(args) = parts.get(1) {
                    if let Some((id_str, content)) = args.split_once(' ') {
                        if let Ok(tab_id) = id_str.parse::<u64>() {
                            self.pending_actions.push(PanelAction::SendTo {
                                tab_id,
                                content: content.to_string(),
                            });
                            self.messages.push(ChatMessage::system(&format!(
                                "Sending input to tab {}.",
                                tab_id
                            )));
                        } else {
                            self.messages.push(ChatMessage::system(
                                "Usage: /send <tab_id> <text>  (tab_id must be a number)",
                            ));
                        }
                    } else {
                        self.messages
                            .push(ChatMessage::system("Usage: /send <tab_id> <text>"));
                    }
                } else {
                    self.messages
                        .push(ChatMessage::system("Usage: /send <tab_id> <text>"));
                }
                self.scroll_to_bottom = true;
            }
            "/focus" => {
                if let Some(args) = parts.get(1) {
                    if let Ok(tab_id) = args.trim().parse::<u64>() {
                        self.pending_actions.push(PanelAction::FocusTab { tab_id });
                        self.messages.push(ChatMessage::system(&format!(
                            "Switching to tab {}.",
                            tab_id
                        )));
                    } else {
                        self.messages.push(ChatMessage::system(
                            "Usage: /focus <tab_id>  (tab_id must be a number)",
                        ));
                    }
                } else {
                    self.messages
                        .push(ChatMessage::system("Usage: /focus <tab_id>"));
                }
                self.scroll_to_bottom = true;
            }
            "/search" => {
                if let Some(query) = parts.get(1) {
                    self.pending_actions.push(PanelAction::SearchTerm {
                        query: query.to_string(),
                    });
                    self.messages.push(ChatMessage::system(&format!(
                        "Searching terminals for: {}",
                        query
                    )));
                } else {
                    self.messages
                        .push(ChatMessage::system("Usage: /search <query>"));
                }
                self.scroll_to_bottom = true;
            }
            "/memory-search" | "/msearch" => {
                if let Some(query) = parts.get(1) {
                    self.pending_actions.push(PanelAction::MemorySearch {
                        query: query.to_string(),
                    });
                    self.messages.push(ChatMessage::system(&format!(
                        "Searching memory for: {}",
                        query
                    )));
                } else {
                    self.messages
                        .push(ChatMessage::system("Usage: /memory-search <query>"));
                }
                self.scroll_to_bottom = true;
            }
            _ => {
                self.messages.push(ChatMessage::system(&format!(
                    "Unknown command: {}. Type /help for available commands.",
                    parts[0]
                )));
                self.scroll_to_bottom = true;
            }
        }
    }

    /// Poll for agent responses. Call every frame — try_recv is non-blocking.
    pub fn tick(&mut self) {
        let rx = match &self.response_rx {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(response) => {
                if let Some(permission_state) = response.permission_state.clone() {
                    self.supervisor_permissions = Some(permission_state.clone());
                    if let Some(handle) = &self.daemon_state {
                        if let Ok(mut shared) = handle.lock() {
                            shared.supervisor_permissions = Some(permission_state);
                        }
                    }
                }
                if response.is_error {
                    self.messages
                        .push(ChatMessage::system(&format!("Error: {}", response.content)));
                    self.state = AgentState::Error(response.content);
                } else {
                    // Record in API history for conversation continuity.
                    backend::record_api_response(&mut self.backend, &response.content);
                    let msg =
                        ChatMessage::agent_with_proposals(&response.content, response.proposals);
                    persistence::append_message(&self.history_path, &msg);
                    self.messages.push(msg);
                    self.state = AgentState::Idle;
                }
                self.scroll_to_bottom = true;
                self.response_rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {
                // Still waiting — nothing to do.
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                // Thread died without sending a response.
                self.messages.push(ChatMessage::system(
                    "Agent query failed — background thread disconnected.",
                ));
                self.state = AgentState::Error("Thread disconnected".to_string());
                self.response_rx = None;
            }
        }
    }

    /// Store cross-pane context for the next query.
    pub fn inject_context(&mut self, insights: &[String]) {
        if insights.is_empty() {
            return;
        }
        self.pending_context = Some(format!(
            "<impulse-context>\n## Worker Pane Activity\n{}\n</impulse-context>",
            insights.join("\n")
        ));
    }

    /// Update the displayed activity feed from terminal insights.
    ///
    /// Called more frequently than `inject_context` (every 5s vs 60s) so the
    /// UI stays responsive even when the LLM context isn't updated yet.
    pub fn update_activity(&mut self, insights: Vec<String>) {
        self.activity_items = insights;
    }

    /// Render the agent panel UI.
    pub fn ui(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        // Escape dismisses error state.
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.clear_error();
        }

        let is_thinking = self.state == AgentState::Thinking;
        let msg_count = self.messages.len();

        // Header with accent bar and rocket icon.
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(3.0, 16.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 1.0, colors::ACCENT);

            ui.add_space(6.0);
            ui.label(egui::RichText::new("\u{1F680}").size(12.0)); // 🚀
            ui.strong(
                egui::RichText::new("Supervisor")
                    .size(13.0)
                    .color(colors::ACCENT),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let state_color = match &self.state {
                    AgentState::Idle => colors::GREEN,
                    AgentState::Thinking => colors::YELLOW,
                    AgentState::Error(_) => colors::RED,
                };
                let (dot_rect, _) =
                    ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(dot_rect.center(), 3.0, state_color);
                ui.label(
                    egui::RichText::new(self.state.label())
                        .small()
                        .color(state_color),
                );
            });
        });

        ui.separator();

        if let Some(permission_state) = &self.supervisor_permissions {
            render_permission_strip(ui, permission_state);
            ui.separator();
        }

        // Activity feed (collapsible, between header and messages).
        if !self.activity_items.is_empty() {
            let activity_count = self.activity_items.len();
            let toggle_label = if self.activity_expanded {
                format!("\u{25BC} Activity ({})", activity_count) // ▼
            } else {
                format!("\u{25B6} Activity ({})", activity_count) // ▶
            };

            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(toggle_label)
                            .small()
                            .color(colors::TEXT_MUTED),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .frame(false),
                )
                .clicked()
            {
                self.activity_expanded = !self.activity_expanded;
            }

            if self.activity_expanded {
                egui::Frame::new()
                    .fill(colors::SURFACE)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .corner_radius(egui::CornerRadius::same(4))
                    .show(ui, |ui| {
                        for item in self.activity_items.iter().take(8) {
                            ui.horizontal(|ui| {
                                // Color-coded dot by insight type.
                                let dot_color = if item.contains("Error") {
                                    colors::RED
                                } else if item.contains("Modified") || item.contains("File") {
                                    colors::BLUE
                                } else if item.contains("Decision") {
                                    colors::YELLOW
                                } else {
                                    colors::GREEN
                                };
                                let (dot_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(6.0, 6.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter()
                                    .circle_filled(dot_rect.center(), 2.5, dot_color);
                                ui.label(egui::RichText::new(item).small().color(colors::TEXT_DIM));
                            });
                        }
                    });
                ui.add_space(2.0);
            }

            ui.separator();
        }

        // Input bar (render at bottom FIRST to reserve space).
        let mut submitted = false;

        egui::TopBottomPanel::bottom("agent_input")
            .exact_height(44.0)
            .show_inside(ui, |ui| {
                ui.add_space(4.0);
                egui::Frame::new()
                    .fill(colors::BG)
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .corner_radius(egui::CornerRadius::same(8))
                    .stroke(egui::Stroke::new(1.0, colors::BORDER))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let text_edit = egui::TextEdit::singleline(&mut self.input_buf)
                                .hint_text(if is_thinking {
                                    "Thinking..."
                                } else {
                                    "Ask Impulse..."
                                })
                                .interactive(!is_thinking)
                                .desired_width(ui.available_width() - 40.0);

                            let response = ui.add(text_edit);

                            if self.focus_requested {
                                response.request_focus();
                                self.focus_requested = false;
                            }

                            // Enter causes the TextEdit to lose focus — that's
                            // our submission signal. Don't also check key_pressed(Enter)
                            // because egui consumes the key event internally before we
                            // can observe it, causing Enter to silently fail.
                            if response.lost_focus() && !is_thinking {
                                submitted = true;
                            }

                            let btn_color = if is_thinking {
                                colors::TEXT_FAINT
                            } else {
                                colors::ACCENT
                            };

                            let send_btn = ui.add_enabled(
                                !is_thinking && !self.input_buf.trim().is_empty(),
                                egui::Button::new(
                                    egui::RichText::new("\u{2191}").size(14.0).color(btn_color), // ↑
                                )
                                .corner_radius(egui::CornerRadius::same(6)),
                            );
                            if send_btn.clicked() {
                                submitted = true;
                            }
                        });
                    });
            });

        // Messages fill remaining space.
        let proposal_actions =
            chat::render_messages(ui, &self.messages, self.scroll_to_bottom, is_thinking);
        self.scroll_to_bottom = false;
        for action in proposal_actions {
            self.handle_panel_action(action);
        }

        if submitted && !self.input_buf.trim().is_empty() {
            let text = std::mem::take(&mut self.input_buf);
            self.send_message(&text);
        }
    }

    /// Update the cached connection status. Called each frame by app.rs
    /// *before* entering the SharedState lock scope, so that ui() and
    /// send_message() can resolve the backend without re-locking.
    pub fn set_connection_status(&mut self, status: ConnectionStatus) {
        self.connection_status = status;
    }

    pub fn set_supervisor_permissions(
        &mut self,
        permission_state: Option<impulse_ops::SupervisorPermissionState>,
    ) {
        self.supervisor_permissions = permission_state;
    }

    /// Request that the input field receives keyboard focus on the next frame.
    pub fn request_focus(&mut self) {
        self.focus_requested = true;
    }

    /// Clear any error state, returning to idle.
    pub fn clear_error(&mut self) {
        if matches!(self.state, AgentState::Error(_)) {
            self.state = AgentState::Idle;
        }
    }

    /// Drain pending actions for dispatch by app.rs.
    pub fn take_actions(&mut self) -> Vec<PanelAction> {
        std::mem::take(&mut self.pending_actions)
    }

    /// Number of messages for status display.
    #[allow(dead_code)] // dead_code: used by tests and planned GUI status bar
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Whether the agent is currently processing a query.
    #[allow(dead_code)] // dead_code: used by tests and planned GUI status bar
    pub fn is_thinking(&self) -> bool {
        self.state == AgentState::Thinking
    }

    fn handle_panel_action(&mut self, action: PanelAction) {
        match action {
            PanelAction::RunSupervisorProposal { proposal, mode } => match mode {
                ProposalExecutionMode::Deny => {
                    let msg = ChatMessage::system(&format!("Denied proposal: {}", proposal.title));
                    persistence::append_message(&self.history_path, &msg);
                    self.messages.push(msg);
                    self.scroll_to_bottom = true;
                }
                ProposalExecutionMode::Run
                | ProposalExecutionMode::AllowThisSession
                | ProposalExecutionMode::SaveDefault => {
                    let msg = ChatMessage::system(&format!(
                        "Queued supervisor action: {}",
                        proposal.title
                    ));
                    persistence::append_message(&self.history_path, &msg);
                    self.messages.push(msg);
                    self.pending_actions
                        .push(PanelAction::RunSupervisorProposal { proposal, mode });
                    self.scroll_to_bottom = true;
                }
            },
            other => self.pending_actions.push(other),
        }
    }
}

fn render_permission_strip(
    ui: &mut egui::Ui,
    permission_state: &impulse_ops::SupervisorPermissionState,
) {
    ui.vertical(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("Supervisor Permissions")
                    .small()
                    .strong()
                    .color(colors::ACCENT),
            );
            if permission_state.session_override_active() {
                ui.label(
                    egui::RichText::new("session override active")
                        .small()
                        .color(colors::YELLOW),
                );
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for action in &permission_state.effective.allowed_actions {
                render_permission_chip(ui, action.as_str(), colors::ACCENT);
            }
            for capability in &permission_state.effective.allowed_tool_capabilities {
                render_permission_chip(ui, capability.as_str(), colors::BLUE);
            }
        });
    });
}

fn render_permission_chip(ui: &mut egui::Ui, label: &str, accent: egui::Color32) {
    egui::Frame::new()
        .fill(colors::SURFACE)
        .corner_radius(egui::CornerRadius::same(255))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .stroke(egui::Stroke::new(0.75, accent))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(label).small().color(colors::TEXT_MUTED));
        });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chat::ChatRole;

    #[test]
    fn test_agent_panel_new() {
        let panel = AgentPanel::new(None);
        // Should have at least the welcome message.
        assert!(!panel.messages.is_empty());
        assert_eq!(panel.state, AgentState::Idle);
        assert!(!panel.is_thinking());
    }

    #[test]
    fn test_agent_state_labels() {
        assert_eq!(AgentState::Idle.label(), "Idle");
        assert_eq!(AgentState::Thinking.label(), "Thinking...");
        assert_eq!(AgentState::Error("test".into()).label(), "Error");
    }

    #[test]
    fn test_send_empty_message_is_noop() {
        let mut panel = AgentPanel::new(None);
        let initial_count = panel.message_count();
        panel.send_message("");
        assert_eq!(panel.message_count(), initial_count);
        panel.send_message("   ");
        assert_eq!(panel.message_count(), initial_count);
    }

    #[test]
    fn test_inject_context_empty() {
        let mut panel = AgentPanel::new(None);
        panel.inject_context(&[]);
        assert!(panel.pending_context.is_none());
    }

    #[test]
    fn test_inject_context_formats_correctly() {
        let mut panel = AgentPanel::new(None);
        panel.inject_context(&[
            "[Claude Code] Modified src/main.rs".to_string(),
            "[Shell] Error: test failed".to_string(),
        ]);
        let ctx = panel.pending_context.as_ref().unwrap();
        assert!(ctx.contains("<impulse-context>"));
        assert!(ctx.contains("Worker Pane Activity"));
        assert!(ctx.contains("Modified src/main.rs"));
        assert!(ctx.contains("Error: test failed"));
        assert!(ctx.contains("</impulse-context>"));
    }

    #[test]
    fn test_tick_with_no_pending_response() {
        let mut panel = AgentPanel::new(None);
        // tick() should be a no-op when no query is in flight.
        panel.tick();
        assert_eq!(panel.state, AgentState::Idle);
    }

    #[test]
    fn test_update_activity_stores_items() {
        let mut panel = AgentPanel::new(None);
        assert!(panel.activity_items.is_empty());
        panel.update_activity(vec![
            "[Claude Code] FileModified: src/main.rs".to_string(),
            "[Shell] Error: test failed".to_string(),
        ]);
        assert_eq!(panel.activity_items.len(), 2);
        assert!(panel.activity_items[0].contains("FileModified"));
    }

    #[test]
    fn test_update_activity_replaces_previous() {
        let mut panel = AgentPanel::new(None);
        panel.update_activity(vec!["first".to_string()]);
        assert_eq!(panel.activity_items.len(), 1);
        panel.update_activity(vec!["second".to_string(), "third".to_string()]);
        assert_eq!(panel.activity_items.len(), 2);
        assert_eq!(panel.activity_items[0], "second");
    }

    #[test]
    fn test_activity_expanded_default_true() {
        let panel = AgentPanel::new(None);
        assert!(panel.activity_expanded);
    }

    #[test]
    fn test_slash_clear_resets_messages() {
        let mut panel = AgentPanel::new(None);
        panel.messages.push(ChatMessage::user("hello"));
        panel.messages.push(ChatMessage::agent("world"));
        assert!(panel.messages.len() >= 3); // welcome + hello + world
        panel.send_message("/clear");
        assert_eq!(panel.messages.len(), 1); // only the "Chat cleared" system msg
        assert_eq!(panel.messages[0].role, ChatRole::System);
    }

    #[test]
    fn test_slash_help_adds_system_message() {
        let mut panel = AgentPanel::new(None);
        let before = panel.messages.len();
        panel.send_message("/help");
        assert_eq!(panel.messages.len(), before + 1);
        assert!(panel.messages.last().unwrap().content.contains("/clear"));
        assert!(panel.messages.last().unwrap().content.contains("/status"));
    }

    #[test]
    fn test_slash_status_shows_backend_info() {
        let mut panel = AgentPanel::new(None);
        let before = panel.messages.len();
        panel.send_message("/status");
        assert_eq!(panel.messages.len(), before + 1);
        assert!(panel.messages.last().unwrap().content.contains("Backend:"));
        assert!(panel.messages.last().unwrap().content.contains("State:"));
    }

    #[test]
    fn test_slash_activity_toggles() {
        let mut panel = AgentPanel::new(None);
        assert!(panel.activity_expanded);
        panel.send_message("/activity");
        assert!(!panel.activity_expanded);
        panel.send_message("/activity");
        assert!(panel.activity_expanded);
    }

    #[test]
    fn test_unknown_command_shows_error() {
        let mut panel = AgentPanel::new(None);
        let before = panel.messages.len();
        panel.send_message("/foobar");
        assert_eq!(panel.messages.len(), before + 1);
        assert!(panel
            .messages
            .last()
            .unwrap()
            .content
            .contains("Unknown command"));
    }

    #[test]
    fn test_slash_commands_case_insensitive() {
        let mut panel = AgentPanel::new(None);
        let before = panel.messages.len();
        panel.send_message("/HELP");
        assert_eq!(panel.messages.len(), before + 1);
        assert!(panel.messages.last().unwrap().content.contains("/clear"));
    }

    #[test]
    fn test_slash_command_does_not_dispatch_to_backend() {
        let mut panel = AgentPanel::new(None);
        panel.send_message("/clear");
        // Should NOT be in Thinking state (no backend dispatch).
        assert_eq!(panel.state, AgentState::Idle);
        assert!(panel.response_rx.is_none());
    }

    #[test]
    fn test_request_focus_sets_flag() {
        let mut panel = AgentPanel::new(None);
        assert!(!panel.focus_requested);
        panel.request_focus();
        assert!(panel.focus_requested);
    }

    #[test]
    fn test_clear_error_returns_to_idle() {
        let mut panel = AgentPanel::new(None);
        panel.state = AgentState::Error("test".into());
        panel.clear_error();
        assert_eq!(panel.state, AgentState::Idle);
    }

    #[test]
    fn test_clear_error_noop_when_idle() {
        let mut panel = AgentPanel::new(None);
        panel.clear_error();
        assert_eq!(panel.state, AgentState::Idle);
    }

    #[test]
    fn test_help_includes_shortcuts() {
        let mut panel = AgentPanel::new(None);
        panel.send_message("/help");
        let last = &panel.messages.last().unwrap().content;
        assert!(last.contains("Ctrl+L"));
        assert!(last.contains("Ctrl+N"));
        assert!(last.contains("Escape"));
    }

    #[test]
    fn test_help_includes_bidirectional_commands() {
        let mut panel = AgentPanel::new(None);
        panel.send_message("/help");
        let last = &panel.messages.last().unwrap().content;
        assert!(last.contains("/inject"));
        assert!(last.contains("/send"));
        assert!(last.contains("/focus"));
        assert!(last.contains("/search"));
    }

    #[test]
    fn test_inject_command_queues_action() {
        let mut panel = AgentPanel::new(None);
        panel.send_message("/inject 0 some context text");
        assert_eq!(panel.pending_actions.len(), 1);
        match &panel.pending_actions[0] {
            PanelAction::InjectTo { tab_id, content } => {
                assert_eq!(*tab_id, 0);
                assert_eq!(content, "some context text");
            }
            other => panic!("Expected InjectTo, got {:?}", other),
        }
    }

    #[test]
    fn test_inject_command_bad_id_shows_usage() {
        let mut panel = AgentPanel::new(None);
        let before = panel.messages.len();
        panel.send_message("/inject abc text");
        assert!(panel.pending_actions.is_empty());
        assert_eq!(panel.messages.len(), before + 1);
        assert!(panel.messages.last().unwrap().content.contains("Usage"));
    }

    #[test]
    fn test_inject_command_missing_args_shows_usage() {
        let mut panel = AgentPanel::new(None);
        let before = panel.messages.len();
        panel.send_message("/inject");
        assert!(panel.pending_actions.is_empty());
        assert_eq!(panel.messages.len(), before + 1);
        assert!(panel.messages.last().unwrap().content.contains("Usage"));
    }

    #[test]
    fn test_send_command_queues_action() {
        let mut panel = AgentPanel::new(None);
        panel.send_message("/send 2 echo hello");
        assert_eq!(panel.pending_actions.len(), 1);
        match &panel.pending_actions[0] {
            PanelAction::SendTo { tab_id, content } => {
                assert_eq!(*tab_id, 2);
                assert_eq!(content, "echo hello");
            }
            other => panic!("Expected SendTo, got {:?}", other),
        }
    }

    #[test]
    fn test_focus_command_queues_action() {
        let mut panel = AgentPanel::new(None);
        panel.send_message("/focus 3");
        assert_eq!(panel.pending_actions.len(), 1);
        match &panel.pending_actions[0] {
            PanelAction::FocusTab { tab_id } => {
                assert_eq!(*tab_id, 3);
            }
            other => panic!("Expected FocusTab, got {:?}", other),
        }
    }

    #[test]
    fn test_focus_command_bad_id_shows_usage() {
        let mut panel = AgentPanel::new(None);
        panel.send_message("/focus xyz");
        assert!(panel.pending_actions.is_empty());
        assert!(panel.messages.last().unwrap().content.contains("Usage"));
    }

    #[test]
    fn test_search_command_queues_action() {
        let mut panel = AgentPanel::new(None);
        panel.send_message("/search error handling");
        assert_eq!(panel.pending_actions.len(), 1);
        match &panel.pending_actions[0] {
            PanelAction::SearchTerm { query } => {
                assert_eq!(query, "error handling");
            }
            other => panic!("Expected SearchTerm, got {:?}", other),
        }
    }

    #[test]
    fn test_search_command_missing_query_shows_usage() {
        let mut panel = AgentPanel::new(None);
        panel.send_message("/search");
        assert!(panel.pending_actions.is_empty());
        assert!(panel.messages.last().unwrap().content.contains("Usage"));
    }

    #[test]
    fn test_set_connection_status_updates_field() {
        use crate::state::ConnectionStatus;

        let mut panel = AgentPanel::new(None);
        assert_eq!(panel.connection_status, ConnectionStatus::Disconnected);
        panel.set_connection_status(ConnectionStatus::Connected);
        assert_eq!(panel.connection_status, ConnectionStatus::Connected);
        panel.set_connection_status(ConnectionStatus::Disconnected);
        assert_eq!(panel.connection_status, ConnectionStatus::Disconnected);
    }

    #[test]
    fn test_take_actions_drains_queue() {
        let mut panel = AgentPanel::new(None);
        panel.send_message("/focus 1");
        panel.send_message("/focus 2");
        assert_eq!(panel.pending_actions.len(), 2);
        let actions = panel.take_actions();
        assert_eq!(actions.len(), 2);
        assert!(panel.pending_actions.is_empty());
    }
}
