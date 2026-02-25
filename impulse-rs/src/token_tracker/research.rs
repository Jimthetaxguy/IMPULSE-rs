//! Token Tracking Algorithm - Research Summary
//!
//! A dynamic algorithm for tracking token usage and measuring distance between
//! autocompaction events across multiple AI coding platforms.
//!
//! ## Research Sources
//!
//! ### OpenAI
//! - Server-side compaction with configurable threshold (compact_threshold)
//! - Standalone compact endpoint for stateless compaction
//! - Encrypted compaction items that carry forward context
//! - Context window management with token counting
//!
//! ### Claude Code
//! - Auto memory with 200-line limit at startup
//! - Hierarchical memory: project/user/auto
//! - Compaction instructions in CLAUDE.md
//! - Memory files in ~/.claude/projects/<project>/memory/
//!
//! ### OpenCode
//! - Pruning thresholds (compaction.auto/prune/reserved)
//! - Hidden compaction agent runs automatically
//! - experimental.session.compacting hook for integration
//! - Prune feature removes older tool outputs
//!
//! ### JetBrains Research
//! - Observation masking can outperform LLM summarization
//! - Combined approach (masking + summarization) delivers additional cost reduction
//! - LLM summarization tends to elongate agent trajectories (~15% longer)
//!
//! ## Algorithm Design
//!
//! ### Three-Tier Working Set
//! | Tier   | Contents                                    | Strategy        |
//! |--------|--------------------------------------------|-----------------|
//! | Hot    | Last few turns, active files, errors       | No compression  |
//! | Warm   | Prior subthreads, long tool outputs        | Mask/prune first|
//! | Cold   | Completed threads, older brainstorms        | Summarize       |
//!
//! ### Token Budget Tiers
//! - Usage < 70%: Normal injection (120 tokens)
//! - Usage 70-90%: Aggressive prune (60 tokens)
//! - Usage >= 90%: Micro-summarize (20 tokens)
//!
//! ### Confidence Decay
//! ```
//! confidence_at_time_t = initial_confidence * e^(-0.03 * t)
//! ```
//! Where t is minutes since last update.
//!
//! ### Stability Score
//! ```
//! stability = time_score * 0.5 + token_score * 0.3 + message_score * 0.2
//! ```
//! Where each score is normalized (0-1) based on thresholds.
//!
//! ## Platform Comparison
//!
//! | Platform    | Context Window | Default Threshold | Auto Compaction | Pruning |
//! |-------------|----------------|-------------------|-----------------|---------|
//! | Claude Code | 200K           | 85%               | Yes             | Yes     |
//! | Codex       | 128K           | 80%               | Yes             | Yes     |
//! | OpenCode    | 100K           | 75%               | Yes             | Yes     |
//! | ChatGPT     | 128K           | 90%               | Yes             | No      |
//! | Gemini      | 1M             | 95%               | Yes             | No      |
//!
//! ## Usage
//!
//! ```rust
//! use token_tracker::{TokenTracker, Platform, CompactionType};
//!
//! let mut tracker = TokenTracker::new();
//!
//! // Record a token event
//! tracker.record_event(
//!     Platform::ClaudeCode,
//!     "session-123",
//!     50_000,   // context tokens
//!     200_000,  // max context
//!     10,       // messages
//!     20,       // tool calls
//! );
//!
//! // Get appropriate token budget
//! let budget = tracker.get_token_budget(0.65); // Returns 120
//! let budget = tracker.get_token_budget(0.80); // Returns 60
//! ```
//!
//! ## Key Insights
//!
//! 1. **Pruning-first approach**: JetBrains Research shows observation masking
//!    can be as effective as summarization at lower cost.
//!
//! 2. **Tiered memory**: Hot/Warm/Cold segmentation allows targeted compression
//!    rather than uniform summarization.
//!
//! 3. **Predictive tracking**: By measuring token growth rate, we can predict
//!    when next compaction will occur and prepare proactively.
//!
//! 4. **Cross-platform analysis**: Different platforms have different thresholds
//!    and capabilities - understanding these helps optimize context management.

#[allow(clippy::module_inception)]
pub mod research {
    pub const OPENAI_COMPACTION_THRESHOLD: f64 = 0.80;
    pub const CLAUDE_CODE_MEMORY_LIMIT_LINES: usize = 200;
    pub const OPENCOD_DEFAULT_THRESHOLD: f64 = 0.75;
    pub const JETBRAINS_DECAY_RATE: f64 = 0.03;
}
