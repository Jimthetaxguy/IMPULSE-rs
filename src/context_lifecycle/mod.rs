//! Context Lifecycle Manager for Impulse.
//!
//! Makes Impulse a bidirectional context manager for hosted AI agents.
//! Monitors context window usage, injects context at spawn and thresholds,
//! detects compaction events, extracts insights from agent output, and
//! cross-pollinates knowledge between panes.
//!
//! ## Capabilities
//!
//! - **Monitor**: Track context window usage + detect agent intent from PTY output
//! - **Inject**: Push relevant context at spawn and at 45/60/80% thresholds
//! - **Extract**: Pull key info from agent sessions (files modified, errors, decisions)
//! - **Refine**: Summarize and cross-pollinate between Claude Code and OpenCode panes

pub mod detector;
pub mod extractor;
pub mod injector;
pub mod intent;
pub mod monitor;
pub mod parser;
pub mod templates;
pub mod types;

// Phase 3 target (Task 18): RuleBasedClassifier, Complexity, IntentContext, IntentConflict
// are intentionally not re-exported — they have zero external callers today.
// AgentIntent is kept because IntentStore methods return it.
pub use intent::{Activity, ActivityType, AgentIntent, AgentType, IntentCategory, IntentStore};
pub use types::{
    AgentKind, ContextTier, ExtractedInsight, InsightType, MonitorAction, PaneContextState,
    PendingInjection,
};
