//! Terminal multiplexer view — spawn and manage agent terminals.
//!
//! Uses `impulse_term::TerminalPanel` for full PTY read/write access,
//! context lifecycle integration, and vt100-based rendering.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use impulse_term::context::{ContextHealth, ContextTier, InsightType};
use impulse_term::TerminalPanel;

use crate::widgets::signal_bus::TabBadge;

use super::terminal_search::TerminalSearch;
use super::{View, ViewId};
use crate::state::{PollerCommand, SharedState};
use crate::theme;
use crate::theme::colors;

/// An AI coding agent that Impulse can spawn.
#[derive(Clone)]
pub struct AgentInfo {
    pub name: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub available: bool,
}

/// A single terminal tab.
pub(super) struct Tab {
    #[allow(dead_code)]
    pub(super) id: u64,
    pub(super) label: String,
    pub(super) agent_name: &'static str,
    pub(super) panel: TerminalPanel,
    #[allow(dead_code)]
    pub(super) target_dir: PathBuf,
    /// Daemon session ID, set asynchronously after CreateTabSession round-trip.
    pub(super) daemon_session_id: Option<String>,
    /// Timestamp when this tab was created (used for spawn animation).
    pub(super) created_at: Instant,
}

/// A pending context injection — waiting for agent startup.
pub(super) struct PendingInjection {
    pub(super) tab_id: u64,
    pub(super) inject_at: Instant,
    pub(super) target_dir: PathBuf,
}

/// State snapshot of a tab for detecting changes between context ticks.
#[derive(Default)]
pub(super) struct TabSnapshot {
    pub(super) insight_count: usize,
    pub(super) compaction_count: u32,
    pub(super) tier: Option<ContextTier>,
    pub(super) modified_files: HashSet<String>,
}

pub struct TerminalsView {
    pub(super) tabs: BTreeMap<u64, Tab>,
    active_tab: Option<u64>,
    next_id: u64,
    pub agents: Vec<AgentInfo>,
    max_tabs: usize,
    last_check: Option<Instant>,
    /// Terminal transcript search state.
    pub search: TerminalSearch,
    /// When set, the project selector should open for this agent name.
    pending_spawn_agent: Option<String>,
    /// Pending context injections waiting for agent startup.
    pub(super) pending_injections: Vec<PendingInjection>,
    /// Path to LIVE_INSIGHTS.jsonl for the active project.
    pub(super) live_insights_path: Option<PathBuf>,
    /// Last injected tier per tab, for detecting tier crossings.
    pub(super) last_injected_tiers: BTreeMap<u64, ContextTier>,
    /// State snapshots for signal change detection.
    pub(super) tab_snapshots: BTreeMap<u64, TabSnapshot>,
    /// Tab badges synced from SignalBus.
    tab_badges: BTreeMap<u64, TabBadge>,
    /// Tab whose badges should be acknowledged (set by tab click, consumed by app.rs).
    badge_acknowledged_tab: Option<u64>,
    /// Tabs closed this frame, pending signal_bus.remove_tab() in app.rs.
    closed_tabs: Vec<u64>,
    /// Active file conflicts per tab: (file_path, conflicting_tab_label)
    pub active_conflicts: HashMap<u64, Vec<(String, String)>>,
    /// Channel to send commands to the poller thread for daemon session management.
    pub(super) poller_cmd: Option<std::sync::mpsc::Sender<PollerCommand>>,
    /// Files already tracked with the daemon for each session (dedup guard).
    pub(super) tracked_files: HashMap<String, HashSet<String>>,
}

impl TerminalsView {
    pub fn new(poller_cmd: Option<std::sync::mpsc::Sender<PollerCommand>>) -> Self {
        let agents = vec![
            AgentInfo {
                name: "Claude Code",
                command: "claude",
                args: &[],
                available: which::which("claude").is_ok(),
            },
            AgentInfo {
                name: "OpenCode",
                command: "opencode",
                args: &[],
                available: which::which("opencode").is_ok(),
            },
            AgentInfo {
                name: "Codex",
                command: "codex",
                args: &[],
                available: which::which("codex").is_ok(),
            },
            AgentInfo {
                name: "Shell",
                command: default_shell(),
                args: &[],
                available: true,
            },
        ];

        // Discover project .impulse/ dir from cwd.
        let live_insights_path = std::env::current_dir()
            .ok()
            .map(|cwd| cwd.join(".impulse").join("LIVE_INSIGHTS.jsonl"));

        Self {
            tabs: BTreeMap::new(),
            active_tab: None,
            next_id: 0,
            agents,
            max_tabs: 10,
            last_check: Some(Instant::now()),
            search: TerminalSearch::new(),
            pending_spawn_agent: None,
            pending_injections: Vec::new(),
            live_insights_path,
            last_injected_tiers: BTreeMap::new(),
            tab_snapshots: BTreeMap::new(),
            tab_badges: BTreeMap::new(),
            badge_acknowledged_tab: None,
            closed_tabs: Vec::new(),
            active_conflicts: HashMap::new(),
            poller_cmd,
            tracked_files: HashMap::new(),
        }
    }

    /// Update the live insights path when the user selects a new project.
    pub fn set_project_dir(&mut self, dir: &Path) {
        self.live_insights_path = Some(dir.join(".impulse").join("LIVE_INSIGHTS.jsonl"));
    }

    /// Check aliveness of all panels (replaces PTY event draining).
    pub fn drain_events(&mut self) {
        // TerminalPanel tracks aliveness internally via TerminalBackend.
        // No explicit event channel needed — we check is_alive() during rendering.
    }

    /// Refresh agent PATH availability.
    pub fn refresh_agents(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_check {
            if now.duration_since(last).as_secs() < 5 {
                return;
            }
        }
        self.last_check = Some(now);

        for agent in &mut self.agents {
            agent.available = which::which(agent.command).is_ok();
        }
    }

    /// Handle terminal-specific keyboard shortcuts.
    pub fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        ctx.input(|input| {
            let ctrl = input.modifiers.contains(egui::Modifiers::CTRL);
            let shift = input.modifiers.contains(egui::Modifiers::SHIFT);

            if ctrl && !shift && input.key_pressed(egui::Key::Tab) {
                self.switch_tab(true);
            } else if ctrl && shift && input.key_pressed(egui::Key::Tab) {
                self.switch_tab(false);
            } else if ctrl && input.key_pressed(egui::Key::W) {
                if let Some(id) = self.active_tab {
                    self.close_tab(id);
                }
            }
            // Ctrl+F: Toggle search overlay.
            else if ctrl && input.key_pressed(egui::Key::F) {
                if self.search.active {
                    self.search.close();
                } else {
                    self.search.open();
                }
            }
            // F3: Next match.  Shift+F3: Previous match.
            else if input.key_pressed(egui::Key::F3) {
                if shift {
                    self.search.prev_match();
                } else {
                    self.search.next_match();
                }
                // Focus the tab containing the current match.
                if let Some(m) = self.search.current() {
                    self.active_tab = Some(m.tab_id);
                }
            }
            // Escape closes search when active.
            else if input.key_pressed(egui::Key::Escape) && self.search.active {
                self.search.close();
            }
        });
    }

    /// Spawn a new terminal tab for an agent in the given target directory.
    pub fn spawn_tab(&mut self, agent: &AgentInfo, target_dir: &Path, _ctx: &egui::Context) {
        if self.tabs.len() >= self.max_tabs {
            log::warn!("Max tabs reached ({})", self.max_tabs);
            return;
        }

        let id = self.next_id;
        self.next_id += 1;

        let args: Vec<String> = agent.args.iter().map(|s| s.to_string()).collect();

        match TerminalPanel::spawn(
            agent.command,
            &args,
            Some(target_dir),
            agent.name,
            id as usize,
        ) {
            Ok(panel) => {
                // Tab label: "AgentName: project-folder"
                let project_name = target_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "~".to_string());
                let label = format!("{}: {}", agent.name, project_name);

                let tab = Tab {
                    id,
                    label: label.clone(),
                    agent_name: agent.name,
                    panel,
                    target_dir: target_dir.to_path_buf(),
                    daemon_session_id: None,
                    created_at: Instant::now(),
                };
                self.tabs.insert(id, tab);
                self.active_tab = Some(id);
                log::info!(
                    "Spawned tab {} for {} in {}",
                    id,
                    agent.name,
                    target_dir.display()
                );

                // Request daemon session creation for this tab.
                if let Some(ref cmd_tx) = self.poller_cmd {
                    let session_name =
                        format!("gui-{}-{}", agent.name.to_lowercase().replace(' ', "-"), id);
                    let _ = cmd_tx.send(PollerCommand::CreateTabSession {
                        tab_id: id,
                        name: session_name,
                        platform: agent.name.to_string(),
                    });
                }

                // Schedule init context injection after startup delay.
                let delay = match agent.name {
                    "Claude Code" => Duration::from_secs(3),
                    "OpenCode" | "Codex" => Duration::from_secs(2),
                    _ => Duration::from_millis(500),
                };
                self.pending_injections.push(PendingInjection {
                    tab_id: id,
                    inject_at: Instant::now() + delay,
                    target_dir: target_dir.to_path_buf(),
                });
            }
            Err(e) => {
                log::error!("Failed to spawn {}: {}", agent.name, e);
            }
        }
    }

    /// Take the pending spawn agent request (consumed by app.rs to open the project selector).
    pub fn take_pending_spawn(&mut self) -> Option<String> {
        self.pending_spawn_agent.take()
    }

    fn close_tab(&mut self, id: u64) {
        // Merge this tab's insights into HISTORY.jsonl before closing.
        if let Some(tab) = self.tabs.get(&id) {
            self.merge_tab_insights_to_history(id, tab.agent_name, tab.label.clone());

            // End daemon session if one was created.
            if let Some(ref session_id) = tab.daemon_session_id {
                let summary = self.build_close_summary(id, tab);
                if let Some(ref cmd_tx) = self.poller_cmd {
                    let _ = cmd_tx.send(PollerCommand::EndTabSession {
                        tab_id: id,
                        session_id: session_id.clone(),
                        summary,
                    });
                }
                self.tracked_files.remove(session_id);
            }

            tab.panel.kill();
        }
        self.tabs.remove(&id);
        self.last_injected_tiers.remove(&id);
        self.tab_snapshots.remove(&id);
        self.tab_badges.remove(&id);
        self.closed_tabs.push(id);
        if self.active_tab == Some(id) {
            self.active_tab = self
                .tabs
                .keys()
                .rev()
                .find(|&&k| k < id)
                .or_else(|| self.tabs.keys().next())
                .copied();
        }
    }

    fn switch_tab(&mut self, forward: bool) {
        let ids: Vec<u64> = self.tabs.keys().copied().collect();
        if ids.is_empty() {
            return;
        }
        if let Some(current) = self.active_tab {
            if let Some(pos) = ids.iter().position(|&id| id == current) {
                let next = if forward {
                    (pos + 1) % ids.len()
                } else if pos == 0 {
                    ids.len() - 1
                } else {
                    pos - 1
                };
                self.active_tab = Some(ids[next]);
            }
        } else {
            self.active_tab = Some(ids[0]);
        }
    }

    /// Clean up all tabs (called on exit).
    ///
    /// Merges all remaining pane insights into HISTORY.jsonl before killing.
    /// Ends all daemon sessions with summaries.
    pub fn shutdown(&mut self) {
        let tab_info: Vec<(u64, &'static str, String)> = self
            .tabs
            .iter()
            .map(|(&id, tab)| (id, tab.agent_name, tab.label.clone()))
            .collect();

        for (id, agent_name, label) in &tab_info {
            self.merge_tab_insights_to_history(*id, agent_name, label.clone());
        }

        // End all daemon sessions.
        if let Some(ref cmd_tx) = self.poller_cmd {
            for (&id, tab) in &self.tabs {
                if let Some(ref session_id) = tab.daemon_session_id {
                    let summary = self.build_close_summary(id, tab);
                    let _ = cmd_tx.send(PollerCommand::EndTabSession {
                        tab_id: id,
                        session_id: session_id.clone(),
                        summary,
                    });
                }
            }
        }

        for tab in self.tabs.values() {
            tab.panel.kill();
        }
        // No need to populate closed_tabs — process exits after shutdown.
        self.tabs.clear();
        self.last_injected_tiers.clear();
        self.tab_snapshots.clear();
        self.tab_badges.clear();
        self.tracked_files.clear();
        self.active_tab = None;
    }

    /// Set tab badges from SignalBus (replaces direct field access).
    pub fn set_tab_badges(&mut self, badges: BTreeMap<u64, TabBadge>) {
        self.tab_badges = badges;
    }

    /// Consume the badge acknowledgment (set by tab click).
    pub fn take_badge_ack(&mut self) -> Option<u64> {
        self.badge_acknowledged_tab.take()
    }

    /// Consume closed tab IDs (for signal_bus.remove_tab calls).
    pub fn take_closed_tabs(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.closed_tabs)
    }

    /// Set the daemon session ID for a tab (called from app.rs on TabSessionCreated).
    pub fn set_daemon_session_id(&mut self, tab_id: u64, session_id: String) {
        if let Some(tab) = self.tabs.get_mut(&tab_id) {
            log::info!("Tab {} linked to daemon session {}", tab_id, session_id);
            tab.daemon_session_id = Some(session_id);
        }
    }

    /// Build a summary string for ending a daemon session on tab close.
    fn build_close_summary(&self, id: u64, tab: &Tab) -> String {
        let insights = tab.panel.insights();
        let file_count = insights
            .iter()
            .filter(|i| i.insight_type == InsightType::FileModified)
            .map(|i| &i.content)
            .collect::<HashSet<_>>()
            .len();
        let error_count = insights
            .iter()
            .filter(|i| i.insight_type == InsightType::ErrorEncountered)
            .count();
        format!(
            "GUI tab {} closed: {} insights, {} files, {} errors",
            id,
            insights.len(),
            file_count,
            error_count
        )
    }

    /// Count of alive tabs.
    #[allow(dead_code)]
    pub fn alive_count(&self) -> usize {
        self.tabs.values().filter(|t| t.panel.is_alive()).count()
    }

    /// Inject context into a terminal pane via its ContextBridge.
    pub fn inject_to_tab(&mut self, tab_id: u64, content: &str) -> bool {
        if let Some(tab) = self.tabs.get_mut(&tab_id) {
            match tab.panel.context_bridge().inject_context(content) {
                Ok(()) => return true,
                Err(e) => {
                    log::warn!("Inject to tab {} failed: {}", tab_id, e);
                    return false;
                }
            }
        }
        log::warn!("Tab {} not found for inject", tab_id);
        false
    }

    /// Send raw input to a terminal pane's PTY.
    pub fn send_to_tab(&self, tab_id: u64, content: &str) -> bool {
        if let Some(tab) = self.tabs.get(&tab_id) {
            match tab.panel.write_input(content.as_bytes()) {
                Ok(()) => return true,
                Err(e) => {
                    log::warn!("Send to tab {} failed: {}", tab_id, e);
                    return false;
                }
            }
        }
        log::warn!("Tab {} not found for send", tab_id);
        false
    }

    /// Switch the active terminal tab by ID.
    pub fn focus_tab(&mut self, tab_id: u64) -> bool {
        if self.tabs.contains_key(&tab_id) {
            self.active_tab = Some(tab_id);
            true
        } else {
            log::warn!("Tab {} not found for focus", tab_id);
            false
        }
    }

    fn resolve_tab_id_for_agent(&self, agent_id: &str, session_id: Option<&str>) -> Option<u64> {
        if let Some(tab_id) = agent_id
            .strip_prefix("tab-")
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|tab_id| self.tabs.contains_key(tab_id))
        {
            return Some(tab_id);
        }

        if let Some(session_id) = session_id {
            return self.tabs.iter().find_map(|(tab_id, tab)| {
                let runtime_id = format!("tab-{}", tab_id);
                (runtime_id == session_id || tab.label == session_id).then_some(*tab_id)
            });
        }

        self.tabs.iter().find_map(|(tab_id, tab)| {
            (tab.label == agent_id || tab.agent_name == agent_id).then_some(*tab_id)
        })
    }

    pub fn focus_agent(&mut self, agent_id: &str, session_id: Option<&str>) -> bool {
        self.resolve_tab_id_for_agent(agent_id, session_id)
            .map(|tab_id| self.focus_tab(tab_id))
            .unwrap_or(false)
    }

    pub fn send_to_agent(&self, agent_id: &str, session_id: Option<&str>, content: &str) -> bool {
        self.resolve_tab_id_for_agent(agent_id, session_id)
            .map(|tab_id| self.send_to_tab(tab_id, content))
            .unwrap_or(false)
    }

    /// Open the terminal search overlay with a pre-filled query.
    pub fn search_terminals(&mut self, query: String) {
        self.search.open_with_query(query);
    }

    fn render_ops_roster(&mut self, ui: &mut egui::Ui, state: &SharedState) {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Agent Fleet")
                        .strong()
                        .color(colors::ACCENT),
                );

                if let Some(snapshot) = &state.ops_snapshot {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} daemon agents  {} alerts",
                            snapshot.agents.len(),
                            snapshot.interventions.len()
                        ))
                        .small()
                        .color(colors::TEXT_MUTED),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("Waiting for daemon snapshot")
                            .small()
                            .color(colors::TEXT_DIM),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} local terminals", self.tabs.len()))
                            .small()
                            .color(colors::TEXT_MUTED),
                    );
                });
            });

            ui.add_space(4.0);

            if let Some(snapshot) = &state.ops_snapshot {
                if snapshot.agents.is_empty() {
                    ui.label(
                        egui::RichText::new("No agents reported by the daemon yet.")
                            .color(colors::TEXT_DIM),
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for agent in &snapshot.agents {
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(&agent.label)
                                                .strong()
                                                .color(theme::agent_color(&agent.backend_kind)),
                                        );
                                        if agent.ephemeral {
                                            ui.label(
                                                egui::RichText::new("telemetry")
                                                    .small()
                                                    .color(colors::YELLOW),
                                            );
                                        }
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{}  {}",
                                                agent.backend_kind, agent.status
                                            ))
                                            .small()
                                            .color(colors::TEXT_MUTED),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "Context {}",
                                                        agent.context.tier
                                                    ))
                                                    .small()
                                                    .color(colors::TEXT_MUTED),
                                                );
                                            },
                                        );
                                    });

                                    if let Some(task) = &agent.current_task {
                                        ui.label(
                                            egui::RichText::new(task).small().color(colors::TEXT),
                                        );
                                    }

                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "dir: {}",
                                                agent.working_directory
                                            ))
                                            .small()
                                            .color(colors::TEXT_DIM),
                                        );
                                        ui.separator();
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "reviews: {}",
                                                agent.context.pending_review_count
                                            ))
                                            .small()
                                            .color(colors::TEXT_DIM),
                                        );
                                        ui.separator();
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "compactions: {}",
                                                agent.context.compaction_count
                                            ))
                                            .small()
                                            .color(colors::TEXT_DIM),
                                        );
                                    });

                                    for warning in agent.warnings.iter().take(2) {
                                        ui.label(
                                            egui::RichText::new(warning)
                                                .small()
                                                .color(colors::YELLOW),
                                        );
                                    }
                                });
                                ui.add_space(4.0);
                            }
                        });
                }
            }

            if !self.tabs.is_empty() {
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new("Attached terminals")
                            .small()
                            .color(colors::TEXT_MUTED),
                    );
                    let tab_ids: Vec<u64> = self.tabs.keys().copied().collect();
                    for id in tab_ids {
                        if let Some(tab) = self.tabs.get(&id) {
                            let selected = self.active_tab == Some(id);
                            let label = format!("{} #{}", tab.label, id);
                            if ui.selectable_label(selected, label).clicked() {
                                self.active_tab = Some(id);
                            }
                        }
                    }
                });
            }
        });
    }
}

impl View for TerminalsView {
    fn id(&self) -> ViewId {
        ViewId::Agents
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, _ctx: &egui::Context) {
        self.render_ops_roster(ui, state);
        ui.add_space(6.0);

        // --- Search overlay (Ctrl+F) ---
        if self.search.active {
            // Collect pane texts (borrow tabs only, not search).
            let panes: Vec<(u64, &str, String)> = self
                .tabs
                .iter()
                .map(|(&id, tab)| (id, tab.agent_name, tab.panel.screen_text()))
                .collect();
            self.search.search(&panes);

            if let Some(focus_tab_id) = self.search.ui(ui) {
                self.active_tab = Some(focus_tab_id);
            }
            ui.add_space(2.0);
        }

        // --- Tab bar ---
        ui.horizontal(|ui| {
            let tab_ids: Vec<u64> = self.tabs.keys().copied().collect();
            let mut close_id = None;

            for id in &tab_ids {
                let Some(tab) = self.tabs.get_mut(id) else {
                    continue;
                };
                let is_active = self.active_tab == Some(*id);
                let color = theme::agent_color(tab.agent_name);

                // Capture values we need before entering the closure.
                let is_alive = tab.panel.is_alive();
                let label = tab.label.clone();
                let health_info = if is_alive {
                    let health = tab.panel.context_bridge().health();
                    Some((health.usage_fraction, health.estimated_tokens))
                } else {
                    None
                };
                let badge = self.tab_badges.get(id).cloned().unwrap_or_default();

                ui.horizontal(|ui| {
                    // Alive dot.
                    let dot_color = if is_alive {
                        colors::GREEN
                    } else {
                        colors::TEXT_DIM
                    };
                    let dot_rect = ui.allocate_space(egui::vec2(8.0, 8.0));
                    ui.painter()
                        .circle_filled(dot_rect.1.center(), 3.5, dot_color);

                    let text = egui::RichText::new(&label).color(if is_active {
                        color
                    } else {
                        colors::TEXT_MUTED
                    });
                    if ui.selectable_label(is_active, text).clicked() {
                        self.active_tab = Some(*id);
                        self.badge_acknowledged_tab = Some(*id);
                    }

                    // Context health indicator for alive panels — compact inline budget bar.
                    if let Some((usage_fraction, estimated_tokens)) = health_info {
                        let (bar_rect, _) =
                            ui.allocate_exact_size(egui::vec2(36.0, 8.0), egui::Sense::hover());

                        // Determine color based on usage fraction (matches render_token_budget).
                        let fill_color = if usage_fraction < 0.45 {
                            colors::GREEN
                        } else if usage_fraction < 0.60 {
                            colors::YELLOW
                        } else if usage_fraction < 0.80 {
                            egui::Color32::from_rgb(0xff, 0x7b, 0x72) // red
                        } else {
                            egui::Color32::from_rgb(0xff, 0x45, 0x45) // bright red
                        };

                        let fill_rect = egui::Rect::from_min_size(
                            bar_rect.min,
                            egui::vec2(
                                bar_rect.width() * usage_fraction.clamp(0.0, 1.0),
                                bar_rect.height(),
                            ),
                        );

                        // Paint the mini progress bar (scoped to avoid holding painter across ui mutations).
                        {
                            let painter = ui.painter();
                            painter.rect_filled(bar_rect, 2.0, colors::BG);
                            painter.rect_filled(fill_rect, 2.0, fill_color);
                            painter.rect_stroke(
                                bar_rect,
                                2.0,
                                egui::Stroke::new(0.5, colors::BORDER),
                                egui::StrokeKind::Outside,
                            );
                        }

                        // Percentage label next to the bar.
                        let pct_text = format!("{:.0}%", usage_fraction * 100.0);
                        ui.label(
                            egui::RichText::new(&pct_text)
                                .small()
                                .color(fill_color)
                                .monospace(),
                        )
                        .on_hover_text(format!(
                            "Context: {:.0}% ({} tokens)",
                            usage_fraction * 100.0,
                            estimated_tokens
                        ));
                    }

                    // Signal badges (from signal bus, synced each frame).
                    if badge.has_conflict {
                        ui.label(egui::RichText::new("\u{26A0}").small().color(colors::RED))
                            .on_hover_text("File conflict with another pane");
                    }
                    if badge.has_error {
                        let (_, dot_rect) = ui.allocate_space(egui::vec2(6.0, 6.0));
                        ui.painter()
                            .circle_filled(dot_rect.center(), 2.5, colors::RED);
                    }
                    if badge.has_task_complete {
                        ui.label(egui::RichText::new("\u{2713}").small().color(colors::GREEN))
                            .on_hover_text("Task completed");
                    }
                    if badge.has_compaction {
                        ui.label(
                            egui::RichText::new("\u{21BB}")
                                .small()
                                .color(colors::YELLOW),
                        )
                        .on_hover_text("Context was compacted");
                    }

                    // Search match count badge.
                    if self.search.active {
                        let tab_matches = self.search.matches_in_tab(*id);
                        if tab_matches > 0 {
                            ui.label(
                                egui::RichText::new(format!("[{}]", tab_matches))
                                    .small()
                                    .color(colors::YELLOW),
                            );
                        }
                    }

                    if ui.small_button("\u{00d7}").clicked() {
                        close_id = Some(*id);
                    }
                });

                ui.separator();
            }

            if let Some(id) = close_id {
                self.close_tab(id);
            }

            // Spawn buttons inline in the tab bar.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for i in (0..self.agents.len()).rev() {
                    let name = self.agents[i].name;
                    let available = self.agents[i].available;
                    let btn = egui::Button::new(egui::RichText::new(name).color(if available {
                        theme::agent_color(name)
                    } else {
                        colors::TEXT_FAINT
                    }))
                    .small();

                    let resp = ui.add_enabled(available, btn);
                    if resp.clicked() {
                        self.pending_spawn_agent = Some(name.to_string());
                    }
                    if !available {
                        resp.on_hover_text(format!("{} not found on PATH", name));
                    }
                }
                ui.weak("Spawn:");
            });
        });

        ui.separator();

        // --- Token budget bar for active terminal ---
        if let Some(active_id) = self.active_tab {
            if let Some(tab) = self.tabs.get(&active_id) {
                if tab.panel.is_alive() {
                    let health = tab.panel.context_health();
                    let history = tab.panel.usage_history();
                    render_token_budget(ui, &health, history);
                    ui.add_space(2.0);
                }
            }
        }

        // --- Terminal or welcome ---
        if let Some(active_id) = self.active_tab {
            if let Some(tab) = self.tabs.get_mut(&active_id) {
                // Show conflict banner if any conflicts exist for this tab
                if let Some(conflicts) = self.active_conflicts.get(&active_id) {
                    if let Some(resp) = crate::widgets::conflict_banner::show(ui, conflicts) {
                        if resp.clicked() {
                            // Acknowledge conflict placeholder
                        }
                    }
                    ui.add_space(4.0);
                }

                tab.panel.set_focused(true);
                tab.panel.show(ui);
            }
        } else {
            ui.vertical_centered_justified(|ui| {
                // --- Header zone ---
                ui.add_space(ui.available_height() / 8.0);
                ui.label(
                    egui::RichText::new("IMPULSE")
                        .monospace()
                        .size(28.0)
                        .color(colors::ACCENT),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Persistent memory for AI coding agents")
                        .small()
                        .color(colors::TEXT_DIM),
                );

                // --- Live context zone ---
                ui.add_space(24.0);
                let recent = self
                    .live_insights_path
                    .as_ref()
                    .is_some_and(|p| p.exists())
                    .then(|| {
                        self.live_insights_path
                            .as_ref()
                            .map(|p| load_recent_insights(p))
                    })
                    .flatten();

                egui::Frame::new()
                    .fill(colors::SURFACE)
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Recent Activity")
                                .small()
                                .strong()
                                .color(colors::TEXT_MUTED),
                        );
                        ui.add_space(8.0);

                        if let Some(insights) = recent {
                            if insights.is_empty() {
                                ui.label(
                                    egui::RichText::new(
                                        "No active sessions — start your first agent below.",
                                    )
                                    .small()
                                    .color(colors::TEXT_DIM),
                                );
                            } else {
                                for (itype, content) in &insights {
                                    ui.horizontal(|ui| {
                                        let (badge_color, badge_text) = match itype.as_str() {
                                            "FileModified" => (colors::GREEN, "file"),
                                            "ErrorEncountered" => (colors::RED, "error"),
                                            "DecisionMade" => (colors::ACCENT, "decision"),
                                            "TaskCompleted" => (colors::GREEN, "done"),
                                            _ => (colors::TEXT_DIM, "?"),
                                        };
                                        ui.label(
                                            egui::RichText::new(format!("[{}]", badge_text))
                                                .small()
                                                .monospace()
                                                .color(badge_color),
                                        );
                                        ui.label(
                                            egui::RichText::new(content)
                                                .small()
                                                .color(colors::TEXT),
                                        );
                                    });
                                    ui.add_space(4.0);
                                }
                            }
                        } else {
                            ui.label(
                                egui::RichText::new("No .impulse directory found in this project.")
                                    .small()
                                    .color(colors::TEXT_DIM),
                            );
                        }
                    });

                // --- Action zone: Quick Launch ---
                ui.add_space(24.0);
                ui.label(
                    egui::RichText::new("Start Agent")
                        .small()
                        .strong()
                        .color(colors::TEXT_MUTED),
                );
                ui.add_space(8.0);

                let available_names: Vec<&'static str> = self
                    .agents
                    .iter()
                    .filter(|a| a.available)
                    .map(|a| a.name)
                    .collect();

                if available_names.is_empty() {
                    ui.label(
                        egui::RichText::new("No agents found on PATH.")
                            .small()
                            .color(colors::TEXT_DIM),
                    );
                } else {
                    ui.horizontal_wrapped(|ui| {
                        for &name in &available_names {
                            let color = theme::agent_color(name);
                            let label = match name {
                                "Claude Code" => "Start Claude Code",
                                "Codex" => "Start Codex",
                                "OpenCode" => "Start OpenCode",
                                _ => name,
                            };
                            let btn = egui::Button::new(egui::RichText::new(label).color(color))
                                .min_size(egui::vec2(120.0, 28.0))
                                .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.3)));

                            if ui.add(btn).clicked() {
                                self.pending_spawn_agent = Some(name.to_string());
                            }
                            ui.add_space(8.0);
                        }
                    });
                }

                ui.add_space(24.0);

                // Shortcuts reference.
                egui::Frame::new()
                    .fill(colors::SURFACE)
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Keyboard Shortcuts")
                                .small()
                                .strong()
                                .color(colors::TEXT_MUTED),
                        );
                        ui.add_space(4.0);
                        for (key, action) in [
                            ("Ctrl+N", "New terminal tab"),
                            ("Ctrl+W", "Close active tab"),
                            ("Ctrl+Tab", "Next tab"),
                            ("Ctrl+L", "Focus agent panel"),
                            ("Ctrl+5", "Toggle agent panel"),
                            ("Ctrl+B", "Toggle sidebar"),
                            ("Ctrl+K", "Search"),
                        ] {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(key)
                                        .small()
                                        .monospace()
                                        .color(colors::ACCENT),
                                );
                                ui.label(
                                    egui::RichText::new(action).small().color(colors::TEXT_DIM),
                                );
                            });
                        }
                    });
            });
        }
    }
}

/// Render a compact token budget bar + sparkline for the active terminal.
fn render_token_budget(
    ui: &mut egui::Ui,
    health: &ContextHealth,
    history: Arc<VecDeque<(Instant, f32)>>,
) {
    let bar_height = 16.0;
    let available_width = ui.available_width();

    egui::Frame::new()
        .fill(colors::SURFACE)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .corner_radius(egui::CornerRadius::same(4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // --- Label ---
                ui.label(
                    egui::RichText::new("Context:")
                        .small()
                        .color(colors::TEXT_DIM),
                );

                // --- Progress bar ---
                let bar_width = (available_width * 0.35).clamp(80.0, 200.0);
                let (bar_rect, _) =
                    ui.allocate_exact_size(egui::vec2(bar_width, bar_height), egui::Sense::hover());

                // Filled portion.
                let fill_fraction = health.usage_fraction.clamp(0.0, 1.0);
                let fill_color = if fill_fraction < 0.45 {
                    colors::GREEN
                } else if fill_fraction < 0.60 {
                    colors::YELLOW
                } else if fill_fraction < 0.80 {
                    egui::Color32::from_rgb(0xff, 0x7b, 0x72) // red
                } else {
                    egui::Color32::from_rgb(0xff, 0x45, 0x45) // bright red
                };

                let fill_rect = egui::Rect::from_min_size(
                    bar_rect.min,
                    egui::vec2(bar_rect.width() * fill_fraction, bar_rect.height()),
                );

                // Paint the bar (scoped to avoid holding painter across ui mutations).
                {
                    let painter = ui.painter();
                    painter.rect_filled(bar_rect, 3.0, colors::BG);
                    painter.rect_filled(fill_rect, 3.0, fill_color);
                    painter.rect_stroke(
                        bar_rect,
                        3.0,
                        egui::Stroke::new(0.5, colors::BORDER),
                        egui::StrokeKind::Outside,
                    );
                }

                // --- Percentage text ---
                ui.label(
                    egui::RichText::new(format!(
                        "{:.0}% ({}/{}K)",
                        fill_fraction * 100.0,
                        health.estimated_tokens / 1000,
                        health.window_tokens / 1000
                    ))
                    .small()
                    .color(colors::TEXT_MUTED),
                );

                // --- Sparkline (mini chart of usage over time) ---
                if history.len() >= 2 {
                    ui.separator();
                    let sparkline_width = (available_width * 0.2).clamp(40.0, 120.0);
                    let sparkline_height = bar_height;
                    let (spark_rect, _) = ui.allocate_exact_size(
                        egui::vec2(sparkline_width, sparkline_height),
                        egui::Sense::hover(),
                    );

                    // Paint sparkline (scoped).
                    {
                        let painter = ui.painter();
                        painter.rect_filled(spark_rect, 2.0, colors::BG);

                        // Draw sparkline as connected line segments.
                        let points: Vec<egui::Pos2> = history
                            .iter()
                            .enumerate()
                            .map(|(i, (_, frac))| {
                                let x = spark_rect.min.x
                                    + (i as f32 / (history.len() - 1).max(1) as f32)
                                        * spark_rect.width();
                                let y =
                                    spark_rect.max.y - frac.clamp(0.0, 1.0) * spark_rect.height();
                                egui::pos2(x, y)
                            })
                            .collect();

                        if points.len() >= 2 {
                            let stroke = egui::Stroke::new(1.5, fill_color.gamma_multiply(0.8));
                            for window in points.windows(2) {
                                painter.line_segment([window[0], window[1]], stroke);
                            }
                        }
                    }
                }

                // --- Compaction/injection counters ---
                if health.compaction_count > 0 || health.injection_count > 0 {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if health.injection_count > 0 {
                            ui.label(
                                egui::RichText::new(format!("\u{2193}{}", health.injection_count))
                                    .small()
                                    .color(colors::ACCENT),
                            )
                            .on_hover_text("Injections");
                        }
                        if health.compaction_count > 0 {
                            ui.label(
                                egui::RichText::new(format!("\u{21BB}{}", health.compaction_count))
                                    .small()
                                    .color(colors::YELLOW),
                            )
                            .on_hover_text("Compactions");
                        }
                    });
                }
            });
        });
}

/// Map usage_fraction to display icon and color.
fn context_tier_display_from(usage_fraction: f32) -> (&'static str, egui::Color32) {
    if usage_fraction < 0.45 {
        ("\u{25CF}", colors::GREEN) // ● green
    } else if usage_fraction < 0.60 {
        ("\u{25D0}", colors::YELLOW) // ◐ yellow
    } else if usage_fraction < 0.80 {
        ("\u{25D1}", colors::RED) // ◑ red
    } else {
        ("\u{25CB}", colors::RED) // ○ red
    }
}

fn default_shell() -> &'static str {
    #[cfg(unix)]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        Box::leak(shell.into_boxed_str())
    }
    #[cfg(windows)]
    {
        "cmd.exe"
    }
}

/// Build the enriched init context payload for a newly-spawned agent pane.
///
/// Includes identity, project info, standing GENOME decisions, last session
/// summary, and a tools reference. Sections are omitted when data is absent.
pub(super) fn build_init_context(identity: &str, target_dir: &Path, agent_name: &str) -> String {
    let project_impulse_dir = target_dir.join(".impulse");

    let decisions = crate::project_context::load_recent_decisions(&project_impulse_dir, 5);
    let last_session = crate::project_context::load_last_session(&project_impulse_dir);

    let mut sections = Vec::new();

    // Identity (from ~/.impulse/CLAUDE.md).
    sections.push(identity.trim().to_string());

    // Project info.
    sections.push(format!(
        "## Your Project\nWorking directory: {}\nPane: {}",
        target_dir.display(),
        agent_name
    ));

    // Standing decisions from GENOME.
    if !decisions.is_empty() {
        let mut text = String::from("## Standing Decisions (from GENOME)");
        for d in &decisions {
            let ts = d.timestamp.as_deref().unwrap_or("unknown");
            text.push_str(&format!("\n- {} ({})", d.description, ts));
        }
        sections.push(text);
    }

    // Last session summary from HISTORY.
    if let Some(session) = last_session {
        let mut text = String::from("## Last Session Summary");
        if let Some(ref summary) = session.summary {
            text.push_str(&format!("\n- {}", summary));
        }
        if !session.files_touched.is_empty() {
            let files: Vec<&str> = session
                .files_touched
                .iter()
                .take(10)
                .map(|s| s.as_str())
                .collect();
            text.push_str(&format!("\n- Modified: {}", files.join(", ")));
        }
        sections.push(text);
    }

    // Tools reference (always included).
    sections.push(
        "## Available Tools\n\
         Run `impulse-rs sync-context` to refresh context at any time.\n\
         Run `impulse-rs add-decision \"description\" --rationale \"why\"` to record decisions."
            .to_string(),
    );

    format!(
        "<impulse-context type=\"init\" version=\"3\">\n{}\n</impulse-context>",
        sections.join("\n\n")
    )
}

/// Load the 3 most recent insights from a LIVE_INSIGHTS.jsonl file.
/// Returns (insight_type_label, truncated_content) pairs.
fn load_recent_insights(path: &Path) -> Vec<(String, String)> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|v| {
            let t = v
                .get("insight_type")?
                .as_str()?
                .strip_prefix("InsightType::")
                .unwrap_or("?")
                .to_string();
            let c = v
                .get("content")?
                .as_str()
                .map(|s| {
                    if s.len() > 60 {
                        format!("{}...", &s[..57])
                    } else {
                        s.to_string()
                    }
                })
                .unwrap_or_default();
            Some((t, c))
        })
        .rev()
        .take(3)
        .collect()
}

const WELCOME_BANNER: &str = r"
  ___ __  __ ____  _   _ _     ____  _____
 |_ _|  \/  |  _ \| | | | |   / ___|| ____|
  | || |\/| | |_) | | | | |   \___ \|  _|
  | || |  | |  __/| |_| | |___ ___) | |___
 |___|_|  |_|_|    \___/|_____|____/|_____|
";

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_build_init_context_minimal() {
        let dir = TempDir::new().unwrap();
        let result = build_init_context("# Identity", dir.path(), "Claude Code");
        assert!(result.contains("version=\"3\""));
        assert!(result.contains("# Identity"));
        assert!(result.contains("Pane: Claude Code"));
        assert!(result.contains("## Available Tools"));
        // No GENOME or HISTORY => no Standing Decisions or Last Session.
        assert!(!result.contains("Standing Decisions"));
        assert!(!result.contains("Last Session"));
    }

    #[test]
    fn test_build_init_context_with_genome() {
        let dir = TempDir::new().unwrap();
        let impulse_dir = dir.path().join(".impulse");
        std::fs::create_dir_all(&impulse_dir).unwrap();
        std::fs::write(
            impulse_dir.join("GENOME.md"),
            r#"{"decisions":[
                {"date":"2026-02-01T00:00:00Z","description":"Use Rust","rationale":null,"tags":[]},
                {"date":"2026-02-02T00:00:00Z","description":"Use egui","rationale":null,"tags":[]}
            ],"preferences":[],"constraints":[],"last_updated":null}"#,
        )
        .unwrap();

        let result = build_init_context("identity", dir.path(), "test");
        assert!(result.contains("Standing Decisions"));
        assert!(result.contains("Use Rust"));
        assert!(result.contains("Use egui"));
    }

    #[test]
    fn test_build_init_context_with_history() {
        let dir = TempDir::new().unwrap();
        let impulse_dir = dir.path().join(".impulse");
        std::fs::create_dir_all(&impulse_dir).unwrap();
        std::fs::write(
            impulse_dir.join("HISTORY.jsonl"),
            r#"{"session_id":"s1","session_name":"test","summary":"Fixed auth bug","files_touched":["src/auth.rs","src/main.rs"],"tools_used":[],"started_at":"2026-01-01T00:00:00Z","ended_at":"2026-01-01T01:00:00Z"}"#,
        )
        .unwrap();

        let result = build_init_context("identity", dir.path(), "test");
        assert!(result.contains("Last Session Summary"));
        assert!(result.contains("Fixed auth bug"));
        assert!(result.contains("src/auth.rs"));
    }

    #[test]
    fn test_build_init_context_version_3() {
        let dir = TempDir::new().unwrap();
        let result = build_init_context("id", dir.path(), "agent");
        assert!(result.starts_with("<impulse-context type=\"init\" version=\"3\">"));
        assert!(result.ends_with("</impulse-context>"));
    }
}
