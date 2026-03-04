//! Terminal panel — assembled egui widget combining backend, renderer, input,
//! theme, and context bridge into a single reusable component.
//!
//! ```text
//! ┌─ Terminal Panel ───────────────────────────────────┐
//! │                                                    │
//! │  Terminal grid (TerminalRenderer)                   │
//! │  - Monospace cells with ANSI colors                │
//! │  - Cursor (block)                                  │
//! │                                                    │
//! ├────────────────────────────────────────────────────┤
//! │ ● bash  80×24  │  ◐ 45% Essential  │ ↓2 ↑1 │ pid │
//! └────────────────────────────────────────────────────┘
//! ```

use std::path::Path;
use std::sync::Arc;

use eframe::egui;

use crate::backend::TerminalBackend;
use crate::context::{AgentKind, ContextBridge, ContextHealth, ContextTier, ExtractedInsight};
use crate::input;
use crate::renderer::TerminalRenderer;
use crate::theme::TerminalTheme;

/// Environment variables to strip before spawning agent processes.
const SANITIZED_ENV_VARS: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_SESSION",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_PARENT_SESSION_ID",
];

/// A complete terminal panel with PTY backend, renderer, input handling,
/// and context lifecycle integration.
pub struct TerminalPanel {
    backend: Arc<TerminalBackend>,
    renderer: TerminalRenderer,
    theme: TerminalTheme,
    context: ContextBridge,
    // UI state.
    focused: bool,
    show_context_overlay: bool,
    title: String,
    agent_name: &'static str,
    scroll_offset: usize,
    /// True when new PTY output arrived while the user is scrolled up.
    has_new_output_while_scrolled: bool,
    /// Tracks bytes at last repaint to avoid redundant repaints.
    last_repaint_bytes: u64,
}

impl TerminalPanel {
    /// Spawn a new terminal panel.
    ///
    /// Strips Claude Code environment variables, sets TERM/COLORTERM/IMPULSE_*,
    /// then spawns the child process in a PTY.
    pub fn spawn(
        command: &str,
        args: &[String],
        working_dir: Option<&Path>,
        agent_name: &'static str,
        pane_id: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let agent_kind = AgentKind::detect(command, agent_name);

        // Sanitize environment variables.
        let saved: Vec<(&str, Option<String>)> = SANITIZED_ENV_VARS
            .iter()
            .map(|var| (*var, std::env::var(var).ok()))
            .collect();

        // SAFETY: Single-threaded egui main loop — no concurrent env var access.
        unsafe {
            for var in SANITIZED_ENV_VARS {
                std::env::remove_var(var);
            }
            std::env::set_var("TERM", "xterm-256color");
            std::env::set_var("COLORTERM", "truecolor");
            std::env::set_var("IMPULSE_TERM_PROGRAM", "impulse-gui");
            std::env::set_var("IMPULSE_VERSION", env!("CARGO_PKG_VERSION"));
        }

        let env_vars = build_env_vars(working_dir, agent_name, pane_id);

        let result = TerminalBackend::spawn(command, args, working_dir, &env_vars, 24, 80, None);

        // Restore original environment.
        // SAFETY: Same reasoning — single-threaded, synchronous operation.
        unsafe {
            for (var, val) in saved {
                if let Some(v) = val {
                    std::env::set_var(var, v);
                }
            }
        }

        let backend = Arc::new(result?);
        let context = ContextBridge::new(pane_id, agent_kind, Arc::clone(&backend));

        Ok(Self {
            backend,
            renderer: TerminalRenderer::default(),
            theme: TerminalTheme::default(),
            context,
            focused: false,
            show_context_overlay: false,
            title: agent_name.to_string(),
            agent_name,
            scroll_offset: 0,
            has_new_output_while_scrolled: false,
            last_repaint_bytes: 0,
        })
    }

    /// Render the terminal panel into the given UI region.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        // Request repaint if there's new output.
        let current_bytes = self.backend.output_bytes();
        if current_bytes != self.last_repaint_bytes {
            self.last_repaint_bytes = current_bytes;
            ui.ctx().request_repaint();
            // If the user is scrolled up, flag that new output is available
            // below — don't force-snap them to the bottom.
            if self.scroll_offset > 0 {
                self.has_new_output_while_scrolled = true;
            }
        }

        // Clear the new-output flag when user returns to the bottom.
        if self.scroll_offset == 0 {
            self.has_new_output_while_scrolled = false;
        }

        // Handle keyboard input.
        self.handle_input(ui);

        // Wrap terminal in a frame with padding for readability.
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(8, 4))
            .fill(self.theme.bg)
            .show(ui, |ui| {
                // Allocate the full available space.
                let available = ui.available_size();
                let status_bar_height = 20.0;
                let scroll_badge_height = if self.scroll_offset > 0 { 18.0 } else { 0.0 };
                let terminal_height =
                    (available.y - status_bar_height - scroll_badge_height).max(0.0);

                // Handle mouse wheel scrolling.
                let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
                if scroll_delta != 0.0 {
                    let cell_h = self.renderer.cell_size().1;
                    let line_delta = if cell_h > 0.0 {
                        (scroll_delta / cell_h).round() as i32
                    } else {
                        (scroll_delta / 13.0).round() as i32
                    };
                    let max_scroll = self.backend.scrollback_len();
                    self.scroll_offset = (self.scroll_offset as i32 - line_delta)
                        .clamp(0, max_scroll as i32)
                        as usize;
                }

                // Handle Shift+PageUp/PageDown for scrolling.
                let page_scroll: Option<i32> = ui.input(|input| {
                    let shift = input.modifiers.shift;
                    if shift && input.key_pressed(egui::Key::PageUp) {
                        Some(-24) // scroll up one page
                    } else if shift && input.key_pressed(egui::Key::PageDown) {
                        Some(24) // scroll down one page
                    } else {
                        None
                    }
                });
                if let Some(delta) = page_scroll {
                    let max_scroll = self.backend.scrollback_len();
                    self.scroll_offset =
                        (self.scroll_offset as i32 - delta).clamp(0, max_scroll as i32) as usize;
                }

                // Terminal grid.
                let grid_width = available.x - 16.0;
                let terminal_response =
                    ui.allocate_ui(egui::vec2(grid_width, terminal_height), |ui| {
                        self.backend.with_parser_mut(|parser| {
                            self.renderer.render(
                                ui,
                                parser,
                                &self.theme,
                                self.focused,
                                self.scroll_offset,
                            )
                        })
                    });

                // Dynamic PTY resize — match terminal dimensions to panel size.
                let (cell_w, cell_h) = self.renderer.cell_size();
                if cell_w > 0.0 && cell_h > 0.0 {
                    let new_cols = (grid_width / cell_w).floor().max(10.0) as u16;
                    let new_rows = (terminal_height / cell_h).floor().max(1.0) as u16;
                    let (cur_cols, cur_rows) = self.backend.size();
                    if new_cols != cur_cols || new_rows != cur_rows {
                        if let Err(e) = self.backend.resize(new_cols, new_rows) {
                            log::warn!("PTY resize to {}x{} failed: {}", new_cols, new_rows, e);
                        }
                    }
                }

                // Check focus — clicked on the terminal area means focused.
                let response = &terminal_response.inner;
                self.focused = response.has_focus()
                    || (ui.input(|i| i.pointer.any_click())
                        && response
                            .rect
                            .contains(ui.input(|i| i.pointer.interact_pos().unwrap_or_default())));

                // Scroll badge (when scrolled up).
                if self.scroll_offset > 0 {
                    ui.horizontal(|ui| {
                        let badge_text = if self.has_new_output_while_scrolled {
                            format!(
                                "\u{2193} New output \u{2014} {} lines up",
                                self.scroll_offset
                            )
                        } else {
                            format!("Scrolled: {} lines up", self.scroll_offset)
                        };
                        ui.label(
                            egui::RichText::new(badge_text)
                                .small()
                                .color(egui::Color32::from_rgb(0xd2, 0x99, 0x22)),
                        );
                        if ui.small_button("Jump to bottom").clicked() {
                            self.scroll_offset = 0;
                        }
                    });
                }

                // Status bar.
                let copy_clicked = self.render_status_bar(ui);
                if copy_clicked {
                    let text = self.backend.screen_text();
                    ui.ctx().copy_text(text);
                }
            });

        // Context overlay (Ctrl+Shift+C toggle).
        if self.show_context_overlay {
            self.render_context_overlay(ui);
        }
    }

    /// Handle keyboard input and write to the PTY.
    fn handle_input(&mut self, ui: &egui::Ui) {
        if !self.focused {
            return;
        }

        let events: Vec<(egui::Key, egui::Modifiers)> = ui.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| {
                    if let egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } = event
                    {
                        Some((*key, *modifiers))
                    } else {
                        None
                    }
                })
                .collect()
        });

        for (key, modifiers) in &events {
            // Ctrl+Shift+C: toggle context overlay.
            if modifiers.ctrl && modifiers.shift && *key == egui::Key::C {
                self.show_context_overlay = !self.show_context_overlay;
                continue;
            }

            // Ctrl+Shift+X: copy visible screen text to clipboard.
            if modifiers.ctrl && modifiers.shift && *key == egui::Key::X {
                let text = self.backend.screen_text();
                ui.ctx().copy_text(text);
                continue;
            }

            // Try to convert to PTY bytes.
            let app_cursor = self
                .backend
                .with_parser(|p| p.screen().application_cursor());
            if let Some(bytes) = input::key_to_pty_bytes(key, modifiers, app_cursor) {
                let _ = self.backend.write_input(&bytes);
            }
        }

        // Handle text input (for IME / composed characters).
        let text_events: Vec<String> = ui.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| {
                    if let egui::Event::Text(text) = event {
                        Some(text.clone())
                    } else {
                        None
                    }
                })
                .collect()
        });

        for text in &text_events {
            let _ = self.backend.write_input(text.as_bytes());
        }

        // Handle paste.
        let paste_text: Option<String> = ui.input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Paste(text) = event {
                    Some(text.clone())
                } else {
                    None
                }
            })
        });

        if let Some(text) = paste_text {
            let pasted = input::bracketed_paste(&text);
            let _ = self.backend.write_input(&pasted);
        }
    }

    /// Render the status bar at the bottom of the panel.
    /// Returns true if the Copy button was clicked.
    fn render_status_bar(&self, ui: &mut egui::Ui) -> bool {
        let health = self.context.health();
        let mut copy_clicked = false;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            // Alive dot.
            let alive_color = if self.backend.is_alive() {
                egui::Color32::from_rgb(0x3f, 0xb9, 0x50)
            } else {
                egui::Color32::from_rgb(0x6e, 0x76, 0x81)
            };
            let dot_rect = ui.allocate_space(egui::vec2(8.0, 8.0));
            ui.painter()
                .circle_filled(dot_rect.1.center(), 3.0, alive_color);

            // Title + dimensions.
            let (cols, rows) = self.backend.size();
            ui.label(
                egui::RichText::new(format!("{} {}x{}", self.title, cols, rows))
                    .small()
                    .color(egui::Color32::from_rgb(0x8b, 0x94, 0x9e)),
            );

            ui.separator();

            // Context health indicator.
            let (tier_icon, tier_color) = context_tier_indicator(&health, &self.theme);
            let usage_pct = (health.usage_fraction * 100.0) as u8;
            ui.label(
                egui::RichText::new(format!(
                    "{} {}% {}",
                    tier_icon,
                    usage_pct,
                    health.tier.as_str()
                ))
                .small()
                .color(tier_color),
            );

            ui.separator();

            // Compaction/injection counters.
            ui.label(
                egui::RichText::new(format!(
                    "\u{2193}{} \u{2191}{}",
                    health.compaction_count, health.injection_count
                ))
                .small()
                .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
            );

            ui.separator();

            // Copy button — copies visible screen text to clipboard.
            if ui
                .small_button(
                    egui::RichText::new("Copy")
                        .small()
                        .color(egui::Color32::from_rgb(0x8b, 0x94, 0x9e)),
                )
                .on_hover_text("Copy screen text (Ctrl+Shift+X)")
                .clicked()
            {
                copy_clicked = true;
            }
        });

        copy_clicked
    }

    /// Render the context overlay (toggled by Ctrl+Shift+C).
    fn render_context_overlay(&self, ui: &mut egui::Ui) {
        let health = self.context.health();

        egui::Window::new("Context Health")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0))
            .show(ui.ctx(), |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "Tier: {} ({:.0}%)",
                        health.tier.as_str(),
                        health.usage_fraction * 100.0
                    ))
                    .strong(),
                );

                ui.label(format!(
                    "Tokens: ~{} / {}",
                    format_tokens(health.estimated_tokens),
                    format_tokens(health.window_tokens)
                ));

                ui.label(format!(
                    "Compactions: {}  |  Injections: {}",
                    health.compaction_count, health.injection_count
                ));

                ui.separator();
                ui.label(egui::RichText::new("Recent Insights:").strong());

                let insights = self.context.insights();
                let recent = if insights.len() > 10 {
                    &insights[insights.len() - 10..]
                } else {
                    insights
                };

                if recent.is_empty() {
                    ui.label(
                        egui::RichText::new("No insights yet")
                            .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                    );
                } else {
                    for insight in recent.iter().rev() {
                        let elapsed = Utc::now()
                            .signed_duration_since(insight.timestamp)
                            .num_minutes();
                        ui.label(format!(
                            "  [{}] {} ({}m ago)",
                            insight.insight_type.as_str(),
                            crate::context::truncate_insight(&insight.content, 50),
                            elapsed
                        ));
                    }
                }
            });
    }

    /// Access the context bridge for external lifecycle operations.
    pub fn context_bridge(&mut self) -> &mut ContextBridge {
        &mut self.context
    }

    /// Whether the child process is still alive.
    pub fn is_alive(&self) -> bool {
        self.backend.is_alive()
    }

    /// The panel title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Set the panel title (e.g., from PTY title change event).
    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    /// The agent name.
    pub fn agent_name(&self) -> &'static str {
        self.agent_name
    }

    /// Write raw bytes to the PTY.
    pub fn write_input(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        self.backend.write_input(data)
    }

    /// Get the full visible screen text (for search, context extraction).
    pub fn screen_text(&self) -> String {
        self.backend.screen_text()
    }

    /// Usage history for sparkline visualization.
    pub fn usage_history(&self) -> &std::collections::VecDeque<(std::time::Instant, f32)> {
        self.context.usage_history()
    }

    /// Kill the child process.
    pub fn kill(&self) {
        self.backend.kill();
    }

    /// Get context health for status bar display.
    pub fn context_health(&self) -> ContextHealth {
        self.context.health()
    }

    /// Get current context tier (immutable).
    pub fn current_tier(&self) -> ContextTier {
        self.context.current_tier()
    }

    /// Get accumulated insights (immutable).
    pub fn insights(&self) -> &[ExtractedInsight] {
        self.context.insights()
    }

    /// Set focus state.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

use chrono::Utc;

/// Map context tier to a status indicator (icon, color).
fn context_tier_indicator(
    health: &ContextHealth,
    theme: &TerminalTheme,
) -> (&'static str, egui::Color32) {
    match health.tier {
        ContextTier::None | ContextTier::Full => {
            ("\u{25CF}", theme.context_health.comfortable) // ●
        }
        ContextTier::Essential => ("\u{25D0}", theme.context_health.essential), // ◐
        ContextTier::Critical => ("\u{25D1}", theme.context_health.critical),   // ◑
        ContextTier::Minimal => ("\u{25CB}", theme.context_health.minimal),     // ○
        ContextTier::PostCompaction => ("\u{25CE}", theme.context_health.essential), // ◎
    }
}

/// Format token count for display.
fn format_tokens(tokens: usize) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.0}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

/// Build environment variables for a spawned terminal pane.
///
/// Extracted for testability. Sets TERM, COLORTERM, IMPULSE_PANE_ID,
/// IMPULSE_PANE_NAME, IMPULSE_HOME, and IMPULSE_SESSION_ID.
fn build_env_vars<'a>(
    working_dir: Option<&Path>,
    agent_name: &'a str,
    pane_id: usize,
) -> Vec<(&'a str, String)> {
    vec![
        ("TERM", "xterm-256color".to_string()),
        ("COLORTERM", "truecolor".to_string()),
        ("IMPULSE_PANE_ID", pane_id.to_string()),
        ("IMPULSE_PANE_NAME", agent_name.to_string()),
        (
            "IMPULSE_HOME",
            working_dir
                .map(|d| d.join(".impulse").display().to_string())
                .unwrap_or_default(),
        ),
        (
            "IMPULSE_SESSION_ID",
            format!(
                "{:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_build_env_vars_includes_impulse_home() {
        let dir = PathBuf::from("/tmp/test-project");
        let vars = build_env_vars(Some(&dir), "Claude Code", 0);
        let home = vars.iter().find(|(k, _)| *k == "IMPULSE_HOME").unwrap();
        assert_eq!(home.1, "/tmp/test-project/.impulse");
    }

    #[test]
    fn test_build_env_vars_impulse_home_empty_without_dir() {
        let vars = build_env_vars(None, "Shell", 0);
        let home = vars.iter().find(|(k, _)| *k == "IMPULSE_HOME").unwrap();
        assert!(home.1.is_empty());
    }

    #[test]
    fn test_build_env_vars_session_id_is_hex() {
        let vars = build_env_vars(None, "Claude Code", 1);
        let sid = vars
            .iter()
            .find(|(k, _)| *k == "IMPULSE_SESSION_ID")
            .unwrap();
        assert!(!sid.1.is_empty(), "Session ID should not be empty");
        assert!(
            u128::from_str_radix(&sid.1, 16).is_ok(),
            "Session ID '{}' should be valid hex",
            sid.1
        );
    }

    #[test]
    fn test_build_env_vars_has_all_expected_keys() {
        let vars = build_env_vars(Some(Path::new("/tmp")), "test", 42);
        let keys: Vec<&str> = vars.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"TERM"));
        assert!(keys.contains(&"COLORTERM"));
        assert!(keys.contains(&"IMPULSE_PANE_ID"));
        assert!(keys.contains(&"IMPULSE_PANE_NAME"));
        assert!(keys.contains(&"IMPULSE_HOME"));
        assert!(keys.contains(&"IMPULSE_SESSION_ID"));
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(1500), "2K");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }

    #[test]
    fn test_format_tokens_boundary_values() {
        // Verify format_tokens handles the copy-display boundaries correctly.
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1000), "1K");
        assert_eq!(format_tokens(999_999), "1000K");
        assert_eq!(format_tokens(1_000_000), "1.0M");
    }

    #[test]
    fn test_truncate_insight_reuse() {
        use crate::context::truncate_insight;
        assert_eq!(truncate_insight("hello", 10), "hello");
        // truncate_insight keeps up to max_len bytes then appends "...".
        assert_eq!(truncate_insight("hello world", 8), "hello wo...");
    }

    /// Simulates the scroll-guard state machine without needing a real PTY.
    /// Mirrors the logic in `TerminalPanel::show()`.
    struct ScrollState {
        scroll_offset: usize,
        has_new_output_while_scrolled: bool,
        last_repaint_bytes: u64,
    }

    impl ScrollState {
        fn new() -> Self {
            Self {
                scroll_offset: 0,
                has_new_output_while_scrolled: false,
                last_repaint_bytes: 0,
            }
        }

        /// Simulate new PTY output arriving.
        fn on_new_output(&mut self, current_bytes: u64) {
            if current_bytes != self.last_repaint_bytes {
                self.last_repaint_bytes = current_bytes;
                if self.scroll_offset > 0 {
                    self.has_new_output_while_scrolled = true;
                }
            }
            if self.scroll_offset == 0 {
                self.has_new_output_while_scrolled = false;
            }
        }

        /// Simulate user scrolling up.
        fn scroll_up(&mut self, lines: usize) {
            self.scroll_offset += lines;
        }

        /// Simulate user jumping to bottom.
        fn jump_to_bottom(&mut self) {
            self.scroll_offset = 0;
        }
    }

    #[test]
    fn test_scroll_offset_preserved_on_new_output() {
        let mut state = ScrollState::new();
        state.scroll_up(50);
        assert_eq!(state.scroll_offset, 50);

        // New output arrives — scroll offset should NOT be reset.
        state.on_new_output(1000);
        assert_eq!(
            state.scroll_offset, 50,
            "scroll_offset must be preserved when user is scrolled up"
        );
    }

    #[test]
    fn test_new_output_indicator_set_when_scrolled() {
        let mut state = ScrollState::new();
        state.scroll_up(10);
        assert!(!state.has_new_output_while_scrolled);

        // New output arrives while scrolled.
        state.on_new_output(500);
        assert!(
            state.has_new_output_while_scrolled,
            "indicator must be set when new output arrives while scrolled"
        );
    }

    #[test]
    fn test_new_output_indicator_cleared_at_bottom() {
        let mut state = ScrollState::new();
        state.scroll_up(10);
        state.on_new_output(500);
        assert!(state.has_new_output_while_scrolled);

        // User jumps to bottom.
        state.jump_to_bottom();
        state.on_new_output(500); // same bytes, but the clear logic runs
        assert!(
            !state.has_new_output_while_scrolled,
            "indicator must be cleared when user returns to bottom"
        );
    }
}
