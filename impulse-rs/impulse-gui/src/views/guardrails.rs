//! Guardrails view — displays active guardrail rules with action/target filtering.

use eframe::egui;

use super::{View, ViewId};
use crate::ipc::GuardRule;
use crate::state::{ConnectionStatus, SharedState};
use crate::theme::colors;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterAction {
    All,
    Block,
    Warn,
    Log,
}

pub struct GuardrailsView {
    filter_action: FilterAction,
    filter_text: String,
}

impl GuardrailsView {
    pub fn new() -> Self {
        Self {
            filter_action: FilterAction::All,
            filter_text: String::new(),
        }
    }
}

impl View for GuardrailsView {
    fn id(&self) -> ViewId {
        ViewId::Settings
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, _ctx: &egui::Context) {
        ui.heading(egui::RichText::new("Guardrails").color(colors::ACCENT));
        ui.separator();

        if state.connection == ConnectionStatus::Disconnected {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 3.0);
                ui.label(
                    egui::RichText::new("Connect to daemon to view guardrail rules.")
                        .color(colors::TEXT_DIM),
                );
            });
            return;
        }

        let rules = &state.guard_rules;

        if rules.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 3.0);
                ui.label(egui::RichText::new("No guardrail rules active.").color(colors::TEXT_DIM));
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Rules can be configured in .impulse/config.json")
                        .small()
                        .color(colors::TEXT_FAINT),
                );
            });
            return;
        }

        // --- Toolbar: action filter tabs + text filter ---
        ui.horizontal(|ui| {
            for (label, action) in [
                ("All", FilterAction::All),
                ("Block", FilterAction::Block),
                ("Warn", FilterAction::Warn),
                ("Log", FilterAction::Log),
            ] {
                let count = match action {
                    FilterAction::All => rules.len(),
                    FilterAction::Block => rules.iter().filter(|r| r.action == "block").count(),
                    FilterAction::Warn => rules.iter().filter(|r| r.action == "warn").count(),
                    FilterAction::Log => rules.iter().filter(|r| r.action == "log").count(),
                };
                let text = format!("{} ({})", label, count);
                let color = if self.filter_action == action {
                    colors::ACCENT
                } else {
                    colors::TEXT_MUTED
                };
                if ui
                    .selectable_label(
                        self.filter_action == action,
                        egui::RichText::new(text).color(color),
                    )
                    .clicked()
                {
                    self.filter_action = action;
                }
            }

            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.filter_text)
                    .hint_text("Filter rules...")
                    .desired_width(160.0),
            );
        });

        ui.separator();

        // --- Rule cards ---
        let filter_lower = self.filter_text.to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt("guardrails_list")
            .show(ui, |ui| {
                let mut shown = 0;
                for rule in rules {
                    if !matches_filter(rule, self.filter_action, &filter_lower) {
                        continue;
                    }
                    shown += 1;
                    ui.add_space(4.0);
                    render_rule_card(ui, rule);
                }
                if shown == 0 {
                    ui.add_space(32.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("No rules match the current filter.")
                                .color(colors::TEXT_DIM),
                        );
                    });
                }
            });
    }
}

fn matches_filter(rule: &GuardRule, action_filter: FilterAction, text_filter: &str) -> bool {
    let action_ok = match action_filter {
        FilterAction::All => true,
        FilterAction::Block => rule.action == "block",
        FilterAction::Warn => rule.action == "warn",
        FilterAction::Log => rule.action == "log",
    };
    if !action_ok {
        return false;
    }
    if text_filter.is_empty() {
        return true;
    }
    rule.id.to_lowercase().contains(text_filter)
        || rule.reason.to_lowercase().contains(text_filter)
        || rule.pattern.to_lowercase().contains(text_filter)
        || rule.target.to_lowercase().contains(text_filter)
}

fn action_color(action: &str) -> egui::Color32 {
    match action {
        "block" => colors::RED,
        "warn" => colors::YELLOW,
        "log" => colors::BLUE,
        _ => colors::TEXT_MUTED,
    }
}

fn render_rule_card(ui: &mut egui::Ui, rule: &GuardRule) {
    let border_color = action_color(&rule.action);
    egui::Frame::new()
        .fill(colors::SURFACE)
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .stroke(egui::Stroke::new(1.0, border_color.gamma_multiply(0.5)))
        .show(ui, |ui| {
            // Header: action badge + rule ID + target badge
            ui.horizontal(|ui| {
                // Action badge
                egui::Frame::new()
                    .fill(action_color(&rule.action).gamma_multiply(0.15))
                    .corner_radius(egui::CornerRadius::same(3))
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(rule.action.to_uppercase())
                                .small()
                                .strong()
                                .color(action_color(&rule.action)),
                        );
                    });

                // Rule ID
                ui.label(egui::RichText::new(&rule.id).strong().color(colors::TEXT));

                // Target badge
                egui::Frame::new()
                    .fill(colors::ACCENT.gamma_multiply(0.1))
                    .corner_radius(egui::CornerRadius::same(3))
                    .inner_margin(egui::Margin::symmetric(4, 1))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(&rule.target)
                                .small()
                                .color(colors::ACCENT),
                        );
                    });

                // Builtin indicator
                if rule.builtin {
                    ui.label(
                        egui::RichText::new("built-in")
                            .small()
                            .color(colors::TEXT_FAINT),
                    );
                }
            });

            // Reason
            if !rule.reason.is_empty() {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(&rule.reason).color(colors::TEXT_MUTED));
            }

            // Pattern (monospace)
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Pattern:")
                        .small()
                        .color(colors::TEXT_DIM),
                );
                ui.label(
                    egui::RichText::new(&rule.pattern)
                        .small()
                        .monospace()
                        .color(colors::TEXT),
                );
            });

            // Suggestion
            if let Some(ref suggestion) = rule.suggestion {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Suggestion:")
                            .small()
                            .color(colors::GREEN),
                    );
                    ui.label(
                        egui::RichText::new(suggestion)
                            .small()
                            .color(colors::TEXT_MUTED),
                    );
                });
            }
        });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rule(id: &str, action: &str, target: &str) -> GuardRule {
        GuardRule {
            id: id.to_string(),
            pattern: "test-pattern".to_string(),
            action: action.to_string(),
            target: target.to_string(),
            reason: "Test reason".to_string(),
            suggestion: None,
            enabled: true,
            builtin: true,
        }
    }

    #[test]
    fn filter_all_matches_everything() {
        let rule = sample_rule("r1", "block", "bash");
        assert!(matches_filter(&rule, FilterAction::All, ""));
    }

    #[test]
    fn filter_action_block_only() {
        let block = sample_rule("r1", "block", "bash");
        let warn = sample_rule("r2", "warn", "bash");
        assert!(matches_filter(&block, FilterAction::Block, ""));
        assert!(!matches_filter(&warn, FilterAction::Block, ""));
    }

    #[test]
    fn filter_action_warn_only() {
        let warn = sample_rule("r1", "warn", "bash");
        let log = sample_rule("r2", "log", "bash");
        assert!(matches_filter(&warn, FilterAction::Warn, ""));
        assert!(!matches_filter(&log, FilterAction::Warn, ""));
    }

    #[test]
    fn filter_action_log_only() {
        let log = sample_rule("r1", "log", "any");
        let block = sample_rule("r2", "block", "any");
        assert!(matches_filter(&log, FilterAction::Log, ""));
        assert!(!matches_filter(&block, FilterAction::Log, ""));
    }

    #[test]
    fn text_filter_matches_id() {
        let rule = sample_rule("no-force-push", "block", "bash");
        assert!(matches_filter(&rule, FilterAction::All, "force"));
        assert!(!matches_filter(&rule, FilterAction::All, "delete"));
    }

    #[test]
    fn text_filter_matches_reason() {
        let mut rule = sample_rule("r1", "block", "bash");
        rule.reason = "Dangerous recursive delete".to_string();
        assert!(matches_filter(&rule, FilterAction::All, "dangerous"));
        assert!(matches_filter(&rule, FilterAction::All, "recursive"));
    }

    #[test]
    fn text_filter_matches_pattern() {
        let mut rule = sample_rule("r1", "block", "bash");
        rule.pattern = "rm\\s+-rf".to_string();
        assert!(matches_filter(&rule, FilterAction::All, "rm"));
    }

    #[test]
    fn text_filter_matches_target() {
        let rule = sample_rule("r1", "block", "filewrite");
        assert!(matches_filter(&rule, FilterAction::All, "file"));
    }

    #[test]
    fn combined_action_and_text_filter() {
        let rule = sample_rule("no-rm-rf", "block", "bash");
        assert!(matches_filter(&rule, FilterAction::Block, "rm"));
        assert!(!matches_filter(&rule, FilterAction::Warn, "rm"));
    }

    #[test]
    fn action_color_mapping() {
        assert_eq!(action_color("block"), colors::RED);
        assert_eq!(action_color("warn"), colors::YELLOW);
        assert_eq!(action_color("log"), colors::BLUE);
        assert_eq!(action_color("unknown"), colors::TEXT_MUTED);
    }

    #[test]
    fn view_id_is_guardrails() {
        let view = GuardrailsView::new();
        assert_eq!(view.id(), ViewId::Guardrails);
    }

    #[test]
    fn default_filter_is_all() {
        let view = GuardrailsView::new();
        assert_eq!(view.filter_action, FilterAction::All);
        assert!(view.filter_text.is_empty());
    }
}
