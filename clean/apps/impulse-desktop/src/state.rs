//! Application state — the signals the Dioxus components read and write.

use impulse_contracts::WorkspaceSummary;
use std::sync::Arc;

/// Which view is currently shown in the shell.
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub enum ViewKind {
    /// Live PTY output.
    #[default]
    Terminal,
    /// Registered workspace roots.
    Workspaces,
    /// Active and historical sessions.
    Sessions,
    /// Orchestrator + MCP health.
    Health,
}

impl ViewKind {
    /// Human-readable label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Workspaces => "Workspaces",
            Self::Sessions => "Sessions",
            Self::Health => "Health",
        }
    }

    /// All views, in display order.
    #[must_use]
    pub fn all() -> &'static [ViewKind] {
        &[
            Self::Terminal,
            Self::Workspaces,
            Self::Sessions,
            Self::Health,
        ]
    }
}

/// Top-level application state held in a Dioxus context.
#[derive(Clone, Debug)]
pub struct AppState {
    /// The view currently shown.
    pub active_view: ViewKind,
    /// Cached list of registered workspaces.
    pub workspaces: Vec<WorkspaceSummary>,
    /// Last terminal chunk displayed.
    pub last_terminal_chunk: String,
    /// Whether the orchestrator is reachable.
    pub orchestrator_online: bool,
    /// Last health snapshot JSON.
    pub health_json: String,
}

impl AppState {
    /// Empty initial state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            active_view: ViewKind::default(),
            workspaces: Vec::new(),
            last_terminal_chunk: String::new(),
            orchestrator_online: false,
            health_json: r#"{"status":"unknown"}"#.to_owned(),
        }
    }

    /// Switch to a different view.
    pub fn switch_to(&mut self, view: ViewKind) {
        self.active_view = view;
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Holder that the desktop app can use to access the orchestrator when running
/// natively. In the web build this is `None` and the views show placeholders.
#[derive(Clone, Debug, Default)]
pub struct OrchestratorSlot {
    /// The orchestrator, if a native host is present.
    pub orchestrator: Option<Arc<impulse_runtime::Orchestrator>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_kinds_have_labels() {
        for v in ViewKind::all() {
            assert!(!v.label().is_empty());
        }
    }

    #[test]
    fn view_kind_default_is_terminal() {
        assert_eq!(ViewKind::default(), ViewKind::Terminal);
    }

    #[test]
    fn app_state_starts_empty() {
        let s = AppState::new();
        assert_eq!(s.active_view, ViewKind::Terminal);
        assert!(s.workspaces.is_empty());
        assert!(!s.orchestrator_online);
    }

    #[test]
    fn app_state_switches_view() {
        let mut s = AppState::new();
        s.switch_to(ViewKind::Sessions);
        assert_eq!(s.active_view, ViewKind::Sessions);
    }

    #[test]
    fn view_kind_all_contains_four() {
        assert_eq!(ViewKind::all().len(), 4);
    }
}
