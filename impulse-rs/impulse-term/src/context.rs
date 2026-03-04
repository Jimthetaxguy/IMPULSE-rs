//! Context bridge — wraps context_lifecycle patterns for the terminal widget.
//!
//! Ports extraction, monitoring, compaction detection, and injection logic
//! from `impulse-rs/src/context_lifecycle/` into a self-contained module that
//! avoids depending on the full impulse-rs crate.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::backend::TerminalBackend;

// ---------------------------------------------------------------------------
// Constants (ported from context_lifecycle/types.rs)
// ---------------------------------------------------------------------------

/// Maximum insights to keep per bridge (bounded buffer).
const MAX_INSIGHTS: usize = 50;

/// Minimum seconds between extractions.
const EXTRACTION_INTERVAL_SECS: u64 = 30;

/// Minimum seconds between injections.
const INJECTION_DEBOUNCE_SECS: u64 = 60;

/// Minimum seconds between compaction scans.
const COMPACTION_DEBOUNCE_SECS: u64 = 60;

/// Token estimation: visible text is ~60% of total context.
/// The remaining ~40% is system prompt, tool call JSON, and user turns.
/// Applied to ANSI-stripped visible chars, not raw PTY bytes.
const VISIBLE_TO_CONTEXT_MULTIPLIER: f64 = 1.6;

/// Characters per token.
const CHARS_PER_TOKEN: f64 = 4.0;

/// Default context window size in tokens.
const DEFAULT_WINDOW_TOKENS: usize = 200_000;

// ---------------------------------------------------------------------------
// Types (ported from context_lifecycle/types.rs)
// ---------------------------------------------------------------------------

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
    /// Detect agent kind from the command name.
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

    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
            Self::GenericShell => "Shell",
        }
    }

    /// Whether this agent uses XML-delimited context.
    pub fn uses_xml_context(&self) -> bool {
        matches!(self, Self::ClaudeCode)
    }
}

/// Context window usage tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextTier {
    None,
    Full,
    Essential,
    Critical,
    Minimal,
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
            Self::FileModified => "FileModified",
            Self::ErrorEncountered => "ErrorEncountered",
            Self::DecisionMade => "DecisionMade",
            Self::TaskCompleted => "TaskCompleted",
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
}

/// Context health summary for display.
#[derive(Debug, Clone)]
pub struct ContextHealth {
    pub tier: ContextTier,
    pub estimated_tokens: usize,
    pub window_tokens: usize,
    pub usage_fraction: f32,
    pub compaction_count: u32,
    pub injection_count: u32,
}

// ---------------------------------------------------------------------------
// Compaction patterns (ported from context_lifecycle/detector.rs)
// ---------------------------------------------------------------------------

const COMPACTION_PATTERNS: &[&str] = &[
    "compressing prior messages",
    "auto-compact",
    "context compressed",
    "compacted conversation",
    "summarizing conversation",
    "conversation is getting long",
    "context window is getting full",
];

// ---------------------------------------------------------------------------
// ContextBridge
// ---------------------------------------------------------------------------

/// Bridges the terminal backend with context lifecycle operations.
///
/// Call `extract_tick()` periodically (every ~3 seconds) to scan for new
/// insights, check compaction, and update token estimates. Call `inject_context()`
/// to push context into the agent's terminal.
pub struct ContextBridge {
    pane_id: usize,
    agent_kind: AgentKind,
    backend: Arc<TerminalBackend>,

    // Token estimation.
    estimated_tokens: usize,
    window_tokens: usize,
    current_tier: ContextTier,
    last_output_bytes: u64,

    // Counters.
    compaction_count: u32,
    injection_count: u32,

    // Insights.
    insights: Vec<ExtractedInsight>,

    // Timing.
    last_extraction_at: Option<Instant>,
    last_injection_at: Option<Instant>,
    last_compaction_scan_at: Option<Instant>,

    // Diff-based extraction.
    previous_screen_text: String,

    // Usage history for sparkline visualization (last 100 samples at 3s intervals = ~5 minutes).
    usage_history: VecDeque<(Instant, f32)>,
}

impl ContextBridge {
    /// Create a new context bridge for a terminal backend.
    pub fn new(pane_id: usize, agent_kind: AgentKind, backend: Arc<TerminalBackend>) -> Self {
        Self {
            pane_id,
            agent_kind,
            backend,
            estimated_tokens: 0,
            window_tokens: DEFAULT_WINDOW_TOKENS,
            current_tier: ContextTier::None,
            last_output_bytes: 0,
            compaction_count: 0,
            injection_count: 0,
            insights: Vec::new(),
            last_extraction_at: None,
            last_injection_at: None,
            last_compaction_scan_at: None,
            previous_screen_text: String::new(),
            usage_history: VecDeque::with_capacity(100),
        }
    }

    /// Get the current context health.
    pub fn health(&self) -> ContextHealth {
        ContextHealth {
            tier: self.current_tier,
            estimated_tokens: self.estimated_tokens,
            window_tokens: self.window_tokens,
            usage_fraction: if self.window_tokens > 0 {
                self.estimated_tokens as f32 / self.window_tokens as f32
            } else {
                0.0
            },
            compaction_count: self.compaction_count,
            injection_count: self.injection_count,
        }
    }

    /// Usage history as (fraction) values for sparkline visualization.
    /// Returns up to 100 recent samples (oldest first).
    pub fn usage_history(&self) -> &VecDeque<(Instant, f32)> {
        &self.usage_history
    }

    /// Run one extraction tick. Call every ~3 seconds.
    ///
    /// 1. Updates token estimate from output bytes.
    /// 2. Scans for compaction events (debounced).
    /// 3. Extracts insights from new screen content (debounced).
    ///
    /// Returns newly extracted insights.
    pub fn extract_tick(&mut self) -> Vec<ExtractedInsight> {
        let current_bytes = self.backend.output_bytes();

        // Update token estimate using visible chars (ANSI-stripped).
        if current_bytes != self.last_output_bytes {
            self.last_output_bytes = current_bytes;
            let visible_chars = self.backend.visible_char_count();
            self.estimated_tokens = estimate_tokens(visible_chars);
            self.current_tier = usage_tier(self.estimated_tokens, self.window_tokens);
        }

        let now = Instant::now();

        // Record usage history for sparkline (bounded to 100 samples).
        let fraction = if self.window_tokens > 0 {
            self.estimated_tokens as f32 / self.window_tokens as f32
        } else {
            0.0
        };
        if self.usage_history.len() >= 100 {
            self.usage_history.pop_front();
        }
        self.usage_history.push_back((now, fraction));
        let screen_text = self.backend.screen_text();

        // Compaction detection (debounced).
        let should_scan_compaction = self
            .last_compaction_scan_at
            .is_none_or(|t| now.duration_since(t).as_secs() >= COMPACTION_DEBOUNCE_SECS);

        if should_scan_compaction {
            self.last_compaction_scan_at = Some(now);
            if scan_compaction(&screen_text) {
                self.compaction_count += 1;
                self.estimated_tokens = self.window_tokens / 10;
                self.current_tier = ContextTier::PostCompaction;
            }
        }

        // Extraction (debounced).
        let should_extract = self
            .last_extraction_at
            .is_none_or(|t| now.duration_since(t).as_secs() >= EXTRACTION_INTERVAL_SECS);

        if !should_extract {
            return Vec::new();
        }
        self.last_extraction_at = Some(now);

        // Find new content by diffing.
        let new_content = diff_new_content(&self.previous_screen_text, &screen_text);
        self.previous_screen_text = screen_text;

        if new_content.is_empty() {
            return Vec::new();
        }

        // Extract insights from new content.
        let new_insights = extract_insights(self.agent_kind, self.pane_id, &new_content);

        // Deduplicate and add to buffer.
        let mut added = Vec::new();
        for insight in new_insights {
            let already_exists = self.insights.iter().any(|existing| {
                existing.content == insight.content && existing.insight_type == insight.insight_type
            });
            if !already_exists {
                if self.insights.len() >= MAX_INSIGHTS {
                    self.insights.remove(0);
                }
                self.insights.push(insight.clone());
                added.push(insight);
            }
        }

        added
    }

    /// Inject context into the agent's terminal.
    ///
    /// Wraps the content in agent-appropriate delimiters and writes it via
    /// bracketed paste to avoid triggering agent hotkeys.
    pub fn inject_context(&mut self, content: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Respect debounce.
        if let Some(last) = self.last_injection_at {
            if last.elapsed().as_secs() < INJECTION_DEBOUNCE_SECS {
                return Ok(());
            }
        }

        let wrapped = self.wrap_injection(content);
        let pasted = crate::input::bracketed_paste(&wrapped);
        self.backend.write_input(&pasted)?;

        self.last_injection_at = Some(Instant::now());
        self.injection_count += 1;
        Ok(())
    }

    /// Preview what would be injected (without writing).
    pub fn preview_injection(&self, content: &str) -> String {
        self.wrap_injection(content)
    }

    /// All accumulated insights (newest last).
    pub fn insights(&self) -> &[ExtractedInsight] {
        &self.insights
    }

    /// Insights since a given timestamp.
    pub fn insights_since(&self, since: DateTime<Utc>) -> Vec<&ExtractedInsight> {
        self.insights
            .iter()
            .filter(|i| i.timestamp >= since)
            .collect()
    }

    /// The agent kind for this bridge.
    pub fn agent_kind(&self) -> AgentKind {
        self.agent_kind
    }

    /// The pane ID.
    pub fn pane_id(&self) -> usize {
        self.pane_id
    }

    /// The current context tier.
    pub fn current_tier(&self) -> ContextTier {
        self.current_tier
    }

    /// Wrap content in agent-appropriate delimiters.
    fn wrap_injection(&self, content: &str) -> String {
        let tier = self.current_tier.as_str();
        if self.agent_kind.uses_xml_context() {
            format!(
                "<impulse-context type=\"refresh\" tier=\"{}\">\n{}\n</impulse-context>\n",
                tier, content
            )
        } else {
            format!("# [Impulse Context — {}]\n{}\n", tier, content)
        }
    }
}

// ---------------------------------------------------------------------------
// Standalone helpers
// ---------------------------------------------------------------------------

/// Estimate tokens from visible character count (ANSI-stripped).
///
/// Uses visible chars (not raw PTY bytes) to avoid inflating the estimate
/// with ANSI escape sequences. The multiplier accounts for context that
/// isn't visible in PTY output (system prompt, tool call JSON, user turns).
fn estimate_tokens(visible_chars: usize) -> usize {
    (visible_chars as f64 * VISIBLE_TO_CONTEXT_MULTIPLIER / CHARS_PER_TOKEN) as usize
}

/// Map token usage to a tier.
fn usage_tier(estimated_tokens: usize, window_tokens: usize) -> ContextTier {
    if window_tokens == 0 {
        return ContextTier::None;
    }
    let pct = (estimated_tokens as f64 / window_tokens as f64 * 100.0) as u8;
    match pct {
        0..=44 => ContextTier::None,
        45..=59 => ContextTier::Essential,
        60..=79 => ContextTier::Critical,
        _ => ContextTier::Minimal,
    }
}

/// Scan text for compaction patterns (case-insensitive).
fn scan_compaction(text: &str) -> bool {
    let lower = text.to_lowercase();
    COMPACTION_PATTERNS.iter().any(|pat| lower.contains(pat))
}

/// Simple diff: return lines in `current` that aren't in `previous`.
fn diff_new_content(previous: &str, current: &str) -> String {
    if previous.is_empty() {
        return current.to_string();
    }
    // Find the first divergence point and return everything after.
    let prev_lines: Vec<&str> = previous.lines().collect();
    let curr_lines: Vec<&str> = current.lines().collect();

    let common = prev_lines
        .iter()
        .zip(curr_lines.iter())
        .take_while(|(a, b)| a == b)
        .count();

    curr_lines[common..].join("\n")
}

/// Extract insights from text (ported from context_lifecycle/extractor.rs).
fn extract_insights(agent_kind: AgentKind, pane_id: usize, text: &str) -> Vec<ExtractedInsight> {
    let mut insights = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // File modification patterns.
        if let Some(path) = extract_file_modified(agent_kind, trimmed) {
            insights.push(ExtractedInsight {
                pane_id,
                agent_kind,
                timestamp: Utc::now(),
                insight_type: InsightType::FileModified,
                content: path,
            });
        }

        // Error patterns.
        if let Some(err) = extract_error(agent_kind, trimmed) {
            insights.push(ExtractedInsight {
                pane_id,
                agent_kind,
                timestamp: Utc::now(),
                insight_type: InsightType::ErrorEncountered,
                content: err,
            });
        }

        // Decision patterns.
        if let Some(decision) = extract_decision(trimmed) {
            insights.push(ExtractedInsight {
                pane_id,
                agent_kind,
                timestamp: Utc::now(),
                insight_type: InsightType::DecisionMade,
                content: decision,
            });
        }

        // Task completion patterns.
        if let Some(task) = extract_task_completed(trimmed) {
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

fn extract_file_modified(agent_kind: AgentKind, line: &str) -> Option<String> {
    match agent_kind {
        AgentKind::ClaudeCode => {
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
        AgentKind::OpenCode | AgentKind::Codex => {
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
            if lower.starts_with("error:") || lower.contains("failed") || lower.contains("panicked")
            {
                Some(truncate_insight(line, 120))
            } else {
                None
            }
        }
        AgentKind::OpenCode | AgentKind::Codex => {
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
    if lower.contains("decision:") || lower.contains("chose ") || lower.contains("using approach") {
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

/// Truncate a string to max_len, adding "..." if truncated. UTF-8 safe.
pub fn truncate_insight(s: &str, max_len: usize) -> String {
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
    fn test_agent_kind_detect() {
        assert_eq!(
            AgentKind::detect("claude", "claude-1"),
            AgentKind::ClaudeCode
        );
        assert_eq!(AgentKind::detect("codex", "codex-1"), AgentKind::Codex);
        assert_eq!(AgentKind::detect("opencode", "oc-1"), AgentKind::OpenCode);
        assert_eq!(
            AgentKind::detect("/bin/bash", "bash"),
            AgentKind::GenericShell
        );
    }

    #[test]
    fn test_context_tier_ordering() {
        assert!(ContextTier::None < ContextTier::Full);
        assert!(ContextTier::Full < ContextTier::Essential);
        assert!(ContextTier::Essential < ContextTier::Critical);
        assert!(ContextTier::Critical < ContextTier::Minimal);
    }

    #[test]
    fn test_estimate_tokens() {
        // Formula: visible_chars * 1.6 / 4.0 = visible_chars * 0.4
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(1000), 400);
        assert_eq!(estimate_tokens(800_000), 320_000);
    }

    #[test]
    fn test_estimate_tokens_lower_than_old_formula() {
        // The old formula was output_bytes * 2.5 / 4.0 = 0.625x.
        // The new formula is visible_chars * 1.6 / 4.0 = 0.4x.
        // For the same input, the new estimate should be lower.
        let old_result = (1000_f64 * 2.5 / 4.0) as usize; // 625
        let new_result = estimate_tokens(1000); // 400
        assert!(
            new_result < old_result,
            "new estimate ({}) should be lower than old ({})",
            new_result,
            old_result
        );
    }

    #[test]
    fn test_estimate_tokens_realistic_session() {
        // A Claude Code session with ~68K actual tokens might show
        // ~170K visible chars on screen. The estimate should be closer
        // to 68K than the old formula's ~106K.
        let visible_chars = 170_000;
        let estimated = estimate_tokens(visible_chars);
        // 170_000 * 0.4 = 68_000
        assert_eq!(estimated, 68_000);
    }

    #[test]
    fn test_usage_tier() {
        assert_eq!(usage_tier(0, 200_000), ContextTier::None);
        assert_eq!(usage_tier(89_000, 200_000), ContextTier::None);
        assert_eq!(usage_tier(90_000, 200_000), ContextTier::Essential);
        assert_eq!(usage_tier(120_000, 200_000), ContextTier::Critical);
        assert_eq!(usage_tier(160_000, 200_000), ContextTier::Minimal);
    }

    #[test]
    fn test_scan_compaction() {
        assert!(scan_compaction("System: compressing prior messages"));
        assert!(scan_compaction("auto-compact triggered"));
        assert!(!scan_compaction("Hello world"));
        assert!(!scan_compaction(""));
    }

    #[test]
    fn test_extract_file_modified_claude() {
        let insights = extract_insights(
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
    fn test_extract_errors() {
        let insights = extract_insights(
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
        let insights = extract_insights(
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
        let insights = extract_insights(
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
    fn test_truncate_insight() {
        assert_eq!(truncate_insight("short", 120), "short");
        let long = "a".repeat(200);
        let truncated = truncate_insight(&long, 120);
        assert!(truncated.ends_with("..."));
        assert!(truncated.len() <= 123); // 120 + "..."
    }

    #[test]
    fn test_diff_new_content_empty_previous() {
        let result = diff_new_content("", "hello\nworld");
        assert_eq!(result, "hello\nworld");
    }

    #[test]
    fn test_diff_new_content_with_overlap() {
        let result = diff_new_content("line1\nline2", "line1\nline2\nline3");
        assert_eq!(result, "line3");
    }

    #[test]
    fn test_diff_new_content_no_change() {
        let result = diff_new_content("same\nlines", "same\nlines");
        assert_eq!(result, "");
    }

    #[test]
    fn test_injection_wrapping_claude() {
        // We can't easily test ContextBridge without a real backend, but we can
        // test the wrapping logic directly.
        let agent = AgentKind::ClaudeCode;
        let content = "test content";
        let tier = "essential";

        let wrapped = if agent.uses_xml_context() {
            format!(
                "<impulse-context type=\"refresh\" tier=\"{}\">\n{}\n</impulse-context>\n",
                tier, content
            )
        } else {
            format!("# [Impulse Context — {}]\n{}\n", tier, content)
        };

        assert!(wrapped.contains("<impulse-context"));
        assert!(wrapped.contains("test content"));
    }

    #[test]
    fn test_injection_wrapping_opencode() {
        let agent = AgentKind::OpenCode;
        let content = "test content";
        let tier = "critical";

        let wrapped = if agent.uses_xml_context() {
            format!(
                "<impulse-context type=\"refresh\" tier=\"{}\">\n{}\n</impulse-context>\n",
                tier, content
            )
        } else {
            format!("# [Impulse Context — {}]\n{}\n", tier, content)
        };

        assert!(wrapped.starts_with("# [Impulse Context"));
        assert!(wrapped.contains("test content"));
    }
}
