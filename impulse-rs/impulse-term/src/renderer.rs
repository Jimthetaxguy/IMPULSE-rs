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
    /// Returns the response and the size of the rendered area in cells (cols, rows).
    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        parser: &vt100::Parser,
        theme: &TerminalTheme,
        focused: bool,
        scroll_offset: usize,
    ) -> egui::Response {
        self.ensure_metrics(ui);

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

            for run in &runs {
                let x_start = origin.x + run.col_start as f32 * self.cell_width;
                let x_end = origin.x + run.col_end as f32 * self.cell_width;

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

                    // For italics, we apply a slight slant via the galley approach.
                    // egui doesn't natively support italic monospace, so we just use
                    // the regular font and note this as a known limitation.
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

        // Cursor rendering (only when focused and no scroll offset).
        if focused && scroll_offset == 0 {
            let cursor_pos = screen.cursor_position();
            let cx = origin.x + cursor_pos.1 as f32 * self.cell_width;
            let cy = origin.y + cursor_pos.0 as f32 * self.cell_height;
            let cursor_rect = egui::Rect::from_min_size(
                egui::pos2(cx, cy),
                egui::vec2(self.cell_width, self.cell_height),
            );
            // Block cursor with semi-transparent fill.
            painter.rect_filled(
                cursor_rect,
                0.0,
                egui::Color32::from_rgba_premultiplied(0xc9, 0xd1, 0xd9, 0x80),
            );
        }

        response
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

            // Try to extend the current run.
            let can_extend = runs.last().is_some_and(|last| {
                last.fg == fg
                    && last.bg == bg
                    && last.bold == bold
                    && last.italic == italic
                    && last.underline == underline
                    && last.col_end == col as usize
            });

            if can_extend {
                let last = runs.last_mut().unwrap();
                last.text.push_str(&ch);
                last.col_end = col as usize + 1;
            } else {
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
}
