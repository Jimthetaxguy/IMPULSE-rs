use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ============================================================================
// Stewardship Mode
// ============================================================================

/// User-configurable stewardship behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum StewardshipMode {
    /// Apply cleanup automatically, log actions
    Auto,
    /// Create proposals, wait for user approval (default)
    #[default]
    Review,
    /// Monitor only, no modifications
    Off,
}

impl StewardshipMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "review" => Some(Self::Review),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Review => "review",
            Self::Off => "off",
        }
    }
}

// ============================================================================
// Threshold Levels
// ============================================================================

/// Progressive context cleanup thresholds
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThresholdLevel {
    /// 0-30%: No action, just log
    Passive,
    /// 30-45%: Track patterns, flag duplicates
    Monitor,
    /// 45-60%: Propose removing obvious duplicates
    Surgical,
    /// 60-80%: Propose rot removal, consolidation
    Thoughtful,
    /// 80%+: Aggressive summarization
    Emergency,
}

impl ThresholdLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Passive => "passive",
            Self::Monitor => "monitor",
            Self::Surgical => "surgical",
            Self::Thoughtful => "thoughtful",
            Self::Emergency => "emergency",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Passive => "Passive",
            Self::Monitor => "Monitoring",
            Self::Surgical => "Surgical Cleanup",
            Self::Thoughtful => "Thoughtful Review",
            Self::Emergency => "Emergency Summarize",
        }
    }
}

impl std::fmt::Display for ThresholdLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ============================================================================
// Stewardship Configuration
// ============================================================================

/// Resolved stewardship config from the main Config struct
#[derive(Debug, Clone)]
pub struct StewardshipConfig {
    pub mode: StewardshipMode,
    pub monitor_threshold: f32,
    pub surgical_threshold: f32,
    pub thoughtful_threshold: f32,
    pub emergency_threshold: f32,
    pub poll_interval_secs: u64,
    pub context_window_tokens: usize,
    pub cross_project_enabled: bool,
}

impl Default for StewardshipConfig {
    fn default() -> Self {
        Self {
            mode: StewardshipMode::Review,
            monitor_threshold: 0.30,
            surgical_threshold: 0.45,
            thoughtful_threshold: 0.60,
            emergency_threshold: 0.80,
            poll_interval_secs: 10,
            context_window_tokens: 200_000,
            cross_project_enabled: true,
        }
    }
}

impl StewardshipConfig {
    /// Build stewardship config from the main Config struct
    pub fn from_config(config: &crate::state::Config) -> Self {
        Self {
            mode: StewardshipMode::parse(&config.stewardship_mode)
                .unwrap_or(StewardshipMode::Review),
            monitor_threshold: config.stewardship_monitor_threshold,
            surgical_threshold: config.stewardship_surgical_threshold,
            thoughtful_threshold: config.stewardship_thoughtful_threshold,
            emergency_threshold: config.stewardship_emergency_threshold,
            poll_interval_secs: config.stewardship_poll_interval_secs,
            context_window_tokens: config.stewardship_context_window_tokens,
            cross_project_enabled: config.stewardship_cross_project_enabled,
        }
    }

    /// Resolve threshold level from context percentage
    pub fn resolve_threshold(&self, pct: f32) -> ThresholdLevel {
        if pct >= self.emergency_threshold {
            ThresholdLevel::Emergency
        } else if pct >= self.thoughtful_threshold {
            ThresholdLevel::Thoughtful
        } else if pct >= self.surgical_threshold {
            ThresholdLevel::Surgical
        } else if pct >= self.monitor_threshold {
            ThresholdLevel::Monitor
        } else {
            ThresholdLevel::Passive
        }
    }
}

// ============================================================================
// Transcript Parsing Types
// ============================================================================

/// A parsed tool use from a transcript message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub input_preview: String,
    pub input_chars: usize,
    /// `file_path` from the full input, read before the preview is cut (a
    /// preview of a larger input is not valid JSON).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

/// A parsed tool result from a transcript message.
///
/// Named `ParsedToolResult` to distinguish from `tooling::traits::ToolResult`
/// which represents tool execution output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedToolResult {
    pub tool_use_id: String,
    pub content_chars: usize,
}

/// A parsed message from a Claude Code session JSONL
#[derive(Debug, Clone)]
pub struct TranscriptMessage {
    pub role: String,
    pub text_content: String,
    pub tool_uses: Vec<ToolUse>,
    pub tool_results: Vec<ParsedToolResult>,
    pub char_count: usize,
    pub estimated_tokens: usize,
}

// ============================================================================
// Session Analysis Types
// ============================================================================

/// A decision extracted from a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedDecision {
    pub description: String,
    pub context: String,
    pub message_index: usize,
}

/// A pattern of repeated tool calls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPattern {
    pub tool_name: String,
    pub count: usize,
    pub input_hash: String,
    pub first_index: usize,
    pub last_index: usize,
}

/// A region of duplicate content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateRegion {
    pub tool_name: String,
    pub occurrences: usize,
    pub indices: Vec<usize>,
    pub estimated_tokens: usize,
    pub input_preview: String,
}

/// A candidate for context rot (early context superseded by later work)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotCandidate {
    pub description: String,
    pub reason: String,
    pub message_range: (usize, usize),
    pub estimated_tokens: usize,
}

/// Complete analysis of a session transcript
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAnalysis {
    pub session_id: String,
    pub project_hash: String,
    pub transcript_path: PathBuf,
    pub analyzed_at: DateTime<Utc>,
    pub message_count: usize,
    pub estimated_tokens: usize,
    pub estimated_context_pct: f32,
    pub decisions: Vec<ExtractedDecision>,
    pub files_touched: Vec<String>,
    pub tool_patterns: Vec<ToolPattern>,
    pub duplicate_regions: Vec<DuplicateRegion>,
    pub rot_candidates: Vec<RotCandidate>,
    pub key_insights: Vec<String>,
}

// ============================================================================
// Cleanup Types
// ============================================================================

/// Strategy for cleaning up context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupStrategy {
    /// Collapse repeated identical tool calls
    Deduplicate,
    /// Summarize verbose tool outputs to key results
    Condense,
    /// Remove early context superseded by later work
    RemoveRot,
    /// Merge similar contexts into unified blocks
    Consolidate,
    /// Aggressive cleanup, preserve only critical decisions
    EmergencySummarize,
}

impl CleanupStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Deduplicate => "deduplicate",
            Self::Condense => "condense",
            Self::RemoveRot => "remove_rot",
            Self::Consolidate => "consolidate",
            Self::EmergencySummarize => "emergency_summarize",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Deduplicate => "Deduplicate",
            Self::Condense => "Condense",
            Self::RemoveRot => "Remove Rot",
            Self::Consolidate => "Consolidate",
            Self::EmergencySummarize => "Emergency Summarize",
        }
    }
}

/// A region targeted by a cleanup proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalRegion {
    pub description: String,
    pub message_indices: Vec<usize>,
    pub estimated_tokens: usize,
}

/// Status of a cleanup proposal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProposalStatus {
    Pending,
    Approved,
    Applied,
    Rejected,
}

impl ProposalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProposalStatus::Pending => "pending",
            ProposalStatus::Approved => "approved",
            ProposalStatus::Applied => "applied",
            ProposalStatus::Rejected => "rejected",
        }
    }
}

/// A cleanup proposal with strategy and target regions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupProposal {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub session_id: String,
    pub threshold: ThresholdLevel,
    pub strategy: CleanupStrategy,
    pub estimated_tokens_freed: usize,
    pub regions: Vec<ProposalRegion>,
    pub preserves: Vec<String>,
    pub status: ProposalStatus,
}

// ============================================================================
// Cross-Project Memory Types
// ============================================================================

/// A pattern observed across multiple projects
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossProjectPattern {
    pub id: String,
    pub pattern_type: String,
    pub description: String,
    pub occurrences: usize,
    pub projects: Vec<String>,
    pub insight: String,
    pub first_seen: String,
    pub last_seen: String,
}

/// Cross-project memory database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossProjectMemory {
    pub version: String,
    pub updated: DateTime<Utc>,
    pub patterns: Vec<CrossProjectPattern>,
    pub learnings: Vec<String>,
    pub stats: CrossProjectStats,
}

impl Default for CrossProjectMemory {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            updated: Utc::now(),
            patterns: Vec::new(),
            learnings: Vec::new(),
            stats: CrossProjectStats::default(),
        }
    }
}

/// Statistics for cross-project memory
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CrossProjectStats {
    pub total_sessions_analyzed: usize,
    pub total_patterns: usize,
    pub total_learnings: usize,
}

/// Per-project memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemory {
    pub project_hash: String,
    pub project_path: String,
    pub updated: DateTime<Utc>,
    pub sessions_analyzed: usize,
    pub patterns: Vec<CrossProjectPattern>,
    pub learnings: Vec<String>,
}

impl ProjectMemory {
    pub fn new(project_hash: String, project_path: String) -> Self {
        Self {
            project_hash,
            project_path,
            updated: Utc::now(),
            sessions_analyzed: 0,
            patterns: Vec::new(),
            learnings: Vec::new(),
        }
    }
}

// ============================================================================
// Monitor Types
// ============================================================================

/// A single monitoring check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorCheck {
    pub timestamp: DateTime<Utc>,
    pub file_size_bytes: u64,
    pub estimated_tokens: usize,
    pub estimated_pct: f32,
    pub threshold: ThresholdLevel,
    pub action_taken: Option<String>,
}

/// Result of processing proposals through the approval workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessResult {
    pub mode: StewardshipMode,
    pub applied: Vec<String>,
    pub queued: usize,
    pub logged: usize,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stewardship_mode_round_trip() {
        for mode in &["auto", "review", "off"] {
            let parsed = StewardshipMode::parse(mode).unwrap();
            assert_eq!(parsed.as_str(), *mode);
        }
        assert!(StewardshipMode::parse("invalid").is_none());
    }

    #[test]
    fn test_threshold_ordering() {
        assert!(ThresholdLevel::Passive < ThresholdLevel::Monitor);
        assert!(ThresholdLevel::Monitor < ThresholdLevel::Surgical);
        assert!(ThresholdLevel::Surgical < ThresholdLevel::Thoughtful);
        assert!(ThresholdLevel::Thoughtful < ThresholdLevel::Emergency);
    }

    #[test]
    fn test_resolve_threshold() {
        let config = StewardshipConfig::default();
        assert_eq!(config.resolve_threshold(0.10), ThresholdLevel::Passive);
        assert_eq!(config.resolve_threshold(0.29), ThresholdLevel::Passive);
        assert_eq!(config.resolve_threshold(0.30), ThresholdLevel::Monitor);
        assert_eq!(config.resolve_threshold(0.44), ThresholdLevel::Monitor);
        assert_eq!(config.resolve_threshold(0.45), ThresholdLevel::Surgical);
        assert_eq!(config.resolve_threshold(0.59), ThresholdLevel::Surgical);
        assert_eq!(config.resolve_threshold(0.60), ThresholdLevel::Thoughtful);
        assert_eq!(config.resolve_threshold(0.79), ThresholdLevel::Thoughtful);
        assert_eq!(config.resolve_threshold(0.80), ThresholdLevel::Emergency);
        assert_eq!(config.resolve_threshold(0.95), ThresholdLevel::Emergency);
    }

    #[test]
    fn test_cleanup_strategy_as_str() {
        assert_eq!(CleanupStrategy::Deduplicate.as_str(), "deduplicate");
        assert_eq!(
            CleanupStrategy::EmergencySummarize.as_str(),
            "emergency_summarize"
        );
    }

    #[test]
    fn test_cross_project_memory_default() {
        let mem = CrossProjectMemory::default();
        assert_eq!(mem.version, "1.0");
        assert!(mem.patterns.is_empty());
        assert!(mem.learnings.is_empty());
        assert_eq!(mem.stats.total_sessions_analyzed, 0);
    }

    #[test]
    fn test_session_analysis_serialization() {
        let analysis = SessionAnalysis {
            session_id: "test-123".to_string(),
            project_hash: "hash-abc".to_string(),
            transcript_path: PathBuf::from("/tmp/test.jsonl"),
            analyzed_at: Utc::now(),
            message_count: 42,
            estimated_tokens: 10000,
            estimated_context_pct: 0.05,
            decisions: vec![],
            files_touched: vec!["src/main.rs".to_string()],
            tool_patterns: vec![],
            duplicate_regions: vec![],
            rot_candidates: vec![],
            key_insights: vec!["Test insight".to_string()],
        };

        let json = serde_json::to_string(&analysis).unwrap();
        let deserialized: SessionAnalysis = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.session_id, "test-123");
        assert_eq!(deserialized.message_count, 42);
    }

    // ---- Serde round-trips for the persisted YAML records ----
    //
    // Proposals, session analyses, and project/cross-project memory are
    // written as YAML under `.impulse/stewardship/`. `approval::list_pending`
    // skips a proposal it cannot parse, so a wire break would hide proposals
    // silently. These types lack PartialEq, so the round-trips go through
    // YAML (the on-disk format) and compare the JSON values of both sides.

    fn fixed_time(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn assert_yaml_round_trip<T>(original: &T) -> T
    where
        T: Serialize + serde::de::DeserializeOwned,
    {
        let yaml = serde_yaml::to_string(original).unwrap();
        let recovered: T = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(
            serde_json::to_value(original).unwrap(),
            serde_json::to_value(&recovered).unwrap()
        );
        recovered
    }

    fn sample_pattern() -> CrossProjectPattern {
        CrossProjectPattern {
            id: "p-1".into(),
            pattern_type: "workflow".into(),
            description: "Re-runs clippy after every edit".into(),
            occurrences: 4,
            projects: vec!["proj-a".into(), "proj-b".into()],
            insight: "Batch lint runs".into(),
            first_seen: "2026-02-23".into(),
            last_seen: "2026-03-01".into(),
        }
    }

    #[test]
    fn round_trip_cleanup_proposal() {
        let original = CleanupProposal {
            id: "prop-1".into(),
            created_at: fixed_time("2026-06-01T12:00:00Z"),
            session_id: "sess-1".into(),
            threshold: ThresholdLevel::Thoughtful,
            strategy: CleanupStrategy::RemoveRot,
            estimated_tokens_freed: 12_000,
            regions: vec![ProposalRegion {
                description: "stale exploration".into(),
                message_indices: vec![3, 4, 9],
                estimated_tokens: 8_000,
            }],
            preserves: vec!["decision: use YAML".into()],
            status: ProposalStatus::Approved,
        };
        let recovered = assert_yaml_round_trip(&original);
        assert_eq!(recovered.threshold, ThresholdLevel::Thoughtful);
        assert_eq!(recovered.strategy, CleanupStrategy::RemoveRot);
        assert_eq!(recovered.status, ProposalStatus::Approved);
        assert_eq!(recovered.regions[0].message_indices, vec![3, 4, 9]);
    }

    #[test]
    fn round_trip_session_analysis_with_nested_records() {
        let original = SessionAnalysis {
            session_id: "sess-1".into(),
            project_hash: "hash-abc".into(),
            transcript_path: PathBuf::from("/tmp/sess-1.jsonl"),
            analyzed_at: fixed_time("2026-06-01T12:00:00Z"),
            message_count: 42,
            estimated_tokens: 10_000,
            estimated_context_pct: 0.25,
            decisions: vec![ExtractedDecision {
                description: "Use YAML".into(),
                context: "storage format".into(),
                message_index: 5,
            }],
            files_touched: vec!["src/main.rs".into()],
            tool_patterns: vec![ToolPattern {
                tool_name: "Read".into(),
                count: 3,
                input_hash: "abc".into(),
                first_index: 1,
                last_index: 8,
            }],
            duplicate_regions: vec![DuplicateRegion {
                tool_name: "Read".into(),
                occurrences: 2,
                indices: vec![1, 8],
                estimated_tokens: 500,
                input_preview: "{\"file_path\":\"src/main.rs\"}".into(),
            }],
            rot_candidates: vec![RotCandidate {
                description: "abandoned approach".into(),
                reason: "superseded".into(),
                message_range: (10, 14),
                estimated_tokens: 1_200,
            }],
            key_insights: vec!["Test insight".into()],
        };
        let recovered = assert_yaml_round_trip(&original);
        assert_eq!(recovered.rot_candidates[0].message_range, (10, 14));
        assert_eq!(recovered.transcript_path, original.transcript_path);
    }

    #[test]
    fn round_trip_cross_project_memory() {
        let original = CrossProjectMemory {
            version: "1.0".into(),
            updated: fixed_time("2026-06-01T12:00:00Z"),
            patterns: vec![sample_pattern()],
            learnings: vec!["Prefer atomic writes".into()],
            stats: CrossProjectStats {
                total_sessions_analyzed: 9,
                total_patterns: 1,
                total_learnings: 1,
            },
        };
        let recovered = assert_yaml_round_trip(&original);
        assert_eq!(recovered.stats.total_sessions_analyzed, 9);
        assert_eq!(recovered.patterns[0].projects, vec!["proj-a", "proj-b"]);
    }

    #[test]
    fn round_trip_project_memory() {
        let original = ProjectMemory {
            project_hash: "hash-abc".into(),
            project_path: "/work/impulse".into(),
            updated: fixed_time("2026-06-01T12:00:00Z"),
            sessions_analyzed: 3,
            patterns: vec![sample_pattern()],
            learnings: vec!["Keep CONTEXT.md current".into()],
        };
        let recovered = assert_yaml_round_trip(&original);
        assert_eq!(recovered.sessions_analyzed, 3);
    }

    #[test]
    fn round_trip_monitor_check_and_process_result() {
        let check = MonitorCheck {
            timestamp: fixed_time("2026-06-01T12:00:00Z"),
            file_size_bytes: 2048,
            estimated_tokens: 512,
            estimated_pct: 0.5,
            threshold: ThresholdLevel::Surgical,
            action_taken: Some("queued proposal".into()),
        };
        let recovered = assert_yaml_round_trip(&check);
        assert_eq!(recovered.threshold, ThresholdLevel::Surgical);

        let result = ProcessResult {
            mode: StewardshipMode::Auto,
            applied: vec!["prop-1".into()],
            queued: 2,
            logged: 1,
        };
        let recovered = assert_yaml_round_trip(&result);
        assert_eq!(recovered.mode, StewardshipMode::Auto);
    }

    #[test]
    fn test_tool_use_without_file_path_deserializes_as_none() {
        let json = r#"{"id":"t1","name":"Bash","input_preview":"ls","input_chars":2}"#;
        let tool: ToolUse = serde_json::from_str(json).unwrap();
        assert_eq!(tool.file_path, None);
        // And `None` is omitted on write, matching the older shape.
        assert!(!serde_json::to_string(&tool).unwrap().contains("file_path"));
    }

    #[test]
    fn test_stewardship_mode_wire_names_are_pinned() {
        for mode in [
            StewardshipMode::Auto,
            StewardshipMode::Review,
            StewardshipMode::Off,
        ] {
            let wire = serde_json::to_value(mode).unwrap();
            assert_eq!(wire.as_str(), Some(mode.as_str()));
            assert_eq!(
                serde_json::from_value::<StewardshipMode>(wire).unwrap(),
                mode
            );
        }
    }

    #[test]
    fn test_threshold_level_wire_names_are_pinned() {
        for level in [
            ThresholdLevel::Passive,
            ThresholdLevel::Monitor,
            ThresholdLevel::Surgical,
            ThresholdLevel::Thoughtful,
            ThresholdLevel::Emergency,
        ] {
            let wire = serde_json::to_value(level).unwrap();
            assert_eq!(wire.as_str(), Some(level.as_str()));
            assert_eq!(
                serde_json::from_value::<ThresholdLevel>(wire).unwrap(),
                level
            );
        }
    }

    #[test]
    fn test_cleanup_strategy_wire_names_are_pinned() {
        for strategy in [
            CleanupStrategy::Deduplicate,
            CleanupStrategy::Condense,
            CleanupStrategy::RemoveRot,
            CleanupStrategy::Consolidate,
            CleanupStrategy::EmergencySummarize,
        ] {
            let wire = serde_json::to_value(strategy).unwrap();
            assert_eq!(wire.as_str(), Some(strategy.as_str()));
            assert_eq!(
                serde_json::from_value::<CleanupStrategy>(wire).unwrap(),
                strategy
            );
        }
    }

    #[test]
    fn test_proposal_status_wire_names_are_pinned() {
        for status in [
            ProposalStatus::Pending,
            ProposalStatus::Approved,
            ProposalStatus::Applied,
            ProposalStatus::Rejected,
        ] {
            let wire = serde_json::to_value(status).unwrap();
            assert_eq!(wire.as_str(), Some(status.as_str()));
            assert_eq!(
                serde_json::from_value::<ProposalStatus>(wire).unwrap(),
                status
            );
        }
    }
}
