//! Signal bus — collects, debounces, and routes GUI signals to visual surfaces.
//!
//! Fed from the context tick loop (every 3s) and drained each frame to route
//! signals to toasts, tab badges, activity feed, and status bar counters.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Signal types
// ---------------------------------------------------------------------------

/// How urgently a signal should be surfaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalUrgency {
    /// Badges + activity feed only — no toast.
    #[allow(dead_code)]
    Ambient,
    /// Toast notification (standard duration).
    Important,
    /// Toast notification (extended duration, red).
    Urgent,
}

/// What kind of signal was detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalKind {
    /// Context window crossed a threshold (60% or 80%).
    ContextThreshold { pct: u8 },
    /// Agent encountered an error.
    ErrorEncountered,
    /// Agent completed a task.
    TaskCompleted,
    /// Context compaction was detected.
    CompactionDetected,
    /// Two panes are editing the same file.
    FileConflict { path: String, other_tab: String },
}

/// A signal emitted from context observation.
pub struct GuiSignal {
    pub kind: SignalKind,
    pub urgency: SignalUrgency,
    pub tab_id: Option<u64>,
    pub message: String,
    pub created_at: Instant,
}

// ---------------------------------------------------------------------------
// Tab badge state
// ---------------------------------------------------------------------------

/// Visual badge state for a single tab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabBadge {
    pub has_error: bool,
    pub has_conflict: bool,
    pub has_compaction: bool,
    pub has_task_complete: bool,
}

impl TabBadge {
    #[allow(dead_code)]
    fn is_empty(&self) -> bool {
        !self.has_error && !self.has_conflict && !self.has_compaction && !self.has_task_complete
    }
}

// ---------------------------------------------------------------------------
// Summary for status bar
// ---------------------------------------------------------------------------

/// Aggregate signal counts for the status bar.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignalSummary {
    pub unread_errors: usize,
    pub active_conflicts: usize,
    pub tasks_completed: usize,
}

// ---------------------------------------------------------------------------
// Debounce windows (in seconds)
// ---------------------------------------------------------------------------

fn debounce_window(kind: &SignalKind) -> Duration {
    match kind {
        // Already debounced upstream in ContextBridge (60s intervals).
        SignalKind::ContextThreshold { .. } => Duration::from_secs(0),
        // Avoid spam from cascading errors.
        SignalKind::ErrorEncountered => Duration::from_secs(10),
        // Group rapid completions.
        SignalKind::TaskCompleted => Duration::from_secs(5),
        // One compaction event per minute max.
        SignalKind::CompactionDetected => Duration::from_secs(60),
        // Don't re-alert same conflict.
        SignalKind::FileConflict { .. } => Duration::from_secs(30),
    }
}

/// Build a debounce key combining signal kind + tab ID.
fn debounce_key(kind: &SignalKind, tab_id: Option<u64>) -> String {
    let tab_str = tab_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "global".to_string());

    match kind {
        SignalKind::ContextThreshold { pct } => format!("ctx_thresh:{}:{}", pct, tab_str),
        SignalKind::ErrorEncountered => format!("error:{}", tab_str),
        SignalKind::TaskCompleted => format!("task:{}", tab_str),
        SignalKind::CompactionDetected => format!("compact:{}", tab_str),
        SignalKind::FileConflict { path, .. } => format!("conflict:{}", path),
    }
}

// ---------------------------------------------------------------------------
// SignalBus
// ---------------------------------------------------------------------------

/// Collects, debounces, and routes signals to visual surfaces.
pub struct SignalBus {
    pending: Vec<GuiSignal>,
    tab_badges: BTreeMap<u64, TabBadge>,
    debounce: HashMap<String, Instant>,
    summary: SignalSummary,
}

impl SignalBus {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            tab_badges: BTreeMap::new(),
            debounce: HashMap::new(),
            summary: SignalSummary::default(),
        }
    }

    /// Emit a signal into the bus. Returns `true` if accepted (not debounced).
    pub fn emit(&mut self, signal: GuiSignal) -> bool {
        let key = debounce_key(&signal.kind, signal.tab_id);
        let window = debounce_window(&signal.kind);

        // Check debounce.
        if !window.is_zero() {
            if let Some(last) = self.debounce.get(&key) {
                if signal.created_at.duration_since(*last) < window {
                    return false;
                }
            }
        }

        // Record debounce timestamp.
        self.debounce.insert(key, signal.created_at);

        // Update tab badge.
        if let Some(tab_id) = signal.tab_id {
            let badge = self.tab_badges.entry(tab_id).or_default();
            match &signal.kind {
                SignalKind::ErrorEncountered => badge.has_error = true,
                SignalKind::TaskCompleted => badge.has_task_complete = true,
                SignalKind::CompactionDetected => badge.has_compaction = true,
                SignalKind::FileConflict { .. } => badge.has_conflict = true,
                SignalKind::ContextThreshold { .. } => {} // no badge, handled by tier icon
            }
        }

        // Update summary counters.
        match &signal.kind {
            SignalKind::ErrorEncountered => self.summary.unread_errors += 1,
            SignalKind::TaskCompleted => self.summary.tasks_completed += 1,
            SignalKind::FileConflict { .. } => self.summary.active_conflicts += 1,
            _ => {}
        }

        self.pending.push(signal);
        true
    }

    /// Drain all pending signals for routing to visual surfaces.
    pub fn drain(&mut self) -> Vec<GuiSignal> {
        std::mem::take(&mut self.pending)
    }

    /// Get the badge state for a specific tab.
    #[allow(dead_code)]
    pub fn tab_badge(&self, id: u64) -> Option<&TabBadge> {
        self.tab_badges.get(&id)
    }

    /// Clear badges when user clicks/acknowledges a tab.
    pub fn acknowledge_tab(&mut self, id: u64) {
        if let Some(badge) = self.tab_badges.get_mut(&id) {
            // Decrement summary counters for cleared badges.
            if badge.has_error {
                self.summary.unread_errors = self.summary.unread_errors.saturating_sub(1);
            }
            if badge.has_conflict {
                self.summary.active_conflicts = self.summary.active_conflicts.saturating_sub(1);
            }
            // Reset badge.
            *badge = TabBadge::default();
        }
    }

    /// Remove a tab's badge state on tab close.
    #[allow(dead_code)]
    pub fn remove_tab(&mut self, id: u64) {
        if let Some(badge) = self.tab_badges.remove(&id) {
            // Decrement summary counters.
            if badge.has_error {
                self.summary.unread_errors = self.summary.unread_errors.saturating_sub(1);
            }
            if badge.has_conflict {
                self.summary.active_conflicts = self.summary.active_conflicts.saturating_sub(1);
            }
        }
    }

    /// Aggregate summary for the status bar.
    pub fn summary(&self) -> &SignalSummary {
        &self.summary
    }

    /// All tab badges (for syncing to the terminals view).
    pub fn all_tab_badges(&self) -> &BTreeMap<u64, TabBadge> {
        &self.tab_badges
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_signal(kind: SignalKind, urgency: SignalUrgency, tab_id: Option<u64>) -> GuiSignal {
        GuiSignal {
            kind,
            urgency,
            tab_id,
            message: "test signal".into(),
            created_at: Instant::now(),
        }
    }

    #[test]
    fn test_new_bus_is_empty() {
        let mut bus = SignalBus::new();
        assert!(bus.drain().is_empty());
        assert_eq!(bus.summary().unread_errors, 0);
        assert_eq!(bus.summary().active_conflicts, 0);
        assert_eq!(bus.summary().tasks_completed, 0);
    }

    #[test]
    fn test_emit_and_drain() {
        let mut bus = SignalBus::new();
        let accepted = bus.emit(make_signal(
            SignalKind::ErrorEncountered,
            SignalUrgency::Important,
            Some(1),
        ));
        assert!(accepted);

        let drained = bus.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].message, "test signal");

        // Drain again is empty.
        assert!(bus.drain().is_empty());
    }

    #[test]
    fn test_debounce_suppresses_duplicate() {
        let mut bus = SignalBus::new();
        let now = Instant::now();

        let sig1 = GuiSignal {
            kind: SignalKind::ErrorEncountered,
            urgency: SignalUrgency::Important,
            tab_id: Some(1),
            message: "error 1".into(),
            created_at: now,
        };
        let sig2 = GuiSignal {
            kind: SignalKind::ErrorEncountered,
            urgency: SignalUrgency::Important,
            tab_id: Some(1),
            message: "error 2".into(),
            created_at: now + Duration::from_secs(1), // within 10s window
        };

        assert!(bus.emit(sig1));
        assert!(!bus.emit(sig2)); // suppressed

        assert_eq!(bus.drain().len(), 1);
    }

    #[test]
    fn test_debounce_allows_after_window() {
        let mut bus = SignalBus::new();
        let now = Instant::now();

        let sig1 = GuiSignal {
            kind: SignalKind::TaskCompleted,
            urgency: SignalUrgency::Important,
            tab_id: Some(1),
            message: "task 1".into(),
            created_at: now,
        };
        let sig2 = GuiSignal {
            kind: SignalKind::TaskCompleted,
            urgency: SignalUrgency::Important,
            tab_id: Some(1),
            message: "task 2".into(),
            created_at: now + Duration::from_secs(6), // past 5s window
        };

        assert!(bus.emit(sig1));
        assert!(bus.emit(sig2));

        assert_eq!(bus.drain().len(), 2);
    }

    #[test]
    fn test_context_threshold_no_debounce() {
        let mut bus = SignalBus::new();
        let now = Instant::now();

        let sig1 = GuiSignal {
            kind: SignalKind::ContextThreshold { pct: 60 },
            urgency: SignalUrgency::Important,
            tab_id: Some(1),
            message: "60%".into(),
            created_at: now,
        };
        let sig2 = GuiSignal {
            kind: SignalKind::ContextThreshold { pct: 60 },
            urgency: SignalUrgency::Important,
            tab_id: Some(1),
            message: "60% again".into(),
            created_at: now, // same instant — 0s debounce means always accepted
        };

        assert!(bus.emit(sig1));
        assert!(bus.emit(sig2));
        assert_eq!(bus.drain().len(), 2);
    }

    #[test]
    fn test_badge_accumulation() {
        let mut bus = SignalBus::new();

        bus.emit(make_signal(
            SignalKind::ErrorEncountered,
            SignalUrgency::Important,
            Some(1),
        ));
        bus.emit(make_signal(
            SignalKind::TaskCompleted,
            SignalUrgency::Important,
            Some(1),
        ));

        let badge = bus.tab_badge(1).unwrap();
        assert!(badge.has_error);
        assert!(badge.has_task_complete);
        assert!(!badge.has_conflict);
        assert!(!badge.has_compaction);
    }

    #[test]
    fn test_badge_acknowledgment_clears() {
        let mut bus = SignalBus::new();

        bus.emit(make_signal(
            SignalKind::ErrorEncountered,
            SignalUrgency::Important,
            Some(1),
        ));
        assert!(bus.tab_badge(1).unwrap().has_error);
        assert_eq!(bus.summary().unread_errors, 1);

        bus.acknowledge_tab(1);
        let badge = bus.tab_badge(1).unwrap();
        assert!(!badge.has_error);
        assert!(badge.is_empty());
        assert_eq!(bus.summary().unread_errors, 0);
    }

    #[test]
    fn test_remove_tab_cleanup() {
        let mut bus = SignalBus::new();

        bus.emit(make_signal(
            SignalKind::FileConflict {
                path: "src/main.rs".into(),
                other_tab: "Tab 2".into(),
            },
            SignalUrgency::Urgent,
            Some(1),
        ));
        assert_eq!(bus.summary().active_conflicts, 1);
        assert!(bus.tab_badge(1).is_some());

        bus.remove_tab(1);
        assert!(bus.tab_badge(1).is_none());
        assert_eq!(bus.summary().active_conflicts, 0);
    }

    #[test]
    fn test_summary_computation() {
        let mut bus = SignalBus::new();

        bus.emit(make_signal(
            SignalKind::ErrorEncountered,
            SignalUrgency::Important,
            Some(1),
        ));
        bus.emit(make_signal(
            SignalKind::ErrorEncountered,
            SignalUrgency::Important,
            Some(2),
        ));
        bus.emit(make_signal(
            SignalKind::TaskCompleted,
            SignalUrgency::Important,
            Some(3),
        ));

        let summary = bus.summary();
        assert_eq!(summary.unread_errors, 2);
        assert_eq!(summary.tasks_completed, 1);
        assert_eq!(summary.active_conflicts, 0);
    }

    #[test]
    fn test_different_tabs_not_debounced() {
        let mut bus = SignalBus::new();
        let now = Instant::now();

        let sig1 = GuiSignal {
            kind: SignalKind::ErrorEncountered,
            urgency: SignalUrgency::Important,
            tab_id: Some(1),
            message: "tab 1 error".into(),
            created_at: now,
        };
        let sig2 = GuiSignal {
            kind: SignalKind::ErrorEncountered,
            urgency: SignalUrgency::Important,
            tab_id: Some(2),
            message: "tab 2 error".into(),
            created_at: now,
        };

        assert!(bus.emit(sig1));
        assert!(bus.emit(sig2)); // different tab — not debounced
        assert_eq!(bus.drain().len(), 2);
    }

    #[test]
    fn test_compaction_badge() {
        let mut bus = SignalBus::new();
        bus.emit(make_signal(
            SignalKind::CompactionDetected,
            SignalUrgency::Important,
            Some(5),
        ));

        let badge = bus.tab_badge(5).unwrap();
        assert!(badge.has_compaction);
        assert!(!badge.has_error);
    }

    #[test]
    fn test_context_threshold_no_badge() {
        let mut bus = SignalBus::new();
        bus.emit(make_signal(
            SignalKind::ContextThreshold { pct: 60 },
            SignalUrgency::Important,
            Some(1),
        ));

        // Context threshold doesn't set any badge (handled by tier icon).
        let badge = bus.tab_badge(1);
        assert!(badge.is_none() || badge.unwrap().is_empty());
    }

    #[test]
    fn test_no_tab_id_signal() {
        let mut bus = SignalBus::new();
        let accepted = bus.emit(make_signal(
            SignalKind::ErrorEncountered,
            SignalUrgency::Important,
            None,
        ));
        assert!(accepted);
        assert_eq!(bus.drain().len(), 1);
        // No badge created for None tab.
        assert!(bus.all_tab_badges().is_empty());
    }

    #[test]
    fn test_file_conflict_debounce_by_path() {
        let mut bus = SignalBus::new();
        let now = Instant::now();

        let sig1 = GuiSignal {
            kind: SignalKind::FileConflict {
                path: "src/main.rs".into(),
                other_tab: "Tab 2".into(),
            },
            urgency: SignalUrgency::Urgent,
            tab_id: Some(1),
            message: "conflict".into(),
            created_at: now,
        };
        let sig2 = GuiSignal {
            kind: SignalKind::FileConflict {
                path: "src/main.rs".into(),
                other_tab: "Tab 3".into(),
            },
            urgency: SignalUrgency::Urgent,
            tab_id: Some(2),
            message: "same file conflict".into(),
            created_at: now + Duration::from_secs(1), // within 30s window
        };

        assert!(bus.emit(sig1));
        assert!(!bus.emit(sig2)); // same path — debounced
    }

    #[test]
    fn test_acknowledge_nonexistent_tab() {
        let mut bus = SignalBus::new();
        // Should not panic.
        bus.acknowledge_tab(999);
        assert!(bus.all_tab_badges().is_empty());
    }

    #[test]
    fn test_badge_is_empty() {
        let badge = TabBadge::default();
        assert!(badge.is_empty());

        let badge = TabBadge {
            has_error: true,
            ..Default::default()
        };
        assert!(!badge.is_empty());
    }
}
