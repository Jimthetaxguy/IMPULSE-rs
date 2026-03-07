//! View abstraction — each egui workbench panel implements `View`.

pub mod artifacts;
pub mod context;
pub mod genome;
pub mod memory;
pub mod memory_persistence;
pub mod overview;
pub mod search;
pub mod sessions;
pub mod settings;
pub mod terminal_search;
pub mod terminals;

use eframe::egui;

use crate::state::SharedState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewId {
    Overview,
    Agents,
    Context,
    Memory,
    Artifacts,
    Settings,
}

impl ViewId {
    pub fn all() -> &'static [ViewId] {
        &[
            ViewId::Overview,
            ViewId::Agents,
            ViewId::Context,
            ViewId::Memory,
            ViewId::Artifacts,
            ViewId::Settings,
        ]
    }

    pub fn title(&self) -> &'static str {
        match self {
            ViewId::Overview => "Workbench",
            ViewId::Agents => "Agents",
            ViewId::Context => "Context (exp)",
            ViewId::Memory => "Memory",
            ViewId::Artifacts => "Artifacts (exp)",
            ViewId::Settings => "Settings",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            ViewId::Overview => "\u{25A6}",
            ViewId::Agents => "\u{2328}",
            ViewId::Context => "\u{25D4}",
            ViewId::Memory => "\u{1F9E0}",
            ViewId::Artifacts => "\u{25A4}",
            ViewId::Settings => "\u{2699}",
        }
    }

    pub fn shortcut_label(&self) -> &'static str {
        match self {
            ViewId::Overview => "Ctrl+1",
            ViewId::Agents => "Ctrl+2",
            ViewId::Context => "Ctrl+3",
            ViewId::Memory => "Ctrl+4",
            ViewId::Artifacts => "Ctrl+5",
            ViewId::Settings => "Ctrl+6",
        }
    }
}

pub trait View {
    #[allow(dead_code)]
    fn id(&self) -> ViewId;
    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, ctx: &egui::Context);
}
