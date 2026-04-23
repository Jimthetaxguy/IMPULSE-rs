//! Insight persistence methods for TerminalsView.
//!
//! Extracted from terminals.rs — persist, merge, search, and collect
//! insights from terminal panes.

use impulse_term_core::context::ExtractedInsight;

use super::terminals::TerminalsView;

impl TerminalsView {
    /// Append insights to LIVE_INSIGHTS.jsonl.
    pub(super) fn persist_insights(&self, insights: &[ExtractedInsight]) {
        let Some(path) = &self.live_insights_path else {
            return;
        };
        super::memory_persistence::persist_insights_to_file(path, insights);
    }

    /// Merge a closing tab's insights into HISTORY.jsonl.
    pub(super) fn merge_tab_insights_to_history(
        &self,
        pane_id: u64,
        agent_name: &str,
        label: String,
    ) {
        let Some(insights_path) = &self.live_insights_path else {
            return;
        };
        let history_path = insights_path
            .parent()
            .map(|p| p.join("HISTORY.jsonl"))
            .unwrap_or_default();
        if !history_path.as_os_str().is_empty() {
            super::memory_persistence::merge_pane_to_history(
                insights_path,
                &history_path,
                pane_id,
                agent_name,
                &label,
            );
        }
    }

    /// Load and search live insights for a query (keyword match).
    pub fn search_live_insights(
        &self,
        query: &str,
    ) -> Vec<super::memory_persistence::LiveInsightResult> {
        let Some(path) = &self.live_insights_path else {
            return Vec::new();
        };
        super::memory_persistence::search_insights(path, query)
    }

    /// Collect recent insights from all alive terminal panes.
    ///
    /// Returns formatted strings like `[Claude Code] Modified src/main.rs`
    /// suitable for injecting into the agent panel as cross-pane context.
    pub fn collected_insights(&mut self) -> Vec<String> {
        let mut insights = Vec::new();
        for tab in self.tabs.values_mut() {
            if tab.panel.is_alive() {
                let bridge = tab.panel.context_bridge();
                for insight in bridge.insights().iter().rev().take(5) {
                    insights.push(format!(
                        "[{}] {}: {}",
                        tab.label,
                        insight.insight_type.as_str(),
                        insight.content
                    ));
                }
            }
        }
        insights.dedup();
        insights
    }
}
