//! ImpulseApp — coordinator that owns views, sidebar, status bar, and IPC state.
//!
//! The app no longer contains terminal logic directly. It delegates to views:
//! - `TerminalsView` — terminal multiplexer (migrated from old app.rs)
//! - `SessionsView` — daemon session list + detail
//! - `GenomeView` — genome decisions viewer
//! - `SearchView` — daemon search
//! - `AgentPanel` — interactive chat with the Impulse coordinator agent
//!
//! Layout: Sidebar (left) | Agent Panel (left, optional) | Active View (center) | Status Bar (bottom)

use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::agent_panel::actions::PanelAction;
use crate::agent_panel::AgentPanel;
use crate::state::{self, PollerCommand, StateHandle};
use crate::views::genome::GenomeView;
use crate::views::search::SearchView;
use crate::views::sessions::SessionsView;
use crate::views::settings::SettingsView;
use crate::views::terminals::TerminalsView;
use crate::views::{View, ViewId};
use crate::widgets::notifications::{NotificationManager, Severity};
use crate::widgets::{sidebar, status_bar};

/// Main application state — thin coordinator.
pub struct ImpulseApp {
    // View system.
    terminals: TerminalsView,
    sessions: SessionsView,
    genome: GenomeView,
    search: SearchView,
    settings: SettingsView,
    active_view: ViewId,

    // Sidebar.
    sidebar_expanded: bool,

    // Agent panel.
    agent_panel: AgentPanel,
    agent_visible: bool,
    last_context_inject: Instant,

    // Daemon IPC.
    shared_state: StateHandle,
    poller_cmd: Sender<PollerCommand>,
    _poller_thread: Option<JoinHandle<()>>,

    // Context lifecycle.
    last_context_tick: Instant,

    // Search tracking (for live insight search).
    last_search_query: String,

    // Notifications.
    notifications: NotificationManager,
}

impl ImpulseApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Start background daemon poller.
        let (shared_state, poller_cmd, poller_thread) = state::start_poller(cc.egui_ctx.clone());

        Self {
            terminals: TerminalsView::new(),
            sessions: SessionsView::new(),
            genome: GenomeView::new(),
            search: SearchView::new(poller_cmd.clone()),
            settings: SettingsView::new(),
            active_view: ViewId::Terminals,

            sidebar_expanded: true,

            agent_panel: AgentPanel::new(Some(shared_state.clone())),
            agent_visible: false,
            last_context_inject: Instant::now(),

            shared_state,
            poller_cmd,
            _poller_thread: Some(poller_thread),

            last_context_tick: Instant::now(),

            last_search_query: String::new(),

            notifications: NotificationManager::new(),
        }
    }

    /// Handle global keyboard shortcuts (view switching, sidebar toggle, etc.).
    fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        ctx.input(|input| {
            let ctrl = input.modifiers.contains(egui::Modifiers::CTRL);

            if !ctrl {
                return;
            }

            // Ctrl+1-4: Switch views.
            if input.key_pressed(egui::Key::Num1) {
                self.active_view = ViewId::Terminals;
            } else if input.key_pressed(egui::Key::Num2) {
                self.active_view = ViewId::Sessions;
            } else if input.key_pressed(egui::Key::Num3) {
                self.active_view = ViewId::Genome;
            } else if input.key_pressed(egui::Key::Num4) {
                self.active_view = ViewId::Search;
            }
            // Ctrl+5: Toggle agent panel.  Ctrl+6: Settings.
            else if input.key_pressed(egui::Key::Num6) {
                self.active_view = ViewId::Settings;
            } else if input.key_pressed(egui::Key::Num5) {
                self.agent_visible = !self.agent_visible;
            }
            // Ctrl+B: Toggle sidebar.
            else if input.key_pressed(egui::Key::B) {
                self.sidebar_expanded = !self.sidebar_expanded;
            }
            // Ctrl+R: Refresh daemon data.
            else if input.key_pressed(egui::Key::R) {
                let _ = self.poller_cmd.send(PollerCommand::Refresh);
            }
            // Ctrl+K: Focus search.
            else if input.key_pressed(egui::Key::K) {
                self.active_view = ViewId::Search;
            }
            // Ctrl+T / Ctrl+N: New terminal tab.
            else if input.key_pressed(egui::Key::T) || input.key_pressed(egui::Key::N) {
                if let Some(agent) = self.terminals.agents.first().cloned() {
                    self.active_view = ViewId::Terminals;
                    self.terminals.spawn_tab(&agent, ctx);
                }
            }
            // Ctrl+L: Focus agent panel (open if hidden).
            else if input.key_pressed(egui::Key::L) {
                if !self.agent_visible {
                    self.agent_visible = true;
                }
                self.agent_panel.request_focus();
            }
        });
    }

    fn build_menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Tab (Ctrl+T)").clicked() {
                        if let Some(agent) = self.terminals.agents.first().cloned() {
                            self.terminals.spawn_tab(&agent, ctx);
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
                    if ui
                        .checkbox(&mut self.sidebar_expanded, "Show Sidebar (Ctrl+B)")
                        .clicked()
                    {
                        ui.close_menu();
                    }
                    if ui
                        .checkbox(&mut self.agent_visible, "Agent Panel (Ctrl+5)")
                        .clicked()
                    {
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .selectable_value(
                            &mut self.active_view,
                            ViewId::Terminals,
                            "Terminals (Ctrl+1)",
                        )
                        .clicked()
                    {
                        ui.close_menu();
                    }
                    if ui
                        .selectable_value(
                            &mut self.active_view,
                            ViewId::Sessions,
                            "Sessions (Ctrl+2)",
                        )
                        .clicked()
                    {
                        ui.close_menu();
                    }
                    if ui
                        .selectable_value(&mut self.active_view, ViewId::Genome, "Genome (Ctrl+3)")
                        .clicked()
                    {
                        ui.close_menu();
                    }
                    if ui
                        .selectable_value(&mut self.active_view, ViewId::Search, "Search (Ctrl+K)")
                        .clicked()
                    {
                        ui.close_menu();
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("About").clicked() {
                        // Could add an about dialog later
                        ui.close_menu();
                    }
                });
            });
        });
    }
}

impl eframe::App for ImpulseApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle window close.
        if ctx.input(|i| i.viewport().close_requested()) {
            self.terminals.shutdown();
            return;
        }

        // Terminal lifecycle checks.
        self.terminals.drain_events();
        self.terminals.refresh_agents();

        // Context lifecycle tick (every 3 seconds).
        let now = Instant::now();
        if now.duration_since(self.last_context_tick) >= Duration::from_secs(3) {
            self.last_context_tick = now;
            self.terminals.context_tick();

            // Check tier crossings and inject refresh context.
            let genome_decisions: Vec<String> = self
                .shared_state
                .lock()
                .ok()
                .and_then(|s| {
                    s.genome.as_ref().map(|g| {
                        g.decisions
                            .iter()
                            .rev()
                            .take(5)
                            .map(|d| d.description.clone())
                            .collect()
                    })
                })
                .unwrap_or_default();
            self.terminals.check_threshold_injections(&genome_decisions);

            // Update activity feed display (every tick, ~3s — cheap Vec swap).
            if self.agent_visible {
                let insights = self.terminals.collected_insights();
                self.agent_panel.update_activity(insights);
            }
        }

        // Inject cross-pane context into agent panel (every 60 seconds).
        if self.agent_visible
            && now.duration_since(self.last_context_inject) >= Duration::from_secs(60)
        {
            self.last_context_inject = now;
            let insights = self.terminals.collected_insights();
            if !insights.is_empty() {
                self.agent_panel.inject_context(&insights);
            }
        }

        // Poll agent responses (non-blocking).
        self.agent_panel.tick();

        // Terminal-specific shortcuts (only when terminal view is active).
        if self.active_view == ViewId::Terminals {
            self.terminals.handle_shortcuts(ctx);
        }

        // Global shortcuts.
        self.handle_global_shortcuts(ctx);

        // --- Menu Bar ---
        self.build_menu_bar(ctx);

        // --- Shared State Locking & UI Layout ---
        let mut state = self.shared_state.lock().unwrap();

        // --- Error Banner ---
        let mut clear_error = false;
        if let Some(ref err) = state.error {
            egui::TopBottomPanel::top("error_banner").show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("\u{26a0}").color(egui::Color32::RED));
                    ui.label(err);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Dismiss").clicked() {
                            clear_error = true;
                        }
                    });
                });
                ui.add_space(4.0);
            });
        }
        if clear_error {
            state.error = None;
        }

        // --- Sidebar ---
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

        // --- Agent Panel (second left SidePanel, shown when toggled) ---
        if self.agent_visible {
            egui::SidePanel::left("agent_panel")
                .resizable(true)
                .default_width(340.0)
                .width_range(280.0..=500.0)
                .show(ctx, |ui| {
                    self.agent_panel.ui(ui, ctx);
                });
        }

        // --- Live search: populate live results when search query changes ---
        {
            let current_query = state.search_query.clone();
            if current_query != self.last_search_query {
                self.last_search_query = current_query.clone();
                if current_query.is_empty() {
                    state.live_search_results.clear();
                } else {
                    let live = self.terminals.search_live_insights(&current_query);
                    state.live_search_results = live
                        .into_iter()
                        .map(|r| crate::state::LiveSearchResult {
                            title: r.title,
                            agent: r.agent,
                            timestamp: r.timestamp,
                        })
                        .collect();
                }
            }
        }

        // --- Dispatch agent panel actions ---
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
                        self.active_view = ViewId::Terminals;
                    } else {
                        self.notifications
                            .notify(Severity::Warning, format!("Tab {} not found", tab_id));
                    }
                }
                PanelAction::SearchTerm { query } => {
                    // For now, switch to search view with the query.
                    // Terminal-level search will be implemented in Task 2.1.
                    self.active_view = ViewId::Search;
                    self.notifications
                        .notify(Severity::Info, format!("Search: {}", query));
                }
            }
        }

        // --- Status bar ---
        let active_agents = self.terminals.active_agent_info();
        status_bar::show(ctx, &state, self.terminals.tab_count(), &active_agents);

        // --- Central panel: active view ---
        egui::CentralPanel::default().show(ctx, |ui| match self.active_view {
            ViewId::Terminals => {
                self.terminals.ui(ui, &state, ctx);
            }
            ViewId::Sessions => {
                self.sessions.ui(ui, &state, ctx);
            }
            ViewId::Genome => {
                self.genome.ui(ui, &state, ctx);
            }
            ViewId::Search => {
                self.search.ui(ui, &state, ctx);
            }
            ViewId::Settings => {
                self.settings.ui(ui, &state, ctx);
            }
        });

        // Release the state lock before rendering overlays.
        drop(state);

        // --- Toast notifications (overlay, above all content) ---
        self.notifications.show(ctx);
    }

    fn on_exit(&mut self, _context: Option<&eframe::glow::Context>) {
        log::info!("Impulse GUI shutting down");

        // Shut down background poller.
        let _ = self.poller_cmd.send(PollerCommand::Shutdown);

        // Clean up terminals.
        self.terminals.shutdown();

        log::info!("Impulse GUI shutdown complete");
    }
}
