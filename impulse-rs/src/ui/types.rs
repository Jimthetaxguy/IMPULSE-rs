use super::*;
use crate::state::Platform;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub(crate) const COLOR_PANEL: Color = Color::Rgb(20, 25, 35);
pub(crate) const COLOR_ACCENT: Color = Color::Cyan;
pub(crate) const COLOR_SUCCESS: Color = Color::Green;
pub(crate) const COLOR_WARNING: Color = Color::Yellow;
pub(crate) const COLOR_ERROR: Color = Color::Red;
pub(crate) const COLOR_TEXT: Color = Color::Gray;
pub(crate) const COLOR_TEXT_BRIGHT: Color = Color::White;
pub(crate) const COLOR_MENU: Color = Color::LightBlue;
pub(crate) const COLOR_MENU_SELECTED: Color = Color::Blue;

#[derive(Debug, Clone, PartialEq)]
pub enum MenuItem {
    File,
    Session,
    View,
    Help,
}

/// Represents a project that Impulse can manage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub path: PathBuf,
    pub name: String,
    pub last_accessed: DateTime<Utc>,
}

impl Project {
    pub fn new(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        Self {
            path,
            name,
            last_accessed: Utc::now(),
        }
    }
}

pub struct TuiState {
    pub state: SharedState,
    pub active_tab: usize,
    pub selected_session: Option<String>,
    pub menu_open: bool,
    pub active_menu: MenuItem,
    pub scroll_offset: usize,
    pub input_mode: bool,
    pub input_text: String,
    pub status_message: Option<String>,
    pub current_session_id: Option<String>,
    pub chat_history: Vec<(bool, String)>, // (is_user, message)
    pub last_refresh: std::time::Instant,
    pub refresh_interval_secs: u64,
    // New fields for augmented TUI
    pub search_query: String,
    pub search_results: Vec<crate::ui::visualization::SearchResult>,
    pub analytics_cache: Option<crate::ui::visualization::AnalyticsSummary>,
    // Filter state
    pub status_filter: Option<crate::state::SessionStatus>,
    // Retrieval status summary
    pub retrieval_health: Option<String>,
    pub last_retrieval_explain: Option<String>,
    pub last_injection_summary: Option<String>,
    // Project management
    pub projects: Vec<Project>,
    pub active_project_index: usize,
    // Terminal tabs (for multiple agent sessions)
    pub terminal_tabs: Vec<TerminalTab>,
    pub active_terminal_tab: usize,
    // Actual PTY panes (integrated terminal instances)
    pub pane_manager: Option<crate::ui::pane_manager::PaneManager>,
    // Injection mode (when 'i' is pressed to inject text to PTY)
    pub injection_mode: bool,
    // Engine state for branding indicator
    pub engine_state: crate::branding::EngineState,
    // Stewardship state
    pub stewardship_proposals: Vec<crate::stewardship::CleanupProposal>,
    pub stewardship_cross_project: Option<crate::stewardship::CrossProjectMemory>,
    pub stewardship_selected_proposal: usize,
    // Context lifecycle state
    pub context_monitor: ContextWindowMonitor,
    pub pending_injections: Vec<PendingInjection>,
    pub last_context_tick: std::time::Instant,
    pub context_lifecycle_enabled: bool,
    // MIER pipeline state
    pub impulse_agent: Option<crate::agent::ImpulseAgent>,
    pub mier_recommendations: Vec<crate::agent::coordinator::Recommendation>,
    pub mier_activity_feed: Vec<MierFeedEntry>,
    pub notification_bus: std::sync::Arc<crate::notification::NotificationBus>,
    pub last_conflict_notification: Option<std::time::Instant>,
    // Conflict resolution UI state
    pub conflicts_panel_open: bool,
    pub selected_conflict_index: usize,
    // Intent detection
    pub intent_store: crate::context_lifecycle::IntentStore,
}

/// Represents a terminal tab (running agent session)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalTab {
    pub id: String,
    pub name: String,
    pub session_id: Option<String>,
    pub platform: Option<Platform>,
    pub is_active: bool,
    pub last_output: String,
    /// The PTY pane ID for context lifecycle lookups (set on spawn)
    #[serde(default)]
    pub pane_id: Option<usize>,
}

/// Entry in the MIER activity feed displayed in the Agent panel.
#[derive(Debug, Clone)]
pub struct MierFeedEntry {
    pub timestamp: std::time::Instant,
    pub kind: MierFeedKind,
    pub message: String,
}

/// Kind of MIER activity feed event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MierFeedKind {
    Injection,
    ThresholdCrossed,
    CompactionDetected,
    Recommendation,
    Extraction,
    PaneSummary,
}

impl TuiState {
    pub fn new(state: SharedState) -> Self {
        // Initialize with current project
        let current_project = Project::new(state.storage().base_path().to_path_buf());

        // Resolve ImpulseAgent from config before moving state into struct
        let impulse_agent = state.config_snapshot().ok().and_then(|c| {
            crate::agent::resolve_from_config(
                c.impulse_agent_provider.as_deref(),
                c.impulse_agent_api_key.as_deref(),
                c.impulse_agent_model.as_deref(),
                c.impulse_agent_harness.as_deref(),
                c.impulse_agent_escalate_model.as_deref(),
            )
        });

        Self {
            state,
            active_tab: 0,
            selected_session: None,
            menu_open: false,
            active_menu: MenuItem::File,
            scroll_offset: 0,
            input_mode: false,
            input_text: String::new(),
            status_message: None,
            current_session_id: None,
            chat_history: vec![(
                false,
                "Welcome to Impulse! Press Tab to switch tabs, m for menu. Use Alt+1-9 to switch projects, Ctrl+1/2/3 to spawn agents.".to_string(),
            )],
            last_refresh: std::time::Instant::now(),
            refresh_interval_secs: 5,
            // New fields
            search_query: String::new(),
            search_results: Vec::new(),
            analytics_cache: None,
            status_filter: None,
            retrieval_health: None,
            last_retrieval_explain: None,
            last_injection_summary: None,
            // Project management - start with current project
            projects: vec![current_project],
            active_project_index: 0,
            // Terminal tabs
            terminal_tabs: Vec::new(),
            active_terminal_tab: 0,
            // PTY pane manager (None until activated)
            pane_manager: None,
            injection_mode: false,
            // Engine state
            engine_state: crate::branding::EngineState::Idle,
            // Stewardship
            stewardship_proposals: Vec::new(),
            stewardship_cross_project: None,
            stewardship_selected_proposal: 0,
            // Context lifecycle
            context_monitor: ContextWindowMonitor::new(200_000),
            pending_injections: Vec::new(),
            last_context_tick: std::time::Instant::now(),
            context_lifecycle_enabled: true,
            // MIER pipeline
            impulse_agent,
            mier_recommendations: Vec::new(),
            mier_activity_feed: Vec::new(),
            notification_bus: std::sync::Arc::new(crate::notification::NotificationBus::new()),
            last_conflict_notification: None,
            // Conflict resolution UI
            conflicts_panel_open: false,
            selected_conflict_index: 0,
            intent_store: crate::context_lifecycle::IntentStore::new(),
        }
    }
}
