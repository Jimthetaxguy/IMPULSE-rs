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
use crate::context::{AgentKind, ContextBridge, ContextHealth, ContextTier};
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

        let env_vars: Vec<(&str, String)> = vec![
            ("TERM", "xterm-256color".to_string()),
            ("COLORTERM", "truecolor".to_string()),
            ("IMPULSE_PANE_ID", pane_id.to_string()),
            ("IMPULSE_PANE_NAME", agent_name.to_string()),
        ];

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
        }

        // Handle keyboard input.
        self.handle_input(ui);

        // Allocate the full available space.
        let available = ui.available_size();
        let status_bar_height = 20.0;
        let terminal_height = (available.y - status_bar_height).max(0.0);

        // Terminal grid.
        let terminal_response = ui.allocate_ui(egui::vec2(available.x, terminal_height), |ui| {
            self.backend.with_parser(|parser| {
                self.renderer
                    .render(ui, parser, &self.theme, self.focused, self.scroll_offset)
            })
        });

        // Check focus — clicked on the terminal area means focused.
        let response = &terminal_response.inner;
        self.focused = response.has_focus()
            || (ui.input(|i| i.pointer.any_click())
                && response
                    .rect
                    .contains(ui.input(|i| i.pointer.interact_pos().unwrap_or_default())));

        // Status bar.
        self.render_status_bar(ui);

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
    fn render_status_bar(&self, ui: &mut egui::Ui) {
        let health = self.context.health();

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
        });
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
                            truncate_display(&insight.content, 50),
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

    /// Kill the child process.
    pub fn kill(&self) {
        self.backend.kill();
    }

    /// Get context health for status bar display.
    pub fn context_health(&self) -> ContextHealth {
        self.context.health()
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

/// Truncate a string for display.
fn truncate_display(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}..", &s[..end])
    }
}
