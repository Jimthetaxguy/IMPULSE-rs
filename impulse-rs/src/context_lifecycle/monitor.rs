//! Context window monitor — estimates token usage and detects threshold crossings.

use std::collections::HashMap;

use super::types::{ContextTier, MonitorAction, PaneContextState};

/// Monitors context window usage across agent panes and triggers refresh actions.
pub struct ContextWindowMonitor {
    /// Per-pane context state, keyed by pane ID.
    pub pane_states: HashMap<usize, PaneContextState>,
    /// Total context window size in tokens (configurable).
    pub window_tokens: usize,
}

/// Token estimation multiplier: PTY output is ~40% of total context.
/// System prompts, user messages, and tool results are not visible in output.
const OUTPUT_TO_CONTEXT_MULTIPLIER: f64 = 2.5;

/// Chars per token (established pattern from token_tracker module).
const CHARS_PER_TOKEN: f64 = 4.0;

impl ContextWindowMonitor {
    pub fn new(window_tokens: usize) -> Self {
        Self {
            pane_states: HashMap::new(),
            window_tokens,
        }
    }

    /// Estimate tokens from output bytes.
    /// Formula: (output_bytes * 2.5 / 4.0) — accounts for hidden context.
    pub fn estimate_tokens(output_bytes: u64) -> usize {
        ((output_bytes as f64) * OUTPUT_TO_CONTEXT_MULTIPLIER / CHARS_PER_TOKEN) as usize
    }

    /// Check a pane's output bytes and return an action if a threshold was crossed.
    pub fn check_pane(
        &mut self,
        pane_id: usize,
        current_output_bytes: u64,
    ) -> Option<MonitorAction> {
        let state = self.pane_states.get_mut(&pane_id)?;

        // Skip if output hasn't changed
        if current_output_bytes == state.output_bytes_at_last_check {
            return None;
        }
        state.output_bytes_at_last_check = current_output_bytes;
        // Estimate usage from output produced since the last compaction baseline
        // (0 until the first compaction), so a freed context window is reflected
        // as a drop rather than climbing forever off the cumulative byte total.
        let bytes_since_baseline = current_output_bytes.saturating_sub(state.output_bytes_baseline);
        state.estimated_tokens = Self::estimate_tokens(bytes_since_baseline);

        let pct = if self.window_tokens > 0 {
            (state.estimated_tokens as f64 / self.window_tokens as f64 * 100.0) as u8
        } else {
            0
        };

        let new_tier = match pct {
            0..=44 => ContextTier::None,
            45..=59 => ContextTier::Essential,
            60..=79 => ContextTier::Critical,
            _ => ContextTier::Minimal,
        };

        // Only fire if we're crossing UP to a new tier, not re-firing same tier
        if new_tier > state.last_threshold && new_tier != ContextTier::None {
            // Check debounce
            if !state.can_inject() {
                return None;
            }
            state.last_threshold = new_tier;
            Some(MonitorAction::RefreshContext {
                pane_id,
                tier: new_tier,
            })
        } else {
            None
        }
    }

    /// Remove state for panes that no longer exist.
    pub fn cleanup_dead_panes(&mut self, alive_pane_ids: &[usize]) {
        self.pane_states.retain(|id, _| alive_pane_ids.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_lifecycle::types::AgentKind;

    #[test]
    fn test_estimate_tokens_zero() {
        assert_eq!(ContextWindowMonitor::estimate_tokens(0), 0);
    }

    #[test]
    fn test_estimate_tokens_small() {
        // 1000 bytes * 2.5 / 4.0 = 625 tokens
        assert_eq!(ContextWindowMonitor::estimate_tokens(1000), 625);
    }

    #[test]
    fn test_estimate_tokens_large() {
        // 800_000 bytes * 2.5 / 4.0 = 500_000 tokens
        assert_eq!(ContextWindowMonitor::estimate_tokens(800_000), 500_000);
    }

    #[test]
    fn test_threshold_crossing_fires_once() {
        let mut monitor = ContextWindowMonitor::new(200_000);
        let pane_id = 1;
        monitor.pane_states.insert(
            pane_id,
            PaneContextState::new(pane_id, AgentKind::ClaudeCode),
        );

        // Below 45% — no action
        // 200k tokens = 800k chars estimated. 45% = 90k tokens = 144k bytes (144000 * 2.5 / 4 = 90k)
        let action = monitor.check_pane(pane_id, 100_000);
        assert!(action.is_none());

        // At 45% — fire Essential
        // 90k tokens needs 144k bytes: 90000 * 4 / 2.5 = 144000
        let action = monitor.check_pane(pane_id, 144_000);
        assert!(matches!(
            action,
            Some(MonitorAction::RefreshContext {
                tier: ContextTier::Essential,
                ..
            })
        ));

        // Same bytes again — no double-fire
        let action = monitor.check_pane(pane_id, 144_000);
        assert!(action.is_none());
    }

    #[test]
    fn test_usage_drops_after_compaction_rebaseline() {
        let mut monitor = ContextWindowMonitor::new(200_000);
        let pane_id = 1;
        monitor.pane_states.insert(
            pane_id,
            PaneContextState::new(pane_id, AgentKind::ClaudeCode),
        );

        // Climb to 45% (Essential).
        assert!(monitor.check_pane(pane_id, 144_000).is_some());

        // Simulate what CompactionDetector does on a compaction event:
        // re-baseline byte accounting to "now" and re-arm the threshold ladder.
        {
            let state = monitor.pane_states.get_mut(&pane_id).unwrap();
            state.output_bytes_baseline = state.output_bytes_at_last_check;
            state.last_threshold = ContextTier::None;
        }

        // Cumulative bytes keep growing, but usage is now measured from the
        // baseline, so 56k new bytes => ~17% => below threshold => no action.
        let action = monitor.check_pane(pane_id, 200_000);
        assert!(
            action.is_none(),
            "post-compaction usage should drop below threshold, got {action:?}"
        );

        // After 144k more bytes since the baseline, we cross 45% again and the
        // re-armed ladder fires a fresh Essential refresh.
        let action = monitor.check_pane(pane_id, 144_000 + 144_000);
        assert!(matches!(
            action,
            Some(MonitorAction::RefreshContext {
                tier: ContextTier::Essential,
                ..
            })
        ));
    }

    #[test]
    fn test_no_action_for_unchanged_bytes() {
        let mut monitor = ContextWindowMonitor::new(200_000);
        let pane_id = 1;
        monitor.pane_states.insert(
            pane_id,
            PaneContextState::new(pane_id, AgentKind::ClaudeCode),
        );

        let action = monitor.check_pane(pane_id, 50_000);
        assert!(action.is_none());

        // Same bytes — no action
        let action = monitor.check_pane(pane_id, 50_000);
        assert!(action.is_none());
    }

    #[test]
    fn test_cleanup_dead_panes() {
        let mut monitor = ContextWindowMonitor::new(200_000);
        monitor
            .pane_states
            .insert(1, PaneContextState::new(1, AgentKind::ClaudeCode));
        monitor
            .pane_states
            .insert(2, PaneContextState::new(2, AgentKind::OpenCode));
        monitor
            .pane_states
            .insert(3, PaneContextState::new(3, AgentKind::GenericShell));

        monitor.cleanup_dead_panes(&[1, 3]);
        assert!(monitor.pane_states.contains_key(&1));
        assert!(!monitor.pane_states.contains_key(&2));
        assert!(monitor.pane_states.contains_key(&3));
    }

    #[test]
    fn test_multi_pane_independent_tracking() {
        let mut monitor = ContextWindowMonitor::new(200_000);
        monitor
            .pane_states
            .insert(1, PaneContextState::new(1, AgentKind::ClaudeCode));
        monitor
            .pane_states
            .insert(2, PaneContextState::new(2, AgentKind::OpenCode));

        // Pane 1 below threshold
        let action1 = monitor.check_pane(1, 50_000);
        assert!(action1.is_none());

        // Pane 2 at 45% — fires independently
        let action2 = monitor.check_pane(2, 144_000);
        assert!(matches!(
            action2,
            Some(MonitorAction::RefreshContext {
                pane_id: 2,
                tier: ContextTier::Essential,
                ..
            })
        ));
    }
}
