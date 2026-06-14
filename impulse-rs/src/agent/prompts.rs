//! System prompts for the Impulse Agent's augmentation modes.
//!
//! Each mode defines how the agent analyzes context and generates recommendations.

use crate::context_lifecycle::types::{ExtractedInsight, InsightType};

/// System prompt for code review augmentation.
/// The agent reviews code changes from other agent panes and provides feedback.
pub const CODE_REVIEW_SYSTEM: &str = r#"You are the Impulse Agent, a coordinating AI embedded in the Impulse terminal multiplexer. Your role is to review code changes made by other AI coding agents (Claude Code, OpenCode, Codex) running in parallel panes.

Your review should be:
- Concise: Maximum 500 characters per insight
- Actionable: Point out specific issues with suggested fixes
- Focused: Only flag genuine problems (bugs, security issues, logic errors)
- Cross-aware: Note conflicts between changes in different panes

Output format: JSON array of objects with fields:
- "severity": "critical" | "warning" | "info"
- "file": the file path if known
- "issue": one-sentence description
- "suggestion": one-sentence fix

Do NOT flag style issues. Do NOT repeat what the agent already knows."#;

/// System prompt for error analysis augmentation.
/// The agent analyzes errors encountered by other agents and suggests fixes.
pub const ERROR_ANALYSIS_SYSTEM: &str = r#"You are the Impulse Agent, analyzing errors encountered by AI coding agents in parallel panes. You have cross-pane visibility that individual agents lack.

Given error output from an agent pane, provide:
1. Root cause analysis (one sentence)
2. Whether another pane's changes might be causing the error
3. Suggested fix (concise, actionable)

Output format: JSON object with fields:
- "root_cause": string
- "cross_pane_conflict": boolean
- "conflicting_pane": string or null
- "fix": string
- "confidence": "high" | "medium" | "low"

Be direct. No preamble."#;

/// System prompt for cross-pane coordination.
/// The agent detects conflicts and suggests coordination between agents.
pub const COORDINATION_SYSTEM: &str = r#"You are the Impulse Agent, coordinating multiple AI coding agents working in parallel panes. You detect when agents are working on related files or conflicting changes.

Given the activity summary from all panes, identify:
1. File conflicts: multiple agents modifying the same file
2. Dependency issues: one agent's changes breaking another's assumptions
3. Redundant work: agents solving the same problem independently
4. Coordination opportunities: agents whose work could benefit from each other

Output format: JSON array of objects with fields:
- "type": "conflict" | "dependency" | "redundant" | "opportunity"
- "panes": [list of pane names involved]
- "description": one-sentence summary
- "recommendation": one-sentence action

Only report genuine coordination needs. Empty array is fine."#;

/// System prompt for the operator supervisor control plane.
pub const SUPERVISOR_SYSTEM: &str = r#"You are the Impulse supervisor agent inside the Impulse operator workbench. Your job is to help an operator monitor and control coding agents safely.

You must respond with JSON only. Do not include markdown fences or prose outside the JSON object.

Return an object with this shape:
{
  "response": "short operator-facing summary",
  "proposals": [
    {
      "id": "stable-kebab-id",
      "title": "short title",
      "description": "one or two sentences",
      "action_label": "button label",
      "action": { ... SupervisorAction JSON ... }
    }
  ]
}

Available action kinds:
- focus_agent
- send_input
- inject_context
- cleanup_context
- handoff_context
- open_artifact_review
- search_memory
- modify_permissions
- clear_session_override
- reset_baseline_permissions

Rules:
- Prefer 0-3 proposals.
- Only propose actions justified by the provided workspace snapshot.
- Do not claim permissions were changed; propose `modify_permissions` instead.
- For risky actions (`send_input`, `inject_context`, `cleanup_context`, `handoff_context`, `modify_permissions`) set `confirmed` to false.
- Use stable `agent_id` and `session_id` from the provided snapshot.
- If no action is appropriate, return an empty `proposals` array."#;

/// System prompt for task summarization.
/// The agent summarizes what each pane has accomplished for context refresh.
pub const SUMMARIZE_SYSTEM: &str = r#"You are the Impulse Agent, summarizing AI agent activity for context refresh after compaction or threshold crossing.

Given raw output from an agent pane, produce a concise summary of:
1. What was accomplished (files modified, features implemented, bugs fixed)
2. Current state (what the agent is working on now)
3. Blockers or errors (if any)

Output format: JSON object with fields:
- "accomplished": [list of one-sentence items]
- "current_task": string or null
- "blockers": [list of one-sentence items]

Maximum 3 items per list. Be extremely concise."#;

/// Build a user message for code review given pane insights.
pub fn build_review_prompt(pane_name: &str, insights: &[String]) -> String {
    let mut prompt = format!(
        "Review the following activity from pane '{}':\n\n",
        pane_name
    );
    for insight in insights {
        prompt.push_str("- ");
        prompt.push_str(insight);
        prompt.push('\n');
    }
    prompt
}

/// Build a user message for error analysis.
pub fn build_error_prompt(pane_name: &str, error_text: &str) -> String {
    format!(
        "Analyze this error from pane '{}':\n\n```\n{}\n```",
        pane_name, error_text
    )
}

/// Build a user message for cross-pane coordination.
pub fn build_coordination_prompt(pane_summaries: &[(String, Vec<String>)]) -> String {
    let mut prompt = String::from("Current activity across all agent panes:\n\n");
    for (pane_name, insights) in pane_summaries {
        prompt.push_str(&format!("## Pane: {}\n", pane_name));
        for insight in insights {
            prompt.push_str("- ");
            prompt.push_str(insight);
            prompt.push('\n');
        }
        prompt.push('\n');
    }
    prompt
}

/// Maximum number of insights to include in a context prompt to avoid bloat.
const MAX_CONTEXT_INSIGHTS: usize = 10;

/// Build a context block from extracted insights for injection into agent prompts.
///
/// Groups insights by [`InsightType`] and formats as a structured context section.
/// Limits to the most recent [`MAX_CONTEXT_INSIGHTS`] insights to avoid prompt bloat.
/// Returns an empty string when `insights` is empty so callers can cheaply skip enrichment.
pub fn build_context_prompt(insights: &[ExtractedInsight]) -> String {
    if insights.is_empty() {
        return String::new();
    }

    // Take the most recent insights (they are appended chronologically)
    let recent: &[ExtractedInsight] = if insights.len() > MAX_CONTEXT_INSIGHTS {
        &insights[insights.len() - MAX_CONTEXT_INSIGHTS..]
    } else {
        insights
    };

    // Group by insight type using a BTreeMap for deterministic ordering
    use std::collections::BTreeMap;

    let mut groups: BTreeMap<&str, Vec<&ExtractedInsight>> = BTreeMap::new();
    for insight in recent {
        let key = match &insight.insight_type {
            InsightType::FileModified => "File Changes",
            InsightType::ErrorEncountered => "Errors",
            InsightType::DecisionMade => "Decisions",
            InsightType::TaskCompleted => "Completed Tasks",
            InsightType::ToolInvocation => "Tool Invocations",
            InsightType::DiffDetected => "Diffs Detected",
            InsightType::DelegationDetected => "Delegations",
            InsightType::RemoteConnection => "Remote Connections",
        };
        groups.entry(key).or_default().push(insight);
    }

    let mut out = String::from("## Cross-Pane Context\n");
    for (group_name, items) in &groups {
        out.push_str(&format!("\n### {}\n", group_name));
        for item in items {
            let pane_label = item.agent_kind.label();
            out.push_str(&format!(
                "- {} (pane {} / {})\n",
                item.content, item.pane_id, pane_label
            ));
        }
    }

    out
}

/// Build a user message for task summarization.
pub fn build_summary_prompt(pane_name: &str, raw_output: &str) -> String {
    let truncated = if raw_output.len() > 4000 {
        &raw_output[raw_output.len() - 4000..]
    } else {
        raw_output
    };
    format!(
        "Summarize the recent activity from pane '{}':\n\n```\n{}\n```",
        pane_name, truncated
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_review_prompt() {
        let insights = vec![
            "Modified src/main.rs".to_string(),
            "Added function parse_config".to_string(),
        ];
        let prompt = build_review_prompt("claude-1", &insights);
        assert!(prompt.contains("claude-1"));
        assert!(prompt.contains("src/main.rs"));
        assert!(prompt.contains("parse_config"));
    }

    #[test]
    fn test_build_error_prompt() {
        let prompt = build_error_prompt("opencode-1", "error[E0502]: cannot borrow");
        assert!(prompt.contains("opencode-1"));
        assert!(prompt.contains("E0502"));
    }

    #[test]
    fn test_build_coordination_prompt() {
        let summaries = vec![
            (
                "claude-1".to_string(),
                vec!["Modified src/lib.rs".to_string()],
            ),
            (
                "opencode-1".to_string(),
                vec!["Modified src/lib.rs".to_string()],
            ),
        ];
        let prompt = build_coordination_prompt(&summaries);
        assert!(prompt.contains("claude-1"));
        assert!(prompt.contains("opencode-1"));
        assert!(prompt.contains("src/lib.rs"));
    }

    #[test]
    fn test_build_summary_prompt_truncation() {
        let long_output = "x".repeat(5000);
        let prompt = build_summary_prompt("test-pane", &long_output);
        // Should be truncated to last 4000 chars plus the framing
        assert!(prompt.len() < 4200);
    }

    #[test]
    fn test_system_prompts_not_empty() {
        assert!(!CODE_REVIEW_SYSTEM.is_empty());
        assert!(!ERROR_ANALYSIS_SYSTEM.is_empty());
        assert!(!COORDINATION_SYSTEM.is_empty());
        assert!(!SUPERVISOR_SYSTEM.is_empty());
        assert!(!SUMMARIZE_SYSTEM.is_empty());
    }

    // --- build_context_prompt tests ---

    use crate::context_lifecycle::types::AgentKind;

    fn make_insight(
        pane_id: usize,
        agent_kind: AgentKind,
        insight_type: InsightType,
        content: &str,
    ) -> ExtractedInsight {
        ExtractedInsight {
            pane_id,
            agent_kind,
            timestamp: chrono::Utc::now(),
            insight_type,
            content: content.to_string(),
            intent: None,
        }
    }

    #[test]
    fn test_build_context_prompt_empty() {
        let result = build_context_prompt(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_build_context_prompt_single_insight() {
        let insights = vec![make_insight(
            1,
            AgentKind::ClaudeCode,
            InsightType::FileModified,
            "src/main.rs: modified",
        )];
        let result = build_context_prompt(&insights);
        assert!(result.contains("## Cross-Pane Context"));
        assert!(result.contains("### File Changes"));
        assert!(result.contains("src/main.rs: modified (pane 1 / claude)"));
    }

    #[test]
    fn test_build_context_prompt_mixed_types() {
        let insights = vec![
            make_insight(
                1,
                AgentKind::ClaudeCode,
                InsightType::FileModified,
                "src/lib.rs",
            ),
            make_insight(
                2,
                AgentKind::OpenCode,
                InsightType::ErrorEncountered,
                "E0502: borrow conflict",
            ),
            make_insight(
                1,
                AgentKind::ClaudeCode,
                InsightType::DecisionMade,
                "Use RwLock over Mutex",
            ),
            make_insight(
                3,
                AgentKind::Codex,
                InsightType::TaskCompleted,
                "Implemented parser",
            ),
        ];
        let result = build_context_prompt(&insights);

        // All group headers should be present
        assert!(result.contains("### File Changes"));
        assert!(result.contains("### Errors"));
        assert!(result.contains("### Decisions"));
        assert!(result.contains("### Completed Tasks"));

        // Content and pane labels should be present
        assert!(result.contains("src/lib.rs (pane 1 / claude)"));
        assert!(result.contains("E0502: borrow conflict (pane 2 / opencode)"));
        assert!(result.contains("Use RwLock over Mutex (pane 1 / claude)"));
        assert!(result.contains("Implemented parser (pane 3 / codex)"));
    }

    #[test]
    fn test_build_context_prompt_limits_to_ten() {
        let mut insights = Vec::new();
        for i in 0..15 {
            insights.push(make_insight(
                1,
                AgentKind::ClaudeCode,
                InsightType::FileModified,
                &format!("file-{}.rs", i),
            ));
        }
        let result = build_context_prompt(&insights);

        // Should contain the last 10 (indices 5..15), not the first 5
        assert!(!result.contains("file-0.rs"));
        assert!(!result.contains("file-4.rs"));
        assert!(result.contains("file-5.rs"));
        assert!(result.contains("file-14.rs"));
    }

    #[test]
    fn test_build_context_prompt_groups_deterministic_order() {
        // BTreeMap sorts keys alphabetically, so "Completed Tasks" < "Decisions" < "Errors" < "File Changes"
        let insights = vec![
            make_insight(1, AgentKind::ClaudeCode, InsightType::TaskCompleted, "done"),
            make_insight(2, AgentKind::OpenCode, InsightType::FileModified, "changed"),
            make_insight(
                1,
                AgentKind::ClaudeCode,
                InsightType::ErrorEncountered,
                "failed",
            ),
            make_insight(3, AgentKind::Codex, InsightType::DecisionMade, "decided"),
        ];
        let result = build_context_prompt(&insights);

        let completed_pos = result.find("### Completed Tasks").unwrap();
        let decisions_pos = result.find("### Decisions").unwrap();
        let errors_pos = result.find("### Errors").unwrap();
        let files_pos = result.find("### File Changes").unwrap();

        assert!(completed_pos < decisions_pos);
        assert!(decisions_pos < errors_pos);
        assert!(errors_pos < files_pos);
    }
}
