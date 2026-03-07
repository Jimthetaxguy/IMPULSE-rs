//! Panel actions — typed commands from the agent panel to other GUI components.
//!
//! The agent panel produces `PanelAction`s in response to slash commands.
//! `ImpulseApp` drains and dispatches them each frame, keeping the agent panel
//! decoupled from the terminals view.

/// How a supervisor proposal should be handled when the operator clicks a card action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalExecutionMode {
    Run,
    AllowThisSession,
    SaveDefault,
    Deny,
}

// ---------------------------------------------------------------------------
// PanelAction
// ---------------------------------------------------------------------------

/// An action the agent panel wants to perform on other GUI components.
#[derive(Debug, Clone)]
pub enum PanelAction {
    /// Inject context into a terminal pane via its ContextBridge.
    InjectTo { tab_id: u64, content: String },

    /// Send raw input to a terminal pane's PTY.
    SendTo { tab_id: u64, content: String },

    /// Switch the active terminal tab.
    FocusTab { tab_id: u64 },

    /// Trigger terminal search (opens search overlay when implemented).
    SearchTerm { query: String },

    /// Execute a structured supervisor proposal from the left chat panel.
    RunSupervisorProposal {
        proposal: Box<impulse_ops::SupervisorProposal>,
        mode: ProposalExecutionMode,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_action_debug() {
        let action = PanelAction::InjectTo {
            tab_id: 42,
            content: "test".to_string(),
        };
        let debug = format!("{:?}", action);
        assert!(debug.contains("InjectTo"));
        assert!(debug.contains("42"));
    }

    #[test]
    fn test_panel_action_clone() {
        let action = PanelAction::SendTo {
            tab_id: 1,
            content: "hello".to_string(),
        };
        let cloned = action.clone();
        assert!(matches!(cloned, PanelAction::SendTo { tab_id: 1, .. }));
    }
}
