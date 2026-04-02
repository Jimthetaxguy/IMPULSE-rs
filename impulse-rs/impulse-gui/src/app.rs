//! ImpulseApp — egui operator workbench for managing coding agents.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::agent_panel::actions::{PanelAction, ProposalExecutionMode};
use crate::agent_panel::AgentPanel;
use crate::global_config::GlobalConfig;
use crate::state::{
    self, ConnectionStatus, PollerCommand, PollerEvent, StateHandle, TaskNoticeLevel,
};
use crate::views::guardrails::GuardrailsView;
use crate::views::memory::MemoryView;
use crate::views::overview::OverviewView;
use crate::views::settings::SettingsView;
use crate::views::terminals::TerminalsView;
use crate::views::{View, ViewId};
use crate::widgets::command_palette::CommandPalette;
use crate::widgets::notifications::{NotificationManager, Severity};
use crate::widgets::project_selector::ProjectSelector;
use crate::widgets::signal_bus::{SignalBus, SignalKind, SignalUrgency};
use crate::widgets::{sidebar, status_bar};

pub struct ImpulseApp {
    overview: OverviewView,
    terminals: TerminalsView,
    memory: MemoryView,
    guardrails: GuardrailsView,
    settings: SettingsView,
    active_view: ViewId,

    sidebar_expanded: bool,
    agent_panel: AgentPanel,
    agent_visible: bool,
    last_context_inject: Instant,

    shared_state: StateHandle,
    poller_cmd: Sender<PollerCommand>,
    poller_events: Receiver<PollerEvent>,
    _poller_thread: Option<JoinHandle<()>>,
    ops_source_id: String,
    last_terminal_ops_publish: Instant,
    last_published_terminal_ops: Option<impulse_ops::TerminalOpsReport>,
    last_memory_view_active: bool,

    last_context_tick: Instant,
    project_selector: ProjectSelector,
    global_config: GlobalConfig,
    current_project: Option<PathBuf>,
    last_search_query: String,

    notifications: NotificationManager,
    signal_bus: SignalBus,
    show_shortcuts_help: bool,
    command_palette: CommandPalette,
}

impl ImpulseApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let impulse_home = GlobalConfig::impulse_home();
        let global_config = GlobalConfig::load(&impulse_home).unwrap_or_default();
        let initial_settings = state::RuntimeSettings::from_map(&global_config.settings);
        let (shared_state, poller_cmd, poller_events, poller_thread) =
            state::start_poller(cc.egui_ctx.clone(), initial_settings);
        let project_selector = ProjectSelector::new(global_config.recent_projects.clone());

        if let Err(e) = crate::identity::ensure_identity_files(&impulse_home) {
            log::warn!("Failed to create identity files: {}", e);
        }

        Self {
            overview: OverviewView::new(),
            terminals: TerminalsView::new(Some(poller_cmd.clone())),
            memory: MemoryView::new(poller_cmd.clone()),
            guardrails: GuardrailsView::new(),
            settings: SettingsView::with_poller(Some(poller_cmd.clone())),
            active_view: ViewId::Overview,
            sidebar_expanded: true,
            agent_panel: AgentPanel::new(Some(shared_state.clone())),
            agent_visible: true,
            last_context_inject: Instant::now(),
            shared_state,
            poller_cmd,
            poller_events,
            _poller_thread: Some(poller_thread),
            ops_source_id: format!("gui-{}-{}", std::process::id(), impulse_ops::now_rfc3339()),
            last_terminal_ops_publish: Instant::now() - Duration::from_secs(2),
            last_published_terminal_ops: None,
            last_memory_view_active: false,
            last_context_tick: Instant::now(),
            project_selector,
            current_project: global_config.last_project.clone(),
            global_config,
            last_search_query: String::new(),
            notifications: NotificationManager::new(),
            signal_bus: SignalBus::new(),
            show_shortcuts_help: false,
            command_palette: CommandPalette::new(),
        }
    }

    fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        ctx.input(|input| {
            let ctrl = input.modifiers.contains(egui::Modifiers::CTRL);
            let ctrl_shift = input
                .modifiers
                .contains(egui::Modifiers::CTRL | egui::Modifiers::SHIFT);
            if ctrl_shift && input.key_pressed(egui::Key::P) {
                self.command_palette.open();
                return;
            }
            if !ctrl {
                return;
            }

            if input.key_pressed(egui::Key::Num1) {
                self.active_view = ViewId::Overview;
            } else if input.key_pressed(egui::Key::Num2) {
                self.active_view = ViewId::Agents;
            } else if input.key_pressed(egui::Key::Num3) {
                self.active_view = ViewId::Memory;
            } else if input.key_pressed(egui::Key::Num4) {
                self.active_view = ViewId::Settings;
            } else if input.key_pressed(egui::Key::B) {
                self.sidebar_expanded = !self.sidebar_expanded;
            } else if input.key_pressed(egui::Key::R) {
                let _ = self.poller_cmd.send(PollerCommand::Refresh);
            } else if input.key_pressed(egui::Key::K) {
                self.active_view = ViewId::Memory;
            } else if input.key_pressed(egui::Key::T) || input.key_pressed(egui::Key::N) {
                if let Some(agent) = self.terminals.agents.first() {
                    self.active_view = ViewId::Agents;
                    self.project_selector.open(Some(agent.name.to_string()));
                }
            } else if input.key_pressed(egui::Key::L) {
                if !self.agent_visible {
                    self.agent_visible = true;
                }
                self.agent_panel.request_focus();
            } else if input.key_pressed(egui::Key::S) {
                let _ = self.poller_cmd.send(PollerCommand::Refresh);
            } else if input.key_pressed(egui::Key::E) {
                self.agent_visible = !self.agent_visible;
            } else if input.key_pressed(egui::Key::Slash) {
                self.show_shortcuts_help = !self.show_shortcuts_help;
            }
        });
    }

    fn show_shortcuts_overlay(&mut self, ctx: &egui::Context) {
        use crate::theme::colors;

        if !self.show_shortcuts_help {
            return;
        }

        let mut open = self.show_shortcuts_help;
        egui::Window::new("Keyboard Shortcuts")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(320.0);

                let shortcuts = [
                    (
                        "Views",
                        vec![
                            ("Ctrl+1", "Workbench"),
                            ("Ctrl+2", "Terminals"),
                            ("Ctrl+3", "Memory"),
                            ("Ctrl+4", "Settings"),
                        ],
                    ),
                    (
                        "Navigation",
                        vec![
                            ("Ctrl+B", "Toggle sidebar"),
                            ("Ctrl+E", "Toggle agent panel"),
                            ("Ctrl+K", "Focus memory search"),
                            ("Ctrl+L", "Focus agent input"),
                            ("Ctrl+R", "Refresh data"),
                            ("Ctrl+S", "Save / sync data"),
                            ("Ctrl+/", "Toggle this help"),
                        ],
                    ),
                    (
                        "Terminals",
                        vec![
                            ("Ctrl+T / Ctrl+N", "New agent tab"),
                            ("Ctrl+W", "Close current tab"),
                            ("Ctrl+Tab", "Cycle tabs"),
                            ("Escape", "Dismiss error / close panel"),
                        ],
                    ),
                ];

                for (group, items) in &shortcuts {
                    ui.label(egui::RichText::new(*group).strong().color(colors::TEXT));
                    egui::Grid::new(format!("shortcuts_{}", group))
                        .num_columns(2)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            for (key, desc) in items {
                                ui.label(
                                    egui::RichText::new(*key).monospace().color(colors::YELLOW),
                                );
                                ui.label(egui::RichText::new(*desc).color(colors::TEXT_DIM));
                                ui.end_row();
                            }
                        });
                    ui.add_space(8.0);
                }
            });
        self.show_shortcuts_help = open;
    }

    fn handle_command_palette(&mut self, ctx: &egui::Context) {
        use crate::views::ViewId;
        use crate::widgets::command_palette::Command;

        if let Some(cmd) = self.command_palette.show(ctx) {
            match cmd {
                Command::NewTab => {
                    if let Some(agent) = self.terminals.agents.first() {
                        self.active_view = ViewId::Agents;
                        self.project_selector.open(Some(agent.name.to_string()));
                    }
                }
                Command::CloseTab => {
                    if let Some(id) = self.terminals.active_tab() {
                        self.terminals.close_tab(id);
                    }
                }
                Command::CycleTabs => {
                    self.terminals.switch_tab(true);
                }
                Command::Refresh => {
                    let _ = self.poller_cmd.send(PollerCommand::Refresh);
                }
                Command::ToggleSidebar => {
                    self.sidebar_expanded = !self.sidebar_expanded;
                }
                Command::ToggleShortcuts => {
                    self.show_shortcuts_help = !self.show_shortcuts_help;
                }
                Command::FocusMemory => {
                    self.active_view = ViewId::Memory;
                }
                Command::FocusOverview => {
                    self.active_view = ViewId::Overview;
                }
                Command::FocusAgents => {
                    self.active_view = ViewId::Agents;
                }
                Command::FocusContext => {
                    self.active_view = ViewId::Memory;
                }
                Command::FocusArtifacts => {
                    self.active_view = ViewId::Memory;
                }
                Command::FocusGuardrails => {
                    self.active_view = ViewId::Settings;
                }
                Command::FocusSettings => {
                    self.active_view = ViewId::Settings;
                }
                Command::ToggleAgentPanel => {
                    self.agent_visible = !self.agent_visible;
                }
            }
        }
    }

    fn build_project_bar(&mut self, ctx: &egui::Context) {
        use crate::theme::colors;

        egui::TopBottomPanel::top("project_bar")
            .exact_height(24.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(
                        egui::RichText::new("Project:")
                            .small()
                            .color(colors::TEXT_MUTED),
                    );
                    if let Some(ref project) = self.current_project {
                        let name = project
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| project.display().to_string());
                        ui.strong(egui::RichText::new(&name).small().color(colors::ACCENT));
                        ui.label(
                            egui::RichText::new(project.display().to_string())
                                .small()
                                .color(colors::TEXT_DIM),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("None selected")
                                .small()
                                .color(colors::TEXT_DIM),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Change...").clicked() {
                            self.project_selector.open(None);
                        }
                    });
                });
            });
    }

    fn build_menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Agent Tab (Ctrl+T)").clicked() {
                        if let Some(agent) = self.terminals.agents.first() {
                            self.project_selector.open(Some(agent.name.to_string()));
                        }
                        ui.close_menu();
                    }
                    if ui.button("Refresh (Ctrl+R)").clicked() {
                        let _ = self.poller_cmd.send(PollerCommand::Refresh);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("View", |ui| {
                    for view in ViewId::all() {
                        if ui
                            .selectable_label(
                                self.active_view == *view,
                                format!("{} ({})", view.title(), view.shortcut_label()),
                            )
                            .clicked()
                        {
                            self.active_view = *view;
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui
                        .checkbox(&mut self.sidebar_expanded, "Show Sidebar (Ctrl+B)")
                        .clicked()
                    {
                        ui.close_menu();
                    }
                    if ui
                        .checkbox(&mut self.agent_visible, "Agent Panel")
                        .clicked()
                    {
                        ui.close_menu();
                    }
                });
            });
        });
    }

    fn show_task_notices(&mut self, notices: Vec<crate::state::TaskNotice>) {
        for notice in notices {
            let severity = match notice.level {
                TaskNoticeLevel::Info => Severity::Info,
                TaskNoticeLevel::Success => Severity::Success,
                TaskNoticeLevel::Warning => Severity::Warning,
                TaskNoticeLevel::Error => Severity::Error,
            };
            self.notifications.notify(severity, notice.message);
        }
    }

    fn handle_artifact_result(&mut self, result: impulse_ops::ArtifactActionResult) {
        let severity = match result.status.as_str() {
            "acknowledged" => Severity::Success,
            "ready_to_apply" => Severity::Success,
            _ => Severity::Info,
        };
        self.notifications.notify(severity, result.message);
    }

    fn confirmed_supervisor_action(
        action: impulse_ops::SupervisorAction,
    ) -> impulse_ops::SupervisorAction {
        match action {
            impulse_ops::SupervisorAction::SendInput {
                agent_id,
                session_id,
                content,
                ..
            } => impulse_ops::SupervisorAction::SendInput {
                agent_id,
                session_id,
                content,
                confirmed: true,
            },
            impulse_ops::SupervisorAction::InjectContext {
                agent_id,
                session_id,
                query,
                ..
            } => impulse_ops::SupervisorAction::InjectContext {
                agent_id,
                session_id,
                query,
                confirmed: true,
            },
            impulse_ops::SupervisorAction::CleanupContext {
                agent_id,
                session_id,
                goal,
                ..
            } => impulse_ops::SupervisorAction::CleanupContext {
                agent_id,
                session_id,
                goal,
                confirmed: true,
            },
            impulse_ops::SupervisorAction::HandoffContext {
                session_id,
                target_tool,
                task,
                notes,
                ..
            } => impulse_ops::SupervisorAction::HandoffContext {
                session_id,
                target_tool,
                task,
                notes,
                confirmed: true,
            },
            impulse_ops::SupervisorAction::ModifyPermissions {
                scope,
                grant_actions,
                grant_tool_capabilities,
                ..
            } => impulse_ops::SupervisorAction::ModifyPermissions {
                scope,
                grant_actions,
                grant_tool_capabilities,
                confirmed: true,
            },
            impulse_ops::SupervisorAction::ClearSessionOverride { .. } => {
                impulse_ops::SupervisorAction::ClearSessionOverride { confirmed: true }
            }
            impulse_ops::SupervisorAction::ResetBaselinePermissions { .. } => {
                impulse_ops::SupervisorAction::ResetBaselinePermissions { confirmed: true }
            }
            other => other,
        }
    }

    fn permission_grant_for_proposal(
        proposal: &impulse_ops::SupervisorProposal,
        scope: impulse_ops::PermissionChangeScope,
    ) -> Option<impulse_ops::SupervisorAction> {
        if proposal.missing_actions.is_empty() && proposal.missing_tool_capabilities.is_empty() {
            return None;
        }

        Some(impulse_ops::SupervisorAction::ModifyPermissions {
            scope,
            grant_actions: proposal.missing_actions.clone(),
            grant_tool_capabilities: proposal.missing_tool_capabilities.clone(),
            confirmed: true,
        })
    }

    fn dispatch_local_supervisor_action(&mut self, action: impulse_ops::SupervisorAction) -> bool {
        match action {
            impulse_ops::SupervisorAction::FocusAgent {
                agent_id,
                session_id,
            } => {
                if self.terminals.focus_agent(&agent_id, session_id.as_deref()) {
                    self.active_view = ViewId::Agents;
                    true
                } else {
                    false
                }
            }
            impulse_ops::SupervisorAction::SendInput {
                agent_id,
                session_id,
                content,
                ..
            } => self
                .terminals
                .send_to_agent(&agent_id, session_id.as_deref(), &content),
            impulse_ops::SupervisorAction::SearchMemory { query } => {
                self.active_view = ViewId::Memory;
                self.memory.focus_search(query);
                true
            }
            _ => false,
        }
    }

    fn handle_supervisor_action_result(&mut self, result: impulse_ops::SupervisorActionResult) {
        let severity = match result.status.as_str() {
            "executed" | "dispatch_local" => Severity::Success,
            "no_candidates" => Severity::Warning,
            _ => Severity::Info,
        };
        self.notifications.notify(severity, &result.message);

        if let Some(artifact_id) = result.artifact_id.as_ref() {
            self.active_view = ViewId::Memory;
            self.notifications.notify(
                Severity::Info,
                format!("Supervisor artifact ready: {}", artifact_id),
            );
        }

        if let Some(local_action) = result.local_action {
            if !self.dispatch_local_supervisor_action(local_action) {
                self.notifications.notify(
                    Severity::Warning,
                    "Supervisor action was approved, but no matching local agent was available.",
                );
            }
        }
    }

    fn drain_poller_events(&mut self) {
        while let Ok(event) = self.poller_events.try_recv() {
            match event {
                PollerEvent::ArtifactActionResult(result) => self.handle_artifact_result(result),
                PollerEvent::SupervisorActionResult(result) => {
                    self.handle_supervisor_action_result(result)
                }
                PollerEvent::TabSessionCreated { tab_id, session_id } => {
                    self.terminals.set_daemon_session_id(tab_id, session_id);
                }
                PollerEvent::TabSessionFailed { tab_id, error } => {
                    log::warn!(
                        "Failed to create daemon session for tab {}: {}",
                        tab_id,
                        error
                    );
                }
            }
        }
    }

    fn maybe_publish_terminal_ops(&mut self) {
        let report = impulse_ops::TerminalOpsReport {
            source_id: self.ops_source_id.clone(),
            published_at: impulse_ops::now_rfc3339(),
            agents: self.terminals.workbench_agents(),
            context: self.terminals.workbench_context(),
            interventions: self.terminals.workbench_interventions(),
        };

        let changed = self
            .last_published_terminal_ops
            .as_ref()
            .map(|previous| {
                previous.agents != report.agents
                    || previous.context != report.context
                    || previous.interventions != report.interventions
            })
            .unwrap_or(true);
        let due = self.last_terminal_ops_publish.elapsed() >= Duration::from_secs(2);

        if changed || due {
            let _ = self.poller_cmd.send(PollerCommand::PublishTerminalOps {
                report: report.clone(),
            });
            self.last_terminal_ops_publish = Instant::now();
            self.last_published_terminal_ops = Some(report);
        }
    }
}

impl eframe::App for ImpulseApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) {
            self.terminals.shutdown();
            return;
        }

        self.terminals.drain_events();
        self.terminals.refresh_agents();
        self.drain_poller_events();

        let now = Instant::now();
        let tick_interval = self
            .shared_state
            .lock()
            .ok()
            .map(|s| s.runtime_settings.context_tick_interval())
            .unwrap_or_else(|| Duration::from_secs(3));
        if now.duration_since(self.last_context_tick) >= tick_interval {
            self.last_context_tick = now;
            self.terminals.context_tick();

            let (genome_decisions, active_sessions, recent_history): (
                Vec<String>,
                Vec<String>,
                Vec<String>,
            ) = self
                .shared_state
                .lock()
                .ok()
                .map(|shared| {
                    let decisions = shared
                        .ops_snapshot
                        .as_ref()
                        .map(|snapshot| {
                            snapshot
                                .interventions
                                .iter()
                                .take(5)
                                .map(|item| item.title.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    let sessions: Vec<String> = shared
                        .sessions
                        .iter()
                        .filter(|s| s.status == "active")
                        .take(5)
                        .map(|s| format!("{}: {} ({})", s.name, s.platform, s.id,))
                        .collect();
                    let history: Vec<String> = shared
                        .history
                        .iter()
                        .take(5)
                        .map(|h| {
                            let name = if h.session_name.is_empty() {
                                &h.session_id
                            } else {
                                &h.session_name
                            };
                            let summary = if h.summary.is_empty() {
                                "(no summary)"
                            } else {
                                &h.summary
                            };
                            format!("{}: {}", name, summary)
                        })
                        .collect();
                    (decisions, sessions, history)
                })
                .unwrap_or_default();
            self.terminals.check_threshold_injections(
                &genome_decisions,
                &active_sessions,
                &recent_history,
            );

            if self.agent_visible {
                let insights = self.terminals.collected_insights();
                self.agent_panel.update_activity(insights);
            }

            for signal in self.terminals.collect_signals() {
                self.signal_bus.emit(signal);
            }

            ctx.request_repaint();
        }

        let drained = self.signal_bus.drain();
        for signal in &drained {
            match signal.urgency {
                SignalUrgency::Urgent => {
                    self.notifications
                        .notify_with_duration(Severity::Error, &signal.message, 10.0);
                }
                SignalUrgency::Important => {
                    let severity = match &signal.kind {
                        SignalKind::TaskCompleted => Severity::Success,
                        SignalKind::ErrorEncountered => Severity::Error,
                        _ => Severity::Warning,
                    };
                    self.notifications.notify(severity, &signal.message);
                }
                SignalUrgency::Ambient => {}
            }
        }

        if self.signal_bus.badges_dirty() {
            self.terminals
                .set_tab_badges(self.signal_bus.all_tab_badges().clone());
            self.signal_bus.mark_badges_clean();
        }

        // Sync signal log snapshot to SharedState for Overview display.
        if let Ok(mut state) = self.shared_state.lock() {
            state.signal_log = self.signal_bus.signal_log_snapshot();
        }

        if let Some(tab_id) = self.terminals.take_badge_ack() {
            self.signal_bus.acknowledge_tab(tab_id);
        }
        for tab_id in self.terminals.take_closed_tabs() {
            self.signal_bus.remove_tab(tab_id);
        }

        self.terminals
            .process_pending_injections(&GlobalConfig::impulse_home());
        self.maybe_publish_terminal_ops();

        if self.agent_visible
            && now.duration_since(self.last_context_inject) >= Duration::from_secs(60)
        {
            self.last_context_inject = now;
            let insights = self.terminals.collected_insights();
            if !insights.is_empty() {
                self.agent_panel.inject_context(&insights);
            }
        }

        self.agent_panel.tick();

        let memory_view_active = self.active_view == ViewId::Memory;
        if memory_view_active != self.last_memory_view_active {
            let _ = self.poller_cmd.send(PollerCommand::SetMemoryView {
                active: memory_view_active,
            });
            self.last_memory_view_active = memory_view_active;
        }

        if self.active_view == ViewId::Agents {
            self.terminals.handle_shortcuts(ctx);
        }
        self.handle_global_shortcuts(ctx);
        self.show_shortcuts_overlay(ctx);
        self.handle_command_palette(ctx);

        if let Some(agent_name) = self.terminals.take_pending_spawn() {
            self.project_selector.open(Some(agent_name));
        }

        if let Some(selected_dir) = self.project_selector.show(ctx) {
            if crate::project_scaffold::needs_scaffold(&selected_dir) {
                if let Err(e) = crate::project_scaffold::scaffold_impulse_dir(&selected_dir) {
                    log::error!("Failed to scaffold .impulse/: {}", e);
                }
            }

            self.global_config.add_recent_project(selected_dir.clone());
            self.global_config.last_project = Some(selected_dir.clone());
            self.current_project = Some(selected_dir.clone());
            self.terminals.set_project_dir(&selected_dir);
            let _ = self.global_config.save(&GlobalConfig::impulse_home());
            self.project_selector
                .update_recents(self.global_config.recent_projects.clone());

            if let Some(agent_name) = self.project_selector.pending_agent() {
                if let Some(agent) = self
                    .terminals
                    .agents
                    .iter()
                    .find(|candidate| candidate.name == agent_name)
                {
                    self.terminals.spawn_tab(&agent.clone(), &selected_dir, ctx);
                }
            }
        }

        self.build_menu_bar(ctx);
        self.build_project_bar(ctx);

        let connection = self
            .shared_state
            .lock()
            .map(|shared| shared.connection)
            .unwrap_or(ConnectionStatus::Disconnected);
        self.agent_panel.set_connection_status(connection);
        let permission_state = self
            .shared_state
            .lock()
            .ok()
            .and_then(|shared| shared.supervisor_permissions.clone());
        self.agent_panel
            .set_supervisor_permissions(permission_state);

        let Ok(mut state) = self.shared_state.lock() else {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.colored_label(
                    egui::Color32::RED,
                    "Internal error: state lock poisoned. Please restart Impulse.",
                );
            });
            return;
        };

        let drained_notices = std::mem::take(&mut state.task_notices);

        let mut clear_error = false;
        if let Some(ref err) = state.error {
            egui::TopBottomPanel::top("error_banner").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("\u{26A0}").color(egui::Color32::RED));
                    ui.label(err);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Dismiss").clicked() {
                            clear_error = true;
                        }
                    });
                });
            });
        }
        if clear_error {
            state.error = None;
        }

        let sidebar_action = sidebar::show(
            ctx,
            self.active_view,
            self.sidebar_expanded,
            self.agent_visible,
            &state,
        );
        if let Some(new_view) = sidebar_action.new_view {
            self.active_view = new_view;
        }
        if sidebar_action.toggle_agent {
            self.agent_visible = !self.agent_visible;
        }

        if self.agent_visible {
            egui::SidePanel::right("agent_panel")
                .resizable(true)
                .default_width(220.0)
                .width_range(160.0..=320.0)
                .show(ctx, |ui| {
                    self.agent_panel.ui(ui, ctx);
                });
        }

        let current_query = state.search_query.clone();
        if current_query != self.last_search_query {
            self.last_search_query = current_query.clone();
            if current_query.is_empty() {
                state.live_search_results.clear();
            } else {
                let live = self.terminals.search_live_insights(&current_query);
                state.live_search_results = live
                    .into_iter()
                    .map(|item| crate::state::LiveSearchResult {
                        title: item.title,
                        agent: item.agent,
                        timestamp: item.timestamp,
                    })
                    .collect();
            }
        }

        for action in self.agent_panel.take_actions() {
            match action {
                PanelAction::InjectTo { tab_id, content } => {
                    if self.terminals.inject_to_tab(tab_id, &content) {
                        self.notifications
                            .notify(Severity::Success, format!("Injected into tab {}", tab_id));
                    } else {
                        self.notifications
                            .notify(Severity::Error, format!("Tab {} not found", tab_id));
                    }
                }
                PanelAction::SendTo { tab_id, content } => {
                    if self.terminals.send_to_tab(tab_id, &content) {
                        self.notifications
                            .notify(Severity::Info, format!("Sent to tab {}", tab_id));
                    } else {
                        self.notifications
                            .notify(Severity::Error, format!("Tab {} not found", tab_id));
                    }
                }
                PanelAction::FocusTab { tab_id } => {
                    if self.terminals.focus_tab(tab_id) {
                        self.active_view = ViewId::Agents;
                    } else {
                        self.notifications
                            .notify(Severity::Warning, format!("Tab {} not found", tab_id));
                    }
                }
                PanelAction::SearchTerm { query } => {
                    self.active_view = ViewId::Agents;
                    self.terminals.search_terminals(query);
                }
                PanelAction::MemorySearch { query } => {
                    self.active_view = ViewId::Memory;
                    self.memory.focus_search(query);
                }
                PanelAction::RunSupervisorProposal { proposal, mode } => {
                    let proposal = *proposal;
                    match mode {
                        ProposalExecutionMode::Deny => {
                            self.notifications.notify(
                                Severity::Info,
                                format!("Denied proposal: {}", proposal.title),
                            );
                        }
                        ProposalExecutionMode::Run => {
                            let _ = self.poller_cmd.send(PollerCommand::RunSupervisorAction {
                                action: Self::confirmed_supervisor_action(proposal.action),
                            });
                        }
                        ProposalExecutionMode::AllowThisSession
                        | ProposalExecutionMode::SaveDefault => {
                            let scope = match mode {
                                ProposalExecutionMode::AllowThisSession => {
                                    impulse_ops::PermissionChangeScope::SessionOverride
                                }
                                ProposalExecutionMode::SaveDefault => {
                                    impulse_ops::PermissionChangeScope::PersistentDefault
                                }
                                ProposalExecutionMode::Run | ProposalExecutionMode::Deny => {
                                    unreachable!()
                                }
                            };
                            if let Some(permission_action) =
                                Self::permission_grant_for_proposal(&proposal, scope)
                            {
                                let _ = self.poller_cmd.send(PollerCommand::RunSupervisorAction {
                                    action: permission_action,
                                });
                            }
                            let _ = self.poller_cmd.send(PollerCommand::RunSupervisorAction {
                                action: Self::confirmed_supervisor_action(proposal.action),
                            });
                        }
                    }
                }
            }
        }

        status_bar::show(ctx, &state);

        // Apply theme if the user changed it in Settings.
        if self.settings.take_theme_changed() {
            crate::theme::apply_theme(ctx, &self.settings.active_theme().palette());
        }

        egui::CentralPanel::default().show(ctx, |ui| match self.active_view {
            ViewId::Overview => self.overview.ui(ui, &state, ctx),
            ViewId::Agents => self.terminals.ui(ui, &state, ctx),
            ViewId::Memory => self.memory.ui(ui, &state, ctx),
            ViewId::Settings => self.settings.ui(ui, &state, ctx),
        });

        for action in self.settings.take_supervisor_actions() {
            let _ = self
                .poller_cmd
                .send(PollerCommand::RunSupervisorAction { action });
        }

        drop(state);
        self.show_task_notices(drained_notices);
        self.notifications.show(ctx);
    }

    fn on_exit(&mut self, _context: Option<&eframe::glow::Context>) {
        log::info!("Impulse GUI shutting down");
        let _ = self.poller_cmd.send(PollerCommand::PublishTerminalOps {
            report: impulse_ops::TerminalOpsReport {
                source_id: self.ops_source_id.clone(),
                published_at: impulse_ops::now_rfc3339(),
                ..Default::default()
            },
        });
        let _ = self.poller_cmd.send(PollerCommand::Shutdown);
        self.terminals.shutdown();
        log::info!("Impulse GUI shutdown complete");
    }
}
