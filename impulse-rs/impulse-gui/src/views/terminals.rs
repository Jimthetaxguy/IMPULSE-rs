//! Terminal multiplexer view — spawn and manage agent terminals.
//!
//! Uses `impulse_term::TerminalPanel` for full PTY read/write access,
//! context lifecycle integration, and vt100-based rendering.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use impulse_term::context::{ContextHealth, ContextTier, ExtractedInsight, InsightType};
use impulse_term::TerminalPanel;

use crate::widgets::signal_bus::{GuiSignal, SignalKind, SignalUrgency, TabBadge};

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
struct Tab {
    #[allow(dead_code)]
    id: u64,
    label: String,
    agent_name: &'static str,
    panel: TerminalPanel,
    #[allow(dead_code)]
    target_dir: PathBuf,
    /// Daemon session ID, set asynchronously after CreateTabSession round-trip.
    daemon_session_id: Option<String>,
}

/// A pending context injection — waiting for agent startup.
struct PendingInjection {
    tab_id: u64,
    inject_at: Instant,
    target_dir: PathBuf,
}

/// State snapshot of a tab for detecting changes between context ticks.
#[derive(Default)]
struct TabSnapshot {
    insight_count: usize,
    compaction_count: u32,
    tier: Option<ContextTier>,
    modified_files: HashSet<String>,
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
    /// When set, the project selector should open for this agent name.
    pending_spawn_agent: Option<String>,
    /// Pending context injections waiting for agent startup.
    pending_injections: Vec<PendingInjection>,
    /// Path to LIVE_INSIGHTS.jsonl for the active project.
    live_insights_path: Option<PathBuf>,
    /// Last injected tier per tab, for detecting tier crossings.
    last_injected_tiers: BTreeMap<u64, ContextTier>,
    /// State snapshots for signal change detection.
    tab_snapshots: BTreeMap<u64, TabSnapshot>,
    /// Tab badges synced from SignalBus.
    tab_badges: BTreeMap<u64, TabBadge>,
    /// Tab whose badges should be acknowledged (set by tab click, consumed by app.rs).
    badge_acknowledged_tab: Option<u64>,
    /// Tabs closed this frame, pending signal_bus.remove_tab() in app.rs.
    closed_tabs: Vec<u64>,
    /// Active file conflicts per tab: (file_path, conflicting_tab_label)
    pub active_conflicts: HashMap<u64, Vec<(String, String)>>,
    /// Channel to send commands to the poller thread for daemon session management.
    poller_cmd: Option<std::sync::mpsc::Sender<PollerCommand>>,
    /// Files already tracked with the daemon for each session (dedup guard).
    tracked_files: HashMap<String, HashSet<String>>,
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

    /// Process pending init injections (called from app.rs update loop).
    pub fn process_pending_injections(&mut self, impulse_home: &Path) {
        let now = Instant::now();

        // Drain ready injections (stable alternative to nightly drain_filter).
        let mut i = 0;
        while i < self.pending_injections.len() {
            if now >= self.pending_injections[i].inject_at {
                let pending = self.pending_injections.remove(i);

                if let Some(tab) = self.tabs.get_mut(&pending.tab_id) {
                    let identity = crate::identity::load_identity(impulse_home).unwrap_or_default();

                    let project_name = pending
                        .target_dir
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    let context =
                        build_init_context(&identity, &pending.target_dir, tab.agent_name);

                    match tab.panel.context_bridge().inject_context(&context) {
                        Ok(()) => log::info!(
                            "Injected init context into tab {} ({})",
                            pending.tab_id,
                            project_name
                        ),
                        Err(e) => log::warn!(
                            "Failed to inject init context into tab {}: {}",
                            pending.tab_id,
                            e
                        ),
                    }
                }
            } else {
                i += 1;
            }
        }
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

    pub fn workbench_agents(&self) -> Vec<impulse_ops::AgentRuntime> {
        self.tabs
            .iter()
            .map(|(id, tab)| {
                let health = tab.panel.context_health();
                let recent_insights = tab
                    .panel
                    .insights()
                    .iter()
                    .rev()
                    .take(5)
                    .map(|insight| impulse_ops::InsightRecord {
                        timestamp: Some(insight.timestamp.to_rfc3339()),
                        agent_label: tab.agent_name.to_string(),
                        kind: insight.insight_type.as_str().to_string(),
                        content: insight.content.clone(),
                    })
                    .collect::<Vec<_>>();

                let mut warnings = Vec::new();
                if matches!(health.tier, ContextTier::Critical | ContextTier::Minimal) {
                    warnings.push(format!(
                        "Context tier is {} and needs review soon",
                        health.tier.as_str()
                    ));
                }
                if !tab.panel.is_alive() {
                    warnings.push("Terminal process is no longer alive".to_string());
                }

                impulse_ops::AgentRuntime {
                    id: format!("tab-{}", id),
                    label: tab.label.clone(),
                    backend_kind: tab.agent_name.to_string(),
                    session_id: None,
                    ephemeral: true,
                    working_directory: tab.target_dir.display().to_string(),
                    status: if tab.panel.is_alive() {
                        "active".to_string()
                    } else {
                        "stopped".to_string()
                    },
                    current_task: recent_insights
                        .first()
                        .map(|insight| insight.content.clone()),
                    active: tab.panel.is_alive(),
                    context: impulse_ops::ContextHealthSummary {
                        tier: health.tier.as_str().to_string(),
                        usage_fraction: health.usage_fraction,
                        estimated_tokens: health.estimated_tokens,
                        window_tokens: health.window_tokens,
                        compaction_count: health.compaction_count,
                        injection_count: health.injection_count,
                        pending_review_count: self.pending_injections.len(),
                        recent_insights,
                    },
                    recent_files: tab
                        .panel
                        .insights()
                        .iter()
                        .filter(|insight| insight.insight_type == InsightType::FileModified)
                        .map(|insight| insight.content.clone())
                        .collect(),
                    recent_tools: Vec::new(),
                    warnings,
                }
            })
            .collect()
    }

    pub fn workbench_context(&self) -> impulse_ops::ContextHealthSummary {
        let mut summary = impulse_ops::ContextHealthSummary {
            tier: "steady".to_string(),
            pending_review_count: self.pending_injections.len(),
            ..Default::default()
        };

        let mut recent_insights = Vec::new();
        for tab in self.tabs.values() {
            let health = tab.panel.context_health();
            if health.usage_fraction > summary.usage_fraction {
                summary.usage_fraction = health.usage_fraction;
                summary.tier = health.tier.as_str().to_string();
                summary.estimated_tokens = health.estimated_tokens;
                summary.window_tokens = health.window_tokens;
            }
            summary.compaction_count += health.compaction_count;
            summary.injection_count += health.injection_count;
            recent_insights.extend(tab.panel.insights().iter().rev().take(4).map(|insight| {
                impulse_ops::InsightRecord {
                    timestamp: Some(insight.timestamp.to_rfc3339()),
                    agent_label: tab.agent_name.to_string(),
                    kind: insight.insight_type.as_str().to_string(),
                    content: insight.content.clone(),
                }
            }));
        }
        recent_insights.truncate(20);
        summary.recent_insights = recent_insights;
        summary
    }

    pub fn workbench_interventions(&self) -> Vec<impulse_ops::InterventionRecommendation> {
        let mut interventions = Vec::new();
        for agent in self.workbench_agents() {
            if matches!(agent.context.tier.as_str(), "critical" | "minimal") {
                interventions.push(impulse_ops::InterventionRecommendation {
                    id: format!("review-{}", agent.id),
                    title: format!("Review {}", agent.label),
                    description: format!(
                        "{} is at context tier {} ({} tokens of {}).",
                        agent.label,
                        agent.context.tier,
                        agent.context.estimated_tokens,
                        agent.context.window_tokens
                    ),
                    severity: if agent.context.tier == "minimal" {
                        "urgent".to_string()
                    } else {
                        "warning".to_string()
                    },
                    action_kind: "focus_agent".to_string(),
                    action_label: "Focus Agent".to_string(),
                    target_agent_id: Some(agent.id.clone()),
                });
            }
        }
        interventions
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

    /// Run context extraction tick on all alive panels.
    ///
    /// Collects newly extracted insights and persists them to LIVE_INSIGHTS.jsonl.
    /// Also forwards FileModified insights to the daemon for session tracking.
    pub fn context_tick(&mut self) {
        let mut new_insights: Vec<ExtractedInsight> = Vec::new();
        let mut file_tracks: Vec<(String, String)> = Vec::new(); // (session_id, file_path)

        for tab in self.tabs.values_mut() {
            if tab.panel.is_alive() {
                let extracted = tab.panel.context_bridge().extract_tick();
                if !extracted.is_empty() {
                    // Track new file modifications with daemon.
                    if let Some(ref session_id) = tab.daemon_session_id {
                        for insight in &extracted {
                            if insight.insight_type == InsightType::FileModified {
                                file_tracks.push((session_id.clone(), insight.content.clone()));
                            }
                        }
                    }
                    new_insights.extend(extracted);
                }
            }
        }

        if !new_insights.is_empty() {
            self.persist_insights(&new_insights);
        }

        // Forward file tracks to daemon (deduplicating per session).
        if let Some(ref cmd_tx) = self.poller_cmd {
            for (session_id, file_path) in file_tracks {
                let already_tracked = self
                    .tracked_files
                    .entry(session_id.clone())
                    .or_default()
                    .contains(&file_path);
                if !already_tracked {
                    self.tracked_files
                        .entry(session_id.clone())
                        .or_default()
                        .insert(file_path.clone());
                    let _ = cmd_tx.send(PollerCommand::TrackFile {
                        session_id,
                        file_path,
                    });
                }
            }
        }
    }

    /// Compare tab states against snapshots and emit signals for changes.
    ///
    /// Called after `context_tick()` in the 3-second tick block. Detects:
    /// - New errors/task completions from insight diffs
    /// - Compaction events from compaction count changes
    /// - Context tier crossings (60%, 80%)
    /// - Cross-tab file conflicts from modified_files set intersection
    pub fn collect_signals(&mut self) -> Vec<GuiSignal> {
        if self.tabs.is_empty() {
            return Vec::new();
        }
        let mut signals = Vec::new();
        let now = Instant::now();
        let tab_ids: Vec<u64> = self.tabs.keys().copied().collect();

        // Phase 1: Collect per-tab signals by comparing against snapshots.
        for &id in &tab_ids {
            let Some(tab) = self.tabs.get(&id) else {
                continue;
            };
            if !tab.panel.is_alive() {
                continue;
            }

            let health = tab.panel.context_health();
            let insights = tab.panel.insights();
            let snapshot = self.tab_snapshots.entry(id).or_default();

            // Check for new insights since last snapshot.
            if insights.len() > snapshot.insight_count {
                let new_insights = &insights[snapshot.insight_count..];
                for insight in new_insights {
                    match insight.insight_type {
                        InsightType::ErrorEncountered => {
                            signals.push(GuiSignal {
                                kind: SignalKind::ErrorEncountered,
                                urgency: SignalUrgency::Important,
                                tab_id: Some(id),
                                message: format!(
                                    "[{}] Error: {}",
                                    tab.label,
                                    impulse_term::context::truncate_insight(&insight.content, 80)
                                ),
                                created_at: now,
                            });
                        }
                        InsightType::TaskCompleted => {
                            signals.push(GuiSignal {
                                kind: SignalKind::TaskCompleted,
                                urgency: SignalUrgency::Important,
                                tab_id: Some(id),
                                message: format!(
                                    "[{}] Task completed: {}",
                                    tab.label,
                                    impulse_term::context::truncate_insight(&insight.content, 60)
                                ),
                                created_at: now,
                            });
                        }
                        InsightType::FileModified => {
                            snapshot.modified_files.insert(insight.content.clone());
                        }
                        InsightType::DecisionMade => {}
                    }
                }
                snapshot.insight_count = insights.len();
            }

            // Check compaction count changes.
            if health.compaction_count > snapshot.compaction_count {
                signals.push(GuiSignal {
                    kind: SignalKind::CompactionDetected,
                    urgency: SignalUrgency::Important,
                    tab_id: Some(id),
                    message: format!(
                        "[{}] Context compacted \u{2014} some memory was lost",
                        tab.label
                    ),
                    created_at: now,
                });
                snapshot.compaction_count = health.compaction_count;
            }

            // Check tier crossings.
            let current_tier = health.tier;
            let previous_tier = snapshot.tier;
            if previous_tier != Some(current_tier) {
                match current_tier {
                    ContextTier::Critical => {
                        signals.push(GuiSignal {
                            kind: SignalKind::ContextThreshold { pct: 60 },
                            urgency: SignalUrgency::Important,
                            tab_id: Some(id),
                            message: format!(
                                "[{}] Context at 60% \u{2014} consider compacting soon",
                                tab.label
                            ),
                            created_at: now,
                        });
                    }
                    ContextTier::Minimal => {
                        signals.push(GuiSignal {
                            kind: SignalKind::ContextThreshold { pct: 80 },
                            urgency: SignalUrgency::Urgent,
                            tab_id: Some(id),
                            message: format!(
                                "[{}] Context at 80% \u{2014} compact or start fresh",
                                tab.label
                            ),
                            created_at: now,
                        });
                    }
                    _ => {}
                }
                snapshot.tier = Some(current_tier);
            }
        }

        // Phase 2: Single-pass conflict detection via file→owners map.
        let mut file_owners: HashMap<&str, Vec<(u64, &str)>> = HashMap::new();
        for &id in &tab_ids {
            if let Some(snap) = self.tab_snapshots.get(&id) {
                let label = self.tabs.get(&id).map(|t| t.label.as_str()).unwrap_or("");
                for file in &snap.modified_files {
                    file_owners
                        .entry(file.as_str())
                        .or_default()
                        .push((id, label));
                }
            }
        }

        self.active_conflicts.clear();
        for (file, owners) in &file_owners {
            if owners.len() < 2 {
                continue;
            }
            let mut push_conflict = |tab_id: u64, other_label: &str| {
                signals.push(GuiSignal {
                    kind: SignalKind::FileConflict {
                        path: file.to_string(),
                        other_tab: other_label.to_string(),
                    },
                    urgency: SignalUrgency::Urgent,
                    tab_id: Some(tab_id),
                    message: format!(
                        "Conflict: {} edited in both tabs (also in {})",
                        file, other_label
                    ),
                    created_at: now,
                });
            };
            for i in 0..owners.len() {
                for j in (i + 1)..owners.len() {
                    let (id_a, label_a) = owners[i];
                    let (id_b, label_b) = owners[j];
                    // Emit one signal per direction (A sees B, B sees A).
                    push_conflict(id_a, label_b);
                    self.active_conflicts
                        .entry(id_a)
                        .or_default()
                        .push((file.to_string(), label_b.to_string()));

                    push_conflict(id_b, label_a);
                    self.active_conflicts
                        .entry(id_b)
                        .or_default()
                        .push((file.to_string(), label_a.to_string()));
                }
            }
        }

        signals
    }

    /// Append insights to LIVE_INSIGHTS.jsonl.
    fn persist_insights(&self, insights: &[ExtractedInsight]) {
        let Some(path) = &self.live_insights_path else {
            return;
        };
        super::memory_persistence::persist_insights_to_file(path, insights);
    }

    /// Check tier crossings and inject refresh context on threshold changes.
    ///
    /// Tracks `last_injected_tier` per tab. When a tier crossing is detected,
    /// builds refresh context with tier info, cross-pane insights, and recent
    /// GENOME decisions, then injects via the ContextBridge.
    pub fn check_threshold_injections(
        &mut self,
        genome_decisions: &[String],
        active_sessions: &[String],
        recent_history: &[String],
    ) {
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

            // Build refresh context via extracted pure function.
            if let Some(refresh) = super::memory_persistence::build_refresh_context(
                current_tier,
                &cross_pane,
                genome_decisions,
                active_sessions,
                recent_history,
            ) {
                injections.push((id, refresh));
            }
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
        let history_path = insights_path
            .parent()
            .map(|p| p.join("HISTORY.jsonl"))
            .unwrap_or_default();
        if !history_path.as_os_str().is_empty() {
            super::memory_persistence::merge_pane_to_history(
                insights_path,
                &history_path,
                pane_id,
                agent_name,
                &label,
            );
        }
    }

    /// Load and search live insights for a query (keyword match).
    pub fn search_live_insights(
        &self,
        query: &str,
    ) -> Vec<super::memory_persistence::LiveInsightResult> {
        let Some(path) = &self.live_insights_path else {
            return Vec::new();
        };
        super::memory_persistence::search_insights(path, query)
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

                let available_names: Vec<&'static str> = self
                    .agents
                    .iter()
                    .filter(|a| a.available)
                    .map(|a| a.name)
                    .collect();

                if available_names.is_empty() {
                    ui.label(
                        egui::RichText::new("No agents found on PATH.").color(colors::TEXT_DIM),
                    );
                } else {
                    ui.horizontal(|ui| {
                        ui.add_space(
                            (ui.available_width()
                                - (available_names.len() as f32 * 100.0)
                                - ((available_names.len() as f32 - 1.0) * 8.0))
                                .max(0.0)
                                / 2.0,
                        );
                        for &name in &available_names {
                            let color = theme::agent_color(name);
                            let btn = egui::Button::new(egui::RichText::new(name).color(color))
                                .min_size(egui::vec2(90.0, 32.0))
                                .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.4)));

                            if ui.add(btn).clicked() {
                                self.pending_spawn_agent = Some(name.to_string());
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

/// Build the enriched init context payload for a newly-spawned agent pane.
///
/// Includes identity, project info, standing GENOME decisions, last session
/// summary, and a tools reference. Sections are omitted when data is absent.
fn build_init_context(identity: &str, target_dir: &Path, agent_name: &str) -> String {
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
