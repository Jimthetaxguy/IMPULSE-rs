//! View abstraction — each egui workbench panel implements `View`.

// Genome and Sessions are embedded in the Memory view (not top-level views).
pub mod genome;
pub mod memory;
pub mod memory_persistence;
pub mod overview;
pub mod search;
pub mod sessions;
pub mod settings;
pub mod terminal_context;
pub mod terminal_insights;
pub mod terminal_search;
pub mod terminals;

use eframe::egui;

use crate::state::SharedState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewId {
    Overview,
    Agents,
    Memory,
    Settings,
}

impl ViewId {
    pub fn all() -> &'static [ViewId] {
        &[
            ViewId::Overview,
            ViewId::Agents,
            ViewId::Memory,
            ViewId::Settings,
        ]
    }

    pub fn title(&self) -> &'static str {
        match self {
            ViewId::Overview => "Workbench",
            ViewId::Agents => "Terminals",
            ViewId::Memory => "Memory",
            ViewId::Settings => "Settings",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            ViewId::Overview => "\u{1F680}", // 🚀
            ViewId::Agents => "\u{2328}",    // ⌨
            ViewId::Memory => "\u{1F9E0}",   // 🧠
            ViewId::Settings => "\u{2699}",  // ⚙
        }
    }

    pub fn shortcut_label(&self) -> &'static str {
        match self {
            ViewId::Overview => "Ctrl+1",
            ViewId::Agents => "Ctrl+2",
            ViewId::Memory => "Ctrl+3",
            ViewId::Settings => "Ctrl+4",
        }
    }
}

pub trait View {
    #[allow(dead_code)] // dead_code: part of the View trait contract
    fn id(&self) -> ViewId;
    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, ctx: &egui::Context);
}
