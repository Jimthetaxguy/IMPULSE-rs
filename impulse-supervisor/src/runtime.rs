// dead_code: when experimental-runtime is on, the bin's `run()` takes
// the launch_desktop branch and the non-runtime helpers (runtime_bootstrap,
// render_bootstrap_console, render_runtime_console, refresh_runtime_model,
// runtime_disabled_message, runtime_model + ShellState transition methods)
// become unused in the bin compile unit. They remain in the API for the
// lib's tests and for downstream consumers; the dead-code is true only
// for this specific bin/feature combination.
#![cfg_attr(feature = "experimental-runtime", allow(dead_code))]

use crate::daemon_bridge::DaemonBridge;
use dioxus::prelude::*;
use impulse_supervisor::layout::{LayoutMode, WorkerGrid};
use impulse_supervisor::state::{ShellState, TerminalState};

const WINDOW_TITLE: &str = "Impulse Supervisor";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBootstrap {
    pub title: &'static str,
    pub layout: LayoutMode,
    pub worker_grid: WorkerGrid,
    pub status_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorSidebarView {
    pub heading: &'static str,
    pub status_label: String,
    pub detail: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorStatusChipView {
    pub label: String,
    pub tone_class: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerPaneStubView {
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHeaderView {
    pub title: &'static str,
    pub status_label: String,
    pub layout_label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRegistrySummaryView {
    pub supervisor_present: bool,
    pub worker_count: usize,
    pub project_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeModel {
    pub bootstrap: RuntimeBootstrap,
    pub header: RuntimeHeaderView,
    pub sidebar: SupervisorSidebarView,
    pub registry_summary: PaneRegistrySummaryView,
    pub worker_panes: Vec<WorkerPaneStubView>,
    pub worker_grid_class: &'static str,
    pub worker_summary: String,
}

pub fn runtime_disabled_message() -> &'static str {
    "impulse-supervisor requires `--features experimental-runtime` to launch the Dioxus desktop runtime."
}

pub fn bootstrap_shell_state() -> ShellState {
    ShellState {
        terminals: TerminalState {
            layout: LayoutMode::SidebarWithGrid,
            worker_grid: Some(WorkerGrid::TwoColumn),
            ..TerminalState::default()
        },
        ..ShellState::default()
    }
}

pub fn runtime_bootstrap_from_state(state: &ShellState) -> RuntimeBootstrap {
    RuntimeBootstrap {
        title: WINDOW_TITLE,
        layout: state.terminals.layout,
        worker_grid: state.terminals.worker_grid.unwrap_or(WorkerGrid::Single),
        status_label: shell_status_label(state),
    }
}

pub fn runtime_bootstrap() -> RuntimeBootstrap {
    runtime_bootstrap_from_state(&bootstrap_shell_state())
}

pub fn shell_status_label(state: &ShellState) -> String {
    let supervisor = if state.terminals.registry.supervisor().is_some() {
        "supervisor ready"
    } else {
        "supervisor pending"
    };
    format!(
        "{} · {} worker panes",
        supervisor,
        state.terminals.registry.worker_count()
    )
}

pub fn supervisor_sidebar_view(bootstrap: &RuntimeBootstrap) -> SupervisorSidebarView {
    SupervisorSidebarView {
        heading: "Supervisor",
        status_label: bootstrap.status_label.clone(),
        detail:
            "Privileged shell entrypoint is wired; panes and daemon bridge land in later loops.",
    }
}

pub fn supervisor_status_chip_view(status_label: &str) -> SupervisorStatusChipView {
    let tone_class = if status_label.contains("ready") {
        "status-chip-ready"
    } else {
        "status-chip-pending"
    };

    SupervisorStatusChipView {
        label: status_label.to_string(),
        tone_class,
    }
}

pub fn worker_grid_class(grid: WorkerGrid) -> &'static str {
    match grid {
        WorkerGrid::Single => "worker-grid-single",
        WorkerGrid::TwoColumn => "worker-grid-two-column",
        WorkerGrid::Quad => "worker-grid-quad",
        WorkerGrid::Tabbed => "worker-grid-tabbed",
    }
}

pub fn worker_pane_stub_views(bootstrap: &RuntimeBootstrap) -> Vec<WorkerPaneStubView> {
    let count = match bootstrap.worker_grid {
        WorkerGrid::Single => 1,
        WorkerGrid::TwoColumn => 2,
        WorkerGrid::Quad => 4,
        WorkerGrid::Tabbed => 3,
    };

    (0..count)
        .map(|index| WorkerPaneStubView {
            title: format!("Worker {}", index + 1),
            detail: format!("Bootstrap placeholder for {:?}", bootstrap.worker_grid),
        })
        .collect()
}

pub fn layout_label(layout: LayoutMode) -> &'static str {
    match layout {
        LayoutMode::SidebarWithGrid => "SidebarWithGrid",
        LayoutMode::WorkerFocus => "WorkerFocus",
        LayoutMode::SupervisorFocus => "SupervisorFocus",
    }
}

pub fn runtime_header_view(bootstrap: &RuntimeBootstrap) -> RuntimeHeaderView {
    RuntimeHeaderView {
        title: bootstrap.title,
        status_label: bootstrap.status_label.clone(),
        layout_label: layout_label(bootstrap.layout),
    }
}

pub fn pane_registry_summary_view(state: &ShellState) -> PaneRegistrySummaryView {
    PaneRegistrySummaryView {
        supervisor_present: state.terminals.registry.supervisor().is_some(),
        worker_count: state.terminals.registry.worker_count(),
        project_count: state.session.registered_projects.len(),
    }
}

pub fn pane_registry_summary_line(view: &PaneRegistrySummaryView) -> String {
    let supervisor = if view.supervisor_present {
        "present"
    } else {
        "pending"
    };

    format!(
        "Supervisor: {supervisor} · Workers: {} · Projects: {}",
        view.worker_count, view.project_count
    )
}

pub fn shell_root_class(layout: LayoutMode) -> &'static str {
    match layout {
        LayoutMode::SidebarWithGrid => "layout-sidebar-with-grid",
        LayoutMode::WorkerFocus => "layout-worker-focus",
        LayoutMode::SupervisorFocus => "layout-supervisor-focus",
    }
}

pub fn worker_summary_label(panes: &[WorkerPaneStubView]) -> String {
    format!("Visible worker panes: {}", panes.len())
}

pub fn render_bootstrap_console(bootstrap: &RuntimeBootstrap) -> String {
    let header = runtime_header_view(bootstrap);
    let sidebar = supervisor_sidebar_view(bootstrap);
    let status_chip = supervisor_status_chip_view(&sidebar.status_label);
    let registry_summary = pane_registry_summary_view(&bootstrap_shell_state());
    let bridge_preview = DaemonBridge::default().startup_preview_line();
    let panes = worker_pane_stub_views(bootstrap);
    let titles = panes
        .iter()
        .map(|pane| pane.title.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "{title} [{status}] | chip={chip}::{tone} | layout={layout} | root={root_class} | bridge={bridge} | {heading}: {detail} | registry={registry} | {grid} | {titles}",
        title = header.title,
        status = header.status_label,
        chip = status_chip.label,
        tone = status_chip.tone_class,
        layout = header.layout_label,
        root_class = shell_root_class(bootstrap.layout),
        bridge = bridge_preview,
        heading = sidebar.heading,
        detail = sidebar.detail,
        registry = pane_registry_summary_line(&registry_summary),
        grid = worker_grid_class(bootstrap.worker_grid),
        titles = titles,
    )
}

pub fn runtime_model() -> RuntimeModel {
    let state = bootstrap_shell_state();
    let bootstrap = runtime_bootstrap();
    let worker_panes = worker_pane_stub_views(&bootstrap);
    RuntimeModel {
        header: runtime_header_view(&bootstrap),
        sidebar: supervisor_sidebar_view(&bootstrap),
        registry_summary: pane_registry_summary_view(&state),
        worker_summary: worker_summary_label(&worker_panes),
        worker_panes,
        worker_grid_class: worker_grid_class(bootstrap.worker_grid),
        bootstrap,
    }
}

pub fn runtime_model_from_state(state: &ShellState) -> RuntimeModel {
    let bootstrap = runtime_bootstrap_from_state(state);
    let worker_panes = worker_pane_stub_views(&bootstrap);
    RuntimeModel {
        header: runtime_header_view(&bootstrap),
        sidebar: supervisor_sidebar_view(&bootstrap),
        registry_summary: pane_registry_summary_view(state),
        worker_summary: worker_summary_label(&worker_panes),
        worker_panes,
        worker_grid_class: worker_grid_class(bootstrap.worker_grid),
        bootstrap,
    }
}

pub fn refresh_runtime_model(model: &mut RuntimeModel, state: &ShellState) {
    *model = runtime_model_from_state(state);
}

pub fn runtime_shell_model(state: &ShellState) -> RuntimeModel {
    runtime_model_from_state(state)
}

pub fn runtime_shell_root_class(state: &ShellState) -> &'static str {
    shell_root_class(state.terminals.layout)
}

pub fn render_runtime_console(model: &RuntimeModel) -> String {
    render_bootstrap_console(&model.bootstrap)
}

#[cfg(feature = "experimental-runtime")]
pub fn launch_desktop() {
    // Dioxus 0.6 unified launch API — picks the desktop backend (wry +
    // system webview) based on the `dioxus/desktop` feature being enabled
    // through `impulse-term-dioxus/desktop` -> `experimental-runtime`.
    dioxus::launch(SupervisorRuntimeApp);
}

pub fn run() {
    #[cfg(feature = "experimental-runtime")]
    {
        launch_desktop();
    }

    #[cfg(not(feature = "experimental-runtime"))]
    {
        let state = bootstrap_shell_state();
        let bootstrap = runtime_bootstrap();
        let baseline_model = runtime_model();
        let mut model = runtime_shell_model(&state);
        let root_class = runtime_shell_root_class(&state);
        refresh_runtime_model(&mut model, &state);
        eprintln!(
            "{} {} | baseline={baseline_console} | shell={root_class} | bootstrap={bootstrap_console}",
            runtime_disabled_message(),
            render_runtime_console(&model),
            baseline_console = render_runtime_console(&baseline_model),
            bootstrap_console = render_bootstrap_console(&bootstrap)
        );
    }
}

#[component]
pub fn SupervisorRuntimeApp() -> Element {
    rsx! {
        RuntimeShell {}
    }
}

#[component]
pub fn RuntimeShell() -> Element {
    let shell_state = use_signal(bootstrap_shell_state);
    let runtime = runtime_shell_model(&shell_state.read());
    let header = runtime.header.clone();
    let root_class = runtime_shell_root_class(&shell_state.read());

    rsx! {
        div {
            id: "impulse-supervisor-runtime",
            class: "{root_class}",
            RuntimeHeader { view: header }
            RuntimeBody { model: runtime }
        }
    }
}

#[component]
pub fn RuntimeHeader(view: RuntimeHeaderView) -> Element {
    rsx! {
        header {
            class: "runtime-header",
            h1 { "{view.title}" }
            p { class: "runtime-status", "{view.status_label}" }
            p { class: "runtime-layout-label", "Layout: {view.layout_label}" }
        }
    }
}

#[component]
pub fn RuntimeBody(model: RuntimeModel) -> Element {
    rsx! {
        div {
            class: "runtime-layout",
            SupervisorSidebar { view: model.sidebar }
            PaneRegistrySummary {
                view: model.registry_summary,
            }
            WorkerGridPanel {
                grid: model.bootstrap.worker_grid,
                panes: model.worker_panes,
            }
            p { class: "worker-summary", "{model.worker_summary}" }
        }
    }
}

#[component]
pub fn SupervisorSidebar(view: SupervisorSidebarView) -> Element {
    let status_chip = supervisor_status_chip_view(&view.status_label);
    rsx! {
        aside {
            class: "supervisor-sidebar",
            h2 { "{view.heading}" }
            SupervisorStatusChip { view: status_chip }
            p { class: "sidebar-status", "{view.status_label}" }
            p { class: "sidebar-detail", "{view.detail}" }
        }
    }
}

#[component]
pub fn SupervisorStatusChip(view: SupervisorStatusChipView) -> Element {
    rsx! {
        span {
            class: "supervisor-status-chip {view.tone_class}",
            "{view.label}"
        }
    }
}

#[component]
pub fn PaneRegistrySummary(view: PaneRegistrySummaryView) -> Element {
    rsx! {
        section {
            class: "pane-registry-summary",
            h2 { "Registry" }
            p { "{pane_registry_summary_line(&view)}" }
        }
    }
}

#[component]
pub fn WorkerGridPanel(grid: WorkerGrid, panes: Vec<WorkerPaneStubView>) -> Element {
    rsx! {
        div {
            class: "worker-grid {worker_grid_class(grid)}",
            h2 { "Workers" }
            p { "Default grid: {grid:?}" }
            for pane in panes {
                WorkerPaneCard {
                    key: "{pane.title}",
                    pane,
                }
            }
        }
    }
}

#[component]
pub fn WorkerPaneCard(pane: WorkerPaneStubView) -> Element {
    let live_inner = render_live_pane_inner(pane.clone());
    rsx! {
        article {
            class: "worker-pane-stub",
            h3 { "{pane.title}" }
            p { "{pane.detail}" }
            {live_inner}
        }
    }
}

// dead_code: invoked from WorkerPaneCard (a Dioxus #[component]); clippy
// can't see through the macro-generated component plumbing.
#[allow(dead_code)]
#[cfg(feature = "experimental-runtime")]
fn render_live_pane_inner(pane: WorkerPaneStubView) -> Element {
    rsx! { LiveWorkerPane { pane } }
}

// dead_code: same reason as above; this branch is the no-op fallback when
// the runtime isn't compiled in.
#[allow(dead_code)]
#[cfg(not(feature = "experimental-runtime"))]
fn render_live_pane_inner(_pane: WorkerPaneStubView) -> Element {
    // No live PTY embed when the experimental runtime is disabled — the
    // stub card alone is enough for the non-runtime build path.
    rsx! { Fragment {} }
}

/// Live PTY-driven worker pane — the L167 integration smoke surface.
///
/// Spawns a `/bin/sh -c 'echo "Impulse worker pane: <title>"; exec /bin/sh'`
/// PTY for each worker pane card, rendering it through `PtyTerminalView`.
/// The `exec /bin/sh` keeps the pane interactive after the welcome `echo`
/// so a user can type into the pane and see real shell output.
///
/// This is deliberately minimal — Phase 3 (L168+) wires the actual pane
/// command, env vars, working directory from the pane registry. The goal
/// of L167 is to prove the chain end-to-end:
///
///   PtySpec → PtySource → TerminalBackend → vt100 parser →
///   GridSnapshot → LiveGrid (per-row signals) → PtyTerminalView
///
/// inside a real `dioxus_desktop::launch` window.
#[cfg(feature = "experimental-runtime")]
#[component]
pub fn LiveWorkerPane(pane: WorkerPaneStubView) -> Element {
    use impulse_term_core::role::PaneRole;
    use impulse_term_dioxus::{PtySpec, PtyTerminalView};
    use std::path::PathBuf;

    // L182: every worker pane gets a stable uuid + the daemon command socket
    // so it can emit `@impulse <verb>` requests that the future hook
    // intercepts on PTY input. Resolved once per component lifetime via
    // use_signal so re-renders don't reroll the uuid.
    let pane_id = use_signal(uuid::Uuid::new_v4);
    let pane_id_value = *pane_id.read();
    let socket_path: Option<PathBuf> = std::env::var("IMPULSE_CMD_SOCKET").ok().map(PathBuf::from);
    let env_vars = PaneRole::Worker.spawn_env_vars(socket_path.as_deref(), Some(pane_id_value));

    // Spawn a login-style sh that prints a marker line then drops to an
    // interactive prompt. The `exec` replaces sh-with-args with sh, so the
    // pane stays alive after the echo runs.
    let title = pane.title.clone();
    let spec = PtySpec {
        command: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            format!("echo 'Impulse worker pane: {title}'; exec /bin/sh"),
        ],
        working_dir: None,
        env_vars,
        rows: 18,
        cols: 80,
        scrollback_lines: Some(2_000),
    };

    rsx! {
        div {
            class: "live-worker-pane",
            style: "border: 1px solid #444; border-radius: 4px; margin-top: 8px; min-height: 380px;",
            // L181: render the live grid AND the OSC 133 block history
            // beneath it. Both surfaces read from the same PtySource so
            // they stay consistent. Empty until the shell emits OSC 133
            // markers (zsh w/ p10k, fish, bash w/ vte integration).
            PtyTerminalView {
                spec,
                font_size_px: 12,
                line_height: 1.25,
                theme: None,
                show_blocks: true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_title_is_stable() {
        assert_eq!(runtime_bootstrap().title, "Impulse Supervisor");
    }

    #[test]
    fn test_runtime_disabled_message_mentions_feature() {
        assert!(runtime_disabled_message().contains("experimental-runtime"));
    }

    #[test]
    fn test_bootstrap_shell_state_uses_sidebar_layout() {
        let state = bootstrap_shell_state();
        assert_eq!(state.terminals.layout, LayoutMode::SidebarWithGrid);
    }

    #[test]
    fn test_bootstrap_shell_state_uses_two_column_grid() {
        let state = bootstrap_shell_state();
        assert_eq!(state.terminals.worker_grid, Some(WorkerGrid::TwoColumn));
    }

    #[test]
    fn test_bootstrap_shell_state_starts_without_workers() {
        let state = bootstrap_shell_state();
        assert_eq!(state.terminals.registry.worker_count(), 0);
    }

    #[test]
    fn test_bootstrap_shell_state_starts_without_supervisor() {
        let state = bootstrap_shell_state();
        assert!(state.terminals.registry.supervisor().is_none());
    }

    #[test]
    fn test_runtime_bootstrap_uses_window_title() {
        let bootstrap = runtime_bootstrap();
        assert_eq!(bootstrap.title, WINDOW_TITLE);
    }

    #[test]
    fn test_runtime_bootstrap_from_state_uses_explicit_grid() {
        let state = bootstrap_shell_state();
        let bootstrap = runtime_bootstrap_from_state(&state);
        assert_eq!(bootstrap.worker_grid, WorkerGrid::TwoColumn);
    }

    #[test]
    fn test_runtime_bootstrap_uses_sidebar_layout() {
        let bootstrap = runtime_bootstrap();
        assert_eq!(bootstrap.layout, LayoutMode::SidebarWithGrid);
    }

    #[test]
    fn test_runtime_bootstrap_uses_two_column_grid() {
        let bootstrap = runtime_bootstrap();
        assert_eq!(bootstrap.worker_grid, WorkerGrid::TwoColumn);
    }

    #[test]
    fn test_shell_status_label_reports_pending_supervisor() {
        let label = shell_status_label(&bootstrap_shell_state());
        assert!(label.contains("supervisor pending"));
    }

    #[test]
    fn test_shell_status_label_reports_zero_workers() {
        let label = shell_status_label(&bootstrap_shell_state());
        assert!(label.contains("0 worker panes"));
    }

    #[test]
    fn test_supervisor_sidebar_view_heading_is_stable() {
        let view = supervisor_sidebar_view(&runtime_bootstrap());
        assert_eq!(view.heading, "Supervisor");
    }

    #[test]
    fn test_supervisor_sidebar_view_reuses_status_label() {
        let bootstrap = runtime_bootstrap();
        let view = supervisor_sidebar_view(&bootstrap);
        assert_eq!(view.status_label, bootstrap.status_label);
    }

    #[test]
    fn test_supervisor_status_chip_pending_tone() {
        let chip = supervisor_status_chip_view("supervisor pending · 0 worker panes");
        assert_eq!(chip.tone_class, "status-chip-pending");
    }

    #[test]
    fn test_supervisor_status_chip_ready_tone() {
        let chip = supervisor_status_chip_view("supervisor ready · 2 worker panes");
        assert_eq!(chip.tone_class, "status-chip-ready");
    }

    #[test]
    fn test_supervisor_status_chip_preserves_label() {
        let chip = supervisor_status_chip_view("supervisor ready · 2 worker panes");
        assert_eq!(chip.label, "supervisor ready · 2 worker panes");
    }

    #[test]
    fn test_worker_grid_class_two_column() {
        assert_eq!(
            worker_grid_class(WorkerGrid::TwoColumn),
            "worker-grid-two-column"
        );
    }

    #[test]
    fn test_worker_grid_class_quad() {
        assert_eq!(worker_grid_class(WorkerGrid::Quad), "worker-grid-quad");
    }

    #[test]
    fn test_worker_pane_stub_views_match_two_column_count() {
        let panes = worker_pane_stub_views(&runtime_bootstrap());
        assert_eq!(panes.len(), 2);
    }

    #[test]
    fn test_worker_pane_stub_views_number_titles() {
        let panes = worker_pane_stub_views(&runtime_bootstrap());
        assert_eq!(panes[0].title, "Worker 1");
        assert_eq!(panes[1].title, "Worker 2");
    }

    #[test]
    fn test_worker_pane_stub_views_include_grid_name() {
        let panes = worker_pane_stub_views(&runtime_bootstrap());
        assert!(panes[0].detail.contains("TwoColumn"));
    }

    #[test]
    fn test_render_bootstrap_console_mentions_sidebar_heading() {
        let console = render_bootstrap_console(&runtime_bootstrap());
        assert!(console.contains("Supervisor"));
    }

    #[test]
    fn test_render_bootstrap_console_mentions_worker_titles() {
        let console = render_bootstrap_console(&runtime_bootstrap());
        assert!(console.contains("Worker 1"));
        assert!(console.contains("Worker 2"));
    }

    #[test]
    fn test_render_runtime_console_mentions_layout_label() {
        let console = render_runtime_console(&runtime_model());
        assert!(console.contains("layout=SidebarWithGrid"));
    }

    #[test]
    fn test_render_runtime_console_mentions_bridge_lifecycle_preview() {
        let console = render_runtime_console(&runtime_model());
        assert!(console.contains("bridge=pending -> waiting protocol=?"));
    }

    #[test]
    fn test_runtime_model_uses_bootstrap_title() {
        let model = runtime_model();
        assert_eq!(model.bootstrap.title, WINDOW_TITLE);
    }

    #[test]
    fn test_layout_label_sidebar_with_grid() {
        assert_eq!(layout_label(LayoutMode::SidebarWithGrid), "SidebarWithGrid");
    }

    #[test]
    fn test_runtime_header_view_uses_status_label() {
        let bootstrap = runtime_bootstrap();
        let header = runtime_header_view(&bootstrap);
        assert_eq!(header.status_label, bootstrap.status_label);
    }

    #[test]
    fn test_shell_root_class_sidebar_with_grid() {
        assert_eq!(
            shell_root_class(LayoutMode::SidebarWithGrid),
            "layout-sidebar-with-grid"
        );
    }

    #[test]
    fn test_runtime_header_view_uses_layout_label() {
        let header = runtime_header_view(&runtime_bootstrap());
        assert_eq!(header.layout_label, "SidebarWithGrid");
    }

    #[test]
    fn test_pane_registry_summary_view_defaults_pending() {
        let summary = pane_registry_summary_view(&bootstrap_shell_state());
        assert!(!summary.supervisor_present);
        assert_eq!(summary.worker_count, 0);
    }

    #[test]
    fn test_pane_registry_summary_line_formats_counts() {
        let summary = PaneRegistrySummaryView {
            supervisor_present: true,
            worker_count: 2,
            project_count: 1,
        };
        assert_eq!(
            pane_registry_summary_line(&summary),
            "Supervisor: present · Workers: 2 · Projects: 1"
        );
    }

    #[test]
    fn test_runtime_model_carries_registry_summary() {
        let model = runtime_model();
        assert_eq!(model.registry_summary.project_count, 0);
        assert_eq!(model.registry_summary.worker_count, 0);
    }

    #[test]
    fn test_runtime_model_carries_sidebar_heading() {
        let model = runtime_model();
        assert_eq!(model.sidebar.heading, "Supervisor");
    }

    #[test]
    fn test_runtime_model_carries_worker_grid_class() {
        let model = runtime_model();
        assert_eq!(model.worker_grid_class, "worker-grid-two-column");
    }

    #[test]
    fn test_runtime_model_carries_header_title() {
        let model = runtime_model();
        assert_eq!(model.header.title, WINDOW_TITLE);
    }

    #[test]
    fn test_runtime_shell_model_matches_runtime_model_from_state() {
        let state = bootstrap_shell_state();
        assert_eq!(
            runtime_shell_model(&state),
            runtime_model_from_state(&state)
        );
    }

    #[test]
    fn test_runtime_shell_root_class_uses_layout() {
        let mut state = bootstrap_shell_state();
        state.terminals.layout = LayoutMode::SupervisorFocus;
        assert_eq!(runtime_shell_root_class(&state), "layout-supervisor-focus");
    }

    #[test]
    fn test_runtime_shell_model_preserves_registry_summary() {
        let mut state = bootstrap_shell_state();
        state.session.registered_projects.push("impulse".into());
        let model = runtime_shell_model(&state);
        assert_eq!(model.registry_summary.project_count, 1);
    }

    #[test]
    fn test_worker_summary_label_uses_pane_count() {
        let panes = worker_pane_stub_views(&runtime_bootstrap());
        assert_eq!(worker_summary_label(&panes), "Visible worker panes: 2");
    }

    #[test]
    fn test_runtime_model_worker_count_matches_stub_views() {
        let model = runtime_model();
        assert_eq!(model.worker_panes.len(), 2);
    }

    #[test]
    fn test_runtime_model_carries_worker_summary() {
        let model = runtime_model();
        assert_eq!(model.worker_summary, "Visible worker panes: 2");
    }

    #[test]
    fn test_worker_pane_stub_titles_are_unique() {
        let panes = worker_pane_stub_views(&runtime_bootstrap());
        assert_ne!(panes[0].title, panes[1].title);
    }

    #[test]
    fn test_worker_pane_stub_detail_mentions_placeholder() {
        let panes = worker_pane_stub_views(&runtime_bootstrap());
        assert!(panes[0].detail.contains("Bootstrap placeholder"));
    }

    #[test]
    fn test_runtime_model_from_state_reflects_supervisor_ready() {
        let state: ShellState = serde_json::from_value(serde_json::json!({
            "session": {
                "active_project": "impulse",
                "registered_projects": ["impulse"],
                "focused_pane": null
            },
            "terminals": {
                "registry": {
                    "panes": {
                        "00000000-0000-0000-0000-000000000001": {
                            "id": "00000000-0000-0000-0000-000000000001",
                            "role": "Supervisor",
                            "project": "impulse",
                            "cwd": "/tmp",
                            "spawned_at": 0
                        },
                        "00000000-0000-0000-0000-000000000002": {
                            "id": "00000000-0000-0000-0000-000000000002",
                            "role": "Worker",
                            "project": "impulse",
                            "cwd": "/tmp",
                            "spawned_at": 0
                        }
                    }
                },
                "layout": "SidebarWithGrid",
                "worker_grid": "TwoColumn"
            },
            "ops": {
                "active_compactions": [],
                "notification_count": 0
            }
        }))
        .unwrap();

        let model = runtime_model_from_state(&state);
        assert!(model.bootstrap.status_label.contains("supervisor ready"));
        assert!(model.registry_summary.supervisor_present);
        assert_eq!(model.registry_summary.project_count, 1);
        assert_eq!(model.worker_summary, "Visible worker panes: 2");
    }

    #[test]
    fn test_refresh_runtime_model_replaces_existing_state() {
        let mut model = runtime_model();
        let state = bootstrap_shell_state();
        refresh_runtime_model(&mut model, &state);
        assert_eq!(model.worker_grid_class, "worker-grid-two-column");
    }
}
