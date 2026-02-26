//! View abstraction — each workbench panel implements `View`.

pub mod genome;
pub mod search;
pub mod sessions;
pub mod terminals;

use eframe::egui;

use crate::state::SharedState;

/// Identifies which view is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewId {
    Terminals,
    Sessions,
    Genome,
    Search,
}

impl ViewId {
    pub fn all() -> &'static [ViewId] {
        &[
            ViewId::Terminals,
            ViewId::Sessions,
            ViewId::Genome,
            ViewId::Search,
        ]
    }

    pub fn title(&self) -> &'static str {
        match self {
            ViewId::Terminals => "Terminals",
            ViewId::Sessions => "Sessions",
            ViewId::Genome => "Genome",
            ViewId::Search => "Search",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            ViewId::Terminals => "\u{2328}", // ⌨
            ViewId::Sessions => "\u{1f4cb}", // 📋
            ViewId::Genome => "\u{1f9ec}",   // 🧬
            ViewId::Search => "\u{1f50d}",   // 🔍
        }
    }

    pub fn shortcut_label(&self) -> &'static str {
        match self {
            ViewId::Terminals => "Ctrl+1",
            ViewId::Sessions => "Ctrl+2",
            ViewId::Genome => "Ctrl+3",
            ViewId::Search => "Ctrl+4",
        }
    }
}

/// Trait implemented by each workbench view.
pub trait View {
    #[allow(dead_code)]
    fn id(&self) -> ViewId;
    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, ctx: &egui::Context);
}
