//! Context injector — builds and writes context blocks into PTY panes.

use super::templates;
use super::types::{AgentKind, ContextTier, ExtractedInsight};

/// Builds context messages for injection into agent panes.
pub struct ContextInjector;

impl ContextInjector {
    /// Build an init context message for a freshly spawned pane.
    pub fn build_init_message(
        agent_kind: AgentKind,
        session_id: Option<&str>,
        pane_name: &str,
        cross_pane_insights: &[ExtractedInsight],
    ) -> String {
        let capabilities = crate::agent_discovery::generate_capabilities_summary();
        templates::build_init_message(
            agent_kind,
            session_id,
            pane_name,
            &capabilities,
            cross_pane_insights,
        )
    }

    /// Build a refresh context message for a threshold crossing or compaction event.
    pub fn build_refresh_message(
        agent_kind: AgentKind,
        tier: ContextTier,
        pane_name: &str,
        cross_pane_insights: &[ExtractedInsight],
    ) -> String {
        let capabilities = crate::agent_discovery::generate_capabilities_summary();
        templates::build_refresh_message(
            agent_kind,
            tier,
            pane_name,
            &capabilities,
            cross_pane_insights,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_init_message_claude() {
        let msg = ContextInjector::build_init_message(
            AgentKind::ClaudeCode,
            Some("test-session-id"),
            "claude-1",
            &[],
        );
        assert!(msg.contains("<impulse-context"));
        assert!(msg.contains("Impulse Capabilities"));
    }

    #[test]
    fn test_build_init_message_shell() {
        let msg = ContextInjector::build_init_message(AgentKind::GenericShell, None, "bash", &[]);
        assert!(msg.contains("# [Impulse Context]"));
    }

    #[test]
    fn test_build_refresh_message() {
        let msg = ContextInjector::build_refresh_message(
            AgentKind::ClaudeCode,
            ContextTier::Essential,
            "claude-1",
            &[],
        );
        assert!(msg.contains("essential"));
        assert!(msg.contains("<impulse-context"));
    }
}
