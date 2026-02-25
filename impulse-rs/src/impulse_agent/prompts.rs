//! System prompts for the Impulse Agent's augmentation modes.
//!
//! Each mode defines how the agent analyzes context and generates recommendations.

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
        assert!(!SUMMARIZE_SYSTEM.is_empty());
    }
}
