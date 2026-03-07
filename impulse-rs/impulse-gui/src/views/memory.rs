use eframe::egui;

use super::genome::GenomeView;
use super::search::SearchView;
use super::sessions::SessionsView;
use super::{View, ViewId};
use crate::state::{PollerCommand, SharedState};
use crate::theme::colors;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MemoryTab {
    Sessions,
    Genome,
    Search,
}

pub struct MemoryView {
    sessions: SessionsView,
    genome: GenomeView,
    search: SearchView,
    tab: MemoryTab,
}

impl MemoryView {
    pub fn new(cmd_tx: std::sync::mpsc::Sender<PollerCommand>) -> Self {
        Self {
            sessions: SessionsView::new(cmd_tx.clone()),
            genome: GenomeView::new(),
            search: SearchView::new(cmd_tx),
            tab: MemoryTab::Sessions,
        }
    }

    pub fn focus_search(&mut self, query: String) {
        self.tab = MemoryTab::Search;
        self.search.set_query(query);
        self.search.submit_current();
    }
}

impl View for MemoryView {
    fn id(&self) -> ViewId {
        ViewId::Memory
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &SharedState, ctx: &egui::Context) {
        ui.label(
            egui::RichText::new("Memory is authoritative only when backed by the daemon and validated through real hook runs.")
                .small()
                .color(colors::TEXT_DIM),
        );
        ui.horizontal(|ui| {
            if selectable_memory_tab(ui, self.tab, MemoryTab::Sessions, "Sessions") {
                self.tab = MemoryTab::Sessions;
            }
            if selectable_memory_tab(ui, self.tab, MemoryTab::Genome, "Genome") {
                self.tab = MemoryTab::Genome;
            }
            if selectable_memory_tab(ui, self.tab, MemoryTab::Search, "Search") {
                self.tab = MemoryTab::Search;
            }

            ui.separator();
            if let Some(snapshot) = state.ops_snapshot.as_ref() {
                ui.label(
                    egui::RichText::new(format!(
                        "{} sessions | {} decisions | {} artifacts",
                        snapshot.memory.active_sessions,
                        snapshot.memory.genome_decisions,
                        snapshot.artifacts.len()
                    ))
                    .small()
                    .color(colors::TEXT_DIM),
                );
            }
        });
        ui.separator();

        match self.tab {
            MemoryTab::Sessions => self.sessions.ui(ui, state, ctx),
            MemoryTab::Genome => self.genome.ui(ui, state, ctx),
            MemoryTab::Search => self.search.ui(ui, state, ctx),
        }
    }
}

fn selectable_memory_tab(
    ui: &mut egui::Ui,
    current: MemoryTab,
    tab: MemoryTab,
    label: &str,
) -> bool {
    ui.selectable_label(
        current == tab,
        egui::RichText::new(label).color(if current == tab {
            colors::ACCENT
        } else {
            colors::TEXT_MUTED
        }),
    )
    .clicked()
}
