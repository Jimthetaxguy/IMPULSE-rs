//! Core types for the context lifecycle manager.
//!
//! Defines agent kinds, context tiers, pane state tracking,
//! and extracted insights for cross-pane awareness.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Instant;

use super::intent::IntentCategory;

/// The kind of AI agent running in a terminal pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    OpenCode,
    /// Google Gemini CLI / Antigravity coding agent.
    Gemini,
    /// Cursor CLI / `cursor-agent` coding agent.
    Cursor,
    GenericShell,
}

impl AgentKind {
    /// Detect agent kind from the command name and optional pane name.
    ///
    /// Matching is substring-based and ordered most-specific first. New CLI/TUI
    /// coding agents are added by extending this chain plus the per-variant
    /// methods below (`startup_delay_ms`, `label`, `uses_xml_context`).
    pub fn detect(command: &str, name: &str) -> Self {
        let cmd_lower = command.to_lowercase();
        let name_lower = name.to_lowercase();
        let matches = |needle: &str| cmd_lower.contains(needle) || name_lower.contains(needle);

        if matches("claude") {
            Self::ClaudeCode
        } else if matches("codex") {
            Self::Codex
        } else if matches("opencode") {
            // Check before the bare "cursor"/"gemini" arms so the longer,
            // more specific name always wins regardless of ordering.
            Self::OpenCode
        } else if matches("gemini") || matches("antigravity") {
            Self::Gemini
        } else if matches("cursor") {
            Self::Cursor
        } else {
            Self::GenericShell
        }
    }

    /// How long to wait after spawn before injecting context (ms).
    /// Agents need time to initialize their REPL/UI before accepting input.
    pub fn startup_delay_ms(&self) -> u64 {
        match self {
            Self::ClaudeCode => 3000,
            Self::Codex | Self::OpenCode | Self::Gemini | Self::Cursor => 2000,
            Self::GenericShell => 500,
        }
    }

    /// Human-readable label for this agent kind.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Gemini => "gemini",
            Self::Cursor => "cursor",
            Self::GenericShell => "shell",
        }
    }

    /// Whether this agent kind uses XML-delimited context (Claude-native).
    pub fn uses_xml_context(&self) -> bool {
        matches!(self, Self::ClaudeCode)
    }
}

/// The level of context detail to inject, based on estimated window usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextTier {
    /// No injection needed (below threshold).
    None,
    /// Full context at spawn: tools + session + history + capabilities.
    Full,
    /// At 45%: tools + active files + key decisions.
    Essential,
    /// At 60%: tools + current task summary.
    Critical,
    /// At 80%: tool list + refresh command.
    Minimal,
    /// After compaction: identity + tools + session state.
    PostCompaction,
}

impl ContextTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Full => "full",
            Self::Essential => "essential",
            Self::Critical => "critical",
            Self::Minimal => "minimal",
            Self::PostCompaction => "post_compaction",
        }
    }
}

/// Type of insight extracted from agent output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsightType {
    FileModified,
    ErrorEncountered,
    DecisionMade,
    TaskCompleted,
    /// A tool invocation was observed (Phase 1A — structured parser).
    ToolInvocation,
    /// Diff output was detected (Phase 1A — structured parser).
    DiffDetected,
    /// A delegation marker was found in agent output (Phase 1B).
    DelegationDetected,
    /// An SSH/remote connection was detected (Phase 3A).
    RemoteConnection,
}

impl InsightType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FileModified => "file_modified",
            Self::ErrorEncountered => "error_encountered",
            Self::DecisionMade => "decision_made",
            Self::TaskCompleted => "task_completed",
            Self::ToolInvocation => "tool_invocation",
            Self::DiffDetected => "diff_detected",
            Self::DelegationDetected => "delegation_detected",
            Self::RemoteConnection => "remote_connection",
        }
    }
}

/// A structured insight extracted from agent PTY output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedInsight {
    pub pane_id: usize,
    pub agent_kind: AgentKind,
    pub timestamp: DateTime<Utc>,
    pub insight_type: InsightType,
    pub content: String,
    /// Classified intent for this insight, populated by `IntentCategory::from_keywords()`
    /// during extraction. Used by the coordinator for intent-based prioritization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<IntentCategory>,
}

/// Tracks the context lifecycle state for a single pane.
pub struct PaneContextState {
    pub pane_id: usize,
    pub agent_kind: AgentKind,
    pub initial_injection_done: bool,
    pub output_bytes_at_last_check: u64,
    /// Cumulative output-byte count at the most recent compaction. The monitor
    /// estimates context-window usage from bytes emitted *since* this baseline,
    /// so a compaction (which frees the agent's context) makes the estimate
    /// drop instead of climbing forever off the cumulative total.
    pub output_bytes_baseline: u64,
    pub estimated_tokens: usize,
    pub last_threshold: ContextTier,
    pub last_injection_at: Option<Instant>,
    pub compaction_count: u32,
    pub extracted_insights: Vec<ExtractedInsight>,
    /// Last time output was scanned for compaction patterns
    pub last_compaction_scan_at: Option<Instant>,
    /// Last time output was scanned for extraction patterns
    pub last_extraction_at: Option<Instant>,
}

/// Maximum insights to keep per pane (bounded buffer).
pub const MAX_INSIGHTS_PER_PANE: usize = 50;

/// Maximum cross-pane insights to include in a context message.
pub const MAX_CROSS_PANE_INSIGHTS: usize = 5;

/// Minimum seconds between injections per pane (debounce).
pub const INJECTION_DEBOUNCE_SECS: u64 = 60;

/// Compaction detection debounce in seconds.
pub const COMPACTION_DEBOUNCE_SECS: u64 = 60;

/// Extraction scan interval in seconds.
pub const EXTRACTION_INTERVAL_SECS: u64 = 30;

impl PaneContextState {
    pub fn new(pane_id: usize, agent_kind: AgentKind) -> Self {
        Self {
            pane_id,
            agent_kind,
            initial_injection_done: false,
            output_bytes_at_last_check: 0,
            output_bytes_baseline: 0,
            estimated_tokens: 0,
            last_threshold: ContextTier::None,
            last_injection_at: None,
            compaction_count: 0,
            extracted_insights: Vec::new(),
            last_compaction_scan_at: None,
            last_extraction_at: None,
        }
    }

    /// Add an insight, evicting the oldest if at capacity.
    pub fn add_insight(&mut self, insight: ExtractedInsight) {
        if self.extracted_insights.len() >= MAX_INSIGHTS_PER_PANE {
            self.extracted_insights.remove(0);
        }
        self.extracted_insights.push(insight);
    }

    /// Check if injection is allowed (debounce).
    pub fn can_inject(&self) -> bool {
        match self.last_injection_at {
            Some(at) => at.elapsed().as_secs() >= INJECTION_DEBOUNCE_SECS,
            None => true,
        }
    }

    /// Record that an injection was performed.
    pub fn mark_injected(&mut self) {
        self.last_injection_at = Some(Instant::now());
    }
}

/// Action returned by the monitor when a threshold is crossed.
#[derive(Debug, Clone)]
pub enum MonitorAction {
    /// Refresh context for a pane at the given tier.
    RefreshContext { pane_id: usize, tier: ContextTier },
    /// Compaction was detected — re-inject identity + tools.
    CompactionDetected { pane_id: usize },
}

/// A pending injection scheduled after pane spawn.
pub struct PendingInjection {
    pub pane_id: usize,
    pub pane_name: String,
    pub agent_kind: AgentKind,
    pub scheduled_at: Instant,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_kind_detect_claude() {
        assert_eq!(
            AgentKind::detect("claude", "claude-1"),
            AgentKind::ClaudeCode
        );
        assert_eq!(
            AgentKind::detect("/usr/bin/claude", "session"),
            AgentKind::ClaudeCode
        );
        assert_eq!(AgentKind::detect("CLAUDE", "test"), AgentKind::ClaudeCode);
    }

    #[test]
    fn test_agent_kind_detect_codex() {
        assert_eq!(AgentKind::detect("codex", "codex-1"), AgentKind::Codex);
        assert_eq!(AgentKind::detect("CODEX", "test"), AgentKind::Codex);
    }

    #[test]
    fn test_agent_kind_detect_opencode() {
        assert_eq!(AgentKind::detect("opencode", "oc-1"), AgentKind::OpenCode);
        assert_eq!(AgentKind::detect("OPENCODE", "test"), AgentKind::OpenCode);
    }

    #[test]
    fn test_agent_kind_detect_gemini() {
        assert_eq!(AgentKind::detect("gemini", "gemini-1"), AgentKind::Gemini);
        assert_eq!(AgentKind::detect("GEMINI", "test"), AgentKind::Gemini);
        // Antigravity is Google's agent surface — alias to Gemini.
        assert_eq!(AgentKind::detect("antigravity", "ag-1"), AgentKind::Gemini);
    }

    #[test]
    fn test_agent_kind_detect_cursor() {
        assert_eq!(AgentKind::detect("cursor", "cursor-1"), AgentKind::Cursor);
        assert_eq!(
            AgentKind::detect("cursor-agent", "session"),
            AgentKind::Cursor
        );
        assert_eq!(AgentKind::detect("CURSOR", "test"), AgentKind::Cursor);
    }

    #[test]
    fn test_agent_kind_detect_specificity() {
        // "opencode" must not be misclassified by a broader substring match.
        assert_eq!(
            AgentKind::detect("opencode", "opencode"),
            AgentKind::OpenCode
        );
    }

    #[test]
    fn test_agent_kind_detect_shell() {
        assert_eq!(
            AgentKind::detect("/bin/bash", "bash"),
            AgentKind::GenericShell
        );
        assert_eq!(AgentKind::detect("zsh", "shell"), AgentKind::GenericShell);
        assert_eq!(AgentKind::detect("fish", "term"), AgentKind::GenericShell);
    }

    #[test]
    fn test_agent_kind_startup_delay() {
        assert_eq!(AgentKind::ClaudeCode.startup_delay_ms(), 3000);
        assert_eq!(AgentKind::Codex.startup_delay_ms(), 2000);
        assert_eq!(AgentKind::OpenCode.startup_delay_ms(), 2000);
        assert_eq!(AgentKind::Gemini.startup_delay_ms(), 2000);
        assert_eq!(AgentKind::Cursor.startup_delay_ms(), 2000);
        assert_eq!(AgentKind::GenericShell.startup_delay_ms(), 500);
        // Labels round-trip to lowercase agent names.
        assert_eq!(AgentKind::Gemini.label(), "gemini");
        assert_eq!(AgentKind::Cursor.label(), "cursor");
    }

    #[test]
    fn test_context_tier_ordering() {
        assert!(ContextTier::None < ContextTier::Full);
        assert!(ContextTier::Full < ContextTier::Essential);
        assert!(ContextTier::Essential < ContextTier::Critical);
        assert!(ContextTier::Critical < ContextTier::Minimal);
        assert!(ContextTier::Minimal < ContextTier::PostCompaction);
    }

    #[test]
    fn test_pane_context_state_new() {
        let state = PaneContextState::new(1, AgentKind::ClaudeCode);
        assert_eq!(state.pane_id, 1);
        assert_eq!(state.agent_kind, AgentKind::ClaudeCode);
        assert!(!state.initial_injection_done);
        assert_eq!(state.estimated_tokens, 0);
        assert_eq!(state.last_threshold, ContextTier::None);
        assert!(state.extracted_insights.is_empty());
    }

    #[test]
    fn test_pane_context_insight_eviction() {
        let mut state = PaneContextState::new(1, AgentKind::ClaudeCode);
        for i in 0..(MAX_INSIGHTS_PER_PANE + 5) {
            state.add_insight(ExtractedInsight {
                pane_id: 1,
                agent_kind: AgentKind::ClaudeCode,
                timestamp: Utc::now(),
                insight_type: InsightType::FileModified,
                content: format!("file-{}.rs", i),
                intent: None,
            });
        }
        assert_eq!(state.extracted_insights.len(), MAX_INSIGHTS_PER_PANE);
        // Oldest entries should have been evicted
        assert!(state.extracted_insights[0].content.contains("file-5"));
    }

    #[test]
    fn test_can_inject_initially_true() {
        let state = PaneContextState::new(1, AgentKind::ClaudeCode);
        assert!(state.can_inject());
    }

    #[test]
    fn test_mark_injected_prevents_immediate_reinjection() {
        let mut state = PaneContextState::new(1, AgentKind::ClaudeCode);
        state.mark_injected();
        // Just marked — debounce should prevent injection
        assert!(!state.can_inject());
    }

    #[test]
    fn test_agent_kind_label() {
        assert_eq!(AgentKind::ClaudeCode.label(), "claude");
        assert_eq!(AgentKind::Codex.label(), "codex");
        assert_eq!(AgentKind::OpenCode.label(), "opencode");
        assert_eq!(AgentKind::GenericShell.label(), "shell");
    }

    #[test]
    fn test_agent_kind_uses_xml_context() {
        assert!(AgentKind::ClaudeCode.uses_xml_context());
        assert!(!AgentKind::OpenCode.uses_xml_context());
        assert!(!AgentKind::Codex.uses_xml_context());
        assert!(!AgentKind::GenericShell.uses_xml_context());
    }

    #[test]
    fn test_context_tier_as_str() {
        assert_eq!(ContextTier::None.as_str(), "none");
        assert_eq!(ContextTier::Full.as_str(), "full");
        assert_eq!(ContextTier::Essential.as_str(), "essential");
        assert_eq!(ContextTier::Critical.as_str(), "critical");
        assert_eq!(ContextTier::Minimal.as_str(), "minimal");
        assert_eq!(ContextTier::PostCompaction.as_str(), "post_compaction");
    }

    #[test]
    fn test_insight_type_as_str() {
        assert_eq!(InsightType::FileModified.as_str(), "file_modified");
        assert_eq!(InsightType::ErrorEncountered.as_str(), "error_encountered");
        assert_eq!(InsightType::DecisionMade.as_str(), "decision_made");
        assert_eq!(InsightType::TaskCompleted.as_str(), "task_completed");
    }

    #[test]
    fn test_detect_agent_from_pane_name() {
        // Detection should also work from pane name, not just command
        assert_eq!(
            AgentKind::detect("some-cmd", "my-claude-pane"),
            AgentKind::ClaudeCode
        );
        assert_eq!(
            AgentKind::detect("some-cmd", "codex-session"),
            AgentKind::Codex
        );
    }

    #[test]
    fn test_agent_kind_round_trip() {
        for kind in [
            AgentKind::ClaudeCode,
            AgentKind::Codex,
            AgentKind::OpenCode,
            AgentKind::GenericShell,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let recovered: AgentKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, recovered);
        }
    }

    #[test]
    fn test_context_tier_round_trip() {
        for tier in [
            ContextTier::None,
            ContextTier::Full,
            ContextTier::Essential,
            ContextTier::Critical,
            ContextTier::Minimal,
            ContextTier::PostCompaction,
        ] {
            let json = serde_json::to_string(&tier).unwrap();
            let recovered: ContextTier = serde_json::from_str(&json).unwrap();
            assert_eq!(tier, recovered);
        }
    }

    #[test]
    fn test_insight_type_round_trip() {
        for insight in [
            InsightType::FileModified,
            InsightType::ErrorEncountered,
            InsightType::DecisionMade,
            InsightType::TaskCompleted,
            InsightType::ToolInvocation,
            InsightType::DiffDetected,
            InsightType::DelegationDetected,
            InsightType::RemoteConnection,
        ] {
            let json = serde_json::to_string(&insight).unwrap();
            let recovered: InsightType = serde_json::from_str(&json).unwrap();
            assert_eq!(insight, recovered);
        }
    }

    #[test]
    fn test_extracted_insight_round_trip() {
        let insight = ExtractedInsight {
            pane_id: 3,
            agent_kind: AgentKind::Codex,
            timestamp: Utc::now(),
            insight_type: InsightType::DecisionMade,
            content: "Chose RwLock over Mutex".to_string(),
            intent: Some(IntentCategory::Implementing),
        };

        let json = serde_json::to_string(&insight).unwrap();
        let recovered: ExtractedInsight = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.pane_id, insight.pane_id);
        assert_eq!(recovered.agent_kind, insight.agent_kind);
        assert_eq!(recovered.timestamp, insight.timestamp);
        assert_eq!(recovered.insight_type, insight.insight_type);
        assert_eq!(recovered.content, insight.content);
        assert_eq!(recovered.intent, insight.intent);
    }
}
