//! Project selector dialog — pick a target directory before spawning a pane.

use std::path::PathBuf;

use eframe::egui;

use crate::theme::colors;

/// Project selector state — tracks whether dialog is open and what's selected.
pub struct ProjectSelector {
    open: bool,
    recent: Vec<PathBuf>,
    selected: Option<PathBuf>,
    /// Agent info passed through for spawn after selection.
    pending_agent: Option<String>,
}

#[allow(dead_code)]
impl ProjectSelector {
    pub fn new(recent_projects: Vec<PathBuf>) -> Self {
        Self {
            open: false,
            recent: recent_projects,
            selected: None,
            pending_agent: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self, agent_name: Option<String>) {
        self.open = true;
        self.selected = None;
        self.pending_agent = agent_name;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.selected = None;
        self.pending_agent = None;
    }

    pub fn select(&mut self, path: PathBuf) {
        self.selected = Some(path);
    }

    pub fn select_default(&mut self) {
        self.selected = Some(dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")));
    }

    pub fn selected_path(&self) -> Option<&PathBuf> {
        self.selected.as_ref()
    }

    pub fn pending_agent(&self) -> Option<&str> {
        self.pending_agent.as_deref()
    }

    pub fn recent_projects(&self) -> &[PathBuf] {
        &self.recent
    }

    pub fn update_recents(&mut self, recents: Vec<PathBuf>) {
        self.recent = recents;
    }

    /// Render the selector dialog. Returns `Some(selected_path)` when user confirms.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<PathBuf> {
        if !self.open {
            return None;
        }

        let mut result = None;

        egui::Window::new("Select Project Directory")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width(400.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("Choose a project folder for this terminal")
                        .color(colors::TEXT_MUTED),
                );
                ui.add_space(8.0);

                // Recent projects list.
                if !self.recent.is_empty() {
                    ui.label(
                        egui::RichText::new("Recent Projects")
                            .strong()
                            .color(colors::TEXT),
                    );
                    ui.add_space(4.0);

                    let mut clicked_path = None;
                    for path in &self.recent {
                        let display = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| path.display().to_string());

                        let is_selected = self.selected.as_ref() == Some(path);
                        let resp = ui.selectable_label(
                            is_selected,
                            egui::RichText::new(&display).color(colors::ACCENT),
                        );
                        if resp.clicked() {
                            clicked_path = Some(path.clone());
                        }
                        resp.on_hover_text(path.display().to_string());
                    }
                    if let Some(p) = clicked_path {
                        self.selected = Some(p);
                    }
                    ui.add_space(8.0);
                }

                // Browse and default buttons.
                ui.horizontal(|ui| {
                    if ui.button("Browse...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_directory(dirs::home_dir().unwrap_or_default())
                            .pick_folder()
                        {
                            self.selected = Some(path);
                        }
                    }
                    if ui.button("Use ~/").clicked() {
                        self.selected =
                            Some(dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")));
                    }
                });

                // Show selected path.
                if let Some(ref path) = self.selected {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!("Selected: {}", path.display()))
                            .color(colors::GREEN),
                    );
                }

                ui.add_space(12.0);

                // Confirm / Cancel buttons.
                ui.horizontal(|ui| {
                    let can_confirm = self.selected.is_some();
                    if ui
                        .add_enabled(can_confirm, egui::Button::new("Open Terminal"))
                        .clicked()
                    {
                        result = self.selected.clone();
                        self.open = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.close();
                    }
                });
            });

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selector_initial_state() {
        let selector = ProjectSelector::new(vec![]);
        assert!(!selector.is_open());
        assert!(selector.selected_path().is_none());
    }

    #[test]
    fn test_selector_with_recent_projects() {
        let recents = vec![PathBuf::from("/tmp/proj-a"), PathBuf::from("/tmp/proj-b")];
        let selector = ProjectSelector::new(recents);
        assert_eq!(selector.recent_projects().len(), 2);
    }

    #[test]
    fn test_selector_open_close() {
        let mut selector = ProjectSelector::new(vec![]);
        selector.open(None);
        assert!(selector.is_open());
        selector.close();
        assert!(!selector.is_open());
    }

    #[test]
    fn test_selector_select_recent() {
        let recents = vec![PathBuf::from("/tmp/proj-a")];
        let mut selector = ProjectSelector::new(recents);
        selector.open(None);
        selector.select(PathBuf::from("/tmp/proj-a"));
        assert_eq!(
            selector.selected_path(),
            Some(&PathBuf::from("/tmp/proj-a"))
        );
    }

    #[test]
    fn test_selector_default_to_home() {
        let mut selector = ProjectSelector::new(vec![]);
        selector.open(None);
        selector.select_default();
        assert!(selector.selected_path().is_some());
    }
}
