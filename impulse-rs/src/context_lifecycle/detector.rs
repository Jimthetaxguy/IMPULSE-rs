//! Compaction detector — pattern-matches PTY output for context compaction events.

use std::time::Instant;

use super::types::{ContextTier, MonitorAction, PaneContextState, COMPACTION_DEBOUNCE_SECS};

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
        !Self::compaction_lines(text).is_empty()
    }

    /// The lines of `text` that announce a compaction. Claude Code's status
    /// line counting down to one ("Context left until auto-compact: 9%")
    /// announces nothing yet.
    fn compaction_lines(text: &str) -> Vec<String> {
        text.lines()
            .map(str::trim)
            .filter(|line| {
                let lower = line.to_lowercase();
                !lower.contains("until auto-compact")
                    && COMPACTION_PATTERNS.iter().any(|pat| lower.contains(pat))
            })
            .map(str::to_string)
            .collect()
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

        // A compaction line still on screen from the last scan is the same
        // compaction seen again; it used to count again on every scan, each
        // time resetting the estimate and injecting context.
        let lines = Self::compaction_lines(screen_text);
        let new_compaction = lines
            .iter()
            .any(|line| !state.last_compaction_lines.contains(line));
        state.last_compaction_lines = lines;

        if new_compaction {
            state.compaction_count += 1;
            // Reset estimated tokens to 10% of window (agent has freed context)
            state.estimated_tokens = window_tokens / 10;
            // Re-baseline byte accounting to "now" so the monitor measures
            // post-compaction growth from ~empty instead of the cumulative total.
            state.output_bytes_baseline = state.output_bytes_at_last_check;
            // Re-arm the threshold ladder. Without this, a pane that already
            // climbed to a high tier (e.g. Minimal) keeps `last_threshold` at
            // that tier forever, so the monitor's `new_tier > last_threshold`
            // guard can never fire again — the post-compaction refresh cycle
            // would be permanently dead for that pane.
            state.last_threshold = ContextTier::None;
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
    fn test_check_pane_resets_last_threshold_on_compaction() {
        // Regression: a pane that already climbed to a high tier must have its
        // threshold ladder re-armed on compaction, otherwise the monitor's
        // `new_tier > last_threshold` guard can never fire again.
        let mut state = PaneContextState::new(1, AgentKind::ClaudeCode);
        state.last_threshold = ContextTier::Minimal;
        state.estimated_tokens = 180_000;
        state.output_bytes_at_last_check = 288_000;

        let action = CompactionDetector::check_pane(
            &mut state,
            "System: compressing prior messages",
            200_000,
        );

        assert!(matches!(
            action,
            Some(MonitorAction::CompactionDetected { pane_id: 1 })
        ));
        assert_eq!(
            state.last_threshold,
            ContextTier::None,
            "last_threshold must reset to None so future tier crossings re-fire"
        );
        assert_eq!(
            state.output_bytes_baseline, 288_000,
            "byte baseline must move to current so usage is measured post-compaction"
        );
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

    /// Scans `text` as if the debounce had passed.
    fn scan_after_debounce(state: &mut PaneContextState, text: &str) -> bool {
        state.last_compaction_scan_at = None;
        CompactionDetector::check_pane(state, text, 200_000).is_some()
    }

    /// Review finding: one compaction message left on screen counted again
    /// on every scan, resetting the token estimate and injecting each time.
    #[test]
    fn test_check_pane_counts_each_compaction_line_once() {
        let mut state = PaneContextState::new(1, AgentKind::ClaudeCode);
        let screen = "prompt\nCompacted conversation to save space\n";
        assert!(scan_after_debounce(&mut state, screen));
        assert!(!scan_after_debounce(&mut state, screen));
        assert!(!scan_after_debounce(&mut state, screen));
        assert_eq!(state.compaction_count, 1);

        // A second compaction prints a line that was not there before.
        let later = "Compacted conversation to save space\nmore work\nauto-compact triggered\n";
        assert!(scan_after_debounce(&mut state, later));
        assert_eq!(state.compaction_count, 2);
    }

    /// The status line counting down to auto-compaction is not a compaction.
    #[test]
    fn test_scan_ignores_the_countdown_to_auto_compact() {
        assert!(!CompactionDetector::scan(
            "Context left until auto-compact: 9%"
        ));
        let mut state = PaneContextState::new(1, AgentKind::ClaudeCode);
        for _ in 0..3 {
            assert!(!scan_after_debounce(
                &mut state,
                "Context left until auto-compact: 9%"
            ));
        }
        assert_eq!(state.compaction_count, 0);
    }
}
