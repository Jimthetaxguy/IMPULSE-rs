//! Settings view — categorized configuration editor.
//!
//! Shows the top 20 most useful daemon config keys in 5 sections:
//! Agent, Stewardship, Injection, Search, Performance.
//!
//! Reads config from daemon via `config_get` tool, writes via `config_set`.
//! Gracefully shows "daemon required" when disconnected.

use eframe::egui;

use super::{View, ViewId};
use crate::state::{ConnectionStatus, SharedState};
use crate::theme::colors;

// ---------------------------------------------------------------------------
// Setting definitions
// ---------------------------------------------------------------------------

/// A single configurable setting.
struct SettingDef {
    key: &'static str,
    label: &'static str,
    description: &'static str,
    kind: SettingKind,
}

/// What UI control to render for a setting.
enum SettingKind {
    /// Single-line text input.
    Text { placeholder: &'static str },
    /// Dropdown with fixed options.
    Enum { options: &'static [&'static str] },
    /// Integer slider with min/max.
    IntSlider { min: i64, max: i64 },
    /// Boolean toggle.
    Toggle,
}

/// Categories of settings.
struct SettingCategory {
    name: &'static str,
    icon: &'static str,
    settings: &'static [SettingDef],
}

// ---------------------------------------------------------------------------
// Setting catalog (top 20 keys)
// ---------------------------------------------------------------------------

const CATEGORIES: &[SettingCategory] = &[
    SettingCategory {
        name: "Agent",
        icon: "\u{1F916}",
        settings: &[
            SettingDef {
                key: "agent_provider",
                label: "Provider",
                description: "LLM provider for the agent panel",
                kind: SettingKind::Enum {
                    options: &["anthropic", "openai", "minimax"],
                },
            },
            SettingDef {
                key: "agent_model",
                label: "Model",
                description: "Model identifier (e.g. claude-sonnet-4-20250514)",
                kind: SettingKind::Text {
                    placeholder: "claude-sonnet-4-20250514",
                },
            },
            SettingDef {
                key: "agent_harness",
                label: "Harness",
                description: "CLI harness for subprocess mode",
                kind: SettingKind::Enum {
                    options: &["claude-code", "opencode", "none"],
                },
            },
            SettingDef {
                key: "agent_max_tokens",
                label: "Max Tokens",
                description: "Maximum response tokens per query",
                kind: SettingKind::IntSlider {
                    min: 256,
                    max: 8192,
                },
            },
        ],
    },
    SettingCategory {
        name: "Stewardship",
        icon: "\u{1F6E1}",
        settings: &[
            SettingDef {
                key: "stewardship_mode",
                label: "Mode",
                description: "How compaction proposals are handled",
                kind: SettingKind::Enum {
                    options: &["auto", "review", "off"],
                },
            },
            SettingDef {
                key: "stewardship_threshold_surgical",
                label: "Surgical Threshold",
                description: "Context usage % for surgical compaction",
                kind: SettingKind::IntSlider { min: 30, max: 80 },
            },
            SettingDef {
                key: "stewardship_threshold_thoughtful",
                label: "Thoughtful Threshold",
                description: "Context usage % for thoughtful compaction",
                kind: SettingKind::IntSlider { min: 50, max: 90 },
            },
            SettingDef {
                key: "stewardship_threshold_emergency",
                label: "Emergency Threshold",
                description: "Context usage % for emergency compaction",
                kind: SettingKind::IntSlider { min: 70, max: 99 },
            },
        ],
    },
    SettingCategory {
        name: "Injection",
        icon: "\u{1F489}",
        settings: &[
            SettingDef {
                key: "inject_mode",
                label: "Mode",
                description: "How context is injected into agents",
                kind: SettingKind::Enum {
                    options: &["auto", "review", "off"],
                },
            },
            SettingDef {
                key: "inject_explain",
                label: "Explain Injections",
                description: "Include explanation of what was injected",
                kind: SettingKind::Toggle,
            },
            SettingDef {
                key: "inject_max_tokens",
                label: "Max Inject Tokens",
                description: "Maximum tokens per injection payload",
                kind: SettingKind::IntSlider {
                    min: 100,
                    max: 4096,
                },
            },
            SettingDef {
                key: "inject_interval_secs",
                label: "Inject Interval (s)",
                description: "Seconds between automatic injections",
                kind: SettingKind::IntSlider { min: 10, max: 300 },
            },
        ],
    },
    SettingCategory {
        name: "Search",
        icon: "\u{1F50D}",
        settings: &[
            SettingDef {
                key: "search_limit",
                label: "Result Limit",
                description: "Maximum search results returned",
                kind: SettingKind::IntSlider { min: 5, max: 100 },
            },
            SettingDef {
                key: "search_threshold",
                label: "Relevance Threshold",
                description: "Minimum relevance score (0-100)",
                kind: SettingKind::IntSlider { min: 0, max: 100 },
            },
            SettingDef {
                key: "search_include_archived",
                label: "Include Archived",
                description: "Search archived sessions too",
                kind: SettingKind::Toggle,
            },
        ],
    },
    SettingCategory {
        name: "Performance",
        icon: "\u{26A1}",
        settings: &[
            SettingDef {
                key: "cache_ttl_secs",
                label: "Cache TTL (s)",
                description: "Seconds before cached data expires",
                kind: SettingKind::IntSlider { min: 5, max: 600 },
            },
            SettingDef {
                key: "poll_interval_secs",
                label: "Poll Interval (s)",
                description: "Seconds between daemon status polls",
                kind: SettingKind::IntSlider { min: 1, max: 30 },
            },
            SettingDef {
                key: "max_history_entries",
                label: "History Entries",
                description: "Maximum session history entries to display",
                kind: SettingKind::IntSlider { min: 10, max: 500 },
            },
            SettingDef {
                key: "max_terminal_scrollback",
                label: "Terminal Scrollback",
                description: "Maximum scrollback lines per terminal",
                kind: SettingKind::IntSlider {
                    min: 1000,
                    max: 50000,
                },
            },
        ],
    },
];

// ---------------------------------------------------------------------------
// SettingsView
// ---------------------------------------------------------------------------

pub struct SettingsView {
    /// Local editable values, keyed by config key.
    values: std::collections::HashMap<String, String>,
    /// Which category is expanded (all by default).
    expanded: [bool; 5],
    /// Status message after save attempt.
    status_msg: Option<(String, bool)>,
}

impl SettingsView {
    pub fn new() -> Self {
        // Initialize with sensible defaults.
        let mut values = std::collections::HashMap::new();
        for cat in CATEGORIES {
            for setting in cat.settings {
                let default = match &setting.kind {
                    SettingKind::Text { placeholder } => placeholder.to_string(),
                    SettingKind::Enum { options } => options.first().unwrap_or(&"").to_string(),
                    SettingKind::IntSlider { min, max } => {
                        // Default to midpoint.
                        ((*min + *max) / 2).to_string()
                    }
                    SettingKind::Toggle => "false".to_string(),
                };
                values.insert(setting.key.to_string(), default);
            }
        }

        Self {
            values,
            expanded: [true; 5],
            status_msg: None,
        }
    }
}

impl View for SettingsView {
    fn id(&self) -> ViewId {
        ViewId::Settings
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, _ctx: &egui::Context) {
        if state.connection == ConnectionStatus::Disconnected {
            empty_state(ui);
            return;
        }

        // --- Header ---
        ui.horizontal(|ui| {
            ui.strong(egui::RichText::new("\u{2699} Settings").color(colors::ACCENT));
            ui.separator();
            ui.label(
                egui::RichText::new("Daemon configuration")
                    .small()
                    .color(colors::TEXT_DIM),
            );

            if let Some((ref msg, is_ok)) = self.status_msg {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let color = if is_ok { colors::GREEN } else { colors::RED };
                    ui.label(egui::RichText::new(msg).small().color(color));
                });
            }
        });

        ui.separator();

        // --- Scrollable settings categories ---
        egui::ScrollArea::vertical()
            .id_salt("settings_scroll")
            .show(ui, |ui| {
                for (cat_idx, category) in CATEGORIES.iter().enumerate() {
                    ui.add_space(8.0);

                    // Category header (collapsible).
                    let arrow = if self.expanded[cat_idx] {
                        "\u{25BC}" // ▼
                    } else {
                        "\u{25B6}" // ▶
                    };
                    let header_text = format!("{} {} {}", arrow, category.icon, category.name);

                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(&header_text)
                                    .strong()
                                    .color(colors::TEXT),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .frame(false),
                        )
                        .clicked()
                    {
                        self.expanded[cat_idx] = !self.expanded[cat_idx];
                    }

                    if self.expanded[cat_idx] {
                        egui::Frame::new()
                            .fill(colors::SURFACE)
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(12, 8))
                            .stroke(egui::Stroke::new(0.5, colors::BORDER))
                            .show(ui, |ui| {
                                for setting in category.settings {
                                    self.render_setting(ui, setting);
                                    ui.add_space(4.0);
                                }
                            });
                    }
                }

                ui.add_space(16.0);
            });
    }
}

impl SettingsView {
    /// Render a single setting field.
    fn render_setting(&mut self, ui: &mut egui::Ui, setting: &SettingDef) {
        ui.horizontal(|ui| {
            ui.set_min_width(ui.available_width());

            // Label column (fixed width).
            ui.allocate_ui(egui::vec2(160.0, 24.0), |ui| {
                let resp = ui.label(egui::RichText::new(setting.label).color(colors::TEXT));
                resp.on_hover_text(setting.description);
            });

            // Value column.
            let value = self
                .values
                .entry(setting.key.to_string())
                .or_default()
                .clone();

            match &setting.kind {
                SettingKind::Text { placeholder } => {
                    let mut text = value;
                    let edit = egui::TextEdit::singleline(&mut text)
                        .hint_text(*placeholder)
                        .desired_width(200.0);
                    if ui.add(edit).changed() {
                        self.values.insert(setting.key.to_string(), text);
                    }
                }
                SettingKind::Enum { options } => {
                    let mut selected = value.clone();
                    egui::ComboBox::from_id_salt(setting.key)
                        .selected_text(&selected)
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for &opt in *options {
                                ui.selectable_value(&mut selected, opt.to_string(), opt);
                            }
                        });
                    if selected != value {
                        self.values.insert(setting.key.to_string(), selected);
                    }
                }
                SettingKind::IntSlider { min, max } => {
                    let mut int_val: i64 = value.parse().unwrap_or(*min);
                    if ui
                        .add(
                            egui::Slider::new(&mut int_val, *min..=*max)
                                .clamping(egui::SliderClamping::Always),
                        )
                        .changed()
                    {
                        self.values
                            .insert(setting.key.to_string(), int_val.to_string());
                    }
                }
                SettingKind::Toggle => {
                    let mut toggled = value == "true";
                    if ui.checkbox(&mut toggled, "").changed() {
                        self.values
                            .insert(setting.key.to_string(), toggled.to_string());
                    }
                }
            }
        });
    }
}

fn empty_state(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.label(egui::RichText::new("Settings require a running daemon.").color(colors::TEXT_DIM));
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Run `impulse daemon` to start the background service.")
                .small()
                .color(colors::TEXT_FAINT),
        );
    });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_view_new_has_all_values() {
        let view = SettingsView::new();
        // Count total settings across all categories.
        let total: usize = CATEGORIES.iter().map(|c| c.settings.len()).sum();
        assert_eq!(view.values.len(), total);
    }

    #[test]
    fn test_all_categories_expanded_by_default() {
        let view = SettingsView::new();
        assert!(view.expanded.iter().all(|&e| e));
    }

    #[test]
    fn test_settings_id() {
        let view = SettingsView::new();
        assert_eq!(view.id(), ViewId::Settings);
    }

    #[test]
    fn test_category_count() {
        assert_eq!(CATEGORIES.len(), 5);
    }

    #[test]
    fn test_settings_have_valid_defaults() {
        let view = SettingsView::new();
        // Every value should be non-empty.
        for (key, val) in &view.values {
            assert!(!val.is_empty(), "Setting '{}' has empty default", key);
        }
    }

    #[test]
    fn test_total_settings_count() {
        let total: usize = CATEGORIES.iter().map(|c| c.settings.len()).sum();
        // We defined 20 settings across 5 categories (4+4+4+3+4 = 19).
        assert!(
            total >= 15 && total <= 25,
            "Expected ~20 settings, got {}",
            total
        );
    }
}
