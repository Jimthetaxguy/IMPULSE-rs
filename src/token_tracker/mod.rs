//! Token Tracker Module
//!
//! A dynamic token tracking algorithm that measures distance between autocompaction events
//! across multiple AI coding platforms (Claude Code, Codex, OpenCode, ChatGPT, Gemini).
//!
//! Based on research from:
//! - OpenAI compaction system (server-side and standalone endpoints)
//! - Claude Code auto memory and context management
//! - OpenCode pruning and compaction hooks
//! - JetBrains Research on observation masking vs summarization
//! - Algorithm validation document (three-tier working set, confidence decay)

pub mod algorithm;
pub mod cross_platform;
pub mod metrics;
pub mod research;
pub mod types;

pub use algorithm::*;
pub use cross_platform::*;
pub use metrics::*;
pub use research::research::{
    CLAUDE_CODE_MEMORY_LIMIT_LINES, JETBRAINS_DECAY_RATE, OPENAI_COMPACTION_THRESHOLD,
    OPENCOD_DEFAULT_THRESHOLD,
};
pub use types::*;
