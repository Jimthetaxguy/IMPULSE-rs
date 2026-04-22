//! Compaction detector — pattern-matches PTY output for context compaction events.

use std::time::Instant;

use super::types::{MonitorAction, PaneContextState, COMPACTION_DEBOUNCE_SECS};

/// Known phrases emitted by AI agents when compacting context.
const COMPACTION_PATTERNS: &[&str] = &[
    "compressing prior messages",
    "auto-compact",
    "context compressed",
    "compacted conversation",
    "summarizing conversation",
    "conversation is getting long",
    "context window is getting full",
];

/// Detects compaction events in agent PTY output.
pub struct CompactionDetector;

impl CompactionDetector {
    /// Scan a screen buffer's text content for compaction patterns.
    /// Returns true if a compaction event was detected.
    pub fn scan(text: &str) -> bool {
        let lower = text.to_lowercase();
        COMPACTION_PATTERNS.iter().any(|pat| lower.contains(pat))
    }

    /// Check a pane's screen output for compaction, respecting debounce.
    /// Returns a MonitorAction if compaction was detected and debounce allows.
    pub fn check_pane(
        state: &mut PaneContextState,
        screen_text: &str,
        window_tokens: usize,
    ) -> Option<MonitorAction> {
        // Debounce: don't scan too frequently
        if let Some(last) = state.last_compaction_scan_at {
            if last.elapsed().as_secs() < COMPACTION_DEBOUNCE_SECS {
                return None;
            }
        }
        state.last_compaction_scan_at = Some(Instant::now());

        if Self::scan(screen_text) {
            state.compaction_count += 1;
            // Reset estimated tokens to 10% of window (agent has freed context)
            state.estimated_tokens = window_tokens / 10;
            Some(MonitorAction::CompactionDetected {
                pane_id: state.pane_id,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_lifecycle::types::AgentKind;

    #[test]
    fn test_scan_detects_known_patterns() {
        assert!(CompactionDetector::scan(
            "System: compressing prior messages in this conversation"
        ));
        assert!(CompactionDetector::scan("auto-compact triggered"));
        assert!(CompactionDetector::scan("Context compressed successfully"));
        assert!(CompactionDetector::scan(
            "Compacted conversation to save space"
        ));
        assert!(CompactionDetector::scan(
            "Summarizing conversation history..."
        ));
    }

    #[test]
    fn test_scan_no_false_positives() {
        assert!(!CompactionDetector::scan("Hello, how can I help you?"));
        assert!(!CompactionDetector::scan("git commit -m 'compress files'"));
        assert!(!CompactionDetector::scan(
            "The code looks good, let me review it"
        ));
        assert!(!CompactionDetector::scan(""));
    }

    #[test]
    fn test_scan_case_insensitive() {
        assert!(CompactionDetector::scan("COMPRESSING PRIOR MESSAGES"));
        assert!(CompactionDetector::scan("Auto-Compact triggered"));
    }

    #[test]
    fn test_check_pane_resets_tokens() {
        let mut state = PaneContextState::new(1, AgentKind::ClaudeCode);
        state.estimated_tokens = 150_000;

        let action = CompactionDetector::check_pane(
            &mut state,
            "System: compressing prior messages",
            200_000,
        );

        assert!(matches!(
            action,
            Some(MonitorAction::CompactionDetected { pane_id: 1 })
        ));
        assert_eq!(state.estimated_tokens, 20_000); // 10% of 200k
        assert_eq!(state.compaction_count, 1);
    }

    #[test]
    fn test_check_pane_debounce() {
        let mut state = PaneContextState::new(1, AgentKind::ClaudeCode);

        // First scan — detects
        let action =
            CompactionDetector::check_pane(&mut state, "compressing prior messages", 200_000);
        assert!(action.is_some());

        // Immediate second scan — debounced
        let action =
            CompactionDetector::check_pane(&mut state, "compressing prior messages", 200_000);
        assert!(action.is_none());
    }
}
