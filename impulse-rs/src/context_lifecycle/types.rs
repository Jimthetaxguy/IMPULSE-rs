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
    GenericShell,
}

impl AgentKind {
    /// Detect agent kind from the command name and optional pane name.
    pub fn detect(command: &str, name: &str) -> Self {
        let cmd_lower = command.to_lowercase();
        let name_lower = name.to_lowercase();

        if cmd_lower.contains("claude") || name_lower.contains("claude") {
            Self::ClaudeCode
        } else if cmd_lower.contains("codex") || name_lower.contains("codex") {
            Self::Codex
        } else if cmd_lower.contains("opencode") || name_lower.contains("opencode") {
            Self::OpenCode
        } else {
            Self::GenericShell
        }
    }

    /// How long to wait after spawn before injecting context (ms).
    /// Agents need time to initialize their REPL/UI before accepting input.
    pub fn startup_delay_ms(&self) -> u64 {
        match self {
            Self::ClaudeCode => 3000,
            Self::Codex => 2000,
            Self::OpenCode => 2000,
            Self::GenericShell => 500,
        }
    }

    /// Human-readable label for this agent kind.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
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
}

impl InsightType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FileModified => "file_modified",
            Self::ErrorEncountered => "error_encountered",
            Self::DecisionMade => "decision_made",
            Self::TaskCompleted => "task_completed",
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
    /// Optional classified intent category for this insight.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<IntentCategory>,
}

/// Tracks the context lifecycle state for a single pane.
pub struct PaneContextState {
    pub pane_id: usize,
    pub agent_kind: AgentKind,
    pub initial_injection_done: bool,
    pub output_bytes_at_last_check: u64,
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
        assert_eq!(AgentKind::GenericShell.startup_delay_ms(), 500);
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
}
