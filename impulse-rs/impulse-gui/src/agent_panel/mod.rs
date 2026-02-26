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

pub mod backend;
pub mod chat;

use std::sync::mpsc;

use eframe::egui;

use crate::theme::colors;

use backend::{AgentBackend, AgentResponse};
use chat::ChatMessage;

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
    response_rx: Option<mpsc::Receiver<AgentResponse>>,
    pending_context: Option<String>,
    scroll_to_bottom: bool,
    /// Recent cross-pane activity for display in the activity feed.
    activity_items: Vec<String>,
    /// Whether the activity feed section is expanded.
    activity_expanded: bool,
    /// Whether the input field should receive focus on the next frame.
    focus_requested: bool,
}

impl AgentPanel {
    /// Create a new agent panel, auto-detecting the best backend.
    pub fn new() -> Self {
        let backend = AgentBackend::detect();
        let welcome = match &backend {
            AgentBackend::Harness { .. } => {
                "Impulse Agent ready (Claude Code). Ask me about your workspace."
            }
            AgentBackend::Api { .. } => {
                "Impulse Agent ready (API mode). Ask me about your workspace."
            }
            AgentBackend::Unavailable => {
                "No agent backend available. Install Claude Code or set ANTHROPIC_API_KEY."
            }
        };

        Self {
            messages: vec![ChatMessage::system(welcome)],
            input_buf: String::new(),
            state: AgentState::Idle,
            backend,
            response_rx: None,
            pending_context: None,
            scroll_to_bottom: true,
            activity_items: Vec::new(),
            activity_expanded: true,
            focus_requested: false,
        }
    }

    /// Send a user message — intercept slash commands, otherwise dispatch to backend.
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

        self.messages.push(ChatMessage::user(trimmed));
        self.state = AgentState::Thinking;
        self.scroll_to_bottom = true;

        let context = self.pending_context.take().unwrap_or_default();
        let rx = backend::dispatch_query(&mut self.backend, trimmed, &context);
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
                     /activity — Toggle activity feed\n\n\
                     Shortcuts:\n\
                     Ctrl+L — Focus agent input\n\
                     Ctrl+5 — Toggle agent panel\n\
                     Ctrl+N — New terminal tab\n\
                     Escape — Dismiss error state",
                ));
                self.scroll_to_bottom = true;
            }
            "/status" => {
                let backend_info = self.backend.label();
                let msg_count = self.messages.len();
                let activity_count = self.activity_items.len();
                let has_context = self.pending_context.is_some();
                self.messages.push(ChatMessage::system(&format!(
                    "Backend: {}\n\
                     State: {}\n\
                     Messages: {}\n\
                     Activity items: {}\n\
                     Pending context: {}",
                    backend_info,
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
                if response.is_error {
                    self.messages
                        .push(ChatMessage::system(&format!("Error: {}", response.content)));
                    self.state = AgentState::Error(response.content);
                } else {
                    // Record in API history for conversation continuity.
                    backend::record_api_response(&mut self.backend, &response.content);
                    self.messages.push(ChatMessage::agent(&response.content));
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

        // Header with accent bar.
        ui.horizontal(|ui| {
            // Purple accent bar.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(3.0, 20.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 1.0, colors::ACCENT);

            ui.add_space(4.0);
            ui.strong(egui::RichText::new("Agent").color(colors::ACCENT));
            ui.separator();
            ui.label(
                egui::RichText::new(self.backend.label())
                    .small()
                    .color(colors::TEXT_MUTED),
            );
            ui.label(
                egui::RichText::new(format!("({})", msg_count))
                    .small()
                    .color(colors::TEXT_DIM),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let state_color = match &self.state {
                    AgentState::Idle => colors::GREEN,
                    AgentState::Thinking => colors::YELLOW,
                    AgentState::Error(_) => colors::RED,
                };
                // Colored state dot.
                let (dot_rect, _) =
                    ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(dot_rect.center(), 3.5, state_color);
                ui.label(
                    egui::RichText::new(self.state.label())
                        .small()
                        .color(state_color),
                );
            });
        });

        ui.separator();

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
                    .inner_margin(egui::Margin::symmetric(6, 4))
                    .corner_radius(egui::CornerRadius::same(6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let text_edit = egui::TextEdit::singleline(&mut self.input_buf)
                                .hint_text(if is_thinking {
                                    "Waiting for response..."
                                } else {
                                    "Ask the Impulse Agent..."
                                })
                                .interactive(!is_thinking)
                                .desired_width(ui.available_width() - 52.0);

                            let response = ui.add(text_edit);

                            if self.focus_requested {
                                response.request_focus();
                                self.focus_requested = false;
                            }

                            if response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                && !is_thinking
                            {
                                submitted = true;
                            }

                            let btn_color = if is_thinking {
                                colors::TEXT_FAINT
                            } else {
                                colors::ACCENT
                            };

                            let send_btn = ui.add_enabled(
                                !is_thinking && !self.input_buf.trim().is_empty(),
                                egui::Button::new(egui::RichText::new("\u{27A4}").color(btn_color)),
                            );
                            if send_btn.clicked() {
                                submitted = true;
                            }
                        });
                    });
            });

        // Messages fill remaining space.
        chat::render_messages(ui, &self.messages, self.scroll_to_bottom, is_thinking);
        self.scroll_to_bottom = false;

        if submitted && !self.input_buf.trim().is_empty() {
            let text = std::mem::take(&mut self.input_buf);
            self.send_message(&text);
        }
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

    /// Number of messages for status display.
    #[allow(dead_code)]
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Whether the agent is currently processing a query.
    #[allow(dead_code)]
    pub fn is_thinking(&self) -> bool {
        self.state == AgentState::Thinking
    }
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
        let panel = AgentPanel::new();
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
        let mut panel = AgentPanel::new();
        let initial_count = panel.message_count();
        panel.send_message("");
        assert_eq!(panel.message_count(), initial_count);
        panel.send_message("   ");
        assert_eq!(panel.message_count(), initial_count);
    }

    #[test]
    fn test_inject_context_empty() {
        let mut panel = AgentPanel::new();
        panel.inject_context(&[]);
        assert!(panel.pending_context.is_none());
    }

    #[test]
    fn test_inject_context_formats_correctly() {
        let mut panel = AgentPanel::new();
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
        let mut panel = AgentPanel::new();
        // tick() should be a no-op when no query is in flight.
        panel.tick();
        assert_eq!(panel.state, AgentState::Idle);
    }

    #[test]
    fn test_update_activity_stores_items() {
        let mut panel = AgentPanel::new();
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
        let mut panel = AgentPanel::new();
        panel.update_activity(vec!["first".to_string()]);
        assert_eq!(panel.activity_items.len(), 1);
        panel.update_activity(vec!["second".to_string(), "third".to_string()]);
        assert_eq!(panel.activity_items.len(), 2);
        assert_eq!(panel.activity_items[0], "second");
    }

    #[test]
    fn test_activity_expanded_default_true() {
        let panel = AgentPanel::new();
        assert!(panel.activity_expanded);
    }

    #[test]
    fn test_slash_clear_resets_messages() {
        let mut panel = AgentPanel::new();
        panel.messages.push(ChatMessage::user("hello"));
        panel.messages.push(ChatMessage::agent("world"));
        assert!(panel.messages.len() >= 3); // welcome + hello + world
        panel.send_message("/clear");
        assert_eq!(panel.messages.len(), 1); // only the "Chat cleared" system msg
        assert_eq!(panel.messages[0].role, ChatRole::System);
    }

    #[test]
    fn test_slash_help_adds_system_message() {
        let mut panel = AgentPanel::new();
        let before = panel.messages.len();
        panel.send_message("/help");
        assert_eq!(panel.messages.len(), before + 1);
        assert!(panel.messages.last().unwrap().content.contains("/clear"));
        assert!(panel.messages.last().unwrap().content.contains("/status"));
    }

    #[test]
    fn test_slash_status_shows_backend_info() {
        let mut panel = AgentPanel::new();
        let before = panel.messages.len();
        panel.send_message("/status");
        assert_eq!(panel.messages.len(), before + 1);
        assert!(panel.messages.last().unwrap().content.contains("Backend:"));
        assert!(panel.messages.last().unwrap().content.contains("State:"));
    }

    #[test]
    fn test_slash_activity_toggles() {
        let mut panel = AgentPanel::new();
        assert!(panel.activity_expanded);
        panel.send_message("/activity");
        assert!(!panel.activity_expanded);
        panel.send_message("/activity");
        assert!(panel.activity_expanded);
    }

    #[test]
    fn test_unknown_command_shows_error() {
        let mut panel = AgentPanel::new();
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
        let mut panel = AgentPanel::new();
        let before = panel.messages.len();
        panel.send_message("/HELP");
        assert_eq!(panel.messages.len(), before + 1);
        assert!(panel.messages.last().unwrap().content.contains("/clear"));
    }

    #[test]
    fn test_slash_command_does_not_dispatch_to_backend() {
        let mut panel = AgentPanel::new();
        panel.send_message("/clear");
        // Should NOT be in Thinking state (no backend dispatch).
        assert_eq!(panel.state, AgentState::Idle);
        assert!(panel.response_rx.is_none());
    }

    #[test]
    fn test_request_focus_sets_flag() {
        let mut panel = AgentPanel::new();
        assert!(!panel.focus_requested);
        panel.request_focus();
        assert!(panel.focus_requested);
    }

    #[test]
    fn test_clear_error_returns_to_idle() {
        let mut panel = AgentPanel::new();
        panel.state = AgentState::Error("test".into());
        panel.clear_error();
        assert_eq!(panel.state, AgentState::Idle);
    }

    #[test]
    fn test_clear_error_noop_when_idle() {
        let mut panel = AgentPanel::new();
        panel.clear_error();
        assert_eq!(panel.state, AgentState::Idle);
    }

    #[test]
    fn test_help_includes_shortcuts() {
        let mut panel = AgentPanel::new();
        panel.send_message("/help");
        let last = &panel.messages.last().unwrap().content;
        assert!(last.contains("Ctrl+L"));
        assert!(last.contains("Ctrl+N"));
        assert!(last.contains("Escape"));
    }
}
