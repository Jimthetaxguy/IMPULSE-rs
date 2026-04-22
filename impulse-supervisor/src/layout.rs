//! Layout primitives for the Dioxus supervisor shell.
//!
//! STATUS: SCAFFOLD. These types model the window's split pattern without
//! pulling Dioxus rsx! into the library surface.

use serde::{Deserialize, Serialize};

/// Top-level window layout: sidebar (supervisor) + main (worker grid).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum LayoutMode {
    /// Sidebar pinned, main area hosts 1-N workers in a responsive grid.
    SidebarWithGrid,
    /// Supervisor temporarily hidden (focus mode on a single worker).
    WorkerFocus,
    /// Supervisor full-width (reviewing cross-pane context).
    SupervisorFocus,
}

impl Default for LayoutMode {
    fn default() -> Self {
        Self::SidebarWithGrid
    }
}

/// How worker panes are arranged inside the main area.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkerGrid {
    /// Single pane full-size.
    Single,
    /// Vertical split (side-by-side).
    TwoColumn,
    /// 2x2 grid.
    Quad,
    /// Tabbed; only one worker visible.
    Tabbed,
}

impl WorkerGrid {
    pub fn max_panes(&self) -> usize {
        match self {
            Self::Single => 1,
            Self::TwoColumn => 2,
            Self::Quad => 4,
            Self::Tabbed => usize::MAX,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_layout_is_sidebar_with_grid() {
        assert_eq!(LayoutMode::default(), LayoutMode::SidebarWithGrid);
    }

    #[test]
    fn test_grid_capacity() {
        assert_eq!(WorkerGrid::Single.max_panes(), 1);
        assert_eq!(WorkerGrid::TwoColumn.max_panes(), 2);
        assert_eq!(WorkerGrid::Quad.max_panes(), 4);
        assert!(WorkerGrid::Tabbed.max_panes() > 1000);
    }

    #[test]
    fn test_layout_serde() {
        for mode in [
            LayoutMode::SidebarWithGrid,
            LayoutMode::WorkerFocus,
            LayoutMode::SupervisorFocus,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let recovered: LayoutMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, recovered);
        }
    }

    #[test]
    fn test_grid_serde() {
        for grid in [
            WorkerGrid::Single,
            WorkerGrid::TwoColumn,
            WorkerGrid::Quad,
            WorkerGrid::Tabbed,
        ] {
            let json = serde_json::to_string(&grid).unwrap();
            let recovered: WorkerGrid = serde_json::from_str(&json).unwrap();
            assert_eq!(grid, recovered);
        }
    }
}
