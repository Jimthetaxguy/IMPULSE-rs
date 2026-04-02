//! Collapsible navigation sidebar.
//!
//! 44px when collapsed (icons only), 180px when expanded (icons + labels).
//! Toggle with `Ctrl+B`. Rocket logo at top, connection status at bottom.

use eframe::egui;

use crate::state::{ConnectionStatus, SharedState};
use crate::theme::colors;
use crate::views::ViewId;

const COLLAPSED_WIDTH: f32 = 44.0;
const EXPANDED_WIDTH: f32 = 180.0;

/// Actions returned by the sidebar.
pub struct SidebarAction {
    /// A new view was selected (if any).
    pub new_view: Option<ViewId>,
    /// The agent panel toggle was clicked.
    pub toggle_agent: bool,
}

/// Render the sidebar and return any actions taken.
pub fn show(
    ctx: &egui::Context,
    active: ViewId,
    expanded: bool,
    agent_visible: bool,
    state: &SharedState,
) -> SidebarAction {
    let mut action = SidebarAction {
        new_view: None,
        toggle_agent: false,
    };
    let width = if expanded {
        EXPANDED_WIDTH
    } else {
        COLLAPSED_WIDTH
    };

    egui::SidePanel::left("sidebar")
        .resizable(false)
        .exact_width(width)
        .show(ctx, |ui| {
            ui.add_space(10.0);

            // Rocket logo + brand name.
            if expanded {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("\u{1F680}").size(18.0)); // 🚀
                    ui.add_space(2.0);
                    ui.strong(
                        egui::RichText::new("IMPULSE")
                            .color(colors::ACCENT)
                            .size(14.0),
                    );
                });
            } else {
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("\u{1F680}").size(20.0)); // 🚀
                });
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // Navigation items — 4 views.
            for &view_id in ViewId::all() {
                let is_active = view_id == active;

                let text = if expanded {
                    format!("{}  {}", view_id.icon(), view_id.title())
                } else {
                    view_id.icon().to_string()
                };

                let color = if is_active {
                    colors::ACCENT
                } else {
                    colors::TEXT_MUTED
                };

                let btn = egui::Button::new(egui::RichText::new(&text).color(color))
                    .fill(if is_active {
                        colors::SURFACE
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .corner_radius(egui::CornerRadius::same(6))
                    .min_size(egui::vec2(width - 16.0, 32.0));

                let resp = ui.add(btn);
                if resp.clicked() {
                    action.new_view = Some(view_id);
                }
                if !expanded {
                    resp.on_hover_text(format!(
                        "{} ({})",
                        view_id.title(),
                        view_id.shortcut_label()
                    ));
                }
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            // Agent panel toggle button.
            let agent_text = if expanded {
                "\u{25CF}  Supervisor".to_string()
            } else {
                "\u{25CF}".to_string()
            };

            let agent_color = if agent_visible {
                colors::GREEN
            } else {
                colors::TEXT_MUTED
            };

            let agent_btn = egui::Button::new(egui::RichText::new(&agent_text).color(agent_color))
                .fill(if agent_visible {
                    colors::ACTIVE_AGENT_BG
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(egui::CornerRadius::same(6))
                .min_size(egui::vec2(width - 16.0, 32.0));

            let agent_resp = ui.add(agent_btn);
            if agent_resp.clicked() {
                action.toggle_agent = true;
            }
            if !expanded {
                agent_resp.on_hover_text("Supervisor Panel (Ctrl+E)");
            }

            // Push remaining content to bottom.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.add_space(8.0);

                // Connection status indicator.
                let (dot_color, label) = match state.connection {
                    ConnectionStatus::Connected => (colors::GREEN, "Online"),
                    ConnectionStatus::Connecting => (colors::YELLOW, "Connecting"),
                    ConnectionStatus::Disconnected => (colors::TEXT_DIM, "Offline"),
                };

                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    let dot = ui.allocate_space(egui::vec2(8.0, 8.0));
                    ui.painter().circle_filled(dot.1.center(), 3.5, dot_color);
                    if expanded {
                        ui.label(egui::RichText::new(label).small().color(colors::TEXT_DIM));
                    }
                });
            });
        });

    action
}
