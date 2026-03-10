use eframe::egui;

use super::{View, ViewId};
use crate::state::SharedState;
use crate::theme::colors;

#[derive(Debug)]
pub enum ArtifactUiAction {
    RunRemote {
        artifact_id: String,
        action_id: String,
        params: serde_json::Value,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderMode {
    Auto,
    Markdown,
    Timeline,
    Table,
    Diff,
    Log,
    RawJson,
}

pub struct ArtifactsView {
    selected_id: Option<String>,
    render_mode: RenderMode,
    pending_actions: Vec<ArtifactUiAction>,
}

impl ArtifactsView {
    pub fn new() -> Self {
        Self {
            selected_id: None,
            render_mode: RenderMode::Auto,
            pending_actions: Vec::new(),
        }
    }

    pub fn take_actions(&mut self) -> Vec<ArtifactUiAction> {
        std::mem::take(&mut self.pending_actions)
    }
}

impl View for ArtifactsView {
    fn id(&self) -> ViewId {
        ViewId::Artifacts
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, _ctx: &egui::Context) {
        let Some(snapshot) = state.ops_snapshot.as_ref() else {
            ui.label(
                egui::RichText::new("Artifact store is waiting for the daemon snapshot.")
                    .color(colors::TEXT_DIM),
            );
            return;
        };

        ui.horizontal(|ui| {
            ui.heading(egui::RichText::new("Experimental Artifacts").color(colors::TEXT));
            ui.label(
                egui::RichText::new(format!("{} project artifacts", snapshot.artifacts.len()))
                    .small()
                    .color(colors::TEXT_DIM),
            );
        });
        ui.label(
            egui::RichText::new(
                "Artifacts are daemon-produced review material. They are not proof that automatic coordination or memory recall succeeded.",
            )
            .small()
            .color(colors::YELLOW),
        );
        ui.add_space(8.0);

        let available = ui.available_size();
        let list_width = (available.x * 0.32).max(260.0);

        ui.horizontal(|ui| {
            ui.allocate_ui(egui::vec2(list_width, available.y), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("artifact_list")
                    .show(ui, |ui| {
                        for artifact in &snapshot.artifacts {
                            let selected = self.selected_id.as_deref() == Some(&artifact.id);
                            let button = egui::Button::new(
                                egui::RichText::new(format!(
                                    "{}\n{}",
                                    artifact.title, artifact.summary
                                ))
                                .color(if selected {
                                    colors::TEXT
                                } else {
                                    colors::TEXT_MUTED
                                }),
                            )
                            .fill(if selected {
                                colors::ACTIVE_BG
                            } else {
                                colors::SURFACE
                            })
                            .min_size(egui::vec2(list_width - 16.0, 56.0));
                            if ui.add(button).clicked() {
                                self.selected_id = Some(artifact.id.clone());
                            }
                            ui.add_space(6.0);
                        }
                    });
            });

            ui.separator();

            ui.allocate_ui(egui::vec2(ui.available_width(), available.y), |ui| {
                let selected_artifact = self
                    .selected_id
                    .as_ref()
                    .and_then(|selected_id| {
                        snapshot.artifacts.iter().find(|a| &a.id == selected_id)
                    })
                    .or_else(|| snapshot.artifacts.first());

                let Some(artifact) = selected_artifact else {
                    ui.label(
                        egui::RichText::new("No artifacts available yet.").color(colors::TEXT_DIM),
                    );
                    return;
                };

                self.selected_id.get_or_insert_with(|| artifact.id.clone());

                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(&artifact.title)
                            .heading()
                            .color(colors::TEXT),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "{} • {} • {}",
                            artifact.kind, artifact.schema, artifact.created_at
                        ))
                        .small()
                        .color(colors::TEXT_DIM),
                    );
                });
                ui.label(
                    egui::RichText::new(&artifact.summary)
                        .small()
                        .color(colors::TEXT_MUTED),
                );
                ui.add_space(8.0);

                ui.horizontal_wrapped(|ui| {
                    render_mode_button(ui, &mut self.render_mode, RenderMode::Auto, "Auto");
                    render_mode_button(ui, &mut self.render_mode, RenderMode::Markdown, "Markdown");
                    render_mode_button(ui, &mut self.render_mode, RenderMode::Timeline, "Timeline");
                    render_mode_button(ui, &mut self.render_mode, RenderMode::Table, "Table");
                    render_mode_button(ui, &mut self.render_mode, RenderMode::Diff, "Diff");
                    render_mode_button(ui, &mut self.render_mode, RenderMode::Log, "Log");
                    render_mode_button(ui, &mut self.render_mode, RenderMode::RawJson, "Raw");
                });

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    for action in &artifact.actions {
                        if ui
                            .button(egui::RichText::new(&action.label).color(
                                match action.kind.as_str() {
                                    "apply" => colors::GREEN,
                                    "acknowledge" => colors::ACCENT,
                                    _ => colors::TEXT,
                                },
                            ))
                            .clicked()
                        {
                            self.pending_actions.push(ArtifactUiAction::RunRemote {
                                artifact_id: artifact.id.clone(),
                                action_id: action.id.clone(),
                                params: serde_json::Value::Null,
                            });
                        }
                    }
                });

                ui.add_space(8.0);
                render_artifact_payload(ui, artifact, self.render_mode);
            });
        });
    }
}

fn render_mode_button(ui: &mut egui::Ui, current: &mut RenderMode, mode: RenderMode, label: &str) {
    if ui
        .selectable_label(*current == mode, egui::RichText::new(label).small())
        .clicked()
    {
        *current = mode;
    }
}

fn render_artifact_payload(
    ui: &mut egui::Ui,
    artifact: &impulse_ops::ArtifactEnvelope,
    requested_mode: RenderMode,
) {
    let mode = if requested_mode == RenderMode::Auto {
        auto_mode(artifact)
    } else {
        requested_mode
    };

    // Check if the requested mode can render this artifact, show feedback if not
    if requested_mode != RenderMode::Auto
        && requested_mode != RenderMode::RawJson
        && !can_render(artifact, requested_mode)
    {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "Render mode {:?} unavailable for this artifact — showing raw JSON.",
                    requested_mode
                ))
                .small()
                .color(colors::YELLOW),
            );
        });
        ui.add_space(4.0);
        render_raw_json(ui, artifact);
        return;
    }

    match mode {
        RenderMode::Markdown => render_markdown(ui, artifact),
        RenderMode::Timeline => render_timeline(ui, artifact),
        RenderMode::Table => render_table(ui, artifact),
        RenderMode::Diff => render_diff(ui, artifact),
        RenderMode::Log => render_log(ui, artifact),
        RenderMode::RawJson | RenderMode::Auto => render_raw_json(ui, artifact),
    }
}

/// Check if an artifact has the expected fields for a given render mode.
fn can_render(artifact: &impulse_ops::ArtifactEnvelope, mode: RenderMode) -> bool {
    match mode {
        RenderMode::Markdown => artifact.payload.get("markdown").is_some(),
        RenderMode::Timeline => artifact
            .payload
            .get("entries")
            .and_then(|v| v.as_array())
            .is_some(),
        RenderMode::Table => artifact
            .payload
            .get("entries")
            .and_then(|v| v.as_array())
            .is_some(),
        RenderMode::Diff => {
            artifact.payload.get("before").is_some() || artifact.payload.get("after").is_some()
        }
        RenderMode::Log => artifact
            .payload
            .get("lines")
            .and_then(|v| v.as_array())
            .is_some(),
        RenderMode::RawJson | RenderMode::Auto => true,
    }
}

fn auto_mode(artifact: &impulse_ops::ArtifactEnvelope) -> RenderMode {
    if artifact
        .view_hints
        .contains(&impulse_ops::ArtifactViewHint::Markdown)
    {
        RenderMode::Markdown
    } else if artifact
        .view_hints
        .contains(&impulse_ops::ArtifactViewHint::Timeline)
    {
        RenderMode::Timeline
    } else if artifact
        .view_hints
        .contains(&impulse_ops::ArtifactViewHint::Table)
    {
        RenderMode::Table
    } else {
        RenderMode::RawJson
    }
}

fn render_markdown(ui: &mut egui::Ui, artifact: &impulse_ops::ArtifactEnvelope) {
    if let Some(markdown) = artifact
        .payload
        .get("markdown")
        .and_then(|value| value.as_str())
    {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(
                egui::RichText::new(markdown)
                    .monospace()
                    .color(colors::TEXT),
            );
        });
    } else {
        render_raw_json(ui, artifact);
    }
}

fn render_timeline(ui: &mut egui::Ui, artifact: &impulse_ops::ArtifactEnvelope) {
    let Some(entries) = artifact
        .payload
        .get("entries")
        .and_then(|value| value.as_array())
    else {
        render_raw_json(ui, artifact);
        return;
    };
    egui::ScrollArea::vertical().show(ui, |ui| {
        for entry in entries {
            let timestamp = entry
                .get("timestamp")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let agent = entry
                .get("agent_label")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let kind = entry
                .get("kind")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let content = entry
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            egui::Frame::new()
                .fill(colors::SURFACE)
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} • {} • {}", timestamp, agent, kind))
                            .small()
                            .color(colors::ACCENT),
                    );
                    ui.label(egui::RichText::new(content).small().color(colors::TEXT));
                });
            ui.add_space(6.0);
        }
    });
}

fn render_table(ui: &mut egui::Ui, artifact: &impulse_ops::ArtifactEnvelope) {
    let Some(entries) = artifact
        .payload
        .get("entries")
        .and_then(|value| value.as_array())
    else {
        render_raw_json(ui, artifact);
        return;
    };

    egui::ScrollArea::both().show(ui, |ui| {
        egui::Grid::new("artifact_table")
            .striped(true)
            .spacing(egui::vec2(12.0, 6.0))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Timestamp").strong());
                ui.label(egui::RichText::new("Agent").strong());
                ui.label(egui::RichText::new("Kind").strong());
                ui.label(egui::RichText::new("Content").strong());
                ui.end_row();

                for entry in entries {
                    ui.label(
                        entry
                            .get("timestamp")
                            .and_then(|value| value.as_str())
                            .unwrap_or(""),
                    );
                    ui.label(
                        entry
                            .get("agent_label")
                            .and_then(|value| value.as_str())
                            .unwrap_or(""),
                    );
                    ui.label(
                        entry
                            .get("kind")
                            .and_then(|value| value.as_str())
                            .unwrap_or(""),
                    );
                    ui.label(
                        entry
                            .get("content")
                            .and_then(|value| value.as_str())
                            .unwrap_or(""),
                    );
                    ui.end_row();
                }
            });
    });
}

fn render_diff(ui: &mut egui::Ui, artifact: &impulse_ops::ArtifactEnvelope) {
    let before = artifact
        .payload
        .get("before")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let after = artifact
        .payload
        .get("after")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if before.is_empty() && after.is_empty() {
        render_raw_json(ui, artifact);
        return;
    }
    ui.columns(2, |columns| {
        columns[0].label(egui::RichText::new(before).monospace().color(colors::RED));
        columns[1].label(egui::RichText::new(after).monospace().color(colors::GREEN));
    });
}

fn render_log(ui: &mut egui::Ui, artifact: &impulse_ops::ArtifactEnvelope) {
    if let Some(lines) = artifact
        .payload
        .get("lines")
        .and_then(|value| value.as_array())
    {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in lines {
                ui.label(
                    egui::RichText::new(line.as_str().unwrap_or(""))
                        .monospace()
                        .small()
                        .color(colors::TEXT),
                );
            }
        });
    } else {
        render_raw_json(ui, artifact);
    }
}

fn render_raw_json(ui: &mut egui::Ui, artifact: &impulse_ops::ArtifactEnvelope) {
    let raw = serde_json::to_string_pretty(&artifact.payload).unwrap_or_else(|_| "{}".to_string());
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.label(
            egui::RichText::new(raw)
                .monospace()
                .small()
                .color(colors::TEXT),
        );
    });
}
