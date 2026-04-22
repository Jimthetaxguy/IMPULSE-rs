//! Terminal renderer — converts vt100::Screen into egui draw calls.
//!
//! Uses run-based rendering: consecutive cells with identical attributes
//! (fg, bg, bold, italic, underline) are grouped into "runs." Each run
//! produces one background rect + one text draw call, reducing draw calls
//! from ~4,800/frame (cell-by-cell for 120x40) to ~100-300/frame.

use eframe::egui;

use crate::theme::TerminalTheme;

/// Renders a vt100 terminal screen into an egui UI region.
pub struct TerminalRenderer {
    pub font_size: f32,
    cell_width: f32,
    cell_height: f32,
    metrics_computed: bool,
}

impl Default for TerminalRenderer {
    fn default() -> Self {
        Self {
            font_size: 13.0,
            cell_width: 0.0,
            cell_height: 0.0,
            metrics_computed: false,
        }
    }
}

/// A run of consecutive cells with identical visual attributes.
struct CellRun {
    text: String,
    col_start: usize,
    col_end: usize,
    fg: egui::Color32,
    bg: egui::Color32,
    bold: bool,
    italic: bool,
    underline: bool,
}

impl TerminalRenderer {
    pub fn new(font_size: f32) -> Self {
        Self {
            font_size,
            ..Default::default()
        }
    }

    /// Compute cell metrics from the font. Called once (or when font_size changes).
    fn ensure_metrics(&mut self, ui: &egui::Ui) {
        if self.metrics_computed {
            return;
        }
        let font_id = egui::FontId::monospace(self.font_size);
        let galley = ui
            .painter()
            .layout_no_wrap("M".to_string(), font_id, egui::Color32::WHITE);
        self.cell_width = galley.rect.width();
        self.cell_height = galley.rect.height();
        if self.cell_width < 1.0 {
            self.cell_width = self.font_size * 0.6;
        }
        if self.cell_height < 1.0 {
            self.cell_height = self.font_size * 1.2;
        }
        self.metrics_computed = true;
    }

    /// Cell dimensions (width, height). Returns (0, 0) before first render.
    pub fn cell_size(&self) -> (f32, f32) {
        (self.cell_width, self.cell_height)
    }

    /// Render the terminal screen into the given egui UI.
    ///
    /// When `scroll_offset > 0`, uses `parser.set_scrollback()` to shift the
    /// viewport into the scrollback buffer, renders normally, then resets.
    ///
    /// Returns the egui Response for the rendered area.
    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        parser: &mut vt100::Parser,
        theme: &TerminalTheme,
        focused: bool,
        scroll_offset: usize,
    ) -> egui::Response {
        self.ensure_metrics(ui);

        // Shift viewport into scrollback if needed.
        if scroll_offset > 0 {
            parser.set_scrollback(scroll_offset);
        }

        let screen = parser.screen();
        let (rows, cols) = screen.size();

        let total_width = self.cell_width * cols as f32;
        let total_height = self.cell_height * rows as f32;

        let (response, painter) =
            ui.allocate_painter(egui::vec2(total_width, total_height), egui::Sense::click());
        let origin = response.rect.min;

        // Fill background.
        painter.rect_filled(response.rect, 0.0, theme.bg);

        // Render each row using runs.
        for row in 0..rows {
            let y = origin.y + row as f32 * self.cell_height;
            let runs = self.build_runs(screen, row, cols, theme);
            self.paint_runs(&painter, &runs, origin.x, y, theme);
        }

        // Cursor rendering (only when focused and not scrolled into history).
        if focused && scroll_offset == 0 {
            let cursor_pos = screen.cursor_position();
            let cx = origin.x + cursor_pos.1 as f32 * self.cell_width;
            let cy = origin.y + cursor_pos.0 as f32 * self.cell_height;
            let cursor_rect = egui::Rect::from_min_size(
                egui::pos2(cx, cy),
                egui::vec2(self.cell_width, self.cell_height),
            );
            painter.rect_filled(
                cursor_rect,
                0.0,
                egui::Color32::from_rgba_premultiplied(
                    theme.cursor.r(),
                    theme.cursor.g(),
                    theme.cursor.b(),
                    0x80,
                ),
            );
        }

        // Reset scrollback viewport.
        if scroll_offset > 0 {
            parser.set_scrollback(0);
        }

        response
    }

    /// Paint a slice of runs to the screen.
    fn paint_runs(
        &self,
        painter: &egui::Painter,
        runs: &[CellRun],
        origin_x: f32,
        y: f32,
        theme: &TerminalTheme,
    ) {
        for run in runs {
            let x_start = origin_x + run.col_start as f32 * self.cell_width;
            let x_end = origin_x + run.col_end as f32 * self.cell_width;

            // Background rect (only if different from terminal bg).
            if run.bg != theme.bg {
                let rect = egui::Rect::from_min_max(
                    egui::pos2(x_start, y),
                    egui::pos2(x_end, y + self.cell_height),
                );
                painter.rect_filled(rect, 0.0, run.bg);
            }

            // Foreground text.
            if !run.text.trim().is_empty() {
                let font_id = if run.bold {
                    egui::FontId::new(self.font_size, egui::FontFamily::Monospace)
                } else {
                    egui::FontId::monospace(self.font_size)
                };

                let pos = egui::pos2(x_start, y);
                painter.text(pos, egui::Align2::LEFT_TOP, &run.text, font_id, run.fg);

                // Underline.
                if run.underline {
                    let underline_y = y + self.cell_height - 1.0;
                    painter.line_segment(
                        [
                            egui::pos2(x_start, underline_y),
                            egui::pos2(x_end, underline_y),
                        ],
                        egui::Stroke::new(1.0, run.fg),
                    );
                }
            }
        }
    }

    /// Build runs for a single row by grouping cells with matching attributes.
    fn build_runs(
        &self,
        screen: &vt100::Screen,
        row: u16,
        cols: u16,
        theme: &TerminalTheme,
    ) -> Vec<CellRun> {
        let mut runs: Vec<CellRun> = Vec::new();

        for col in 0..cols {
            let cell = screen.cell(row, col);
            let (ch, fg, bg, bold, italic, underline) = match cell {
                Some(cell) => {
                    let ch = cell.contents();
                    let fg = theme.resolve_fg(cell.fgcolor());
                    let bg = theme.resolve_bg(cell.bgcolor());
                    // Swap fg/bg for inverse video.
                    let (fg, bg) = if cell.inverse() { (bg, fg) } else { (fg, bg) };
                    // Bold bright: if bold and fg is a standard color 0-7, use bright variant.
                    let fg = if cell.bold() {
                        match cell.fgcolor() {
                            vt100::Color::Idx(idx) if idx < 8 => {
                                theme.resolve_fg(vt100::Color::Idx(idx + 8))
                            }
                            _ => fg,
                        }
                    } else {
                        fg
                    };
                    (ch, fg, bg, cell.bold(), cell.italic(), cell.underline())
                }
                None => (" ".to_string(), theme.fg, theme.bg, false, false, false),
            };

            // Try to extend the current run. Using nested-if instead of let-chains
            // to remain compatible with Rust 2021 edition (let-chains require 2024).
            // This also eliminates the TOCTOU window that existed with the old
            // separate can_extend guard + unwrap() pattern.
            let extended = if let Some(last) = runs.last_mut() {
                if last.fg == fg
                    && last.bg == bg
                    && last.bold == bold
                    && last.italic == italic
                    && last.underline == underline
                    && last.col_end == col as usize
                {
                    last.text.push_str(&ch);
                    last.col_end = col as usize + 1;
                    true
                } else {
                    false
                }
            } else {
                false
            };

            if !extended {
                runs.push(CellRun {
                    text: ch,
                    col_start: col as usize,
                    col_end: col as usize + 1,
                    fg,
                    bg,
                    bold,
                    italic,
                    underline,
                });
            }
        }

        runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_default_metrics() {
        let renderer = TerminalRenderer::default();
        assert_eq!(renderer.font_size, 13.0);
        assert_eq!(renderer.cell_width, 0.0); // Not computed yet
        assert_eq!(renderer.cell_height, 0.0);
    }

    #[test]
    fn test_renderer_custom_font_size() {
        let renderer = TerminalRenderer::new(16.0);
        assert_eq!(renderer.font_size, 16.0);
    }

    #[test]
    fn test_build_runs_single_cell_row() {
        // Create a parser with a single cell containing 'X' at col 0.
        let mut parser = vt100::Parser::new(1, 80, 0);
        parser.process(b"X");
        let screen = parser.screen();
        let (_, cols) = screen.size();

        let renderer = TerminalRenderer::new(13.0);
        let theme = TerminalTheme::default();
        let runs = renderer.build_runs(screen, 0, cols, &theme);

        // First run should start with 'X' at col 0. Subsequent empty cells
        // extend the run since they share the same default styling.
        assert!(!runs.is_empty());
        assert!(runs[0].text.starts_with('X'));
        assert_eq!(runs[0].col_start, 0);
    }

    #[test]
    fn test_build_runs_multiple_runs_same_row() {
        // Create a parser with two color runs: red 'A' then reset+white 'B'.
        let mut parser = vt100::Parser::new(1, 80, 0);
        parser.process(b"\x1B[31mA\x1B[0mB"); // Red A, then reset, then white B
        let screen = parser.screen();
        let (_, cols) = screen.size();

        let renderer = TerminalRenderer::new(13.0);
        let theme = TerminalTheme::default();
        let runs = renderer.build_runs(screen, 0, cols, &theme);

        // Should produce multiple runs: red A, then spaces, then B.
        assert!(runs.len() >= 2);
        // First run should contain 'A'
        assert!(runs[0].text.contains('A'));
        // Last run should contain 'B'
        assert!(runs[runs.len() - 1].text.contains('B'));
    }

    #[test]
    fn test_build_runs_no_panic_on_empty_row() {
        // Create a parser with an empty screen (no content written).
        let parser = vt100::Parser::new(1, 80, 0);
        let screen = parser.screen();
        let (_, cols) = screen.size();

        let renderer = TerminalRenderer::new(13.0);
        let theme = TerminalTheme::default();

        // This should not panic — the fix replaces unwrap() with if-let.
        let runs = renderer.build_runs(screen, 0, cols, &theme);

        // Empty row should produce at least one run (the empty cell default).
        assert!(!runs.is_empty());
    }
}
