//! Collapsible navigation sidebar.
//!
//! 48px when collapsed (icons only), 160px when expanded (icons + labels).
//! Toggle with `Ctrl+B`.

use eframe::egui;

use crate::state::{ConnectionStatus, SharedState};
use crate::theme::colors;
use crate::views::ViewId;

const COLLAPSED_WIDTH: f32 = 48.0;
const EXPANDED_WIDTH: f32 = 160.0;

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
            ui.add_space(8.0);

            // Logo / brand.
            if expanded {
                ui.horizontal(|ui| {
                    ui.strong(egui::RichText::new("IMPULSE").color(colors::ACCENT));
                });
            } else {
                ui.vertical_centered(|ui| {
                    ui.strong(egui::RichText::new("I").color(colors::ACCENT).size(18.0));
                });
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // Navigation items.
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
                        colors::ACTIVE_BG
                    } else {
                        egui::Color32::TRANSPARENT
                    })
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
                "\u{25CF}  Agent".to_string()
            } else {
                "AG".to_string()
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
                .min_size(egui::vec2(width - 16.0, 32.0));

            let agent_resp = ui.add(agent_btn);
            if agent_resp.clicked() {
                action.toggle_agent = true;
            }
            if !expanded {
                agent_resp.on_hover_text("Agent Panel (Ctrl+5)");
            }

            // Push remaining content to bottom.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.add_space(8.0);

                if let Some(snapshot) = state.ops_snapshot.as_ref() {
                    let pending_reviews = snapshot.context.pending_review_count;
                    let interventions = snapshot.interventions.len();
                    if expanded {
                        ui.label(
                            egui::RichText::new(format!(
                                "Experimental ops {}  Reviews {}",
                                interventions, pending_reviews
                            ))
                            .small()
                            .color(if pending_reviews > 0 {
                                colors::YELLOW
                            } else {
                                colors::TEXT_DIM
                            }),
                        );
                    }
                    ui.add_space(6.0);
                }

                // Connection status indicator.
                let (dot_color, label) = match state.connection {
                    ConnectionStatus::Connected => (colors::GREEN, "Online"),
                    ConnectionStatus::Connecting => (colors::YELLOW, "Connecting"),
                    ConnectionStatus::Disconnected => (colors::TEXT_DIM, "Offline"),
                };

                ui.horizontal(|ui| {
                    let dot = ui.allocate_space(egui::vec2(10.0, 10.0));
                    ui.painter().circle_filled(dot.1.center(), 4.0, dot_color);
                    if expanded {
                        ui.label(egui::RichText::new(label).small().color(colors::TEXT_DIM));
                    }
                });
            });
        });

    action
}
