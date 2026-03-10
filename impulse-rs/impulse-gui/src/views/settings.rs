//! Settings view — categorized configuration editor.
//!
//! Shows the top 20 most useful daemon config keys in 5 sections:
//! Agent, Stewardship, Injection, Search, Performance.
//!
//! Reads config from daemon via `config_get` tool, writes via `config_set`.
//! Gracefully shows "daemon required" when disconnected.

use eframe::egui;

use super::{View, ViewId};
use crate::state::SharedState;
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
    /// Whether values have been modified since last save.
    dirty: bool,
    /// Supervisor actions queued from the secondary permission surface.
    pending_supervisor_actions: Vec<impulse_ops::SupervisorAction>,
    /// Command sender to push settings updates to the poller thread.
    poller_cmd: Option<std::sync::mpsc::Sender<crate::state::PollerCommand>>,
}

impl SettingsView {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::with_poller(None)
    }

    pub fn with_poller(
        poller_cmd: Option<std::sync::mpsc::Sender<crate::state::PollerCommand>>,
    ) -> Self {
        // Load saved settings from GlobalConfig, falling back to defaults.
        let impulse_home = crate::global_config::GlobalConfig::impulse_home();
        let config = crate::global_config::GlobalConfig::load(&impulse_home).unwrap_or_default();

        let mut values = std::collections::HashMap::new();
        for cat in CATEGORIES {
            for setting in cat.settings {
                let default = default_for_setting(setting);
                // Use saved value if present, otherwise use default.
                let value = config.settings.get(setting.key).cloned().unwrap_or(default);
                values.insert(setting.key.to_string(), value);
            }
        }

        Self {
            values,
            expanded: [true; 5],
            status_msg: None,
            dirty: false,
            pending_supervisor_actions: Vec::new(),
            poller_cmd,
        }
    }

    /// Save current settings to `~/.impulse/config.json` and push to poller.
    fn save_settings(&mut self) {
        let impulse_home = crate::global_config::GlobalConfig::impulse_home();
        let mut config =
            crate::global_config::GlobalConfig::load(&impulse_home).unwrap_or_default();

        config.settings = self.values.clone();

        match config.save(&impulse_home) {
            Ok(()) => {
                // Push updated settings to the poller thread so they take effect immediately.
                let runtime = crate::state::RuntimeSettings::from_map(&self.values);
                if let Some(ref tx) = self.poller_cmd {
                    let _ = tx.send(crate::state::PollerCommand::UpdateSettings(runtime));
                }
                self.status_msg = Some(("Settings saved".to_string(), true));
                self.dirty = false;
            }
            Err(e) => {
                self.status_msg = Some((format!("Save failed: {}", e), false));
            }
        }
    }

    /// Reset all settings in a category to their defaults.
    fn reset_category(&mut self, cat_idx: usize) {
        if let Some(category) = CATEGORIES.get(cat_idx) {
            for setting in category.settings {
                let default = default_for_setting(setting);
                self.values.insert(setting.key.to_string(), default);
            }
            self.dirty = true;
        }
    }

    pub fn take_supervisor_actions(&mut self) -> Vec<impulse_ops::SupervisorAction> {
        std::mem::take(&mut self.pending_supervisor_actions)
    }
}

/// Get the default value for a setting definition.
fn default_for_setting(setting: &SettingDef) -> String {
    match &setting.kind {
        SettingKind::Text { placeholder } => placeholder.to_string(),
        SettingKind::Enum { options } => options.first().unwrap_or(&"").to_string(),
        SettingKind::IntSlider { min, max } => ((*min + *max) / 2).to_string(),
        SettingKind::Toggle => "false".to_string(),
    }
}

impl View for SettingsView {
    fn id(&self) -> ViewId {
        ViewId::Settings
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, _ctx: &egui::Context) {
        // --- Header ---
        ui.horizontal(|ui| {
            ui.strong(egui::RichText::new("\u{2699} Settings").color(colors::ACCENT));
            ui.separator();
            ui.label(
                egui::RichText::new("Application configuration")
                    .small()
                    .color(colors::TEXT_DIM),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Status message.
                if let Some((ref msg, is_ok)) = self.status_msg {
                    let color = if is_ok { colors::GREEN } else { colors::RED };
                    ui.label(egui::RichText::new(msg).small().color(color));
                }

                // Save button (highlighted when dirty).
                let save_color = if self.dirty {
                    colors::ACCENT
                } else {
                    colors::TEXT_DIM
                };
                if ui
                    .add_enabled(
                        self.dirty,
                        egui::Button::new(egui::RichText::new("Save").color(save_color)),
                    )
                    .clicked()
                {
                    self.save_settings();
                }
            });
        });

        ui.separator();

        self.render_supervisor_permissions(ui, state);
        ui.add_space(8.0);

        // --- Scrollable settings categories ---
        egui::ScrollArea::vertical()
            .id_salt("settings_scroll")
            .show(ui, |ui| {
                let mut reset_cat: Option<usize> = None;

                for (cat_idx, category) in CATEGORIES.iter().enumerate() {
                    ui.add_space(8.0);

                    // Category header (collapsible) with reset button.
                    ui.horizontal(|ui| {
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
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("Reset").clicked() {
                                        reset_cat = Some(cat_idx);
                                    }
                                },
                            );
                        }
                    });

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

                if let Some(idx) = reset_cat {
                    self.reset_category(idx);
                }

                ui.add_space(16.0);
            });
    }
}

impl SettingsView {
    fn render_supervisor_permissions(&mut self, ui: &mut egui::Ui, state: &SharedState) {
        egui::Frame::new()
            .fill(colors::SURFACE)
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(12, 10))
            .stroke(egui::Stroke::new(0.5, colors::BORDER))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Supervisor Permissions")
                            .strong()
                            .color(colors::ACCENT),
                    );
                    if state
                        .supervisor_permissions
                        .as_ref()
                        .map(|permission_state| permission_state.session_override_active())
                        .unwrap_or(false)
                    {
                        ui.label(
                            egui::RichText::new("session override active")
                                .small()
                                .color(colors::YELLOW),
                        );
                    }
                });
                ui.add_space(4.0);

                let Some(permission_state) = state.supervisor_permissions.as_ref() else {
                    ui.label(
                        egui::RichText::new("Waiting for daemon-backed supervisor policy.")
                            .small()
                            .color(colors::TEXT_DIM),
                    );
                    return;
                };

                // Interactive permission toggles — click to grant/deny.
                ui.label(
                    egui::RichText::new("Action Permissions (click to toggle)")
                        .small()
                        .color(colors::TEXT_MUTED),
                );
                ui.horizontal_wrapped(|ui| {
                    for &perm in ALL_PERMISSIONS {
                        let allowed = permission_state.effective.allows_action(perm);
                        let needs_confirm = permission_state.effective.requires_confirmation(perm);
                        let (color, tooltip) = if allowed && needs_confirm {
                            (
                                colors::YELLOW,
                                "Allowed (requires confirmation) — click to deny",
                            )
                        } else if allowed {
                            (colors::GREEN, "Allowed — click to deny")
                        } else {
                            (colors::TEXT_DIM, "Denied — click to grant for this session")
                        };
                        let resp = render_toggle_chip(ui, perm.as_str(), color, allowed);
                        if resp.clicked() && !allowed {
                            self.pending_supervisor_actions.push(
                                impulse_ops::SupervisorAction::ModifyPermissions {
                                    scope: impulse_ops::PermissionChangeScope::SessionOverride,
                                    grant_actions: vec![perm],
                                    grant_tool_capabilities: vec![],
                                    confirmed: true,
                                },
                            );
                        }
                        resp.on_hover_text(tooltip);
                    }
                });

                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Tool Capabilities")
                        .small()
                        .color(colors::TEXT_MUTED),
                );
                ui.horizontal_wrapped(|ui| {
                    for capability in &permission_state.effective.allowed_tool_capabilities {
                        render_chip(ui, capability.as_str(), colors::BLUE);
                    }
                });

                if permission_state.session_override_active() {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("Session override grants are active above.")
                            .small()
                            .color(colors::YELLOW),
                    );
                }

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Clear Session Override").clicked() {
                        self.pending_supervisor_actions.push(
                            impulse_ops::SupervisorAction::ClearSessionOverride { confirmed: true },
                        );
                    }
                    if ui.button("Reset Baseline to Defaults").clicked() {
                        self.pending_supervisor_actions.push(
                            impulse_ops::SupervisorAction::ResetBaselinePermissions {
                                confirmed: true,
                            },
                        );
                    }
                });
            });
    }

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
                        self.dirty = true;
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
                        self.dirty = true;
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
                        self.dirty = true;
                    }
                }
                SettingKind::Toggle => {
                    let mut toggled = value == "true";
                    if ui.checkbox(&mut toggled, "").changed() {
                        self.values
                            .insert(setting.key.to_string(), toggled.to_string());
                        self.dirty = true;
                    }
                }
            }
        });
    }
}

/// All supervisor action permissions for the interactive toggle display.
const ALL_PERMISSIONS: &[impulse_ops::SupervisorActionPermission] = &[
    impulse_ops::SupervisorActionPermission::MonitorAgents,
    impulse_ops::SupervisorActionPermission::FocusAgent,
    impulse_ops::SupervisorActionPermission::OpenReview,
    impulse_ops::SupervisorActionPermission::SearchMemory,
    impulse_ops::SupervisorActionPermission::SendInput,
    impulse_ops::SupervisorActionPermission::InjectContext,
    impulse_ops::SupervisorActionPermission::CleanupContext,
    impulse_ops::SupervisorActionPermission::HandoffContext,
    impulse_ops::SupervisorActionPermission::ModifyPermissions,
];

fn render_toggle_chip(
    ui: &mut egui::Ui,
    label: &str,
    accent: egui::Color32,
    active: bool,
) -> egui::Response {
    let fill = if active { colors::SURFACE } else { colors::BG };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(255))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .stroke(egui::Stroke::new(if active { 1.0 } else { 0.5 }, accent))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(label).small().color(accent));
        })
        .response
}

fn render_chip(ui: &mut egui::Ui, label: &str, accent: egui::Color32) {
    egui::Frame::new()
        .fill(colors::BG)
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
    fn test_settings_not_dirty_on_init() {
        let view = SettingsView::new();
        assert!(!view.dirty);
    }

    #[test]
    fn test_reset_category_marks_dirty() {
        let mut view = SettingsView::new();
        assert!(!view.dirty);
        view.reset_category(0);
        assert!(view.dirty);
    }

    #[test]
    fn test_default_for_setting_text() {
        let setting = SettingDef {
            key: "test_text",
            label: "Test",
            description: "A test",
            kind: SettingKind::Text {
                placeholder: "hello",
            },
        };
        assert_eq!(default_for_setting(&setting), "hello");
    }

    #[test]
    fn test_default_for_setting_toggle() {
        let setting = SettingDef {
            key: "test_toggle",
            label: "Test",
            description: "A test",
            kind: SettingKind::Toggle,
        };
        assert_eq!(default_for_setting(&setting), "false");
    }

    #[test]
    fn test_total_settings_count() {
        let total: usize = CATEGORIES.iter().map(|c| c.settings.len()).sum();
        // We defined 20 settings across 5 categories (4+4+4+3+4 = 19).
        assert!(
            (15..=25).contains(&total),
            "Expected ~20 settings, got {}",
            total
        );
    }
}
