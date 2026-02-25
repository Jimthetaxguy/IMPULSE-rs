//! Output extractor — parses agent PTY output for structured insights.

use chrono::Utc;
use std::time::Instant;

use super::types::{
    AgentKind, ExtractedInsight, InsightType, PaneContextState, EXTRACTION_INTERVAL_SECS,
};

/// Extracts structured insights from agent terminal output.
pub struct OutputExtractor;

impl OutputExtractor {
    /// Extract insights from screen text for a given agent kind.
    /// Returns new insights found in this scan.
    pub fn extract(agent_kind: AgentKind, pane_id: usize, text: &str) -> Vec<ExtractedInsight> {
        let mut insights = Vec::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // File modification patterns
            if let Some(path) = Self::extract_file_modified(agent_kind, trimmed) {
                insights.push(ExtractedInsight {
                    pane_id,
                    agent_kind,
                    timestamp: Utc::now(),
                    insight_type: InsightType::FileModified,
                    content: path,
                });
            }

            // Error patterns
            if let Some(err) = Self::extract_error(agent_kind, trimmed) {
                insights.push(ExtractedInsight {
                    pane_id,
                    agent_kind,
                    timestamp: Utc::now(),
                    insight_type: InsightType::ErrorEncountered,
                    content: err,
                });
            }

            // Decision patterns
            if let Some(decision) = Self::extract_decision(trimmed) {
                insights.push(ExtractedInsight {
                    pane_id,
                    agent_kind,
                    timestamp: Utc::now(),
                    insight_type: InsightType::DecisionMade,
                    content: decision,
                });
            }

            // Task completion patterns
            if let Some(task) = Self::extract_task_completed(trimmed) {
                insights.push(ExtractedInsight {
                    pane_id,
                    agent_kind,
                    timestamp: Utc::now(),
                    insight_type: InsightType::TaskCompleted,
                    content: task,
                });
            }
        }

        insights
    }

    /// Check and extract from a pane, respecting the extraction interval.
    pub fn check_pane(state: &mut PaneContextState, screen_text: &str) -> Vec<ExtractedInsight> {
        // Debounce: don't extract too frequently
        if let Some(last) = state.last_extraction_at {
            if last.elapsed().as_secs() < EXTRACTION_INTERVAL_SECS {
                return Vec::new();
            }
        }
        state.last_extraction_at = Some(Instant::now());

        let insights = Self::extract(state.agent_kind, state.pane_id, screen_text);

        // Add insights to pane state (deduplicating by content)
        for insight in &insights {
            let already_exists = state.extracted_insights.iter().any(|existing| {
                existing.content == insight.content && existing.insight_type == insight.insight_type
            });
            if !already_exists {
                state.add_insight(insight.clone());
            }
        }

        insights
    }

    fn extract_file_modified(agent_kind: AgentKind, line: &str) -> Option<String> {
        match agent_kind {
            AgentKind::ClaudeCode => {
                // Claude Code patterns: "Write(path)", "Edit(path)", "Created file: path"
                if let Some(rest) = line.strip_prefix("Write(") {
                    return rest.strip_suffix(')').map(|s| s.to_string());
                }
                if let Some(rest) = line.strip_prefix("Edit(") {
                    return rest.strip_suffix(')').map(|s| s.to_string());
                }
                if let Some(rest) = line.strip_prefix("Created file: ") {
                    return Some(rest.trim().to_string());
                }
                None
            }
            AgentKind::OpenCode => {
                // OpenCode patterns: "wrote path", "modified path", "created path"
                for prefix in &["wrote ", "modified ", "created "] {
                    let lower = line.to_lowercase();
                    if let Some(rest) = lower.strip_prefix(prefix) {
                        let path = rest.trim();
                        if !path.is_empty() && (path.contains('/') || path.contains('.')) {
                            return Some(path.to_string());
                        }
                    }
                }
                None
            }
            AgentKind::Codex => {
                // Codex uses similar patterns to OpenCode
                let lower = line.to_lowercase();
                for prefix in &["wrote ", "modified ", "created "] {
                    if let Some(rest) = lower.strip_prefix(prefix) {
                        let path = rest.trim();
                        if !path.is_empty() && (path.contains('/') || path.contains('.')) {
                            return Some(path.to_string());
                        }
                    }
                }
                None
            }
            AgentKind::GenericShell => None,
        }
    }

    fn extract_error(agent_kind: AgentKind, line: &str) -> Option<String> {
        let lower = line.to_lowercase();
        match agent_kind {
            AgentKind::ClaudeCode => {
                if lower.starts_with("error:")
                    || lower.contains("failed")
                    || lower.contains("panicked")
                {
                    Some(truncate_insight(line, 120))
                } else {
                    None
                }
            }
            AgentKind::OpenCode => {
                if lower.starts_with("error:") || lower.contains("fail") {
                    Some(truncate_insight(line, 120))
                } else {
                    None
                }
            }
            AgentKind::Codex => {
                if lower.starts_with("error:") || lower.contains("fail") {
                    Some(truncate_insight(line, 120))
                } else {
                    None
                }
            }
            AgentKind::GenericShell => {
                if lower.starts_with("error:") {
                    Some(truncate_insight(line, 120))
                } else {
                    None
                }
            }
        }
    }

    fn extract_decision(line: &str) -> Option<String> {
        let lower = line.to_lowercase();
        if lower.contains("decision:")
            || lower.contains("chose ")
            || lower.contains("using approach")
        {
            Some(truncate_insight(line, 120))
        } else {
            None
        }
    }

    fn extract_task_completed(line: &str) -> Option<String> {
        let lower = line.to_lowercase();
        if lower.contains("test passed")
            || lower.contains("tests passed")
            || lower.contains("build succeeded")
            || lower.contains("deployed")
        {
            Some(truncate_insight(line, 120))
        } else {
            None
        }
    }
}

/// Truncate a string to max_len, adding "..." if truncated.
/// Safe for multi-byte UTF-8.
fn truncate_insight(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_file_modified_claude() {
        let insights = OutputExtractor::extract(
            AgentKind::ClaudeCode,
            1,
            "Write(src/main.rs)\nEdit(src/lib.rs)\nCreated file: src/new.rs\nSome other line",
        );
        let files: Vec<_> = insights
            .iter()
            .filter(|i| i.insight_type == InsightType::FileModified)
            .map(|i| i.content.as_str())
            .collect();
        assert_eq!(files, vec!["src/main.rs", "src/lib.rs", "src/new.rs"]);
    }

    #[test]
    fn test_extract_file_modified_opencode() {
        let insights = OutputExtractor::extract(
            AgentKind::OpenCode,
            2,
            "wrote src/handler.rs\nmodified src/config.rs\nrandom line",
        );
        let files: Vec<_> = insights
            .iter()
            .filter(|i| i.insight_type == InsightType::FileModified)
            .map(|i| i.content.as_str())
            .collect();
        assert_eq!(files, vec!["src/handler.rs", "src/config.rs"]);
    }

    #[test]
    fn test_extract_errors() {
        let insights = OutputExtractor::extract(
            AgentKind::ClaudeCode,
            1,
            "error: cannot find value `x`\nAll good here\nTest failed at line 42",
        );
        let errors: Vec<_> = insights
            .iter()
            .filter(|i| i.insight_type == InsightType::ErrorEncountered)
            .collect();
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn test_extract_decisions() {
        let insights = OutputExtractor::extract(
            AgentKind::ClaudeCode,
            1,
            "decision: use HashMap instead of BTreeMap\nSome other line",
        );
        let decisions: Vec<_> = insights
            .iter()
            .filter(|i| i.insight_type == InsightType::DecisionMade)
            .collect();
        assert_eq!(decisions.len(), 1);
    }

    #[test]
    fn test_extract_task_completed() {
        let insights = OutputExtractor::extract(
            AgentKind::ClaudeCode,
            1,
            "All 47 tests passed\nbuild succeeded\n",
        );
        let tasks: Vec<_> = insights
            .iter()
            .filter(|i| i.insight_type == InsightType::TaskCompleted)
            .collect();
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn test_no_extraction_from_shell() {
        let insights = OutputExtractor::extract(
            AgentKind::GenericShell,
            1,
            "Write(src/main.rs)\nwrote src/lib.rs",
        );
        // GenericShell should not extract file modifications
        let files: Vec<_> = insights
            .iter()
            .filter(|i| i.insight_type == InsightType::FileModified)
            .collect();
        assert!(files.is_empty());
    }

    #[test]
    fn test_insight_dedup_in_pane_state() {
        let mut state = PaneContextState::new(1, AgentKind::ClaudeCode);
        let text = "Write(src/main.rs)\nWrite(src/main.rs)";

        // First extraction
        let insights1 = OutputExtractor::check_pane(&mut state, text);
        assert_eq!(insights1.len(), 2); // raw extraction returns both

        // But pane state should dedup
        assert_eq!(state.extracted_insights.len(), 1);
    }
}
