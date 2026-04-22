//! Context message templates per agent kind and context tier.

use super::types::{AgentKind, ContextTier, ExtractedInsight};

/// Build a context init message for a freshly spawned agent pane.
pub fn build_init_message(
    agent_kind: AgentKind,
    session_id: Option<&str>,
    pane_name: &str,
    capabilities_summary: &str,
    cross_pane_insights: &[ExtractedInsight],
) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let session_line = session_id
        .map(|id| format!("Session: {} | ", &id[..id.len().min(8)]))
        .unwrap_or_default();
    let cross_pane = format_cross_pane_section(cross_pane_insights);

    if agent_kind.uses_xml_context() {
        format!(
            r#"<impulse-context type="init" version="1">
You are running inside Impulse v{version}, a terminal multiplexer for AI coding agents.
{session_line}Pane: {pane_name}

{capabilities_summary}
{cross_pane}
## Self-Refresh
Run `impulse-rs sync-context` to get fresh context at any time.
Impulse monitors your context window and will refresh automatically.
</impulse-context>"#
        )
    } else {
        format!(
            r#"# [Impulse Context]
# You are running inside Impulse v{version}, a terminal multiplexer for AI coding agents.
# {session_line}Pane: {pane_name}
#
{capabilities_summary}
{cross_pane}
# Run `impulse-rs sync-context` to get fresh context at any time."#
        )
    }
}

/// Build a refresh message for a threshold crossing or post-compaction event.
pub fn build_refresh_message(
    agent_kind: AgentKind,
    tier: ContextTier,
    pane_name: &str,
    capabilities_summary: &str,
    cross_pane_insights: &[ExtractedInsight],
) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let cross_pane = format_cross_pane_section(cross_pane_insights);

    let body = match tier {
        ContextTier::Essential => format!(
            "Context refresh (essential tier). Pane: {pane_name}\n\n\
             {capabilities_summary}\n\
             {cross_pane}"
        ),
        ContextTier::Critical => format!(
            "Context refresh (critical tier). Pane: {pane_name}\n\n\
             {capabilities_summary}\n\
             Run `impulse-rs sync-context` for full refresh."
        ),
        ContextTier::Minimal => "Context refresh (minimal). Run `impulse-rs sync-context` for full refresh.\n\
             Run `impulse-rs tooling-list` to see available tools.".to_string(),
        ContextTier::PostCompaction => format!(
            "Context restored after compaction. You are in Impulse v{version}, pane: {pane_name}\n\n\
             {capabilities_summary}\n\
             {cross_pane}\n\
             Run `impulse-rs sync-context` for full refresh."
        ),
        _ => format!("Impulse context refresh. Pane: {pane_name}"),
    };

    if agent_kind.uses_xml_context() {
        format!(
            "<impulse-context type=\"refresh\" tier=\"{}\" version=\"1\">\n{}\n</impulse-context>",
            tier.as_str(),
            body
        )
    } else {
        format!("# [Impulse Refresh: {}]\n{}", tier.as_str(), body)
    }
}

/// Format cross-pane insights for inclusion in context messages.
fn format_cross_pane_section(insights: &[ExtractedInsight]) -> String {
    if insights.is_empty() {
        return String::new();
    }

    let mut lines = vec!["## Cross-Pane Activity".to_string()];
    for insight in insights.iter().take(super::types::MAX_CROSS_PANE_INSIGHTS) {
        let age = Utc::now()
            .signed_duration_since(insight.timestamp)
            .num_minutes();
        let age_str = if age < 1 {
            "just now".to_string()
        } else {
            format!("{} min ago", age)
        };
        lines.push(format!(
            "- [{}] {}: {} ({})",
            insight.agent_kind.label(),
            insight.insight_type.as_str(),
            insight.content,
            age_str
        ));
    }
    lines.join("\n")
}

use chrono::Utc;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_lifecycle::types::AgentKind;

    #[test]
    fn test_init_message_claude_contains_xml() {
        let msg = build_init_message(
            AgentKind::ClaudeCode,
            Some("abc12345"),
            "claude-1",
            "## Tools\n- tool1",
            &[],
        );
        assert!(msg.contains("<impulse-context"));
        assert!(msg.contains("</impulse-context>"));
        assert!(msg.contains("claude-1"));
        assert!(msg.contains("abc12345"));
    }

    #[test]
    fn test_init_message_shell_uses_comments() {
        let msg = build_init_message(
            AgentKind::GenericShell,
            None,
            "bash",
            "## Tools\n- tool1",
            &[],
        );
        assert!(msg.contains("# [Impulse Context]"));
        assert!(!msg.contains("<impulse-context"));
    }

    #[test]
    fn test_init_message_opencode_uses_comments() {
        let msg = build_init_message(
            AgentKind::OpenCode,
            Some("sess-001"),
            "opencode-1",
            "## Tools",
            &[],
        );
        assert!(msg.contains("# [Impulse Context]"));
    }

    #[test]
    fn test_init_message_codex_uses_comments() {
        let msg = build_init_message(AgentKind::Codex, None, "codex-1", "## Tools", &[]);
        assert!(msg.contains("# [Impulse Context]"));
    }

    #[test]
    fn test_refresh_message_includes_tier() {
        let msg = build_refresh_message(
            AgentKind::ClaudeCode,
            ContextTier::Essential,
            "claude-1",
            "## Tools",
            &[],
        );
        assert!(msg.contains("essential"));
    }

    #[test]
    fn test_init_message_includes_capabilities() {
        let caps = "## Available Tools\n- session_query\n- memory_search";
        let msg = build_init_message(AgentKind::ClaudeCode, None, "claude-1", caps, &[]);
        assert!(msg.contains("session_query"));
        assert!(msg.contains("memory_search"));
    }

    #[test]
    fn test_refresh_message_minimal_tier() {
        let msg = build_refresh_message(
            AgentKind::ClaudeCode,
            ContextTier::Minimal,
            "claude-1",
            "## Tools",
            &[],
        );
        assert!(msg.contains("minimal"));
        assert!(msg.contains("sync-context"));
        assert!(msg.contains("tooling-list"));
    }

    #[test]
    fn test_refresh_message_post_compaction() {
        let msg = build_refresh_message(
            AgentKind::ClaudeCode,
            ContextTier::PostCompaction,
            "claude-1",
            "## Tools",
            &[],
        );
        assert!(msg.contains("post_compaction"));
        assert!(msg.contains("compaction"));
    }

    #[test]
    fn test_refresh_message_critical_tier() {
        let msg = build_refresh_message(
            AgentKind::ClaudeCode,
            ContextTier::Critical,
            "claude-1",
            "## Tools",
            &[],
        );
        assert!(msg.contains("critical"));
        assert!(msg.contains("sync-context"));
    }

    #[test]
    fn test_cross_pane_section_empty() {
        let section = format_cross_pane_section(&[]);
        assert!(section.is_empty());
    }

    #[test]
    fn test_cross_pane_section_with_insights() {
        use crate::context_lifecycle::types::{ExtractedInsight, InsightType};
        let insights = vec![ExtractedInsight {
            pane_id: 2,
            agent_kind: AgentKind::OpenCode,
            timestamp: chrono::Utc::now(),
            insight_type: InsightType::FileModified,
            content: "src/main.rs".to_string(),
            intent: None,
        }];
        let section = format_cross_pane_section(&insights);
        assert!(section.contains("Cross-Pane Activity"));
        assert!(section.contains("opencode"));
        assert!(section.contains("src/main.rs"));
    }

    #[test]
    fn test_refresh_message_non_xml_agent() {
        let msg = build_refresh_message(
            AgentKind::OpenCode,
            ContextTier::Essential,
            "opencode-1",
            "## Tools",
            &[],
        );
        assert!(msg.contains("# [Impulse Refresh: essential]"));
        assert!(!msg.contains("<impulse-context"));
    }
}
