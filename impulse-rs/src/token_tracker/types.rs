//! Token Tracker Types
//!
//! Core data structures for token tracking and compaction measurement

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::hash::Hash;

/// Platform identifiers for cross-platform analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    ClaudeCode,
    Codex,
    OpenCode,
    ChatGPT,
    Gemini,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::ClaudeCode => "claude_code",
            Platform::Codex => "codex",
            Platform::OpenCode => "opencode",
            Platform::ChatGPT => "chatgpt",
            Platform::Gemini => "gemini",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude_code" | "claude" => Some(Platform::ClaudeCode),
            "codex" => Some(Platform::Codex),
            "opencode" => Some(Platform::OpenCode),
            "chatgpt" => Some(Platform::ChatGPT),
            "gemini" => Some(Platform::Gemini),
            _ => None,
        }
    }
}

/// Memory tier levels (from hot to cold)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTier {
    /// Last few turns, active files, latest errors, current plan
    Hot,
    /// Prior resolved subthreads, long tool outputs, repeated file reads  
    Warm,
    /// Completed exploratory threads, older brainstorms
    Cold,
}

impl MemoryTier {
    /// Get the default token budget for each tier
    pub fn default_budget(&self) -> u32 {
        match self {
            MemoryTier::Hot => 0,   // No compression
            MemoryTier::Warm => 60, // Compress to 60 tokens
            MemoryTier::Cold => 20, // Micro-summary 20 tokens
        }
    }
}

/// Token budget thresholds based on context usage percentage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudget {
    /// Threshold for aggressive pruning (70%)
    pub soft_threshold: f64,
    /// Threshold for micro-summarization (90%)
    pub hard_threshold: f64,
    /// Token budget below soft threshold
    pub normal_budget: u32,
    /// Token budget between thresholds
    pub aggressive_budget: u32,
    /// Token budget above hard threshold
    pub micro_budget: u32,
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self {
            soft_threshold: 0.70,
            hard_threshold: 0.90,
            normal_budget: 120,
            aggressive_budget: 60,
            micro_budget: 20,
        }
    }
}

/// A single token usage event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenEvent {
    /// Unique identifier for this event
    pub id: String,
    /// Platform where this event occurred
    pub platform: Platform,
    /// Session identifier
    pub session_id: String,
    /// Timestamp of the event
    pub timestamp: DateTime<Utc>,
    /// Current context token count
    pub context_tokens: u32,
    /// Maximum context window for the platform/model
    pub max_context: u32,
    /// Context usage percentage (0.0 - 1.0)
    pub usage_ratio: f64,
    /// Number of messages in context
    pub message_count: u32,
    /// Number of tool calls in context
    pub tool_call_count: u32,
}

/// An autocompaction event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionEvent {
    /// Unique identifier for this compaction
    pub id: String,
    /// Platform where compaction occurred
    pub platform: Platform,
    /// Session identifier
    pub session_id: String,
    /// Timestamp when compaction started
    pub started_at: DateTime<Utc>,
    /// Timestamp when compaction completed
    pub completed_at: DateTime<Utc>,
    /// Duration of compaction in milliseconds
    pub duration_ms: u64,
    /// Token count before compaction
    pub tokens_before: u32,
    /// Token count after compaction
    pub tokens_after: u32,
    /// Compression ratio (after/before)
    pub compression_ratio: f64,
    /// Type of compaction performed
    pub compaction_type: CompactionType,
    /// Whether this was triggered automatically or manually
    pub is_automatic: bool,
}

/// Input for recording a compaction event before derived metrics are computed.
#[derive(Debug, Clone)]
pub struct CompactionRecord {
    /// Platform where compaction occurred
    pub platform: Platform,
    /// Session identifier
    pub session_id: String,
    /// Timestamp when compaction started
    pub started_at: DateTime<Utc>,
    /// Timestamp when compaction completed
    pub completed_at: DateTime<Utc>,
    /// Token count before compaction
    pub tokens_before: u32,
    /// Token count after compaction
    pub tokens_after: u32,
    /// Type of compaction performed
    pub compaction_type: CompactionType,
    /// Whether this was triggered automatically or manually
    pub is_automatic: bool,
}

/// Type of compaction performed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionType {
    /// Pruned low-value tool outputs (masking)
    Prune,
    /// Extracted durable state into structured working set
    Extract,
    /// Summarized specific stale segments
    Summarize,
    /// Full context rewrite
    FullRewrite,
    /// No compaction (below threshold)
    None,
}

/// Distance metrics between compaction events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionDistance {
    /// ID of the earlier compaction event
    pub event_id_1: String,
    /// ID of the later compaction event
    pub event_id_2: String,
    /// Time distance in seconds
    pub time_distance_seconds: i64,
    /// Token distance (tokens processed between events)
    pub token_distance: u32,
    /// Message distance (messages between events)
    pub message_distance: u32,
    /// Computed stability score (0.0 - 1.0)
    /// Higher = more stable (longer between compactions)
    pub stability_score: f64,
}

/// Platform-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformConfig {
    /// Platform identifier
    pub platform: Platform,
    /// Default context window size
    pub default_context_window: u32,
    /// Default compaction threshold
    pub default_threshold: f64,
    /// Whether platform supports automatic compaction
    pub supports_auto_compaction: bool,
    /// Whether platform supports pruning
    pub supports_pruning: bool,
    /// Whether platform supports hooks
    pub supports_hooks: bool,
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            platform: Platform::ClaudeCode,
            default_context_window: 200_000,
            default_threshold: 0.85,
            supports_auto_compaction: true,
            supports_pruning: true,
            supports_hooks: true,
        }
    }
}

impl PlatformConfig {
    pub fn for_platform(platform: Platform) -> Self {
        match platform {
            Platform::ClaudeCode => Self {
                platform,
                default_context_window: 200_000,
                default_threshold: 0.85,
                supports_auto_compaction: true,
                supports_pruning: true,
                supports_hooks: true,
            },
            Platform::Codex => Self {
                platform,
                default_context_window: 128_000,
                default_threshold: 0.80,
                supports_auto_compaction: true,
                supports_pruning: true,
                supports_hooks: true,
            },
            Platform::OpenCode => Self {
                platform,
                default_context_window: 100_000,
                default_threshold: 0.75,
                supports_auto_compaction: true,
                supports_pruning: true,
                supports_hooks: true,
            },
            Platform::ChatGPT => Self {
                platform,
                default_context_window: 128_000,
                default_threshold: 0.90,
                supports_auto_compaction: true,
                supports_pruning: false,
                supports_hooks: false,
            },
            Platform::Gemini => Self {
                platform,
                default_context_window: 1_000_000,
                default_threshold: 0.95,
                supports_auto_compaction: true,
                supports_pruning: false,
                supports_hooks: false,
            },
        }
    }
}

/// Confidence decay configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceDecayConfig {
    /// Decay rate per minute (lambda in e^(-lambda * t))
    pub decay_rate: f64,
    /// Minimum confidence threshold
    pub min_confidence: f64,
}

impl Default for ConfidenceDecayConfig {
    fn default() -> Self {
        Self {
            decay_rate: 0.03,
            min_confidence: 0.70,
        }
    }
}

/// Token tracker statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenTrackerStats {
    /// Total token events recorded
    pub total_events: u64,
    /// Total compaction events recorded
    pub total_compactions: u64,
    /// Average tokens per event
    pub avg_tokens_per_event: f64,
    /// Average time between compactions (seconds)
    pub avg_compaction_interval: f64,
    /// Average compression ratio
    pub avg_compression_ratio: f64,
    /// Most common compaction type
    pub most_common_compaction: CompactionType,
    /// Platform with most compactions
    pub most_active_platform: Platform,
}
