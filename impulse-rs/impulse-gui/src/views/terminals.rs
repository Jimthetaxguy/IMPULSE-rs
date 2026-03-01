//! Terminal multiplexer view — spawn and manage agent terminals.
//!
//! Uses `impulse_term::TerminalPanel` for full PTY read/write access,
//! context lifecycle integration, and vt100-based rendering.

use std::collections::{BTreeMap, VecDeque};
use std::fs::OpenOptions;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use impulse_term::context::{ContextHealth, ContextTier, ExtractedInsight};
use impulse_term::TerminalPanel;

use super::terminal_search::TerminalSearch;
use super::{View, ViewId};
use crate::state::SharedState;
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
struct Tab {
    #[allow(dead_code)]
    id: u64,
    label: String,
    agent_name: &'static str,
    panel: TerminalPanel,
}

pub struct TerminalsView {
    tabs: BTreeMap<u64, Tab>,
    active_tab: Option<u64>,
    next_id: u64,
    pub agents: Vec<AgentInfo>,
    max_tabs: usize,
    last_check: Option<Instant>,
    /// Terminal transcript search state.
    pub search: TerminalSearch,
    /// Path to LIVE_INSIGHTS.jsonl for the active project.
    live_insights_path: Option<PathBuf>,
    /// Last injected tier per tab, for detecting tier crossings.
    last_injected_tiers: BTreeMap<u64, ContextTier>,
}

impl TerminalsView {
    pub fn new() -> Self {
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
            live_insights_path,
            last_injected_tiers: BTreeMap::new(),
        }
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

    /// Spawn a new terminal tab for an agent.
    pub fn spawn_tab(&mut self, agent: &AgentInfo, _ctx: &egui::Context) {
        if self.tabs.len() >= self.max_tabs {
            log::warn!("Max tabs reached ({})", self.max_tabs);
            return;
        }

        let id = self.next_id;
        self.next_id += 1;

        let args: Vec<String> = agent.args.iter().map(|s| s.to_string()).collect();
        let working_dir = std::env::current_dir().ok();

        match TerminalPanel::spawn(
            agent.command,
            &args,
            working_dir.as_deref(),
            agent.name,
            id as usize,
        ) {
            Ok(panel) => {
                let tab = Tab {
                    id,
                    label: agent.name.to_string(),
                    agent_name: agent.name,
                    panel,
                };
                self.tabs.insert(id, tab);
                self.active_tab = Some(id);
                log::info!("Spawned tab {} for {}", id, agent.name);
            }
            Err(e) => {
                log::error!("Failed to spawn {}: {}", agent.name, e);
            }
        }
    }

    fn close_tab(&mut self, id: u64) {
        // Merge this tab's insights into HISTORY.jsonl before closing.
        if let Some(tab) = self.tabs.get(&id) {
            self.merge_tab_insights_to_history(id, tab.agent_name, tab.label.clone());
            tab.panel.kill();
        }
        self.tabs.remove(&id);
        self.last_injected_tiers.remove(&id);
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
    pub fn shutdown(&mut self) {
        let tab_info: Vec<(u64, &'static str, String)> = self
            .tabs
            .iter()
            .map(|(&id, tab)| (id, tab.agent_name, tab.label.clone()))
            .collect();

        for (id, agent_name, label) in tab_info {
            self.merge_tab_insights_to_history(id, agent_name, label);
        }

        for tab in self.tabs.values() {
            tab.panel.kill();
        }
        self.tabs.clear();
        self.last_injected_tiers.clear();
        self.active_tab = None;
    }

    /// Tab count for status bar display.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Active agent info for the status bar.
    pub fn active_agent_info(&self) -> Vec<crate::widgets::status_bar::ActiveAgent> {
        self.tabs
            .values()
            .map(|tab| crate::widgets::status_bar::ActiveAgent {
                name: tab.agent_name,
                alive: tab.panel.is_alive(),
            })
            .collect()
    }

    /// Count of alive tabs.
    #[allow(dead_code)]
    pub fn alive_count(&self) -> usize {
        self.tabs.values().filter(|t| t.panel.is_alive()).count()
    }

    /// Collect recent insights from all alive terminal panes.
    ///
    /// Returns formatted strings like `[Claude Code] Modified src/main.rs`
    /// suitable for injecting into the agent panel as cross-pane context.
    pub fn collected_insights(&mut self) -> Vec<String> {
        let mut insights = Vec::new();
        for tab in self.tabs.values_mut() {
            if tab.panel.is_alive() {
                let bridge = tab.panel.context_bridge();
                for insight in bridge.insights().iter().rev().take(5) {
                    insights.push(format!(
                        "[{}] {}: {}",
                        tab.label,
                        insight.insight_type.as_str(),
                        insight.content
                    ));
                }
            }
        }
        insights.dedup();
        insights
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

    /// Run context extraction tick on all alive panels.
    ///
    /// Collects newly extracted insights and persists them to LIVE_INSIGHTS.jsonl.
    pub fn context_tick(&mut self) {
        let mut new_insights: Vec<ExtractedInsight> = Vec::new();

        for tab in self.tabs.values_mut() {
            if tab.panel.is_alive() {
                let extracted = tab.panel.context_bridge().extract_tick();
                if !extracted.is_empty() {
                    new_insights.extend(extracted);
                }
            }
        }

        if !new_insights.is_empty() {
            self.persist_insights(&new_insights);
        }
    }

    /// Append insights to LIVE_INSIGHTS.jsonl.
    fn persist_insights(&self, insights: &[ExtractedInsight]) {
        let Some(path) = &self.live_insights_path else {
            return;
        };

        // Ensure parent directory exists.
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
            log::warn!("Failed to open {:?} for insight persistence", path);
            return;
        };

        for insight in insights {
            if let Ok(json) = serde_json::to_string(insight) {
                let _ = writeln!(file, "{}", json);
            }
        }
    }

    /// Check tier crossings and inject refresh context on threshold changes.
    ///
    /// Tracks `last_injected_tier` per tab. When a tier crossing is detected,
    /// builds refresh context with tier info, cross-pane insights, and recent
    /// GENOME decisions, then injects via the ContextBridge.
    pub fn check_threshold_injections(&mut self, genome_decisions: &[String]) {
        // Phase 1: Collect info immutably — which tabs need injection and what context.
        let mut injections: Vec<(u64, String)> = Vec::new();

        let tab_ids: Vec<u64> = self.tabs.keys().copied().collect();

        for &id in &tab_ids {
            let Some(tab) = self.tabs.get(&id) else {
                continue;
            };
            if !tab.panel.is_alive() {
                continue;
            }
            let current_tier = tab.panel.current_tier();

            // Only inject on meaningful tiers (not None, not PostCompaction).
            let should_inject = matches!(
                current_tier,
                ContextTier::Essential | ContextTier::Critical | ContextTier::Minimal
            );
            if !should_inject {
                continue;
            }

            // Check if this is a new tier crossing.
            let last_tier = self.last_injected_tiers.get(&id).copied();
            if last_tier == Some(current_tier) {
                continue;
            }
            self.last_injected_tiers.insert(id, current_tier);

            // Build refresh context.
            let tier_desc = match current_tier {
                ContextTier::Essential => "Context at ~50%. Prioritizing essential information.",
                ContextTier::Critical => "Context at ~70%. Only critical context follows.",
                ContextTier::Minimal => {
                    "Context at ~80%+. Minimal context — highest priority only."
                }
                _ => continue,
            };

            // Collect cross-pane insights from other alive panes (immutable access).
            let mut cross_pane = Vec::new();
            for (&other_id, other_tab) in &self.tabs {
                if other_id == id || !other_tab.panel.is_alive() {
                    continue;
                }
                for insight in other_tab.panel.insights().iter().rev().take(3) {
                    cross_pane.push(format!(
                        "  - [{}] {}: {}",
                        other_tab.label,
                        insight.insight_type.as_str(),
                        insight.content
                    ));
                }
            }

            let mut refresh = format!("{}\n", tier_desc);
            if !cross_pane.is_empty() {
                refresh.push_str("\nCross-pane activity:\n");
                for line in &cross_pane {
                    refresh.push_str(line);
                    refresh.push('\n');
                }
            }
            if !genome_decisions.is_empty() {
                refresh.push_str("\nRecent decisions:\n");
                for d in genome_decisions.iter().take(5) {
                    refresh.push_str("  - ");
                    refresh.push_str(d);
                    refresh.push('\n');
                }
            }

            injections.push((id, refresh));
        }

        // Phase 2: Inject via ContextBridge (requires &mut).
        for (id, refresh) in injections {
            if let Some(tab) = self.tabs.get_mut(&id) {
                match tab.panel.context_bridge().inject_context(&refresh) {
                    Ok(()) => {
                        log::info!("Injected refresh context into tab {}", id);
                    }
                    Err(e) => {
                        log::warn!("Threshold injection failed for tab {}: {}", id, e);
                    }
                }
            }
        }
    }

    /// Merge a closing tab's insights into HISTORY.jsonl.
    fn merge_tab_insights_to_history(&self, pane_id: u64, agent_name: &str, label: String) {
        let Some(insights_path) = &self.live_insights_path else {
            return;
        };

        // Read LIVE_INSIGHTS.jsonl and filter for this pane.
        let pane_insights = load_live_insights_for_pane(insights_path, pane_id);
        if pane_insights.is_empty() {
            return;
        }

        // Build a HISTORY.jsonl entry.
        let files: Vec<String> = pane_insights
            .iter()
            .filter(|i| i.insight_type == impulse_term::context::InsightType::FileModified)
            .map(|i| i.content.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let summary = format!(
            "GUI session: {} ({} insights, {} files)",
            label,
            pane_insights.len(),
            files.len()
        );

        let entry = serde_json::json!({
            "session_id": format!("gui-pane-{}", pane_id),
            "session_name": label,
            "platform": agent_name,
            "started_at": pane_insights.first().map(|i| i.timestamp.to_rfc3339()).unwrap_or_default(),
            "ended_at": chrono::Utc::now().to_rfc3339(),
            "summary": summary,
            "files_touched": files,
            "tools_used": [],
            "insight_count": pane_insights.len(),
        });

        // Append to HISTORY.jsonl (sibling of LIVE_INSIGHTS.jsonl).
        let history_path = insights_path
            .parent()
            .map(|p| p.join("HISTORY.jsonl"))
            .unwrap_or_default();

        if !history_path.as_os_str().is_empty() {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&history_path)
            {
                if let Ok(json) = serde_json::to_string(&entry) {
                    let _ = writeln!(file, "{}", json);
                    log::info!(
                        "Merged {} insights from pane {} to HISTORY",
                        pane_insights.len(),
                        pane_id
                    );
                }
            }
        }
    }

    /// Load and search live insights for a query (keyword match).
    ///
    /// Returns matching insights as search-result-like tuples: (title, snippet, timestamp).
    pub fn search_live_insights(&self, query: &str) -> Vec<LiveInsightResult> {
        let Some(path) = &self.live_insights_path else {
            return Vec::new();
        };
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        let Ok(content) = std::fs::read_to_string(path) else {
            return Vec::new();
        };

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(insight) = serde_json::from_str::<ExtractedInsight>(line) else {
                continue;
            };
            if insight.content.to_lowercase().contains(&query_lower)
                || insight
                    .insight_type
                    .as_str()
                    .to_lowercase()
                    .contains(&query_lower)
            {
                results.push(LiveInsightResult {
                    title: format!("[{}] {}", insight.insight_type.as_str(), insight.content),
                    agent: insight.agent_kind.label().to_string(),
                    timestamp: insight.timestamp.to_rfc3339(),
                });
            }
        }

        results
    }
}

/// A search result from live insights.
pub struct LiveInsightResult {
    pub title: String,
    pub agent: String,
    pub timestamp: String,
}

/// Load insights from LIVE_INSIGHTS.jsonl filtered by pane ID.
fn load_live_insights_for_pane(path: &std::path::Path, pane_id: u64) -> Vec<ExtractedInsight> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<ExtractedInsight>(line).ok())
        .filter(|i| i.pane_id as u64 == pane_id)
        .collect()
}

impl View for TerminalsView {
    fn id(&self) -> ViewId {
        ViewId::Terminals
    }

    fn ui(&mut self, ui: &mut egui::Ui, _state: &SharedState, _ctx: &egui::Context) {
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
                let tab = self.tabs.get_mut(id).unwrap();
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
                    }

                    // Context health indicator for alive panels.
                    if let Some((usage_fraction, estimated_tokens)) = health_info {
                        let (tier_icon, tier_color) = context_tier_display_from(usage_fraction);
                        let resp =
                            ui.label(egui::RichText::new(tier_icon).small().color(tier_color));
                        resp.on_hover_text(format!(
                            "Context: {:.0}% ({} tokens)",
                            usage_fraction * 100.0,
                            estimated_tokens
                        ));
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
                let agents = self.agents.clone();
                for agent in agents.iter().rev() {
                    let btn = egui::Button::new(egui::RichText::new(agent.name).color(
                        if agent.available {
                            theme::agent_color(agent.name)
                        } else {
                            colors::TEXT_FAINT
                        },
                    ))
                    .small();

                    let resp = ui.add_enabled(agent.available, btn);
                    if resp.clicked() {
                        self.spawn_tab(agent, _ctx);
                    }
                    if !agent.available {
                        resp.on_hover_text(format!("{} not found on PATH", agent.name));
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
                tab.panel.set_focused(true);
                tab.panel.show(ui);
            }
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 6.0);
                ui.heading(
                    egui::RichText::new(WELCOME_BANNER)
                        .monospace()
                        .color(colors::ACCENT),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Persistent memory for AI coding agents")
                        .color(colors::TEXT_MUTED),
                );

                ui.add_space(24.0);

                // Quick action buttons for available agents.
                ui.label(
                    egui::RichText::new("Quick Launch")
                        .strong()
                        .color(colors::TEXT),
                );
                ui.add_space(8.0);

                let agents = self.agents.clone();
                let available: Vec<_> = agents.iter().filter(|a| a.available).collect();

                if available.is_empty() {
                    ui.label(
                        egui::RichText::new("No agents found on PATH.").color(colors::TEXT_DIM),
                    );
                } else {
                    ui.horizontal(|ui| {
                        ui.add_space(
                            (ui.available_width()
                                - (available.len() as f32 * 100.0)
                                - ((available.len() as f32 - 1.0) * 8.0))
                                .max(0.0)
                                / 2.0,
                        );
                        for agent in &available {
                            let color = theme::agent_color(agent.name);
                            let btn =
                                egui::Button::new(egui::RichText::new(agent.name).color(color))
                                    .min_size(egui::vec2(90.0, 32.0))
                                    .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.4)));

                            if ui.add(btn).clicked() {
                                self.spawn_tab(agent, _ctx);
                            }
                            ui.add_space(4.0);
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
    history: &VecDeque<(Instant, f32)>,
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

const WELCOME_BANNER: &str = r"
  ___ __  __ ____  _   _ _     ____  _____
 |_ _|  \/  |  _ \| | | | |   / ___|| ____|
  | || |\/| | |_) | | | | |   \___ \|  _|
  | || |  | |  __/| |_| | |___ ___) | |___
 |___|_|  |_|_|    \___/|_____|____/|_____|
";
