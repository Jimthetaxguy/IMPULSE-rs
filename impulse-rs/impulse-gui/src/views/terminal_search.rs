//! Terminal transcript search — Ctrl+F overlay that searches across all panes.
//!
//! The search overlay appears at the top of the terminals view when activated.
//! It searches each pane's `screen_text()` for the query string and shows
//! match counts per tab with navigation (F3 / Shift+F3).

use eframe::egui;

use crate::theme::colors;

// ---------------------------------------------------------------------------
// SearchResult
// ---------------------------------------------------------------------------

/// A match found in a terminal pane.
#[derive(Debug, Clone)]
pub struct SearchMatch {
    /// Tab ID where the match was found.
    pub tab_id: u64,
    /// Tab label (agent name).
    pub tab_label: String,
    /// The matched line text.
    #[allow(dead_code)]
    pub line: String,
    /// Line number within the screen text (0-based).
    pub line_number: usize,
}

// ---------------------------------------------------------------------------
// TerminalSearch
// ---------------------------------------------------------------------------

/// State for the terminal search overlay.
pub struct TerminalSearch {
    /// Whether the search overlay is visible.
    pub active: bool,
    /// Current search query.
    pub query: String,
    /// Whether search is case-sensitive.
    pub case_sensitive: bool,
    /// All matches from the last search.
    matches: Vec<SearchMatch>,
    /// Index of the currently highlighted match.
    current_match: usize,
    /// Whether the input field should receive focus.
    focus_requested: bool,
}

impl TerminalSearch {
    pub fn new() -> Self {
        Self {
            active: false,
            query: String::new(),
            case_sensitive: false,
            matches: Vec::new(),
            current_match: 0,
            focus_requested: false,
        }
    }

    /// Activate the search overlay and request focus.
    pub fn open(&mut self) {
        self.active = true;
        self.focus_requested = true;
    }

    /// Activate the search overlay with a pre-filled query.
    pub fn open_with_query(&mut self, query: String) {
        self.query = query;
        self.active = true;
        self.focus_requested = true;
    }

    /// Close the search overlay and clear results.
    pub fn close(&mut self) {
        self.active = false;
        self.query.clear();
        self.matches.clear();
        self.current_match = 0;
    }

    /// Run the search across a set of pane screen texts.
    ///
    /// `panes` is a list of `(tab_id, label, screen_text)`.
    pub fn search(&mut self, panes: &[(u64, &str, String)]) {
        self.matches.clear();
        self.current_match = 0;

        if self.query.is_empty() {
            return;
        }

        let query = if self.case_sensitive {
            self.query.clone()
        } else {
            self.query.to_lowercase()
        };

        for (tab_id, label, text) in panes {
            for (line_number, line) in text.lines().enumerate() {
                let haystack = if self.case_sensitive {
                    line.to_string()
                } else {
                    line.to_lowercase()
                };

                if haystack.contains(&query) {
                    self.matches.push(SearchMatch {
                        tab_id: *tab_id,
                        tab_label: label.to_string(),
                        line: line.to_string(),
                        line_number,
                    });
                }
            }
        }
    }

    /// Navigate to the next match.
    pub fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = (self.current_match + 1) % self.matches.len();
        }
    }

    /// Navigate to the previous match.
    pub fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            if self.current_match == 0 {
                self.current_match = self.matches.len() - 1;
            } else {
                self.current_match -= 1;
            }
        }
    }

    /// Get the currently highlighted match (if any).
    pub fn current(&self) -> Option<&SearchMatch> {
        self.matches.get(self.current_match)
    }

    /// Total match count.
    #[allow(dead_code)]
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// Match count for a specific tab.
    pub fn matches_in_tab(&self, tab_id: u64) -> usize {
        self.matches.iter().filter(|m| m.tab_id == tab_id).count()
    }

    /// Render the search overlay bar. Returns the tab_id to focus (if user navigated).
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<u64> {
        if !self.active {
            return None;
        }

        let mut focus_tab = None;

        egui::Frame::new()
            .fill(colors::SURFACE)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .stroke(egui::Stroke::new(0.5, colors::BORDER))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Search icon.
                    ui.label(egui::RichText::new("\u{1F50D}").color(colors::ACCENT));

                    // Search input.
                    let text_edit = egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Search terminals...")
                        .desired_width(200.0);
                    let response = ui.add(text_edit);

                    if self.focus_requested {
                        response.request_focus();
                        self.focus_requested = false;
                    }

                    // Enter triggers search (handled by caller via query change).
                    // Escape closes overlay.
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.close();
                        return;
                    }

                    // Case sensitivity toggle.
                    let case_label = if self.case_sensitive { "Aa" } else { "aa" };
                    let case_btn = ui.add(
                        egui::Button::new(egui::RichText::new(case_label).small().color(
                            if self.case_sensitive {
                                colors::ACCENT
                            } else {
                                colors::TEXT_DIM
                            },
                        ))
                        .fill(if self.case_sensitive {
                            colors::HOVER
                        } else {
                            egui::Color32::TRANSPARENT
                        }),
                    );
                    if case_btn.clicked() {
                        self.case_sensitive = !self.case_sensitive;
                    }

                    ui.separator();

                    // Match counter.
                    if self.matches.is_empty() && !self.query.is_empty() {
                        ui.label(
                            egui::RichText::new("No matches")
                                .small()
                                .color(colors::TEXT_DIM),
                        );
                    } else if !self.matches.is_empty() {
                        ui.label(
                            egui::RichText::new(format!(
                                "{}/{}",
                                self.current_match + 1,
                                self.matches.len()
                            ))
                            .small()
                            .color(colors::TEXT),
                        );

                        // Navigation buttons.
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("\u{25B2}").small(), // ▲
                            ))
                            .on_hover_text("Previous match (Shift+F3)")
                            .clicked()
                        {
                            self.prev_match();
                        }
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("\u{25BC}").small(), // ▼
                            ))
                            .on_hover_text("Next match (F3)")
                            .clicked()
                        {
                            self.next_match();
                        }

                        // Show which tab the current match is in.
                        if let Some(current) = self.current() {
                            focus_tab = Some(current.tab_id);
                            ui.separator();
                            ui.label(
                                egui::RichText::new(format!(
                                    "in [{}] line {}",
                                    current.tab_label,
                                    current.line_number + 1
                                ))
                                .small()
                                .color(colors::TEXT_MUTED),
                            );
                        }
                    }

                    // Close button (right-aligned).
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("\u{2715}").color(colors::TEXT_DIM),
                            ))
                            .on_hover_text("Close (Escape)")
                            .clicked()
                        {
                            self.close();
                        }
                    });
                });
            });

        focus_tab
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_panes() -> Vec<(u64, &'static str, String)> {
        vec![
            (
                0,
                "Claude Code",
                "Hello world\nerror: something failed\nDone.".to_string(),
            ),
            (
                1,
                "Shell",
                "$ cargo test\nrunning 5 tests\ntest result: ok".to_string(),
            ),
        ]
    }

    #[test]
    fn test_new_search_is_inactive() {
        let search = TerminalSearch::new();
        assert!(!search.active);
        assert!(search.query.is_empty());
        assert_eq!(search.match_count(), 0);
    }

    #[test]
    fn test_open_activates() {
        let mut search = TerminalSearch::new();
        search.open();
        assert!(search.active);
        assert!(search.focus_requested);
    }

    #[test]
    fn test_close_clears_state() {
        let mut search = TerminalSearch::new();
        search.open();
        search.query = "test".to_string();
        search.close();
        assert!(!search.active);
        assert!(search.query.is_empty());
        assert_eq!(search.match_count(), 0);
    }

    #[test]
    fn test_search_finds_matches() {
        let mut search = TerminalSearch::new();
        search.query = "error".to_string();
        search.search(&sample_panes());
        assert_eq!(search.match_count(), 1);
        assert_eq!(search.matches[0].tab_id, 0);
        assert_eq!(search.matches[0].line_number, 1);
    }

    #[test]
    fn test_search_case_insensitive_by_default() {
        let mut search = TerminalSearch::new();
        search.query = "ERROR".to_string();
        search.search(&sample_panes());
        assert_eq!(search.match_count(), 1);
    }

    #[test]
    fn test_search_case_sensitive() {
        let mut search = TerminalSearch::new();
        search.query = "ERROR".to_string();
        search.case_sensitive = true;
        search.search(&sample_panes());
        assert_eq!(search.match_count(), 0);
    }

    #[test]
    fn test_search_across_panes() {
        let mut search = TerminalSearch::new();
        search.query = "test".to_string();
        search.search(&sample_panes());
        // "cargo test" in shell line 0, "running 5 tests" in shell line 1, "test result" in shell line 2
        assert!(search.match_count() >= 2);
    }

    #[test]
    fn test_search_empty_query_no_matches() {
        let mut search = TerminalSearch::new();
        search.query.clear();
        search.search(&sample_panes());
        assert_eq!(search.match_count(), 0);
    }

    #[test]
    fn test_next_match_wraps() {
        let mut search = TerminalSearch::new();
        search.query = "test".to_string();
        search.search(&sample_panes());
        let total = search.match_count();
        assert!(total > 0);
        for _ in 0..total {
            search.next_match();
        }
        assert_eq!(search.current_match, 0); // Wrapped around.
    }

    #[test]
    fn test_prev_match_wraps() {
        let mut search = TerminalSearch::new();
        search.query = "test".to_string();
        search.search(&sample_panes());
        assert!(search.match_count() > 0);
        search.prev_match(); // From 0, wraps to last.
        assert_eq!(search.current_match, search.match_count() - 1);
    }

    #[test]
    fn test_matches_in_tab() {
        let mut search = TerminalSearch::new();
        search.query = "test".to_string();
        search.search(&sample_panes());
        assert_eq!(search.matches_in_tab(0), 0); // "test" not in Claude Code pane
        assert!(search.matches_in_tab(1) >= 2); // Multiple in Shell pane
    }

    #[test]
    fn test_current_returns_none_when_empty() {
        let search = TerminalSearch::new();
        assert!(search.current().is_none());
    }

    #[test]
    fn test_current_returns_match() {
        let mut search = TerminalSearch::new();
        search.query = "error".to_string();
        search.search(&sample_panes());
        assert!(search.current().is_some());
        assert_eq!(search.current().unwrap().tab_label, "Claude Code");
    }
}
