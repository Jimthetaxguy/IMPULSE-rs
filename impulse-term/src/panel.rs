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

use parking_lot::Mutex;

use eframe::egui;

use crate::backend::TerminalBackend;
use crate::context::{
    AgentKind, ContextBridge, ContextHealth, ContextTier, ExtractedInsight, InsightType,
};
use crate::input;
use crate::renderer::TerminalRenderer;
use crate::role::PaneRole;
use crate::status_bar;
use crate::theme::TerminalTheme;

/// Filter for the insights overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InsightFilter {
    All,
    Errors,
    Decisions,
    Files,
}

/// Environment variables to strip before spawning agent processes.
const SANITIZED_ENV_VARS: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_SESSION",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_PARENT_SESSION_ID",
];

/// RAII guard — snapshots env vars, modifies them for the PTY spawn,
/// and restores the originals on drop. No unsafe needed.
///
/// Workers sanitize parent-agent env vars (CLAUDECODE, etc.) before spawn.
/// Supervisors intentionally skip sanitization — they are privileged and
/// INTEND to see the ambient Impulse env (first-principles rule #6).
struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
    /// True if this guard sanitized env vars; false for supervisor panes.
    /// dead_code: read only in tests via `is_sanitized()`, but always set so
    /// the Drop impl sees consistent state.
    #[allow(dead_code)]
    sanitized: bool,
}

impl EnvGuard {
    /// Sanitizing guard — removes parent-agent vars, then sets terminal defaults.
    /// Use for Worker panes.
    fn new(vars: &[&'static str]) -> Self {
        let saved: Vec<_> = vars
            .iter()
            .map(|var| (*var, std::env::var(var).ok()))
            .collect();
        for var in vars {
            std::env::remove_var(var);
        }
        std::env::set_var("TERM", "xterm-256color");
        std::env::set_var("COLORTERM", "truecolor");
        std::env::set_var("IMPULSE_TERM_PROGRAM", "impulse-gui");
        std::env::set_var("IMPULSE_VERSION", env!("CARGO_PKG_VERSION"));
        Self {
            saved,
            sanitized: true,
        }
    }

    /// Non-sanitizing guard — leaves parent-agent vars intact, but still sets
    /// terminal defaults and records nothing to restore. Use for Supervisor
    /// panes which INTEND to inherit the ambient Impulse env.
    fn new_privileged() -> Self {
        std::env::set_var("TERM", "xterm-256color");
        std::env::set_var("COLORTERM", "truecolor");
        std::env::set_var("IMPULSE_TERM_PROGRAM", "impulse-gui");
        std::env::set_var("IMPULSE_VERSION", env!("CARGO_PKG_VERSION"));
        Self {
            saved: Vec::new(),
            sanitized: false,
        }
    }

    /// Choose the right guard for the role.
    fn for_role(role: PaneRole, vars: &[&'static str]) -> Self {
        if role.should_sanitize_env() {
            Self::new(vars)
        } else {
            Self::new_privileged()
        }
    }

    /// Whether this guard performed parent-agent env sanitization.
    #[cfg(test)]
    fn is_sanitized(&self) -> bool {
        self.sanitized
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (var, val) in std::mem::take(&mut self.saved) {
            if let Some(v) = val {
                std::env::set_var(var, v);
            } else {
                std::env::remove_var(var);
            }
        }
    }
}

/// A complete terminal panel with PTY backend, renderer, input handling,
/// and context lifecycle integration.
pub struct TerminalPanel {
    backend: Arc<TerminalBackend>,
    renderer: TerminalRenderer,
    theme: TerminalTheme,
    /// Shared with StatusBar via Arc<Mutex> so both can access the same
    /// ContextBridge. The Mutex allows StatusBar::show (which has &mut self) to
    /// call health() while TerminalPanel methods use MutexGuard::deref().
    context: Arc<Mutex<ContextBridge>>,
    status_bar: status_bar::StatusBar,
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
    /// Active filter for the insights overlay.
    insight_filter: InsightFilter,
    /// Role assigned at spawn time — immutable for the panel's lifetime.
    role: PaneRole,
}

impl TerminalPanel {
    /// Spawn a new terminal panel as a Worker (default role).
    ///
    /// Backwards-compatible — existing callers keep the 5-arg signature.
    /// For supervisor panes, use [`Self::spawn_supervisor`].
    pub fn spawn(
        command: &str,
        args: &[String],
        working_dir: Option<&Path>,
        agent_name: &'static str,
        pane_id: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::spawn_with_theme(command, args, working_dir, agent_name, pane_id, None)
    }

    /// Spawn a new terminal panel with an explicit theme (Worker role).
    pub fn spawn_with_theme(
        command: &str,
        args: &[String],
        working_dir: Option<&Path>,
        agent_name: &'static str,
        pane_id: usize,
        custom_theme: Option<TerminalTheme>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::spawn_with_role(
            command,
            args,
            working_dir,
            agent_name,
            pane_id,
            custom_theme,
            PaneRole::Worker,
            None,
        )
    }

    /// Spawn a Worker pane — explicit, no supervisor env vars.
    ///
    /// Equivalent to [`Self::spawn`] but documents intent at the call site.
    pub fn spawn_worker(
        command: &str,
        args: &[String],
        working_dir: Option<&Path>,
        agent_name: &'static str,
        pane_id: usize,
        custom_theme: Option<TerminalTheme>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::spawn_with_role(
            command,
            args,
            working_dir,
            agent_name,
            pane_id,
            custom_theme,
            PaneRole::Worker,
            None,
        )
    }

    /// Spawn a Supervisor pane — privileged, receives `IMPULSE_SUPERVISOR=1`
    /// and (if provided) `IMPULSE_CMD_SOCKET=<cmd_socket_path>`.
    ///
    /// `EnvGuard` sanitization is SKIPPED so the supervisor can orchestrate
    /// the ambient Impulse env.
    pub fn spawn_supervisor(
        command: &str,
        args: &[String],
        working_dir: Option<&Path>,
        agent_name: &'static str,
        pane_id: usize,
        custom_theme: Option<TerminalTheme>,
        cmd_socket_path: Option<&Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::spawn_with_role(
            command,
            args,
            working_dir,
            agent_name,
            pane_id,
            custom_theme,
            PaneRole::Supervisor,
            cmd_socket_path,
        )
    }

    /// Spawn a new terminal panel with an explicit role and optional cmd socket path.
    ///
    /// This is the canonical spawn entrypoint. The role determines:
    /// - which env vars are injected (`PaneRole::spawn_env_vars`)
    /// - whether parent-agent env sanitization runs (`PaneRole::should_sanitize_env`)
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_with_role(
        command: &str,
        args: &[String],
        working_dir: Option<&Path>,
        agent_name: &'static str,
        pane_id: usize,
        custom_theme: Option<TerminalTheme>,
        role: PaneRole,
        cmd_socket_path: Option<&Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let agent_kind = AgentKind::detect(command, agent_name);

        // RAII guard: role-aware. Workers sanitize CLAUDECODE-style vars.
        // Supervisors skip sanitization (first-principles rule #6).
        let _env_guard = EnvGuard::for_role(role, SANITIZED_ENV_VARS);

        let env_vars =
            build_env_vars_for_role(working_dir, agent_name, pane_id, role, cmd_socket_path);
        // Convert owned (String, String) into the (&str, String) shape
        // TerminalBackend::spawn expects.
        let env_ref: Vec<(&str, String)> = env_vars
            .iter()
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect();

        let result = TerminalBackend::spawn(command, args, working_dir, &env_ref, 24, 80, None);

        // EnvGuard restores original env vars when dropped — no unsafe needed.
        // If spawn fails, the guard still drops and restores, so the caller
        // sees the original environment (important for error messages).
        let backend = Arc::new(result?);
        let context = Arc::new(Mutex::new(ContextBridge::new(
            pane_id,
            agent_kind,
            Arc::clone(&backend),
        )));
        let theme = custom_theme.unwrap_or_default();
        let title = agent_name.to_string();
        let status_bar = status_bar::StatusBar::new(
            Arc::clone(&backend),
            Arc::clone(&context),
            title.clone(),
            theme.clone(),
        );

        Ok(Self {
            backend,
            renderer: TerminalRenderer::default(),
            theme,
            context,
            status_bar,
            focused: false,
            show_context_overlay: false,
            title,
            agent_name,
            scroll_offset: 0,
            has_new_output_while_scrolled: false,
            last_repaint_bytes: 0,
            insight_filter: InsightFilter::All,
            role,
        })
    }

    /// The role assigned at spawn time — immutable for the panel's lifetime.
    pub fn role(&self) -> PaneRole {
        self.role
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
                                .color(self.theme.context_health.essential), // yellow
                        );
                        if ui.small_button("Jump to bottom").clicked() {
                            self.scroll_offset = 0;
                        }
                    });
                }

                // Status bar — Copy button is wired directly inside StatusBar::show().
                self.status_bar.show(ui);
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
                let _ = self.backend.write_queue().write_user_input(&bytes);
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
            let _ = self.backend.write_queue().write_user_input(text.as_bytes());
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
            let _ = self.backend.write_queue().write_user_input(&pasted);
        }
    }

    /// Render the context overlay (toggled by Ctrl+Shift+C).
    ///
    /// Shows context health stats, a filter bar, and grouped insights in a
    /// scrollable area. Each insight type is shown under a collapsible header
    /// with a count badge. The filter bar lets users narrow to a single type.
    fn render_context_overlay(&mut self, ui: &mut egui::Ui) {
        let health = self.context.lock().health();

        // Snapshot insights under the lock, then release before building UI.
        let all_insights: Vec<ExtractedInsight> = self.context.lock().insights().to_vec();

        // Capture filter as a local so the closure can mutate self.insight_filter
        // through a separate mutable binding without borrowing all of `self`.
        let mut current_filter = self.insight_filter;

        egui::Window::new("Context Health")
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(340.0, 420.0))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0))
            .show(ui.ctx(), |ui| {
                // -- Health summary --
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

                // -- Filter bar --
                ui.horizontal(|ui| {
                    for (filter, label) in [
                        (InsightFilter::All, "All"),
                        (InsightFilter::Errors, "Errors"),
                        (InsightFilter::Decisions, "Decisions"),
                        (InsightFilter::Files, "Files"),
                    ] {
                        let is_active = current_filter == filter;
                        let text = if is_active {
                            egui::RichText::new(label).strong()
                        } else {
                            egui::RichText::new(label)
                        };
                        if ui.selectable_label(is_active, text).clicked() {
                            current_filter = filter;
                        }
                    }
                });

                ui.separator();

                // -- Partition insights by type --
                let file_insights: Vec<&ExtractedInsight> = all_insights
                    .iter()
                    .filter(|i| i.insight_type == InsightType::FileModified)
                    .collect();
                let error_insights: Vec<&ExtractedInsight> = all_insights
                    .iter()
                    .filter(|i| i.insight_type == InsightType::ErrorEncountered)
                    .collect();
                let decision_insights: Vec<&ExtractedInsight> = all_insights
                    .iter()
                    .filter(|i| i.insight_type == InsightType::DecisionMade)
                    .collect();
                let task_insights: Vec<&ExtractedInsight> = all_insights
                    .iter()
                    .filter(|i| i.insight_type == InsightType::TaskCompleted)
                    .collect();
                let remote_insights: Vec<&ExtractedInsight> = all_insights
                    .iter()
                    .filter(|i| i.insight_type == InsightType::RemoteConnection)
                    .collect();

                // -- Scrollable insight groups --
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        if all_insights.is_empty() {
                            ui.label(
                                egui::RichText::new("No insights yet")
                                    .color(self.theme.ansi_colors[8]), // bright black (muted)
                            );
                            return;
                        }

                        // Helper: render a collapsible group of insights.
                        let render_group =
                            |ui: &mut egui::Ui,
                             header: &str,
                             count: usize,
                             insights: &[&ExtractedInsight]| {
                                if count == 0 {
                                    return;
                                }
                                egui::CollapsingHeader::new(
                                    egui::RichText::new(format!("{header} ({count})")).strong(),
                                )
                                .default_open(true)
                                .show(ui, |ui| {
                                    for insight in insights.iter().rev() {
                                        let elapsed = Utc::now()
                                            .signed_duration_since(insight.timestamp)
                                            .num_minutes();
                                        ui.label(format!(
                                            "  {} ({}m ago)",
                                            crate::context::truncate_insight(&insight.content, 60),
                                            elapsed
                                        ));
                                    }
                                });
                            };

                        let show_errors =
                            matches!(current_filter, InsightFilter::All | InsightFilter::Errors);
                        let show_decisions = matches!(
                            current_filter,
                            InsightFilter::All | InsightFilter::Decisions
                        );
                        let show_files =
                            matches!(current_filter, InsightFilter::All | InsightFilter::Files);
                        let show_tasks = matches!(current_filter, InsightFilter::All);

                        if show_errors {
                            render_group(ui, "Errors", error_insights.len(), &error_insights);
                        }
                        if show_decisions {
                            render_group(
                                ui,
                                "Decisions",
                                decision_insights.len(),
                                &decision_insights,
                            );
                        }
                        if show_files {
                            render_group(ui, "Files Modified", file_insights.len(), &file_insights);
                        }
                        if show_tasks {
                            render_group(
                                ui,
                                "Tasks Completed",
                                task_insights.len(),
                                &task_insights,
                            );
                        }
                        if matches!(current_filter, InsightFilter::All) {
                            render_group(
                                ui,
                                "Remote Connections",
                                remote_insights.len(),
                                &remote_insights,
                            );
                        }
                    });
            });

        // Write the (possibly changed) filter back.
        self.insight_filter = current_filter;
    }

    /// Access the context bridge for external lifecycle operations.
    /// Returns a MutexGuard so callers get &mut ContextBridge via Deref.
    pub fn context_bridge(&mut self) -> parking_lot::MutexGuard<'_, ContextBridge> {
        self.context.lock()
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

    /// Write raw bytes to the PTY via the serialized write queue.
    pub fn write_input(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        self.backend.write_queue().write_user_input(data)
    }

    /// Get the full visible screen text (for search, context extraction).
    pub fn screen_text(&self) -> String {
        self.backend.screen_text()
    }

    /// Usage history for sparkline visualization.
    pub fn usage_history(
        &self,
    ) -> std::sync::Arc<std::collections::VecDeque<(std::time::Instant, f32)>> {
        self.context.lock().usage_history()
    }

    /// Kill the child process.
    pub fn kill(&self) {
        self.backend.kill();
    }

    /// Get context health for status bar display.
    pub fn context_health(&self) -> ContextHealth {
        self.context.lock().health()
    }

    /// Get current context tier (immutable).
    pub fn current_tier(&self) -> ContextTier {
        self.context.lock().current_tier()
    }

    /// Get accumulated insights (immutable).
    pub fn insights(&self) -> Vec<ExtractedInsight> {
        self.context.lock().insights().to_vec()
    }

    /// Recent remote or mux ownership hints, newest first.
    pub fn recent_remote_connections(&self, limit: usize) -> Vec<String> {
        self.context.lock().recent_remote_connections(limit)
    }

    /// Set focus state.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

use chrono::Utc;

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

/// Build environment variables with role-specific additions.
///
/// Superset of [`build_env_vars`] — returns the same base keys, plus
/// `IMPULSE_PANE_ROLE` for all roles and `IMPULSE_CMD_SOCKET` +
/// `IMPULSE_SUPERVISOR` for `Supervisor` when a socket path is given.
///
/// Returns owned `(String, String)` pairs because the role-specific keys
/// are already `String` and copying the base keys keeps the return type
/// uniform.
fn build_env_vars_for_role(
    working_dir: Option<&Path>,
    agent_name: &str,
    pane_id: usize,
    role: PaneRole,
    cmd_socket_path: Option<&Path>,
) -> Vec<(String, String)> {
    let mut vars: Vec<(String, String)> = build_env_vars(working_dir, agent_name, pane_id)
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    // pane_id=None: the egui panel uses a usize pane_id baked into
    // build_env_vars; the uuid-based IMPULSE_WORKER_PANE_ID is supervisor-only.
    vars.extend(role.spawn_env_vars(cmd_socket_path, None));
    vars
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

    #[test]
    fn test_env_guard_restores_on_drop() {
        // Set a test env var.
        std::env::set_var("IMPULSE_TEST_VAR", "original_value");

        {
            // Guard takes ownership of the test var and replaces it.
            let _guard = EnvGuard::new(&["IMPULSE_TEST_VAR"]);
            assert_eq!(
                std::env::var("IMPULSE_TEST_VAR").ok(),
                None,
                "guard should remove the existing var"
            );
            std::env::set_var("IMPULSE_TEST_VAR", "new_value");
            assert_eq!(std::env::var("IMPULSE_TEST_VAR").unwrap(), "new_value");
            // guard drops here, restoring original
        }

        assert_eq!(
            std::env::var("IMPULSE_TEST_VAR").unwrap(),
            "original_value",
            "env var should be restored after guard drop"
        );

        // Cleanup.
        std::env::remove_var("IMPULSE_TEST_VAR");
    }

    // -----------------------------------------------------------------
    // PaneRole integration tests (Loop 115-116)
    // -----------------------------------------------------------------

    #[test]
    fn test_build_env_vars_for_role_worker_has_role_key() {
        let vars = build_env_vars_for_role(None, "claude", 0, PaneRole::Worker, None);
        let role = vars
            .iter()
            .find(|(k, _)| k == "IMPULSE_PANE_ROLE")
            .expect("worker must emit IMPULSE_PANE_ROLE");
        assert_eq!(role.1, "worker");
    }

    #[test]
    fn test_build_env_vars_for_role_worker_has_no_supervisor_env() {
        // L182 design change: workers NOW receive IMPULSE_CMD_SOCKET so they
        // can emit `@impulse <verb>` commands that the daemon hook intercepts.
        // The privilege boundary is IMPULSE_SUPERVISOR=1, NOT socket access:
        // the daemon enforces what each pane is allowed to do based on the
        // PaneRole + IMPULSE_WORKER_PANE_ID it sees in the request payload.
        let sock = PathBuf::from("/tmp/impulse.sock");
        let vars = build_env_vars_for_role(None, "claude", 0, PaneRole::Worker, Some(&sock));
        assert!(
            !vars.iter().any(|(k, _)| k == "IMPULSE_SUPERVISOR"),
            "worker must not set IMPULSE_SUPERVISOR"
        );
        assert!(
            vars.iter().any(|(k, _)| k == "IMPULSE_CMD_SOCKET"),
            "worker must receive IMPULSE_CMD_SOCKET to send @impulse commands"
        );
    }

    #[test]
    fn test_build_env_vars_for_role_supervisor_has_all_supervisor_env() {
        let sock = PathBuf::from("/tmp/impulse-daemon.sock");
        let vars = build_env_vars_for_role(
            Some(Path::new("/tmp/proj")),
            "supervisor",
            0,
            PaneRole::Supervisor,
            Some(&sock),
        );

        let get = |k: &str| vars.iter().find(|(vk, _)| vk == k).map(|(_, v)| v.as_str());
        assert_eq!(get("IMPULSE_PANE_ROLE"), Some("supervisor"));
        assert_eq!(get("IMPULSE_SUPERVISOR"), Some("1"));
        assert_eq!(get("IMPULSE_CMD_SOCKET"), Some("/tmp/impulse-daemon.sock"));
        // Base keys are still present.
        assert!(get("IMPULSE_PANE_ID").is_some());
        assert!(get("IMPULSE_HOME").is_some());
    }

    #[test]
    fn test_build_env_vars_for_role_supervisor_without_socket() {
        // Supervisor without a socket path still gets IMPULSE_PANE_ROLE=supervisor
        // but NOT IMPULSE_SUPERVISOR / IMPULSE_CMD_SOCKET (graceful degradation).
        let vars = build_env_vars_for_role(None, "supervisor", 0, PaneRole::Supervisor, None);
        let role = vars.iter().find(|(k, _)| k == "IMPULSE_PANE_ROLE").unwrap();
        assert_eq!(role.1, "supervisor");
        assert!(
            !vars.iter().any(|(k, _)| k == "IMPULSE_SUPERVISOR"),
            "supervisor without socket must not claim privileged flag"
        );
        assert!(
            !vars.iter().any(|(k, _)| k == "IMPULSE_CMD_SOCKET"),
            "supervisor without socket must not set IMPULSE_CMD_SOCKET"
        );
    }

    #[test]
    fn test_env_guard_for_role_worker_sanitizes() {
        // Worker panes sanitize parent-agent env vars.
        // Use a dedicated var so parallel tests don't race on CLAUDECODE.
        const TEST_VARS: &[&str] = &["IMPULSE_TEST_WORKER_SANITIZE_VAR"];
        std::env::set_var(TEST_VARS[0], "should-be-hidden");
        {
            let guard = EnvGuard::for_role(PaneRole::Worker, TEST_VARS);
            assert!(guard.is_sanitized(), "worker guard must sanitize");
            assert!(
                std::env::var(TEST_VARS[0]).is_err(),
                "test var should be removed for worker spawn"
            );
            // guard drops here, restoring original
        }
        assert_eq!(
            std::env::var(TEST_VARS[0]).unwrap_or_default(),
            "should-be-hidden",
            "original test var must be restored after worker spawn"
        );
        std::env::remove_var(TEST_VARS[0]);
    }

    #[test]
    fn test_env_guard_for_role_supervisor_does_not_sanitize() {
        // Supervisor panes INTEND to inherit ambient Impulse env, so the guard
        // does NOT remove parent-agent vars. Use a dedicated var so parallel
        // tests don't race on shared env state.
        const TEST_VARS: &[&str] = &["IMPULSE_TEST_SUPERVISOR_NOSANITIZE_VAR"];
        std::env::set_var(TEST_VARS[0], "supervisor-sees-this");
        {
            let guard = EnvGuard::for_role(PaneRole::Supervisor, TEST_VARS);
            assert!(!guard.is_sanitized(), "supervisor guard must NOT sanitize");
            assert_eq!(
                std::env::var(TEST_VARS[0]).unwrap_or_default(),
                "supervisor-sees-this",
                "supervisor must see ambient test var"
            );
        }
        // Cleanup.
        std::env::remove_var(TEST_VARS[0]);
    }

    #[test]
    fn test_env_guard_for_role_supervisor_still_sets_terminal_defaults() {
        // Even without sanitization, the guard should still set TERM etc.
        // NOTE: this test depends on ambient TERM state; it is still valuable
        // because it proves `new_privileged` writes the default TERM/COLORTERM
        // even when called repeatedly. We check via the guard's own setters.
        {
            let _guard = EnvGuard::new_privileged();
            assert_eq!(
                std::env::var("IMPULSE_TERM_PROGRAM").unwrap(),
                "impulse-gui",
                "supervisor guard must still set terminal defaults"
            );
            assert_eq!(std::env::var("TERM").unwrap(), "xterm-256color");
        }
    }

    #[test]
    fn test_build_env_vars_for_role_supervisor_never_drops_base_keys() {
        // Base keys from build_env_vars must still appear for Supervisor.
        let vars = build_env_vars_for_role(
            Some(Path::new("/tmp")),
            "sup",
            5,
            PaneRole::Supervisor,
            Some(Path::new("/tmp/s.sock")),
        );
        let keys: Vec<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
        for expected in &[
            "TERM",
            "COLORTERM",
            "IMPULSE_PANE_ID",
            "IMPULSE_PANE_NAME",
            "IMPULSE_HOME",
            "IMPULSE_SESSION_ID",
        ] {
            assert!(
                keys.contains(expected),
                "supervisor env must include base key {expected}"
            );
        }
    }

    #[test]
    fn test_build_env_vars_for_role_worker_always_has_role() {
        // A worker without socket still has IMPULSE_PANE_ROLE=worker.
        let vars = build_env_vars_for_role(None, "c", 1, PaneRole::Worker, None);
        let role = vars.iter().find(|(k, _)| k == "IMPULSE_PANE_ROLE").unwrap();
        assert_eq!(role.1, "worker");
    }

    // -----------------------------------------------------------------
    // Existing env guard tests
    // -----------------------------------------------------------------

    #[test]
    fn test_env_guard_restores_on_panic() {
        use std::panic;

        // Set a test env var.
        std::env::set_var("IMPULSE_TEST_PANIC_VAR", "pre_panic_value");

        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            let _guard = EnvGuard::new(&["IMPULSE_TEST_PANIC_VAR"]);
            assert_eq!(
                std::env::var("IMPULSE_TEST_PANIC_VAR").ok(),
                None,
                "var should be removed by guard"
            );
            panic!("simulate panic");
        }));

        assert!(result.is_err(), "panic should be propagated");
        assert_eq!(
            std::env::var("IMPULSE_TEST_PANIC_VAR").unwrap(),
            "pre_panic_value",
            "env var should be restored even after panic"
        );

        // Cleanup.
        std::env::remove_var("IMPULSE_TEST_PANIC_VAR");
    }
}
