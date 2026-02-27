//! Chat message model and rendering for the Agent Panel.
//!
//! User messages: right-aligned, purple-tinted background.
//! Agent messages: left-aligned, surface background.
//! System messages: centered, muted italic.

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::theme::colors;

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

/// Who sent a chat message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Agent,
    System,
}

/// A single message in the agent chat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl ChatMessage {
    pub fn user(content: &str) -> Self {
        Self {
            role: ChatRole::User,
            content: content.to_string(),
            timestamp: chrono::Utc::now(),
        }
    }

    pub fn agent(content: &str) -> Self {
        Self {
            role: ChatRole::Agent,
            content: content.to_string(),
            timestamp: chrono::Utc::now(),
        }
    }

    pub fn system(content: &str) -> Self {
        Self {
            role: ChatRole::System,
            content: content.to_string(),
            timestamp: chrono::Utc::now(),
        }
    }

    /// Seconds since this message was created.
    #[allow(dead_code)]
    pub fn age_secs(&self) -> i64 {
        chrono::Utc::now()
            .signed_duration_since(self.timestamp)
            .num_seconds()
    }
}

// ---------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------

/// Purple-tinted background for user message bubbles (chat-specific).
const USER_BG: egui::Color32 = egui::Color32::from_rgb(0x2d, 0x1b, 0x4e);
/// Lighter text for user messages (chat-specific).
const USER_TEXT: egui::Color32 = egui::Color32::from_rgb(0xe0, 0xd0, 0xff);

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Render a list of chat messages into a scrollable area.
///
/// If `scroll_to_bottom` is true, the scroll area will jump to the bottom
/// on the next frame (used when a new message arrives).
pub fn render_messages(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    scroll_to_bottom: bool,
    is_thinking: bool,
) {
    let scroll_id = ui.id().with("agent_chat_scroll");

    let mut area = egui::ScrollArea::vertical()
        .id_salt(scroll_id)
        .auto_shrink([false, false]);

    if scroll_to_bottom {
        area = area.stick_to_bottom(true);
    }

    area.show(ui, |ui| {
        ui.set_min_width(ui.available_width());

        if messages.is_empty() && !is_thinking {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    egui::RichText::new("No messages yet. Type below to start.")
                        .color(colors::TEXT_DIM)
                        .italics(),
                );
            });
            return;
        }

        for msg in messages {
            ui.add_space(8.0);
            render_single_message(ui, msg);
        }

        if is_thinking {
            ui.add_space(8.0);
            render_thinking_indicator(ui);
        }

        ui.add_space(8.0);
    });
}

/// Render a single chat message bubble with role label.
fn render_single_message(ui: &mut egui::Ui, msg: &ChatMessage) {
    let available_width = ui.available_width();
    let max_bubble_width = (available_width * 0.85).min(500.0);

    match msg.role {
        ChatRole::User => {
            // Right-aligned role label.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                ui.label(egui::RichText::new("You").small().color(colors::TEXT_DIM));
            });
            ui.add_space(2.0);
            // Right-aligned bubble.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                egui::Frame::new()
                    .fill(USER_BG)
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_max_width(max_bubble_width);
                        ui.label(egui::RichText::new(&msg.content).color(USER_TEXT));
                    });
            });
        }
        ChatRole::Agent => {
            // Left-aligned role label.
            ui.label(egui::RichText::new("Agent").small().color(colors::TEXT_DIM));
            ui.add_space(2.0);
            // Left-aligned bubble with purple accent.
            let frame_resp = egui::Frame::new()
                .fill(colors::SURFACE)
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(10, 6))
                .stroke(egui::Stroke::new(0.5, colors::BORDER))
                .show(ui, |ui| {
                    ui.set_max_width(max_bubble_width);
                    ui.label(egui::RichText::new(&msg.content).color(colors::TEXT));
                });
            // 2px purple accent on left edge.
            let rect = frame_resp.response.rect;
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    rect.left_top(),
                    egui::pos2(rect.left() + 2.0, rect.bottom()),
                ),
                1.0,
                colors::ACCENT,
            );
        }
        ChatRole::System => {
            // Centered, muted italic.
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(&msg.content)
                        .color(colors::TEXT_DIM)
                        .italics()
                        .small(),
                );
            });
        }
    }
}

/// Render a typing indicator when the agent is thinking.
fn render_thinking_indicator(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Agent").small().color(colors::TEXT_DIM));
    ui.add_space(2.0);
    let frame_resp = egui::Frame::new()
        .fill(colors::SURFACE)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .stroke(egui::Stroke::new(0.5, colors::BORDER))
        .show(ui, |ui| {
            ui.set_max_width(120.0);
            ui.label(egui::RichText::new("\u{25CF}  \u{25CF}  \u{25CF}").color(colors::TEXT_MUTED));
        });
    // 2px purple accent on left edge.
    let rect = frame_resp.response.rect;
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + 2.0, rect.bottom()),
        ),
        1.0,
        colors::ACCENT,
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_message_constructors() {
        let u = ChatMessage::user("hello");
        assert_eq!(u.role, ChatRole::User);
        assert_eq!(u.content, "hello");

        let a = ChatMessage::agent("response");
        assert_eq!(a.role, ChatRole::Agent);
        assert_eq!(a.content, "response");

        let s = ChatMessage::system("info");
        assert_eq!(s.role, ChatRole::System);
        assert_eq!(s.content, "info");
    }

    #[test]
    fn test_chat_message_age() {
        let msg = ChatMessage::user("test");
        // Age should be very small (< 1 second).
        assert!(msg.age_secs() < 2, "age_secs was {}", msg.age_secs());
    }

    #[test]
    fn test_chat_message_serialization_roundtrip() {
        let msg = ChatMessage::user("hello world");
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, ChatRole::User);
        assert_eq!(parsed.content, "hello world");
    }

    #[test]
    fn test_chat_role_eq() {
        assert_eq!(ChatRole::User, ChatRole::User);
        assert_ne!(ChatRole::User, ChatRole::Agent);
        assert_ne!(ChatRole::Agent, ChatRole::System);
    }
}
