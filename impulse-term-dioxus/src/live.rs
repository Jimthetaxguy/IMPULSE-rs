//! Live grid with per-row version counters for damage tracking.
//!
//! # The damage-tracking pattern
//!
//! Dioxus components auto-memoize on prop `PartialEq`. If a child component's
//! props are unchanged between renders, Dioxus skips its subtree entirely.
//! For a terminal grid this is exactly what we want — when only row 5
//! changes, rows 0-4 and 6-N should not re-render.
//!
//! Naive approach: pass `Vec<CellRun>` directly as the prop. Equality is
//! O(n) per row (each `CellRun::PartialEq` walks fg/bg/attrs/text). For an
//! 80-col row that's ~80 comparisons per equality check, times 60 rows per
//! frame = 4,800 comparisons per render-skip decision.
//!
//! Better: pair the runs with a monotonic `version` counter. The component
//! compares `version` only — O(1). The version bumps **only** when the row
//! actually changes. This collapses the equality check to a single integer
//! compare per row.
//!
//! `LiveGrid` is the coordinator: it ingests a `GridSnapshot` (built from a
//! `vt100::Screen`), compares each row's runs against the previous snapshot,
//! and bumps version counters only for changed rows. Returns the set of
//! changed row indices so the caller (the reader thread or a coroutine
//! task) can choose to skip a Dioxus signal write entirely if nothing
//! changed.
//!
//! # Why a `&mut` API instead of `Signal` here
//!
//! `LiveGrid` is the toolkit-neutral diff engine. The Dioxus-specific
//! plumbing — wrapping each `RowSnapshot` in a `Signal` so changes propagate
//! into the component tree — lives at the component layer (L164). Splitting
//! diff (here) from reactive plumbing (component) means the diff logic is
//! testable without a Dioxus runtime.

use impulse_term_core::{CellRun, GridSnapshot};

/// A single row's worth of runs plus a monotonic version counter.
///
/// The version bumps each time the row's runs actually change. Components
/// compare versions for memoization rather than walking the runs vector,
/// turning per-row equality from O(n) into O(1).
#[derive(Debug, Clone, PartialEq)]
pub struct RowSnapshot {
    pub runs: Vec<CellRun>,
    pub version: u64,
}

impl RowSnapshot {
    pub fn empty(_cols: u16) -> Self {
        Self {
            runs: Vec::new(),
            version: 0,
        }
    }
}

impl Default for RowSnapshot {
    fn default() -> Self {
        Self::empty(0)
    }
}

/// Grid coordinator that diffs incoming snapshots and tracks which rows
/// changed.
///
/// Owns the previous snapshot so it can detect change. Callers feed it
/// `GridSnapshot`s (built from `vt100::Screen` via `GridSnapshot::from_screen`)
/// and receive the set of row indices that actually changed since the last
/// update — typically a tiny subset of the full grid for streaming output.
#[derive(Debug, Clone)]
pub struct LiveGrid {
    pub rows: u16,
    pub cols: u16,
    pub row_snapshots: Vec<RowSnapshot>,
}

/// Result of `LiveGrid::update_from_snapshot`. Reports which rows changed
/// so the caller can selectively notify the UI layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateReport {
    pub changed_rows: Vec<u16>,
    /// True if `rows` or `cols` changed (forces full repaint at the
    /// component level — row count differs so Dioxus's per-row memoization
    /// doesn't apply uniformly).
    pub resized: bool,
}

impl UpdateReport {
    pub fn is_clean(&self) -> bool {
        !self.resized && self.changed_rows.is_empty()
    }
}

impl LiveGrid {
    /// Create an empty live grid sized to `rows × cols`.
    ///
    /// All row versions start at 0. The first `update_from_snapshot` call
    /// will bump versions for any rows with non-empty runs.
    pub fn new(rows: u16, cols: u16) -> Self {
        let row_snapshots = (0..rows).map(|_| RowSnapshot::empty(cols)).collect();
        Self {
            rows,
            cols,
            row_snapshots,
        }
    }

    /// Diff `incoming` against the current state and bump version counters
    /// for rows that actually changed.
    ///
    /// Returns the set of changed row indices (sorted ascending). If
    /// `incoming.rows` or `incoming.cols` differs from the current grid
    /// dimensions, the grid resizes and `UpdateReport.resized` is set —
    /// in that case all rows are considered "changed" because the
    /// component tree's row count differs.
    pub fn update_from_snapshot(&mut self, incoming: &GridSnapshot) -> UpdateReport {
        let resized = incoming.rows != self.rows || incoming.cols != self.cols;

        if resized {
            self.rows = incoming.rows;
            self.cols = incoming.cols;
            // Reuse existing version counters where possible to avoid
            // resetting components when only the column count changed.
            let mut new_rows: Vec<RowSnapshot> = Vec::with_capacity(incoming.rows as usize);
            for (i, runs) in incoming.row_runs.iter().enumerate() {
                let prev_version = self.row_snapshots.get(i).map(|r| r.version).unwrap_or(0);
                new_rows.push(RowSnapshot {
                    runs: runs.clone(),
                    version: prev_version + 1,
                });
            }
            self.row_snapshots = new_rows;
            return UpdateReport {
                changed_rows: (0..incoming.rows).collect(),
                resized: true,
            };
        }

        let mut changed_rows = Vec::new();
        for (idx, runs) in incoming.row_runs.iter().enumerate() {
            let row_idx = idx as u16;
            let current = &self.row_snapshots[idx];
            if current.runs != *runs {
                self.row_snapshots[idx] = RowSnapshot {
                    runs: runs.clone(),
                    version: current.version + 1,
                };
                changed_rows.push(row_idx);
            }
        }

        UpdateReport {
            changed_rows,
            resized: false,
        }
    }

    /// Borrow a row snapshot by index. Returns `None` for out-of-range
    /// indices (which means the caller's component-tree row count is out
    /// of sync — typically caused by a missed resize event).
    pub fn row(&self, idx: usize) -> Option<&RowSnapshot> {
        self.row_snapshots.get(idx)
    }

    /// Total version count across all rows. A coarse "anything changed"
    /// indicator useful for top-level `Signal<u64>` damage triggers.
    pub fn total_version(&self) -> u64 {
        self.row_snapshots.iter().map(|r| r.version).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_from_bytes(rows: u16, cols: u16, bytes: &[u8]) -> GridSnapshot {
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(bytes);
        GridSnapshot::from_screen(parser.screen())
    }

    #[test]
    fn test_new_live_grid_has_zero_versions() {
        let grid = LiveGrid::new(3, 5);
        assert_eq!(grid.rows, 3);
        assert_eq!(grid.cols, 5);
        assert_eq!(grid.row_snapshots.len(), 3);
        for row in &grid.row_snapshots {
            assert_eq!(row.version, 0);
            assert!(row.runs.is_empty());
        }
        assert_eq!(grid.total_version(), 0);
    }

    #[test]
    fn test_first_update_bumps_versions_for_nonempty_rows() {
        let mut grid = LiveGrid::new(2, 5);
        let incoming = snapshot_from_bytes(2, 5, b"hi");
        let report = grid.update_from_snapshot(&incoming);

        // Both rows are "non-empty" because vt100 fills with spaces.
        // The empty initial state has zero runs; the incoming snapshot has
        // one run per row of spaces (or "hi   " on row 0). Both differ.
        assert_eq!(report.changed_rows, vec![0, 1]);
        assert!(!report.resized);
        assert_eq!(grid.row_snapshots[0].version, 1);
        assert_eq!(grid.row_snapshots[1].version, 1);
    }

    #[test]
    fn test_identical_update_does_not_bump_versions() {
        let mut grid = LiveGrid::new(2, 5);
        let snap = snapshot_from_bytes(2, 5, b"hi");
        grid.update_from_snapshot(&snap);
        let v_before = grid.total_version();

        // Re-apply the same snapshot.
        let report = grid.update_from_snapshot(&snap);
        assert!(report.is_clean(), "expected clean report, got {report:?}");
        assert_eq!(grid.total_version(), v_before);
    }

    #[test]
    fn test_change_in_one_row_bumps_only_that_row() {
        let mut grid = LiveGrid::new(3, 10);
        let s1 = snapshot_from_bytes(3, 10, b"row0\r\nrow1\r\nrow2");
        grid.update_from_snapshot(&s1);
        let v0_before = grid.row_snapshots[0].version;
        let v1_before = grid.row_snapshots[1].version;
        let v2_before = grid.row_snapshots[2].version;

        // Change only row 1 (the second row).
        let s2 = snapshot_from_bytes(3, 10, b"row0\r\nROW1\r\nrow2");
        let report = grid.update_from_snapshot(&s2);

        assert_eq!(report.changed_rows, vec![1]);
        assert_eq!(grid.row_snapshots[0].version, v0_before);
        assert_eq!(grid.row_snapshots[1].version, v1_before + 1);
        assert_eq!(grid.row_snapshots[2].version, v2_before);
    }

    #[test]
    fn test_streaming_appends_only_change_last_row() {
        // Simulate appending characters to the last row (typical streaming).
        let mut grid = LiveGrid::new(3, 20);
        let mut parser = vt100::Parser::new(3, 20, 0);

        parser.process(b"first\r\nsecond\r\nthi");
        let s1 = GridSnapshot::from_screen(parser.screen());
        grid.update_from_snapshot(&s1);

        parser.process(b"rd");
        let s2 = GridSnapshot::from_screen(parser.screen());
        let report = grid.update_from_snapshot(&s2);

        assert_eq!(
            report.changed_rows,
            vec![2],
            "streaming append should only change row 2, got {report:?}"
        );
    }

    #[test]
    fn test_resize_reports_all_rows_changed() {
        let mut grid = LiveGrid::new(2, 5);
        grid.update_from_snapshot(&snapshot_from_bytes(2, 5, b"hi"));

        let bigger = snapshot_from_bytes(4, 10, b"bigger");
        let report = grid.update_from_snapshot(&bigger);

        assert!(report.resized);
        assert_eq!(report.changed_rows, vec![0, 1, 2, 3]);
        assert_eq!(grid.rows, 4);
        assert_eq!(grid.cols, 10);
        assert_eq!(grid.row_snapshots.len(), 4);
    }

    #[test]
    fn test_resize_preserves_existing_versions_with_bump() {
        let mut grid = LiveGrid::new(2, 5);
        grid.update_from_snapshot(&snapshot_from_bytes(2, 5, b"x"));
        // Both rows version=1 after first update.

        let resized = snapshot_from_bytes(3, 5, b"y");
        grid.update_from_snapshot(&resized);

        // Existing rows 0,1 had version 1 → now version 2 (bump).
        // New row 2 starts at version 1 (0 + 1).
        assert_eq!(grid.row_snapshots[0].version, 2);
        assert_eq!(grid.row_snapshots[1].version, 2);
        assert_eq!(grid.row_snapshots[2].version, 1);
    }

    #[test]
    fn test_row_accessor_returns_none_for_out_of_range() {
        let grid = LiveGrid::new(3, 5);
        assert!(grid.row(0).is_some());
        assert!(grid.row(2).is_some());
        assert!(grid.row(3).is_none());
        assert!(grid.row(usize::MAX).is_none());
    }

    #[test]
    fn test_total_version_sums_rows() {
        let mut grid = LiveGrid::new(2, 5);
        assert_eq!(grid.total_version(), 0);
        grid.update_from_snapshot(&snapshot_from_bytes(2, 5, b"x"));
        // Both rows version=1 → total=2.
        assert_eq!(grid.total_version(), 2);
        grid.update_from_snapshot(&snapshot_from_bytes(2, 5, b"xy"));
        // Row 0 changes again (+1), row 1 unchanged → total=3.
        assert_eq!(grid.total_version(), 3);
    }

    #[test]
    fn test_update_report_is_clean_when_nothing_changes() {
        let report = UpdateReport {
            changed_rows: vec![],
            resized: false,
        };
        assert!(report.is_clean());

        let dirty = UpdateReport {
            changed_rows: vec![1],
            resized: false,
        };
        assert!(!dirty.is_clean());

        let resized = UpdateReport {
            changed_rows: vec![],
            resized: true,
        };
        assert!(!resized.is_clean());
    }
}
