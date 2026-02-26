//! Terminal multiplexer view — spawn and manage agent terminals.
//!
//! Uses `impulse_term::TerminalPanel` for full PTY read/write access,
//! context lifecycle integration, and vt100-based rendering.

use std::collections::BTreeMap;
use std::time::Instant;

use eframe::egui;
use impulse_term::TerminalPanel;

use super::{View, ViewId};
use crate::state::SharedState;
use crate::theme;

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
}

impl TerminalsView {
    pub fn new() -> Self {
        let agents = vec![
            AgentInfo {
                name: "Claude Code",
                command: "claude",
                args: &["-p"],
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

        Self {
            tabs: BTreeMap::new(),
            active_tab: None,
            next_id: 0,
            agents,
            max_tabs: 10,
            last_check: Some(Instant::now()),
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
        if let Some(tab) = self.tabs.get(&id) {
            tab.panel.kill();
        }
        self.tabs.remove(&id);
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
    pub fn shutdown(&mut self) {
        for tab in self.tabs.values() {
            tab.panel.kill();
        }
        self.tabs.clear();
        self.active_tab = None;
    }

    /// Tab count for status bar display.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Count of alive tabs.
    #[allow(dead_code)]
    pub fn alive_count(&self) -> usize {
        self.tabs.values().filter(|t| t.panel.is_alive()).count()
    }

    /// Run context extraction tick on all alive panels.
    pub fn context_tick(&mut self) {
        for tab in self.tabs.values_mut() {
            if tab.panel.is_alive() {
                let _ = tab.panel.context_bridge().extract_tick();
            }
        }
    }
}

impl View for TerminalsView {
    fn id(&self) -> ViewId {
        ViewId::Terminals
    }

    fn ui(&mut self, ui: &mut egui::Ui, _state: &SharedState, _ctx: &egui::Context) {
        // --- Tab bar ---
        ui.horizontal(|ui| {
            let tab_ids: Vec<u64> = self.tabs.keys().copied().collect();
            let mut close_id = None;

            for id in &tab_ids {
                let tab = &self.tabs[id];
                let is_active = self.active_tab == Some(*id);
                let color = theme::agent_color(tab.agent_name);

                ui.horizontal(|ui| {
                    // Alive dot.
                    let dot_color = if tab.panel.is_alive() {
                        egui::Color32::from_rgb(0x3f, 0xb9, 0x50)
                    } else {
                        egui::Color32::from_rgb(0x6e, 0x76, 0x81)
                    };
                    let dot_rect = ui.allocate_space(egui::vec2(8.0, 8.0));
                    ui.painter()
                        .circle_filled(dot_rect.1.center(), 3.5, dot_color);

                    let text = egui::RichText::new(&tab.label).color(if is_active {
                        color
                    } else {
                        egui::Color32::from_rgb(0x8b, 0x94, 0x9e)
                    });
                    if ui.selectable_label(is_active, text).clicked() {
                        self.active_tab = Some(*id);
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
                            egui::Color32::from_rgb(0x48, 0x4f, 0x58)
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

        // --- Terminal or welcome ---
        if let Some(active_id) = self.active_tab {
            if let Some(tab) = self.tabs.get_mut(&active_id) {
                tab.panel.set_focused(true);
                tab.panel.show(ui);
            }
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 4.0);
                ui.heading(
                    egui::RichText::new(WELCOME_BANNER)
                        .monospace()
                        .color(egui::Color32::from_rgb(0x8b, 0x5c, 0xf6)),
                );
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("Persistent memory for AI coding agents")
                        .color(egui::Color32::from_rgb(0x8b, 0x94, 0x9e)),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Click a spawn button above to open a terminal.")
                        .color(egui::Color32::from_rgb(0x6e, 0x76, 0x81)),
                );
            });
        }
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
