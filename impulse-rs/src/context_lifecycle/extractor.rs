//! Output extractor — parses agent PTY output for structured insights.
//!
//! Uses the structured parser (Phase 1A) for line classification, falling back
//! to heuristic matching for PlainText lines.

use chrono::Utc;
use std::time::Instant;

use super::intent::IntentCategory;
use super::parser::{self, LineClassification, ToolKind};
use super::types::{
    AgentKind, ExtractedInsight, InsightType, PaneContextState, EXTRACTION_INTERVAL_SECS,
};

/// Extracts structured insights from agent terminal output.
pub struct OutputExtractor;

impl OutputExtractor {
    /// Classify content text into an IntentCategory by splitting into words
    /// and running keyword-based classification.
    fn classify_content(content: &str) -> Option<IntentCategory> {
        let words: Vec<&str> = content.split_whitespace().collect();
        if words.is_empty() {
            return None;
        }
        let category = IntentCategory::from_keywords(&words);
        Some(category)
    }

    /// Extract insights from screen text for a given agent kind.
    /// Uses the structured parser for line classification, falling back
    /// to heuristic matching for unclassified (PlainText) lines.
    pub fn extract(agent_kind: AgentKind, pane_id: usize, text: &str) -> Vec<ExtractedInsight> {
        let mut insights = Vec::new();
        let parsed = parser::parse_output(text, agent_kind);

        // Emit insights from structured parser results
        for (tool_kind, target) in &parsed.tool_invocations {
            // Tool invocations that modify files also count as file modifications
            match tool_kind {
                ToolKind::Write | ToolKind::Edit => {
                    insights.push(ExtractedInsight {
                        pane_id,
                        agent_kind,
                        timestamp: Utc::now(),
                        insight_type: InsightType::FileModified,
                        content: target.clone(),
                        intent: Self::classify_content(target),
                    });
                    let tool_content = format!("{:?} → {}", tool_kind, target);
                    insights.push(ExtractedInsight {
                        pane_id,
                        agent_kind,
                        timestamp: Utc::now(),
                        insight_type: InsightType::ToolInvocation,
                        content: tool_content.clone(),
                        intent: Self::classify_content(&tool_content),
                    });
                }
                _ => {
                    let tool_content = format!("{:?} → {}", tool_kind, target);
                    insights.push(ExtractedInsight {
                        pane_id,
                        agent_kind,
                        timestamp: Utc::now(),
                        insight_type: InsightType::ToolInvocation,
                        content: tool_content.clone(),
                        intent: Self::classify_content(&tool_content),
                    });
                }
            }
        }

        // Diff summary as a single insight
        if parsed.diff_summary.files_changed > 0 {
            let diff_content = format!(
                "{} files, +{} -{}",
                parsed.diff_summary.files_changed,
                parsed.diff_summary.lines_added,
                parsed.diff_summary.lines_removed,
            );
            insights.push(ExtractedInsight {
                pane_id,
                agent_kind,
                timestamp: Utc::now(),
                insight_type: InsightType::DiffDetected,
                content: diff_content.clone(),
                intent: Self::classify_content(&diff_content),
            });
        }

        // Delegation detection
        if parsed.delegation_detected {
            let deleg_content = "delegation marker detected".to_string();
            insights.push(ExtractedInsight {
                pane_id,
                agent_kind,
                timestamp: Utc::now(),
                insight_type: InsightType::DelegationDetected,
                content: deleg_content.clone(),
                intent: Self::classify_content(&deleg_content),
            });
        }

        // Error lines from parser
        for (i, classification) in parsed.lines.iter().enumerate() {
            if *classification == LineClassification::ErrorLine {
                if let Some(line) = text.lines().nth(i) {
                    let error_content = truncate_insight(line.trim(), 120);
                    insights.push(ExtractedInsight {
                        pane_id,
                        agent_kind,
                        timestamp: Utc::now(),
                        insight_type: InsightType::ErrorEncountered,
                        content: error_content.clone(),
                        intent: Self::classify_content(&error_content),
                    });
                }
            }
        }

        // Fallback: run heuristic extraction on PlainText lines
        // for decisions and task completions (parser doesn't classify these)
        for (i, classification) in parsed.lines.iter().enumerate() {
            if *classification == LineClassification::PlainText {
                if let Some(line) = text.lines().nth(i) {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Some(decision) = Self::extract_decision(trimmed) {
                        let intent = Self::classify_content(&decision);
                        insights.push(ExtractedInsight {
                            pane_id,
                            agent_kind,
                            timestamp: Utc::now(),
                            insight_type: InsightType::DecisionMade,
                            content: decision,
                            intent,
                        });
                    }
                    if let Some(task) = Self::extract_task_completed(trimmed) {
                        let intent = Self::classify_content(&task);
                        insights.push(ExtractedInsight {
                            pane_id,
                            agent_kind,
                            timestamp: Utc::now(),
                            insight_type: InsightType::TaskCompleted,
                            content: task,
                            intent,
                        });
                    }

                    // Remote connection detection (Phase 3A)
                    if let Some(remote) = Self::extract_remote_connection(trimmed) {
                        let intent = Self::classify_content(&remote);
                        insights.push(ExtractedInsight {
                            pane_id,
                            agent_kind,
                            timestamp: Utc::now(),
                            insight_type: InsightType::RemoteConnection,
                            content: remote,
                            intent,
                        });
                    }
                }
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

    // Note: extract_file_modified and extract_error replaced by structured parser.
    // Kept as dead code reference for agent-specific patterns if needed.

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

    /// Detect SSH/remote connection patterns (Phase 3A).
    fn extract_remote_connection(line: &str) -> Option<String> {
        let trimmed = line.trim();
        // ssh user@host patterns
        if trimmed.starts_with("ssh ") && trimmed.contains('@') {
            return Some(truncate_insight(trimmed, 120));
        }
        // tmux session creation
        if trimmed.contains("tmux new-session") || trimmed.contains("tmux new -s") {
            return Some(truncate_insight(trimmed, 120));
        }
        None
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
        let insights =
            OutputExtractor::extract(AgentKind::GenericShell, 1, "hello world\nls -la\necho done");
        // GenericShell with plain commands should not extract file modifications
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

        // First extraction — parser produces FileModified + ToolInvocation per Write()
        let insights1 = OutputExtractor::check_pane(&mut state, text);
        assert_eq!(insights1.len(), 4); // 2 per Write() call

        // But pane state should dedup by (type, content)
        assert_eq!(state.extracted_insights.len(), 2); // FileModified + ToolInvocation (deduplicated)
    }

    #[test]
    fn test_extract_file_modified_codex() {
        let insights = OutputExtractor::extract(
            AgentKind::Codex,
            3,
            "wrote src/utils.rs\ncreated src/new_module.rs\nno file here",
        );
        let files: Vec<_> = insights
            .iter()
            .filter(|i| i.insight_type == InsightType::FileModified)
            .map(|i| i.content.as_str())
            .collect();
        assert_eq!(files, vec!["src/utils.rs", "src/new_module.rs"]);
    }

    #[test]
    fn test_truncate_insight_short() {
        assert_eq!(truncate_insight("short", 120), "short");
    }

    #[test]
    fn test_truncate_insight_exact() {
        let s = "x".repeat(120);
        assert_eq!(truncate_insight(&s, 120), s);
    }

    #[test]
    fn test_truncate_insight_long() {
        let s = "y".repeat(200);
        let result = truncate_insight(&s, 120);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 123); // 120 + "..."
    }

    #[test]
    fn test_extract_empty_input() {
        let insights = OutputExtractor::extract(AgentKind::ClaudeCode, 1, "");
        assert!(insights.is_empty());
    }

    #[test]
    fn test_extract_whitespace_only() {
        let insights = OutputExtractor::extract(AgentKind::ClaudeCode, 1, "   \n  \n   ");
        assert!(insights.is_empty());
    }

    // ─── Intent classification tests ────────────────────────────────────

    #[test]
    fn test_classify_content_testing() {
        use super::super::intent::IntentCategory;
        let result = OutputExtractor::classify_content("running test suite");
        assert_eq!(result, Some(IntentCategory::Testing));
    }

    #[test]
    fn test_classify_content_debugging() {
        use super::super::intent::IntentCategory;
        let result = OutputExtractor::classify_content("fix the error in auth");
        assert_eq!(result, Some(IntentCategory::Debugging));
    }

    #[test]
    fn test_classify_content_unknown() {
        use super::super::intent::IntentCategory;
        let result = OutputExtractor::classify_content("src/main.rs");
        // A path name alone has no intent keywords — should return Unknown
        assert_eq!(result, Some(IntentCategory::Unknown));
    }

    #[test]
    fn test_classify_content_empty() {
        let result = OutputExtractor::classify_content("");
        assert_eq!(result, None);
    }

    #[test]
    fn test_extracted_insights_have_intent() {
        // Error lines should get intent classified (error → Debugging)
        let insights =
            OutputExtractor::extract(AgentKind::ClaudeCode, 1, "error: cannot find value `x`\n");
        let errors: Vec<_> = insights
            .iter()
            .filter(|i| i.insight_type == InsightType::ErrorEncountered)
            .collect();
        assert!(!errors.is_empty());
        for error in &errors {
            assert!(
                error.intent.is_some(),
                "Error insight should have intent populated"
            );
        }
    }

    #[test]
    fn test_task_completed_intent_populated() {
        use super::super::intent::IntentCategory;
        let insights = OutputExtractor::extract(
            AgentKind::ClaudeCode,
            1,
            "All 47 tests passed\nbuild succeeded\n",
        );
        let tasks: Vec<_> = insights
            .iter()
            .filter(|i| i.insight_type == InsightType::TaskCompleted)
            .collect();
        assert!(!tasks.is_empty());
        for task in &tasks {
            assert!(
                task.intent.is_some(),
                "TaskCompleted insight should have intent populated"
            );
        }
        // "tests passed" should classify as Testing
        assert_eq!(tasks[0].intent, Some(IntentCategory::Testing));
        // "build succeeded" should classify as Deploying (contains "build")
        assert_eq!(tasks[1].intent, Some(IntentCategory::Deploying));
    }

    #[test]
    fn test_decision_intent_populated() {
        let insights = OutputExtractor::extract(
            AgentKind::ClaudeCode,
            1,
            "decision: use HashMap instead of BTreeMap\n",
        );
        let decisions: Vec<_> = insights
            .iter()
            .filter(|i| i.insight_type == InsightType::DecisionMade)
            .collect();
        assert!(!decisions.is_empty());
        for decision in &decisions {
            assert!(
                decision.intent.is_some(),
                "DecisionMade insight should have intent populated"
            );
        }
    }

    #[test]
    fn test_tool_invocation_intent_populated() {
        let insights =
            OutputExtractor::extract(AgentKind::ClaudeCode, 1, "Write(src/test_utils.rs)\n");
        let tools: Vec<_> = insights
            .iter()
            .filter(|i| i.insight_type == InsightType::ToolInvocation)
            .collect();
        assert!(!tools.is_empty());
        for tool in &tools {
            assert!(
                tool.intent.is_some(),
                "ToolInvocation insight should have intent populated"
            );
        }
    }
}
