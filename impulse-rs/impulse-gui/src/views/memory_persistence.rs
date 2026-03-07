//! Memory persistence — extracted file I/O for the memory loop.
//!
//! These free functions operate purely on filesystem paths and
//! `ExtractedInsight` structs, making the persist → search → merge
//! cycle testable without live PTY processes.

use std::collections::HashSet;
use std::path::Path;

use impulse_term::context::ExtractedInsight;

/// A search result from live insights.
pub struct LiveInsightResult {
    pub title: String,
    pub agent: String,
    pub timestamp: String,
}

/// Append insights to a JSONL file, creating parent directories if needed.
pub fn persist_insights_to_file(path: &Path, insights: &[ExtractedInsight]) {
    let mut content = std::fs::read_to_string(path).unwrap_or_default();
    for insight in insights {
        if let Ok(json) = serde_json::to_string(insight) {
            content.push_str(&json);
            content.push('\n');
        }
    }
    if let Err(err) = impulse_ops::atomic_write_path(path, content.as_bytes()) {
        log::warn!("Failed to persist insights to {:?}: {}", path, err);
    }
}

/// Load insights from a JSONL file filtered by pane ID.
pub fn load_live_insights_for_pane(path: &Path, pane_id: u64) -> Vec<ExtractedInsight> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<ExtractedInsight>(line).ok())
        .filter(|i| i.pane_id as u64 == pane_id)
        .collect()
}

/// Search live insights for a query (case-insensitive keyword match).
pub fn search_insights(path: &Path, query: &str) -> Vec<LiveInsightResult> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(insight) = serde_json::from_str::<ExtractedInsight>(line) else {
            continue;
        };
        if insight.content.to_lowercase().contains(&query_lower)
            || insight
                .insight_type
                .as_str()
                .to_lowercase()
                .contains(&query_lower)
        {
            results.push(LiveInsightResult {
                title: format!("[{}] {}", insight.insight_type.as_str(), insight.content),
                agent: insight.agent_kind.label().to_string(),
                timestamp: insight.timestamp.to_rfc3339(),
            });
        }
    }

    results
}

/// Merge a pane's insights into a HISTORY.jsonl entry.
///
/// Returns `Some(entry)` if insights were found and merged, `None` otherwise.
pub fn merge_pane_to_history(
    insights_path: &Path,
    history_path: &Path,
    pane_id: u64,
    agent_name: &str,
    label: &str,
) -> Option<serde_json::Value> {
    let pane_insights = load_live_insights_for_pane(insights_path, pane_id);
    if pane_insights.is_empty() {
        return None;
    }

    let files: Vec<String> = pane_insights
        .iter()
        .filter(|i| i.insight_type == impulse_term::context::InsightType::FileModified)
        .map(|i| i.content.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let summary = format!(
        "GUI session: {} ({} insights, {} files)",
        label,
        pane_insights.len(),
        files.len()
    );

    let entry = serde_json::json!({
        "session_id": format!("gui-pane-{}", pane_id),
        "session_name": label,
        "platform": agent_name,
        "started_at": pane_insights.first().map(|i| i.timestamp.to_rfc3339()).unwrap_or_default(),
        "ended_at": chrono::Utc::now().to_rfc3339(),
        "summary": summary,
        "files_touched": files,
        "tools_used": [],
        "insight_count": pane_insights.len(),
    });

    if let Ok(json) = serde_json::to_string(&entry) {
        let mut content = std::fs::read_to_string(history_path).unwrap_or_default();
        content.push_str(&json);
        content.push('\n');
        if let Err(err) = impulse_ops::atomic_write_path(history_path, content.as_bytes()) {
            log::warn!(
                "Failed to merge pane history into {:?}: {}",
                history_path,
                err
            );
        } else {
            log::info!(
                "Merged {} insights from pane {} to HISTORY",
                pane_insights.len(),
                pane_id
            );
        }
    }

    Some(entry)
}

/// Build a refresh context string for threshold injection.
///
/// Returns `None` for `ContextTier::None` and other non-injectable tiers.
pub fn build_refresh_context(
    tier: impulse_term::context::ContextTier,
    cross_pane_insights: &[String],
    genome_decisions: &[String],
    active_sessions: &[String],
    recent_history: &[String],
) -> Option<String> {
    use impulse_term::context::ContextTier;

    let tier_desc = match tier {
        ContextTier::Essential => "Context at ~50%. Prioritizing essential information.",
        ContextTier::Critical => "Context at ~70%. Only critical context follows.",
        ContextTier::Minimal => "Context at ~80%+. Minimal context — highest priority only.",
        _ => return None,
    };

    let mut refresh = format!("{}\n", tier_desc);
    if !cross_pane_insights.is_empty() {
        refresh.push_str("\nCross-pane activity:\n");
        for line in cross_pane_insights {
            refresh.push_str(line);
            refresh.push('\n');
        }
    }
    if !genome_decisions.is_empty() {
        refresh.push_str("\nRecent decisions:\n");
        for d in genome_decisions.iter().take(5) {
            refresh.push_str("  - ");
            refresh.push_str(d);
            refresh.push('\n');
        }
    }
    if !active_sessions.is_empty() {
        refresh.push_str("\nActive sessions:\n");
        for s in active_sessions.iter().take(5) {
            refresh.push_str("  - ");
            refresh.push_str(s);
            refresh.push('\n');
        }
    }
    if !recent_history.is_empty() {
        refresh.push_str("\nRecent session history:\n");
        for h in recent_history.iter().take(5) {
            refresh.push_str("  - ");
            refresh.push_str(h);
            refresh.push('\n');
        }
    }

    Some(refresh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use impulse_term::context::{AgentKind, InsightType};
    use tempfile::TempDir;

    /// Helper to create a test insight with specific fields.
    fn make_test_insight(
        pane_id: usize,
        insight_type: InsightType,
        content: &str,
    ) -> ExtractedInsight {
        ExtractedInsight {
            pane_id,
            agent_kind: AgentKind::ClaudeCode,
            timestamp: chrono::Utc::now(),
            insight_type,
            content: content.to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // Persistence tests (Step 6)
    // -----------------------------------------------------------------------

    #[test]
    fn test_persist_writes_valid_jsonl() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("LIVE_INSIGHTS.jsonl");
        let insights = vec![
            make_test_insight(0, InsightType::FileModified, "src/main.rs"),
            make_test_insight(0, InsightType::ErrorEncountered, "compile error in lib.rs"),
            make_test_insight(1, InsightType::DecisionMade, "switched to tokio"),
        ];

        persist_insights_to_file(&path, &insights);

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        for line in &lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(parsed.get("pane_id").is_some());
            assert!(parsed.get("content").is_some());
            assert!(parsed.get("insight_type").is_some());
        }
    }

    #[test]
    fn test_persist_appends_not_overwrites() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("LIVE_INSIGHTS.jsonl");

        let batch1 = vec![
            make_test_insight(0, InsightType::FileModified, "a.rs"),
            make_test_insight(0, InsightType::FileModified, "b.rs"),
        ];
        let batch2 = vec![
            make_test_insight(0, InsightType::FileModified, "c.rs"),
            make_test_insight(0, InsightType::FileModified, "d.rs"),
        ];

        persist_insights_to_file(&path, &batch1);
        persist_insights_to_file(&path, &batch2);

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.lines().count(), 4);
    }

    #[test]
    fn test_persist_creates_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let path = tmp
            .path()
            .join("nested")
            .join("deep")
            .join("LIVE_INSIGHTS.jsonl");
        let insights = vec![make_test_insight(0, InsightType::TaskCompleted, "done")];

        persist_insights_to_file(&path, &insights);

        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.lines().count(), 1);
    }

    #[test]
    fn test_load_filters_by_pane() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("LIVE_INSIGHTS.jsonl");
        let insights = vec![
            make_test_insight(0, InsightType::FileModified, "pane0.rs"),
            make_test_insight(1, InsightType::FileModified, "pane1.rs"),
            make_test_insight(2, InsightType::FileModified, "pane2.rs"),
            make_test_insight(1, InsightType::DecisionMade, "pane1 decision"),
        ];
        persist_insights_to_file(&path, &insights);

        let pane1 = load_live_insights_for_pane(&path, 1);
        assert_eq!(pane1.len(), 2);
        assert!(pane1.iter().all(|i| i.pane_id == 1));
    }

    #[test]
    fn test_load_missing_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nonexistent.jsonl");
        let result = load_live_insights_for_pane(&path, 0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_load_empty_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("LIVE_INSIGHTS.jsonl");
        std::fs::write(&path, "").unwrap();
        let result = load_live_insights_for_pane(&path, 0);
        assert!(result.is_empty());
    }

    // -----------------------------------------------------------------------
    // Search tests (Step 7)
    // -----------------------------------------------------------------------

    #[test]
    fn test_search_finds_by_content() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("LIVE_INSIGHTS.jsonl");
        let insights = vec![
            make_test_insight(0, InsightType::FileModified, "src/main.rs"),
            make_test_insight(0, InsightType::ErrorEncountered, "type error"),
            make_test_insight(0, InsightType::FileModified, "src/lib.rs"),
        ];
        persist_insights_to_file(&path, &insights);

        let results = search_insights(&path, "main.rs");
        assert_eq!(results.len(), 1);
        assert!(results[0].title.contains("main.rs"));
    }

    #[test]
    fn test_search_finds_by_type() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("LIVE_INSIGHTS.jsonl");
        let insights = vec![
            make_test_insight(0, InsightType::FileModified, "a.rs"),
            make_test_insight(0, InsightType::ErrorEncountered, "oops"),
            make_test_insight(0, InsightType::FileModified, "b.rs"),
        ];
        persist_insights_to_file(&path, &insights);

        let results = search_insights(&path, "FileModified");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("LIVE_INSIGHTS.jsonl");
        let insights = vec![make_test_insight(
            0,
            InsightType::DecisionMade,
            "Fixed BUG in auth",
        )];
        persist_insights_to_file(&path, &insights);

        let results = search_insights(&path, "fixed bug");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_no_matches() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("LIVE_INSIGHTS.jsonl");
        let insights = vec![make_test_insight(0, InsightType::FileModified, "a.rs")];
        persist_insights_to_file(&path, &insights);

        let results = search_insights(&path, "nonexistent");
        assert!(results.is_empty());
    }

    // -----------------------------------------------------------------------
    // Merge/history tests (Step 8)
    // -----------------------------------------------------------------------

    #[test]
    fn test_merge_correct_structure() {
        let tmp = TempDir::new().unwrap();
        let insights_path = tmp.path().join("LIVE_INSIGHTS.jsonl");
        let history_path = tmp.path().join("HISTORY.jsonl");
        let insights = vec![
            make_test_insight(0, InsightType::FileModified, "src/main.rs"),
            make_test_insight(0, InsightType::FileModified, "src/lib.rs"),
            make_test_insight(0, InsightType::ErrorEncountered, "compile error"),
            make_test_insight(0, InsightType::DecisionMade, "use async"),
            make_test_insight(0, InsightType::TaskCompleted, "feature done"),
        ];
        persist_insights_to_file(&insights_path, &insights);

        let entry = merge_pane_to_history(
            &insights_path,
            &history_path,
            0,
            "Claude Code",
            "test-session",
        )
        .expect("should produce an entry");

        assert_eq!(entry["session_id"], "gui-pane-0");
        assert_eq!(entry["platform"], "Claude Code");
        assert_eq!(entry["session_name"], "test-session");
        assert_eq!(entry["insight_count"], 5);

        let files = entry["files_touched"].as_array().unwrap();
        assert_eq!(files.len(), 2);

        let summary = entry["summary"].as_str().unwrap();
        assert!(summary.contains("5 insights"));
        assert!(summary.contains("2 files"));

        // Verify HISTORY.jsonl was written.
        let history = std::fs::read_to_string(&history_path).unwrap();
        assert_eq!(history.lines().count(), 1);
    }

    #[test]
    fn test_merge_deduplicates_files() {
        let tmp = TempDir::new().unwrap();
        let insights_path = tmp.path().join("LIVE_INSIGHTS.jsonl");
        let history_path = tmp.path().join("HISTORY.jsonl");
        let insights = vec![
            make_test_insight(0, InsightType::FileModified, "src/main.rs"),
            make_test_insight(0, InsightType::FileModified, "src/main.rs"),
            make_test_insight(0, InsightType::FileModified, "src/lib.rs"),
        ];
        persist_insights_to_file(&insights_path, &insights);

        let entry = merge_pane_to_history(
            &insights_path,
            &history_path,
            0,
            "Claude Code",
            "dedup-test",
        )
        .unwrap();

        let files = entry["files_touched"].as_array().unwrap();
        assert_eq!(files.len(), 2, "duplicate files should be deduplicated");
    }

    #[test]
    fn test_merge_empty_pane() {
        let tmp = TempDir::new().unwrap();
        let insights_path = tmp.path().join("LIVE_INSIGHTS.jsonl");
        let history_path = tmp.path().join("HISTORY.jsonl");
        // Only pane 1 insights — pane 0 is empty.
        let insights = vec![make_test_insight(1, InsightType::FileModified, "other.rs")];
        persist_insights_to_file(&insights_path, &insights);

        let result =
            merge_pane_to_history(&insights_path, &history_path, 0, "Claude Code", "empty");
        assert!(result.is_none(), "empty pane should return None");
        assert!(!history_path.exists(), "no HISTORY file should be written");
    }

    #[test]
    fn test_merge_appends_to_existing() {
        let tmp = TempDir::new().unwrap();
        let insights_path = tmp.path().join("LIVE_INSIGHTS.jsonl");
        let history_path = tmp.path().join("HISTORY.jsonl");

        // Pre-existing history entry.
        std::fs::write(&history_path, "{\"session_id\":\"old\"}\n").unwrap();

        let insights = vec![make_test_insight(0, InsightType::FileModified, "new.rs")];
        persist_insights_to_file(&insights_path, &insights);
        merge_pane_to_history(
            &insights_path,
            &history_path,
            0,
            "Claude Code",
            "new-session",
        );

        let history = std::fs::read_to_string(&history_path).unwrap();
        assert_eq!(history.lines().count(), 2, "should append, not overwrite");
    }

    // -----------------------------------------------------------------------
    // Full cycle + shutdown tests (Step 9)
    // -----------------------------------------------------------------------

    #[test]
    fn test_full_cycle_persist_search_merge() {
        let tmp = TempDir::new().unwrap();
        let insights_path = tmp.path().join("LIVE_INSIGHTS.jsonl");
        let history_path = tmp.path().join("HISTORY.jsonl");

        // Persist insights for 2 panes.
        let insights = vec![
            make_test_insight(0, InsightType::FileModified, "src/main.rs"),
            make_test_insight(0, InsightType::DecisionMade, "use serde"),
            make_test_insight(1, InsightType::FileModified, "src/lib.rs"),
            make_test_insight(1, InsightType::ErrorEncountered, "missing import"),
        ];
        persist_insights_to_file(&insights_path, &insights);

        // Search works across all panes.
        let search_results = search_insights(&insights_path, "src/");
        assert_eq!(search_results.len(), 2);

        // Load per pane works.
        let pane0 = load_live_insights_for_pane(&insights_path, 0);
        assert_eq!(pane0.len(), 2);
        let pane1 = load_live_insights_for_pane(&insights_path, 1);
        assert_eq!(pane1.len(), 2);

        // Merge each pane.
        merge_pane_to_history(&insights_path, &history_path, 0, "Claude Code", "pane-0");
        merge_pane_to_history(&insights_path, &history_path, 1, "OpenCode", "pane-1");

        let history = std::fs::read_to_string(&history_path).unwrap();
        assert_eq!(history.lines().count(), 2);
    }

    #[test]
    fn test_shutdown_merges_all_panes() {
        let tmp = TempDir::new().unwrap();
        let insights_path = tmp.path().join("LIVE_INSIGHTS.jsonl");
        let history_path = tmp.path().join("HISTORY.jsonl");

        // 3 panes with insights.
        let insights = vec![
            make_test_insight(0, InsightType::FileModified, "a.rs"),
            make_test_insight(1, InsightType::FileModified, "b.rs"),
            make_test_insight(2, InsightType::FileModified, "c.rs"),
        ];
        persist_insights_to_file(&insights_path, &insights);

        // Simulate shutdown: merge all panes.
        let pane_ids = vec![
            (0u64, "Claude Code", "pane-0"),
            (1, "OpenCode", "pane-1"),
            (2, "Shell", "pane-2"),
        ];
        for (id, agent, label) in &pane_ids {
            merge_pane_to_history(&insights_path, &history_path, *id, agent, label);
        }

        let history = std::fs::read_to_string(&history_path).unwrap();
        assert_eq!(history.lines().count(), 3);
    }

    // -----------------------------------------------------------------------
    // Threshold / refresh context tests (Step 9)
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_refresh_context_essential() {
        let cross = vec!["  - [pane-1] FileModified: lib.rs".to_string()];
        let decisions = vec!["Use tokio for async".to_string()];
        let result = build_refresh_context(
            impulse_term::context::ContextTier::Essential,
            &cross,
            &decisions,
            &[],
            &[],
        );
        let text = result.expect("Essential should produce context");
        assert!(text.contains("~50%"));
        assert!(text.contains("Cross-pane activity"));
        assert!(text.contains("lib.rs"));
        assert!(text.contains("Recent decisions"));
        assert!(text.contains("tokio"));
    }

    #[test]
    fn test_build_refresh_context_critical() {
        let result = build_refresh_context(
            impulse_term::context::ContextTier::Critical,
            &[],
            &[],
            &[],
            &[],
        );
        let text = result.expect("Critical should produce context");
        assert!(text.contains("~70%"));
    }

    #[test]
    fn test_build_refresh_context_minimal() {
        let result = build_refresh_context(
            impulse_term::context::ContextTier::Minimal,
            &[],
            &[],
            &[],
            &[],
        );
        let text = result.expect("Minimal should produce context");
        assert!(text.contains("80%+"));
    }

    #[test]
    fn test_build_refresh_context_none_returns_none() {
        let result =
            build_refresh_context(impulse_term::context::ContextTier::None, &[], &[], &[], &[]);
        assert!(result.is_none(), "Tier None should not produce context");
    }

    #[test]
    fn test_build_refresh_context_with_sessions() {
        let sessions = vec![
            "gui-claude-code-0: active (3 files)".to_string(),
            "gui-opencode-1: active (1 file)".to_string(),
        ];
        let result = build_refresh_context(
            impulse_term::context::ContextTier::Essential,
            &[],
            &[],
            &sessions,
            &[],
        );
        let text = result.expect("Should produce context");
        assert!(text.contains("Active sessions"));
        assert!(text.contains("gui-claude-code-0"));
        assert!(text.contains("gui-opencode-1"));
    }

    #[test]
    fn test_build_refresh_context_with_history() {
        let history = vec![
            "Previous session: 12 insights, 5 files".to_string(),
            "Earlier session: 3 insights, 1 file".to_string(),
        ];
        let result = build_refresh_context(
            impulse_term::context::ContextTier::Critical,
            &[],
            &[],
            &[],
            &history,
        );
        let text = result.expect("Should produce context");
        assert!(text.contains("Recent session history"));
        assert!(text.contains("12 insights"));
        assert!(text.contains("Earlier session"));
    }
}
