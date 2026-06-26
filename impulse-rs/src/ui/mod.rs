//! TUI terminal interface for Impulse.
//!
//! Provides `TuiApp` — a ratatui-based terminal UI with panel layout,
//! keybindings, session overview, file tracking, and daemon status display.
//! Primary interactive surface for CLI users outside the desktop host.

use std::path::PathBuf;
use std::time::Duration;

use chrono::Local;
use crossterm::{
    event::KeyEventKind,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs},
    Frame, Terminal,
};

// Re-exported for UI submodules that reference it via `use super::*`
// (runner.rs, render_status.rs, render_tabs.rs).
pub(crate) use crate::agent::coordinator::RecommendationType;
use crate::context_lifecycle::detector::CompactionDetector;
use crate::context_lifecycle::extractor::OutputExtractor;
use crate::context_lifecycle::injector::ContextInjector;
use crate::context_lifecycle::monitor::ContextWindowMonitor;
use crate::context_lifecycle::{AgentKind, ContextTier, PaneContextState, PendingInjection};
use crate::state::SharedState;

pub mod pane_manager;
pub mod terminal_pane;
pub mod visualization;
pub use visualization::*;

pub mod agent_terminal;
pub mod lifecycle;
pub mod render_content;
pub mod render_dashboard;
pub mod render_menu;
pub mod render_status;
pub mod render_tabs;
pub mod runner;
pub mod types;

pub(crate) use agent_terminal::*;
pub(crate) use lifecycle::*;
pub(crate) use render_content::*;
pub(crate) use render_dashboard::*;
pub(crate) use render_menu::*;
pub(crate) use render_status::*;
pub(crate) use render_tabs::*;
pub use runner::*;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;
    // RecommendationType comes from the parent `super::*` re-export.
    use crate::agent::coordinator::{ConflictResolution, Recommendation, TrackedConflict};
    use tempfile::TempDir;

    fn create_test_state() -> TuiState {
        let temp_dir = TempDir::new().unwrap();
        let state =
            std::sync::Arc::new(crate::state::State::new(temp_dir.path().to_path_buf()).unwrap());
        TuiState::new(state)
    }

    #[test]
    fn test_conflict_state_initialization() {
        let state = create_test_state();
        assert!(!state.conflicts_panel_open);
        assert_eq!(state.selected_conflict_index, 0);
    }

    #[test]
    fn test_conflict_navigation_up() {
        let mut state = create_test_state();

        // Add mock conflict recommendations
        state.mier_recommendations.push(Recommendation {
            recommendation_type: RecommendationType::FileConflict,
            panes_involved: vec!["pane-1".to_string(), "pane-2".to_string()],
            description: "Multiple agents modifying: src/main.rs".to_string(),
            action: "Coordinate changes to avoid merge conflicts".to_string(),
            priority: 50,
        });
        state.mier_recommendations.push(Recommendation {
            recommendation_type: RecommendationType::FileConflict,
            panes_involved: vec!["pane-1".to_string(), "pane-3".to_string()],
            description: "Multiple agents modifying: src/lib.rs".to_string(),
            action: "Coordinate changes to avoid merge conflicts".to_string(),
            priority: 50,
        });

        // Open conflicts panel
        state.conflicts_panel_open = true;

        // Navigate up (should stay at 0)
        handle_navigation(&mut state, -1);
        assert_eq!(state.selected_conflict_index, 0);
    }

    #[test]
    fn test_conflict_navigation_down() {
        let mut state = create_test_state();

        // Add mock conflict recommendations
        state.mier_recommendations.push(Recommendation {
            recommendation_type: RecommendationType::FileConflict,
            panes_involved: vec!["pane-1".to_string(), "pane-2".to_string()],
            description: "Multiple agents modifying: src/main.rs".to_string(),
            action: "Coordinate changes to avoid merge conflicts".to_string(),
            priority: 50,
        });
        state.mier_recommendations.push(Recommendation {
            recommendation_type: RecommendationType::FileConflict,
            panes_involved: vec!["pane-1".to_string(), "pane-3".to_string()],
            description: "Multiple agents modifying: src/lib.rs".to_string(),
            action: "Coordinate changes to avoid merge conflicts".to_string(),
            priority: 50,
        });

        // Open conflicts panel
        state.conflicts_panel_open = true;

        // Navigate down
        handle_navigation(&mut state, 1);
        assert_eq!(state.selected_conflict_index, 1);

        // Navigate down again (should stay at max)
        handle_navigation(&mut state, 1);
        assert_eq!(state.selected_conflict_index, 1);
    }

    #[tokio::test]
    async fn test_conflict_resolution_updates_recommendation() {
        let mut state = create_test_state();

        // Add mock conflict
        state.mier_recommendations.push(Recommendation {
            recommendation_type: RecommendationType::FileConflict,
            panes_involved: vec!["pane-1".to_string(), "pane-2".to_string()],
            description: "Multiple agents modifying: src/main.rs".to_string(),
            action: "Coordinate changes to avoid merge conflicts".to_string(),
            priority: 50,
        });

        // Open conflicts panel and resolve
        state.conflicts_panel_open = true;
        handle_conflict_resolution(&mut state, ConflictResolution::Merge);

        // Check the recommendation was updated
        let rec = &state.mier_recommendations[0];
        assert!(rec.description.contains("RESOLVED"));
        assert!(rec.action.contains("Resolved via"));
    }

    #[test]
    fn test_conflict_resolve_nonexistent() {
        let mut state = create_test_state();

        // No conflicts - should show appropriate message (handled in function)
        state.conflicts_panel_open = true;
        handle_conflict_resolution(&mut state, ConflictResolution::AcceptMine);

        // No panic - just returns early
        assert!(state.conflicts_panel_open);
    }

    #[test]
    fn test_conflict_panel_toggle() {
        let mut state = create_test_state();

        // Initially closed
        assert!(!state.conflicts_panel_open);

        // Toggle open
        state.conflicts_panel_open = true;
        assert!(state.conflicts_panel_open);

        // Toggle closed
        state.conflicts_panel_open = false;
        assert!(!state.conflicts_panel_open);
    }

    #[test]
    fn test_tracked_conflict_is_resolved() {
        let conflict = TrackedConflict::new(
            "src/main.rs".to_string(),
            vec!["pane-1".to_string(), "pane-2".to_string()],
        );

        assert!(!conflict.is_resolved());

        let mut resolved = conflict;
        resolved.resolve(ConflictResolution::Merge);

        assert!(resolved.is_resolved());
        assert!(resolved.resolved_at.is_some());
        assert_eq!(resolved.resolution, Some(ConflictResolution::Merge));
    }

    #[test]
    fn test_conflict_resolution_as_str() {
        assert_eq!(ConflictResolution::Merge.as_str(), "merge");
        assert_eq!(ConflictResolution::AcceptTheirs.as_str(), "accept_theirs");
        assert_eq!(ConflictResolution::AcceptMine.as_str(), "accept_mine");
        assert_eq!(ConflictResolution::Rebase.as_str(), "rebase");
    }

    #[test]
    fn test_conflict_resolution_description() {
        assert_eq!(
            ConflictResolution::Merge.description(),
            "Manually merge changes from both agents"
        );
        assert_eq!(
            ConflictResolution::AcceptTheirs.description(),
            "Accept the other agent's changes"
        );
        assert_eq!(
            ConflictResolution::AcceptMine.description(),
            "Keep my changes"
        );
        assert_eq!(
            ConflictResolution::Rebase.description(),
            "Rebase on top of other agent's changes"
        );
    }
}
