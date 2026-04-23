//! Toolkit-neutral grid snapshot types.
//!
//! Renderers (egui, Dioxus, ratatui) consume `GridSnapshot` and map
//! `TermColor` to whatever native color type they use. The run-grouping
//! algorithm is identical across renderers — consecutive cells with matching
//! attributes are coalesced into a single `CellRun`, reducing per-cell
//! overhead by ~10–40× on typical terminal output.
//!
//! # Why one snapshot type, not a renderer trait
//!
//! A snapshot is cheap to build (~few KB for an 80×40 grid), trivial to send
//! across thread boundaries, and lets each renderer iterate at its own pace
//! without holding a parser lock. A renderer-trait approach would couple the
//! parser lifetime to the GUI frame, which is exactly the immediate-mode
//! coupling we're moving away from.

use serde::{Deserialize, Serialize};

/// Toolkit-neutral terminal color.
///
/// Mirrors `vt100::Color` (the parser's color enum) but does not leak the
/// vt100 type into the public API of this crate. Renderers map this to their
/// native color type (egui::Color32, CSS color string, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TermColor {
    /// Use the renderer's default fg/bg color.
    Default,
    /// Indexed color: 0–7 standard, 8–15 bright, 16–231 6×6×6 cube, 232–255 grayscale.
    Indexed(u8),
    /// 24-bit truecolor.
    Rgb(u8, u8, u8),
}

impl From<vt100::Color> for TermColor {
    fn from(c: vt100::Color) -> Self {
        match c {
            vt100::Color::Default => Self::Default,
            vt100::Color::Idx(i) => Self::Indexed(i),
            vt100::Color::Rgb(r, g, b) => Self::Rgb(r, g, b),
        }
    }
}

/// Per-cell visual attributes (excluding color, which lives on `CellRun`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CellAttrs {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
    pub strikethrough: bool,
}

/// A run of consecutive cells with identical visual attributes.
///
/// Renderers paint one background rect + one text draw per run, instead of
/// one per cell. For typical output this is the difference between ~4,800
/// draw calls per frame and ~100–300.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellRun {
    pub text: String,
    pub col_start: usize,
    pub col_end: usize,
    pub fg: TermColor,
    pub bg: TermColor,
    pub attrs: CellAttrs,
}

/// A snapshot of the terminal screen organized as runs per row.
///
/// Cheap to build, trivial to clone across threads, and the unit each
/// renderer consumes. The reader thread builds these on demand; the GUI
/// thread reads them into per-row signals (Dioxus) or paints them directly
/// (egui). Either way, the parser is not held across the GUI frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridSnapshot {
    pub rows: u16,
    pub cols: u16,
    /// One `Vec<CellRun>` per row, top to bottom.
    pub row_runs: Vec<Vec<CellRun>>,
}

impl GridSnapshot {
    /// Build a snapshot from a vt100 screen.
    ///
    /// Bold + foreground-index 0–7 maps to bright (8–15) by convention — this
    /// is what xterm and every modern terminal does. Inverse video is
    /// applied here (fg/bg swapped) so the renderer doesn't need to know
    /// about it.
    pub fn from_screen(screen: &vt100::Screen) -> Self {
        let (rows, cols) = screen.size();
        let mut row_runs: Vec<Vec<CellRun>> = Vec::with_capacity(rows as usize);

        for row in 0..rows {
            let mut runs: Vec<CellRun> = Vec::new();

            for col in 0..cols {
                let cell = screen.cell(row, col);
                let (text, fg_raw, bg_raw, attrs) = match cell {
                    Some(cell) => {
                        // vt100 returns "" for unwritten cells; canonicalize to " "
                        // so renderers don't have to special-case empty runs.
                        let ch = cell.contents();
                        let ch = if ch.is_empty() { " ".to_string() } else { ch };
                        let mut fg = TermColor::from(cell.fgcolor());
                        let mut bg = TermColor::from(cell.bgcolor());

                        if cell.inverse() {
                            std::mem::swap(&mut fg, &mut bg);
                        }

                        // Bold + standard fg color → bright variant.
                        if cell.bold() {
                            if let TermColor::Indexed(i) = fg {
                                if i < 8 {
                                    fg = TermColor::Indexed(i + 8);
                                }
                            }
                        }

                        let attrs = CellAttrs {
                            bold: cell.bold(),
                            italic: cell.italic(),
                            underline: cell.underline(),
                            reverse: cell.inverse(),
                            strikethrough: false, // vt100 0.15 doesn't expose this
                        };
                        (ch, fg, bg, attrs)
                    }
                    None => (
                        " ".to_string(),
                        TermColor::Default,
                        TermColor::Default,
                        CellAttrs::default(),
                    ),
                };

                let extended = if let Some(last) = runs.last_mut() {
                    last.fg == fg_raw
                        && last.bg == bg_raw
                        && last.attrs == attrs
                        && last.col_end == col as usize
                        && {
                            last.text.push_str(&text);
                            last.col_end = col as usize + 1;
                            true
                        }
                } else {
                    false
                };

                if !extended {
                    runs.push(CellRun {
                        text,
                        col_start: col as usize,
                        col_end: col as usize + 1,
                        fg: fg_raw,
                        bg: bg_raw,
                        attrs,
                    });
                }
            }

            row_runs.push(runs);
        }

        Self {
            rows,
            cols,
            row_runs,
        }
    }

    /// Total visible cell count (`rows * cols`).
    pub fn cell_count(&self) -> usize {
        self.rows as usize * self.cols as usize
    }

    /// Total run count across all rows. A useful efficiency metric:
    /// runs ≪ cells means the renderer has less to do.
    pub fn run_count(&self) -> usize {
        self.row_runs.iter().map(|r| r.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_term_color_from_vt100_default() {
        assert_eq!(TermColor::from(vt100::Color::Default), TermColor::Default);
    }

    #[test]
    fn test_term_color_from_vt100_idx() {
        assert_eq!(TermColor::from(vt100::Color::Idx(7)), TermColor::Indexed(7));
    }

    #[test]
    fn test_term_color_from_vt100_rgb() {
        assert_eq!(
            TermColor::from(vt100::Color::Rgb(0xAA, 0xBB, 0xCC)),
            TermColor::Rgb(0xAA, 0xBB, 0xCC)
        );
    }

    #[test]
    fn test_term_color_serde_round_trip() {
        for color in &[
            TermColor::Default,
            TermColor::Indexed(0),
            TermColor::Indexed(255),
            TermColor::Rgb(0x12, 0x34, 0x56),
        ] {
            let json = serde_json::to_string(color).unwrap();
            let recovered: TermColor = serde_json::from_str(&json).unwrap();
            assert_eq!(*color, recovered);
        }
    }

    #[test]
    fn test_cell_attrs_default_all_false() {
        let a = CellAttrs::default();
        assert!(!a.bold);
        assert!(!a.italic);
        assert!(!a.underline);
        assert!(!a.reverse);
        assert!(!a.strikethrough);
    }

    #[test]
    fn test_cell_attrs_serde_round_trip() {
        let attrs = CellAttrs {
            bold: true,
            italic: false,
            underline: true,
            reverse: false,
            strikethrough: true,
        };
        let json = serde_json::to_string(&attrs).unwrap();
        let recovered: CellAttrs = serde_json::from_str(&json).unwrap();
        assert_eq!(attrs, recovered);
    }

    #[test]
    fn test_snapshot_from_empty_screen() {
        let parser = vt100::Parser::new(5, 10, 0);
        let snapshot = GridSnapshot::from_screen(parser.screen());
        assert_eq!(snapshot.rows, 5);
        assert_eq!(snapshot.cols, 10);
        assert_eq!(snapshot.row_runs.len(), 5);
        // Empty screen — each row collapses into one run of spaces.
        for runs in &snapshot.row_runs {
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].text, " ".repeat(10));
        }
    }

    #[test]
    fn test_snapshot_run_grouping_plain_text() {
        let mut parser = vt100::Parser::new(1, 20, 0);
        parser.process(b"hello world");
        let snapshot = GridSnapshot::from_screen(parser.screen());
        assert_eq!(snapshot.rows, 1);
        // "hello world" + trailing spaces = one run (all default attrs).
        assert_eq!(snapshot.row_runs[0].len(), 1);
        let run = &snapshot.row_runs[0][0];
        assert!(run.text.starts_with("hello world"));
        assert_eq!(run.col_start, 0);
        assert_eq!(run.col_end, 20);
        assert_eq!(run.fg, TermColor::Default);
    }

    #[test]
    fn test_snapshot_run_breaks_on_color_change() {
        let mut parser = vt100::Parser::new(1, 20, 0);
        // \x1b[31m = red fg, \x1b[0m = reset
        parser.process(b"AB\x1b[31mCD\x1b[0mEF");
        let snapshot = GridSnapshot::from_screen(parser.screen());
        // Expect at least 3 runs: default "AB", red "CD", default "EF…trailing".
        assert!(
            snapshot.row_runs[0].len() >= 3,
            "expected ≥3 runs, got {}",
            snapshot.row_runs[0].len()
        );
        // The middle run must be red (Indexed(1)).
        let red_run = snapshot.row_runs[0]
            .iter()
            .find(|r| r.fg == TermColor::Indexed(1))
            .expect("expected a red run");
        assert_eq!(red_run.text, "CD");
    }

    #[test]
    fn test_snapshot_bold_promotes_standard_color_to_bright() {
        let mut parser = vt100::Parser::new(1, 5, 0);
        // \x1b[1;31m = bold + red. With bold-bright, fg should become bright red (Indexed(9)).
        parser.process(b"\x1b[1;31mX");
        let snapshot = GridSnapshot::from_screen(parser.screen());
        let x_run = snapshot.row_runs[0]
            .iter()
            .find(|r| r.text.contains('X'))
            .expect("expected an 'X' run");
        assert_eq!(x_run.fg, TermColor::Indexed(9));
        assert!(x_run.attrs.bold);
    }

    #[test]
    fn test_snapshot_inverse_swaps_fg_bg() {
        let mut parser = vt100::Parser::new(1, 5, 0);
        // \x1b[7m = inverse. With default fg/bg both Default, swap is a no-op
        // observable only via the attrs flag — verify the flag is set.
        parser.process(b"\x1b[7mY");
        let snapshot = GridSnapshot::from_screen(parser.screen());
        let y_run = snapshot.row_runs[0]
            .iter()
            .find(|r| r.text.contains('Y'))
            .expect("expected a 'Y' run");
        assert!(y_run.attrs.reverse);
    }

    #[test]
    fn test_snapshot_cell_and_run_counts() {
        let mut parser = vt100::Parser::new(2, 5, 0);
        parser.process(b"ABCDE\x1b[31mFGHIJ");
        let snapshot = GridSnapshot::from_screen(parser.screen());
        assert_eq!(snapshot.cell_count(), 10);
        // Row 0: "ABCDE" default. Row 1: "FGHIJ" red. Each row = 1 run.
        assert_eq!(snapshot.run_count(), 2);
    }

    #[test]
    fn test_snapshot_serde_round_trip() {
        let mut parser = vt100::Parser::new(1, 10, 0);
        parser.process(b"hi\x1b[33mLO");
        let snapshot = GridSnapshot::from_screen(parser.screen());
        let json = serde_json::to_string(&snapshot).unwrap();
        let recovered: GridSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snapshot, recovered);
    }
}
