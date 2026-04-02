//! Command palette — searchable command overlay (Ctrl+Shift+P).

use eframe::egui;

use crate::theme::colors;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    NewTab,
    CloseTab,
    CycleTabs,
    Refresh,
    ToggleSidebar,
    ToggleShortcuts,
    FocusMemory,
    FocusOverview,
    FocusAgents,
    FocusSettings,
    ToggleAgentPanel,
    // Kept for backwards compat with app.rs match arms.
    FocusContext,
    FocusArtifacts,
    FocusGuardrails,
}

impl Command {
    fn label(&self) -> &'static str {
        match self {
            Command::NewTab => "New Agent Tab",
            Command::CloseTab => "Close Current Tab",
            Command::CycleTabs => "Cycle Tabs",
            Command::Refresh => "Refresh Context",
            Command::ToggleSidebar => "Toggle Sidebar",
            Command::ToggleShortcuts => "Show Keyboard Shortcuts",
            Command::FocusMemory => "Go to Memory",
            Command::FocusOverview => "Go to Workbench",
            Command::FocusAgents => "Go to Terminals",
            Command::FocusSettings => "Go to Settings",
            Command::ToggleAgentPanel => "Toggle Supervisor Panel",
            Command::FocusContext | Command::FocusArtifacts | Command::FocusGuardrails => {
                "Go to Memory"
            }
        }
    }

    fn shortcut(&self) -> Option<&'static str> {
        match self {
            Command::NewTab => Some("Ctrl+T"),
            Command::CloseTab => Some("Ctrl+W"),
            Command::CycleTabs => Some("Ctrl+Tab"),
            Command::Refresh => Some("Ctrl+R"),
            Command::ToggleSidebar => Some("Ctrl+B"),
            Command::ToggleShortcuts => Some("Ctrl+?"),
            Command::FocusMemory => Some("Ctrl+3"),
            Command::FocusOverview => Some("Ctrl+1"),
            Command::FocusAgents => Some("Ctrl+2"),
            Command::FocusSettings => Some("Ctrl+4"),
            Command::ToggleAgentPanel => Some("Ctrl+E"),
            Command::FocusContext | Command::FocusArtifacts | Command::FocusGuardrails => None,
        }
    }
}

pub struct CommandPalette {
    open: bool,
    query: String,
    selected_index: usize,
    commands: Vec<Command>,
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandPalette {
    pub fn new() -> Self {
        Self {
            open: false,
            query: String::new(),
            selected_index: 0,
            commands: vec![
                Command::NewTab,
                Command::CloseTab,
                Command::CycleTabs,
                Command::Refresh,
                Command::ToggleSidebar,
                Command::ToggleShortcuts,
                Command::ToggleAgentPanel,
                Command::FocusOverview,
                Command::FocusAgents,
                Command::FocusMemory,
                Command::FocusSettings,
            ],
        }
    }

    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected_index = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected_index = 0;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn filtered_commands(&self) -> Vec<Command> {
        if self.query.is_empty() {
            return self.commands.clone();
        }
        let q = self.query.to_lowercase();
        self.commands
            .iter()
            .filter(|c| c.label().to_lowercase().contains(&q))
            .copied()
            .collect()
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<Command> {
        if !self.open {
            return None;
        }

        let filtered = self.filtered_commands();
        if filtered.is_empty() {
            // Show "no results" but still render
            self.selected_index = 0;
        } else if self.selected_index >= filtered.len() {
            self.selected_index = filtered.len() - 1;
        }

        let mut chosen: Option<Command> = None;
        let available_width = ctx.available_rect().width();
        let palette_width = 420.0_f32.min(available_width * 0.8);

        egui::Window::new("")
            .title_bar(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
            .fixed_size(egui::vec2(palette_width, 320.0))
            .movable(false)
            .resizable(false)
            .open(&mut true)
            .show(ctx, |ui| {
                ui.add_space(4.0);

                // Search input.
                let input_response = ui.add(
                    egui::TextEdit::singleline(&mut self.query).hint_text("Type a command..."),
                );
                input_response.request_focus();

                // Reset selection when query changes.
                if input_response.changed() {
                    self.selected_index = 0;
                }

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                // Scroll area for commands.
                let row_height = 32.0;
                let max_visible = 8;
                let visible_height =
                    (filtered.len().min(max_visible) as f32 * row_height).min(256.0);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(visible_height)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        for (i, &cmd) in filtered.iter().enumerate() {
                            let is_selected = i == self.selected_index;
                            let label = cmd.label();
                            let shortcut = cmd.shortcut().unwrap_or("");

                            let row_response = ui.allocate_rect(
                                egui::Rect::from_min_size(
                                    ui.cursor().min,
                                    egui::vec2(ui.available_width(), row_height),
                                ),
                                egui::Sense::click(),
                            );
                            let row_rect = row_response.rect;

                            if is_selected {
                                ui.painter().rect_filled(row_rect, 2.0, colors::HOVER);
                            }

                            if row_response.contains_pointer()
                                && ui.input(|i| i.pointer.any_click())
                            {
                                chosen = Some(cmd);
                                self.close();
                            }

                            // Label column.
                            let label_rect = egui::Rect::from_min_max(
                                row_rect.min,
                                egui::pos2(row_rect.center().x, row_rect.max.y),
                            );
                            ui.painter().text(
                                egui::pos2(label_rect.min.x + 8.0, row_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                label,
                                egui::FontId::proportional(14.0),
                                if is_selected {
                                    colors::ACCENT
                                } else {
                                    colors::TEXT
                                },
                            );

                            // Shortcut column.
                            if !shortcut.is_empty() {
                                ui.painter().text(
                                    egui::pos2(row_rect.max.x - 8.0, row_rect.center().y),
                                    egui::Align2::RIGHT_CENTER,
                                    shortcut,
                                    egui::FontId::monospace(12.0),
                                    colors::TEXT_DIM,
                                );
                            }
                        }
                    });

                ui.add_space(4.0);
                ui.separator();
                ui.label(
                    egui::RichText::new("\u{2191}\u{2193} navigate  \u{21B5} select  Esc close")
                        .small()
                        .color(colors::TEXT_FAINT),
                );
            });

        // Handle keyboard input for navigation.
        ctx.input(|input| {
            if input.key_pressed(egui::Key::Escape) {
                self.close();
            } else if input.key_pressed(egui::Key::ArrowDown) {
                self.selected_index =
                    (self.selected_index + 1).min(filtered.len().saturating_sub(1));
            } else if input.key_pressed(egui::Key::ArrowUp) {
                self.selected_index = self.selected_index.saturating_sub(1);
            } else if input.key_pressed(egui::Key::Enter) && !filtered.is_empty() {
                chosen = Some(filtered[self.selected_index]);
                self.close();
            }
        });

        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_palette_has_11_commands() {
        let palette = CommandPalette::new();
        assert_eq!(palette.commands.len(), 11);
    }

    #[test]
    fn test_all_commands_have_labels() {
        let palette = CommandPalette::new();
        for cmd in &palette.commands {
            assert!(!cmd.label().is_empty(), "{:?} has empty label", cmd);
        }
    }

    #[test]
    fn test_no_stale_view_commands_in_palette() {
        let palette = CommandPalette::new();
        for cmd in &palette.commands {
            assert_ne!(
                *cmd,
                Command::FocusContext,
                "FocusContext should not be in the palette"
            );
            assert_ne!(
                *cmd,
                Command::FocusArtifacts,
                "FocusArtifacts should not be in the palette"
            );
            assert_ne!(
                *cmd,
                Command::FocusGuardrails,
                "FocusGuardrails should not be in the palette"
            );
        }
    }

    #[test]
    fn test_focus_memory_shortcut_is_ctrl3() {
        assert_eq!(Command::FocusMemory.shortcut(), Some("Ctrl+3"));
    }

    #[test]
    fn test_focus_settings_shortcut_is_ctrl4() {
        assert_eq!(Command::FocusSettings.shortcut(), Some("Ctrl+4"));
    }

    #[test]
    fn test_open_close_toggle() {
        let mut palette = CommandPalette::new();
        assert!(!palette.is_open());
        palette.open();
        assert!(palette.is_open());
        palette.close();
        assert!(!palette.is_open());
    }
}
