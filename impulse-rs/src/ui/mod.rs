use std::io;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs},
    Frame, Terminal,
};
use serde::{Deserialize, Serialize};

use crate::context_lifecycle::detector::CompactionDetector;
use crate::context_lifecycle::extractor::OutputExtractor;
use crate::context_lifecycle::injector::ContextInjector;
use crate::context_lifecycle::monitor::ContextWindowMonitor;
use crate::context_lifecycle::{AgentKind, ContextTier, PaneContextState, PendingInjection};
use crate::state::{Platform, SessionStatus, SharedState};

pub mod pane_manager;
pub mod terminal_pane;
pub mod visualization;
pub use visualization::*;

const COLOR_PANEL: Color = Color::Rgb(20, 25, 35);
const COLOR_ACCENT: Color = Color::Cyan;
const COLOR_SUCCESS: Color = Color::Green;
const COLOR_WARNING: Color = Color::Yellow;
const COLOR_ERROR: Color = Color::Red;
const COLOR_TEXT: Color = Color::Gray;
const COLOR_TEXT_BRIGHT: Color = Color::White;
const COLOR_MENU: Color = Color::LightBlue;
const COLOR_MENU_SELECTED: Color = Color::Blue;

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
    pub impulse_agent: Option<crate::impulse_agent::ImpulseAgent>,
    pub mier_recommendations: Vec<crate::impulse_agent::coordinator::Recommendation>,
    pub mier_activity_feed: Vec<MierFeedEntry>,
    pub notification_bus: std::sync::Arc<crate::notification::NotificationBus>,
    // Intent detection
    pub intent_store: crate::intent::IntentStore,
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
}

impl TuiState {
    pub fn new(state: SharedState) -> Self {
        // Initialize with current project
        let current_project = Project::new(state.storage().base_path().to_path_buf());

        // Resolve ImpulseAgent from config before moving state into struct
        let impulse_agent = state.config_snapshot().ok().and_then(|c| {
            crate::impulse_agent::resolve_from_config(
                c.impulse_agent_provider.as_deref(),
                c.impulse_agent_api_key.as_deref(),
                c.impulse_agent_model.as_deref(),
                c.impulse_agent_harness.as_deref(),
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
                "Welcome to Impulse! Press Tab to switch tabs, m for menu. Use Cmd+1/2/3 to switch projects.".to_string(),
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
            intent_store: crate::intent::IntentStore::new(),
        }
    }
}

pub fn run_ui(state: SharedState) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let state = TuiState::new(state);
    let result = run_app(&mut terminal, state);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut state: TuiState,
) -> anyhow::Result<()> {
    loop {
        // Check for auto-refresh
        let elapsed = state.last_refresh.elapsed().as_secs();
        if elapsed >= state.refresh_interval_secs && !state.input_mode {
            state.last_refresh = std::time::Instant::now();
            // Auto-refresh handled implicitly by re-rendering
        }

        // Context lifecycle tick (every 5 seconds)
        if state.last_context_tick.elapsed().as_secs() >= 5 {
            state.last_context_tick = std::time::Instant::now();
            context_lifecycle_tick(&mut state);
        }

        terminal.draw(|f| ui(f, &mut state))?;

        // Poll with timeout so background ticks fire even when idle
        if !event::poll(Duration::from_millis(200))? {
            continue;
        }

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                // Handle input mode (for chat)
                if state.input_mode {
                    match key.code {
                        KeyCode::Esc => {
                            state.input_mode = false;
                            state.input_text.clear();
                            state.injection_mode = false;
                        }
                        KeyCode::Enter => {
                            let input = state.input_text.clone();
                            state.input_text.clear();
                            state.input_mode = false;

                            // Handle commands
                            if input.starts_with("/track ") {
                                let file_path =
                                    input.trim_start_matches("/track ").trim().to_string();
                                if let Some(ref sid) = state.selected_session {
                                    if !file_path.is_empty() {
                                        let _ = tokio::runtime::Handle::current()
                                            .block_on(state.state.track_file(sid, &file_path));
                                        state.status_message =
                                            Some(format!("Tracked: {}", file_path));
                                    }
                                }
                            } else if input.starts_with("/tool ") {
                                let tool_name =
                                    input.trim_start_matches("/tool ").trim().to_string();
                                if let Some(ref sid) = state.selected_session {
                                    if !tool_name.is_empty() {
                                        let _ = tokio::runtime::Handle::current()
                                            .block_on(state.state.track_tool(sid, &tool_name));
                                        state.status_message =
                                            Some(format!("Tracked tool: {}", tool_name));
                                    }
                                }
                            } else if input.starts_with("/tag ") {
                                let tag = input.trim_start_matches("/tag ").trim().to_string();
                                if let Some(ref sid) = state.selected_session {
                                    if !tag.is_empty() {
                                        let _ = tokio::runtime::Handle::current()
                                            .block_on(state.state.add_tag(sid, &tag));
                                        state.status_message = Some(format!("Added tag: {}", tag));
                                    }
                                }
                            } else if input.starts_with("/session ") {
                                let session_name =
                                    input.trim_start_matches("/session ").trim().to_string();
                                if !session_name.is_empty() {
                                    let result = tokio::runtime::Handle::current().block_on(
                                        state.state.create_session(
                                            session_name,
                                            Some(Platform::ClaudeCode),
                                        ),
                                    );
                                    if let Ok(session) = result {
                                        state.current_session_id = Some(session.id.clone());
                                        state.status_message =
                                            Some(format!("Created: {}", session.name));
                                    }
                                }
                            } else if !input.is_empty() {
                                // Check if we're on search tab
                                if state.active_tab == 5 && input.starts_with("/search ") {
                                    // Handle search command
                                    state.search_query =
                                        input.trim_start_matches("/search ").trim().to_string();
                                    state.status_message =
                                        Some(format!("Searching: {}", state.search_query));
                                } else if state.active_tab == 5 {
                                    // Also allow plain search without prefix when on search tab
                                    state.search_query = input.clone();
                                    state.status_message =
                                        Some(format!("Searching: {}", state.search_query));
                                } else {
                                    // Add as chat message (tab 6)
                                    state.chat_history.push((true, input));
                                }
                            }
                        }
                        KeyCode::Backspace => {
                            state.input_text.pop();
                        }
                        KeyCode::Char(c) => {
                            state.input_text.push(c);
                        }
                        _ => {}
                    }
                    continue;
                }

                // Handle menu mode
                if state.menu_open {
                    match key.code {
                        KeyCode::Esc => {
                            state.menu_open = false;
                        }
                        KeyCode::Left => {
                            state.active_menu = match state.active_menu {
                                MenuItem::File => MenuItem::Help,
                                MenuItem::Session => MenuItem::File,
                                MenuItem::View => MenuItem::Session,
                                MenuItem::Help => MenuItem::View,
                            };
                        }
                        KeyCode::Right => {
                            state.active_menu = match state.active_menu {
                                MenuItem::File => MenuItem::Session,
                                MenuItem::Session => MenuItem::View,
                                MenuItem::View => MenuItem::Help,
                                MenuItem::Help => MenuItem::File,
                            };
                        }
                        KeyCode::Enter => {
                            state.menu_open = false;
                            // Handle menu action
                            match state.active_menu {
                                MenuItem::File => {
                                    state.status_message =
                                        Some("File menu: Quit with q".to_string());
                                }
                                MenuItem::Session => {
                                    state.status_message =
                                        Some("Session menu: n=new, r=refresh".to_string());
                                }
                                MenuItem::View => {
                                    state.active_tab = (state.active_tab + 1) % 10;
                                }
                                MenuItem::Help => {
                                    state.status_message = Some(
                                        "Help: q=quit, m=menu, n=new session, r=refresh"
                                            .to_string(),
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('c')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        return Ok(());
                    }
                    // Project switching with Alt+1-9
                    KeyCode::Char(n @ '1'..='9')
                        if key.modifiers.contains(crossterm::event::KeyModifiers::ALT) =>
                    {
                        let idx = (n as usize) - ('1' as usize);
                        if idx < state.projects.len() {
                            state.active_project_index = idx;
                            let proj = &state.projects[idx];
                            state.status_message =
                                Some(format!("Switched to project: {}", proj.name));
                        }
                    }
                    // Terminal tab switching with Ctrl+1-9
                    KeyCode::Char(n @ '1'..='9')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        let idx = (n as usize) - ('1' as usize);
                        if idx < state.terminal_tabs.len() {
                            state.active_terminal_tab = idx;
                            let tab = &state.terminal_tabs[idx];
                            state.status_message =
                                Some(format!("Switched to terminal: {}", tab.name));
                        }
                    }
                    // Create new terminal tab with Ctrl+n
                    KeyCode::Char('n')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        let tab_name = format!("term-{}", state.terminal_tabs.len() + 1);
                        let new_tab = TerminalTab {
                            id: uuid::Uuid::new_v4().to_string(),
                            name: tab_name.clone(),
                            session_id: None,
                            platform: None,
                            is_active: true,
                            last_output: String::new(),
                            pane_id: None,
                        };
                        state.terminal_tabs.push(new_tab);
                        state.active_terminal_tab = state.terminal_tabs.len() - 1;
                        state.status_message = Some(
                            "Created new terminal tab (Ctrl+1/2/3 to spawn agent)".to_string(),
                        );
                    }
                    // Spawn Claude Code in terminal (Ctrl+1)
                    KeyCode::Char('1')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        spawn_agent_in_terminal(&mut state, "claude", Platform::ClaudeCode);
                    }
                    // Spawn OpenCode in terminal (Ctrl+2)
                    KeyCode::Char('2')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        spawn_agent_in_terminal(&mut state, "opencode", Platform::OpenCode);
                    }
                    // Spawn Codex in terminal (Ctrl+3)
                    KeyCode::Char('3')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        spawn_agent_in_terminal(&mut state, "codex", Platform::OpenCode);
                        // Codex uses opencode
                    }
                    // Close terminal tab with Ctrl+w
                    KeyCode::Char('w')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        if !state.terminal_tabs.is_empty() {
                            state.terminal_tabs.remove(state.active_terminal_tab);
                            if state.active_terminal_tab >= state.terminal_tabs.len() {
                                state.active_terminal_tab =
                                    state.terminal_tabs.len().saturating_sub(1);
                            }
                            state.status_message = Some("Closed terminal tab".to_string());
                        }
                    }
                    KeyCode::Tab | KeyCode::Right => {
                        state.active_tab = (state.active_tab + 1) % 10;
                    }
                    KeyCode::Left => {
                        state.active_tab = if state.active_tab == 0 {
                            9
                        } else {
                            state.active_tab - 1
                        };
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        handle_navigation(&mut state, -1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        handle_navigation(&mut state, 1);
                    }
                    KeyCode::Enter => {
                        handle_selection(&mut state);
                    }
                    KeyCode::Char('m') => {
                        state.menu_open = true;
                        state.active_menu = MenuItem::File;
                    }
                    KeyCode::Char('n') => {
                        // New session
                        let session_name = format!("session-{}", Local::now().format("%H%M"));
                        let result = tokio::runtime::Handle::current().block_on(
                            state
                                .state
                                .create_session(session_name, Some(Platform::ClaudeCode)),
                        );
                        if let Ok(session) = result {
                            state.current_session_id = Some(session.id.clone());
                            state.status_message =
                                Some(format!("Created session: {}", session.name));
                        }
                    }
                    KeyCode::Char('e') => {
                        // End selected session
                        if let Some(ref sid) = state.selected_session {
                            let sid_clone = sid.clone();
                            let result = tokio::runtime::Handle::current().block_on(
                                state
                                    .state
                                    .end_session(&sid_clone, "Ended from TUI".to_string()),
                            );
                            if result.is_ok() {
                                state.status_message = Some(format!(
                                    "Ended session: {}",
                                    &sid_clone[..sid_clone.len().min(8)]
                                ));
                                state.selected_session = None;
                                if state.current_session_id.as_ref() == Some(&sid_clone) {
                                    state.current_session_id = None;
                                }
                            }
                        }
                    }
                    KeyCode::Char('t') => {
                        // Track file (opens input mode for file path)
                        if state.active_tab == 1 && state.selected_session.is_some() {
                            state.input_mode = true;
                            state.input_text = "/track ".to_string();
                            state.status_message = Some("Enter file path to track".to_string());
                        }
                    }
                    KeyCode::Char('T') => {
                        // Track tool (opens input mode for tool name)
                        if state.active_tab == 1 && state.selected_session.is_some() {
                            state.input_mode = true;
                            state.input_text = "/tool ".to_string();
                            state.status_message = Some("Enter tool name to track".to_string());
                        }
                    }
                    KeyCode::Char('s') => {
                        // Show session details
                        if let Some(ref sid) = state.selected_session {
                            state.active_tab = 1; // Switch to sessions tab to see details
                            state.status_message =
                                Some(format!("Selected: {}", &sid[..sid.len().min(12)]));
                        }
                    }
                    KeyCode::Char('r') => {
                        // Refresh
                        state.last_refresh = std::time::Instant::now();
                        state.status_message = Some("Refreshed".to_string());
                    }
                    KeyCode::Char('f') => {
                        // Toggle filter on Sessions tab
                        state.status_filter = match state.status_filter {
                            None => Some(SessionStatus::Active),
                            Some(SessionStatus::Active) => Some(SessionStatus::Idle),
                            Some(SessionStatus::Idle) => Some(SessionStatus::Waiting),
                            Some(SessionStatus::Waiting) => Some(SessionStatus::Completed),
                            Some(SessionStatus::Completed) => Some(SessionStatus::Error),
                            Some(SessionStatus::Error) => None,
                        };
                        let filter_msg = match state.status_filter {
                            Some(SessionStatus::Active) => "Filter: Active",
                            Some(SessionStatus::Idle) => "Filter: Idle",
                            Some(SessionStatus::Waiting) => "Filter: Waiting",
                            Some(SessionStatus::Completed) => "Filter: Completed",
                            Some(SessionStatus::Error) => "Filter: Error",
                            None => "Filter: None (showing all)",
                        };
                        state.status_message = Some(filter_msg.to_string());
                    }
                    KeyCode::Char('d') => {
                        // Show session details
                        if let Some(ref sid) = state.selected_session {
                            if let Ok(sessions) = tokio::runtime::Handle::current()
                                .block_on(state.state.list_sessions())
                            {
                                if let Some(session) = sessions.iter().find(|s| &s.id == sid) {
                                    let details = format!(
                                        "Session: {}\nID: {}\nStatus: {:?}\nPlatform: {:?}\nFiles: {}\nTools: {}\nCreated: {}\nLast: {}",
                                        session.name,
                                        session.id,
                                        session.status,
                                        session.platform,
                                        session.active_files.len(),
                                        session.recent_tools.len(),
                                        session.created_at.format("%Y-%m-%d %H:%M"),
                                        session.last_activity.format("%H:%M")
                                    );
                                    state
                                        .chat_history
                                        .push((false, format!("Session Details:\n{}", details)));
                                    state.active_tab = 7; // Switch to chat to show details
                                    state.status_message =
                                        Some("Session details shown in chat".to_string());
                                }
                            }
                        }
                    }
                    KeyCode::Char('i') => {
                        // Context injection into active agent pane
                        if state.injection_mode {
                            // If already in injection mode, send the text to PTY
                            if !state.input_text.is_empty() {
                                if let Some(ref mut pm) = state.pane_manager {
                                    let text = format!("{}\n", state.input_text);
                                    if pm.inject_to_active(&text).is_ok() {
                                        state.status_message =
                                            Some("Injected to agent".to_string());
                                    } else {
                                        state.status_message = Some("Failed to inject".to_string());
                                    }
                                } else {
                                    state.status_message =
                                        Some("No active terminal pane".to_string());
                                }
                                state.input_text.clear();
                                state.injection_mode = false;
                            } else {
                                state.injection_mode = false;
                            }
                        } else if state.active_tab == 7 {
                            // Chat input mode (tab 7)
                            state.input_mode = true;
                        } else if !state.terminal_tabs.is_empty() {
                            // Enter injection mode for terminal tabs
                            state.injection_mode = true;
                            state.input_mode = true;
                            state.input_text.clear();
                            state.status_message =
                                Some("Type context to inject to agent (Esc to cancel)".to_string());
                        }
                    }
                    KeyCode::Char('/') => {
                        // Start search mode (tab 5)
                        if state.active_tab == 5 {
                            state.input_mode = true;
                            state.search_query.clear();
                            state.input_text = "/search ".to_string();
                        }
                    }
                    KeyCode::Char('g') => {
                        // Go to Genome (tab 4)
                        state.active_tab = 4;
                        state.status_message = Some("Genome".to_string());
                    }
                    KeyCode::Char('a') => {
                        // Go to Analytics (tab 6)
                        state.active_tab = 6;
                        state.status_message = Some("Analytics".to_string());
                    }
                    KeyCode::Char('h') => {
                        // Go to History (tab 3)
                        state.active_tab = 3;
                        state.status_message = Some("History".to_string());
                    }
                    KeyCode::Char('0') => {
                        // Go to Dashboard (tab 0)
                        state.active_tab = 0;
                        state.status_message = Some("Dashboard".to_string());
                    }
                    _ => {}
                }
            }
        }
    }
}

fn spawn_agent_in_terminal(state: &mut TuiState, agent_cmd: &str, platform: Platform) {
    // Create pane manager if it doesn't exist
    if state.pane_manager.is_none() {
        state.pane_manager = Some(crate::ui::pane_manager::PaneManager::new());
    }

    if let Some(ref mut pm) = state.pane_manager {
        // Get current working directory from first project
        let cwd = state.projects.first().map(|p| p.path.as_path());

        // Generate session ID
        let session_id = format!(
            "{}-{}-{}",
            agent_cmd,
            chrono::Local::now().format("%H%M%S"),
            &uuid::Uuid::new_v4().to_string()[..8]
        );

        // Default PTY size
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };

        // Create the pane
        match pm.create_pane(
            agent_cmd.to_string(),
            agent_cmd,
            &[],
            cwd,
            size,
            state.active_project_index,
            None,
            None,
            Some(&session_id),
            Some(match platform {
                Platform::ClaudeCode => "Claude Code",
                Platform::OpenCode => "OpenCode",
            }),
        ) {
            Ok(pane_id) => {
                // Also create UI terminal tab for display
                let tab_name = format!("{}-{}", agent_cmd, pane_id);
                let new_tab = TerminalTab {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: tab_name.clone(),
                    session_id: Some(session_id.clone()),
                    platform: Some(platform),
                    is_active: true,
                    last_output: String::new(),
                    pane_id: Some(pane_id),
                };
                state.terminal_tabs.push(new_tab);
                state.active_terminal_tab = state.terminal_tabs.len() - 1;

                // Schedule context lifecycle injection after startup delay
                let agent_kind = AgentKind::detect(agent_cmd, &tab_name);
                state.pending_injections.push(PendingInjection {
                    pane_id,
                    pane_name: tab_name,
                    agent_kind,
                    scheduled_at: std::time::Instant::now(),
                });
                // Register pane in context monitor
                state
                    .context_monitor
                    .pane_states
                    .insert(pane_id, PaneContextState::new(pane_id, agent_kind));

                state.status_message = Some(format!(
                    "Spawned {} with session {}",
                    agent_cmd,
                    &session_id[..session_id.len().min(12)]
                ));
            }
            Err(e) => {
                state.status_message = Some(format!("Failed to spawn {}: {}", agent_cmd, e));
            }
        }
    } else {
        state.status_message = Some("Pane manager unavailable".to_string());
    }
}

/// Process context lifecycle events: pending injections, threshold monitoring,
/// compaction detection, and output extraction.
/// Called from the event loop every 5 seconds.
fn context_lifecycle_tick(state: &mut TuiState) {
    if !state.context_lifecycle_enabled {
        return;
    }

    let pm = match state.pane_manager.as_ref() {
        Some(pm) => pm,
        None => return,
    };

    // 1. Process pending injections (initial context after spawn delay)
    let mut completed_injections = Vec::new();
    for (idx, pending) in state.pending_injections.iter().enumerate() {
        let elapsed_ms = pending.scheduled_at.elapsed().as_millis() as u64;
        let delay = pending.agent_kind.startup_delay_ms();

        if elapsed_ms < delay {
            continue;
        }

        // Find the pane and check it has produced output
        if let Some(pane) = pm.find_by_id(pending.pane_id) {
            if !pane.is_alive() || pane.output_bytes() == 0 {
                continue;
            }

            // Gather cross-pane insights from other panes
            let cross_insights: Vec<_> = state
                .context_monitor
                .pane_states
                .values()
                .filter(|s| s.pane_id != pending.pane_id)
                .flat_map(|s| s.extracted_insights.iter().cloned())
                .take(crate::context_lifecycle::types::MAX_CROSS_PANE_INSIGHTS)
                .collect();

            let msg = ContextInjector::build_init_message(
                pending.agent_kind,
                None,
                &pending.pane_name,
                &cross_insights,
            );

            if pane.write_input(msg.as_bytes()).is_ok() {
                let _ = pane.write_input(b"\n");
                // Mark injection done in monitor state
                if let Some(pane_state) =
                    state.context_monitor.pane_states.get_mut(&pending.pane_id)
                {
                    pane_state.initial_injection_done = true;
                    pane_state.mark_injected();
                }
                // Emit notification
                let bus = state.notification_bus.clone();
                let pname = pending.pane_name.clone();
                let msg_len = msg.len();
                tokio::runtime::Handle::current().block_on(bus.publish(
                    crate::notification::NotificationEvent::ContextRefreshed {
                        pane_id: pending.pane_id,
                        pane_name: pname.clone(),
                        tier: "init".to_string(),
                        size_chars: msg_len,
                    },
                ));
                state.mier_activity_feed.push(MierFeedEntry {
                    timestamp: std::time::Instant::now(),
                    kind: MierFeedKind::Injection,
                    message: format!("Init injection → {} ({} chars)", pname, msg_len),
                });
            }
            completed_injections.push(idx);
        }
    }
    // Remove completed injections (reverse order to preserve indices)
    for idx in completed_injections.into_iter().rev() {
        state.pending_injections.remove(idx);
    }

    // 2. For each alive pane: monitor thresholds, detect compaction, extract insights
    let window_tokens = state.context_monitor.window_tokens;
    let alive_ids: Vec<usize> = pm
        .panes
        .iter()
        .filter(|p| p.is_alive())
        .map(|p| p.id)
        .collect();

    // Collect pane data with scrollback scan (up to 200 lines back)
    let pane_data: Vec<(usize, u64, String)> = pm
        .panes
        .iter()
        .filter(|p| p.is_alive())
        .map(|p| {
            let current = p.screen_snapshot().contents();
            let scrollback = p.scrollback_len();
            let combined = if scrollback > 0 {
                let mut pages = Vec::new();
                let mut offset = 24;
                while offset <= scrollback.min(200) {
                    pages.push(p.screen_snapshot_at_offset(offset).contents());
                    offset += 24;
                }
                if pages.is_empty() {
                    current
                } else {
                    // Scrollback is older content, so prepend it
                    pages.reverse();
                    pages.push(current);
                    pages.join("\n")
                }
            } else {
                current
            };
            (p.id, p.output_bytes(), combined)
        })
        .collect();

    let mut refresh_actions = Vec::new();

    for (pane_id, output_bytes, screen_text) in &pane_data {
        // Token threshold monitoring
        if let Some(action) = state.context_monitor.check_pane(*pane_id, *output_bytes) {
            refresh_actions.push(action);
        }

        // Compaction detection
        if let Some(pane_state) = state.context_monitor.pane_states.get_mut(pane_id) {
            if let Some(action) =
                CompactionDetector::check_pane(pane_state, screen_text, window_tokens)
            {
                refresh_actions.push(action);
            }

            // Output extraction (every 30s per pane)
            OutputExtractor::check_pane(pane_state, screen_text);
        }
    }

    // 2b. Refine phase: cross-pane coordination via ImpulseAgent
    {
        let all_insights: Vec<_> = state
            .context_monitor
            .pane_states
            .values()
            .flat_map(|s| s.extracted_insights.iter().cloned())
            .collect();

        if !all_insights.is_empty() {
            // Feed insights to intent detection
            for insight in &all_insights {
                let agent_type = match insight.agent_kind {
                    AgentKind::ClaudeCode => crate::intent::AgentType::Claude,
                    AgentKind::Codex => crate::intent::AgentType::Codex,
                    AgentKind::OpenCode => crate::intent::AgentType::OpenCode,
                    AgentKind::GenericShell => crate::intent::AgentType::Shell,
                };
                let activity_type = match insight.insight_type {
                    crate::context_lifecycle::types::InsightType::FileModified => {
                        crate::intent::ActivityType::FileEdit
                    }
                    crate::context_lifecycle::types::InsightType::ErrorEncountered => {
                        crate::intent::ActivityType::Error
                    }
                    crate::context_lifecycle::types::InsightType::TaskCompleted
                    | crate::context_lifecycle::types::InsightType::DecisionMade => {
                        crate::intent::ActivityType::Output
                    }
                };
                let activity = crate::intent::Activity::new(
                    format!("pane-{}", insight.pane_id),
                    agent_type,
                    activity_type,
                )
                .with_target(insight.content.clone())
                .with_details(vec![insight.insight_type.as_str().to_string()]);
                state.intent_store.detect(activity);
            }

            // Run local coordination (file conflicts, cross-pane errors)
            if let Some(ref mut agent) = state.impulse_agent {
                let new_recs = agent.coordinate_local(&all_insights);
                for rec in &new_recs {
                    state.mier_activity_feed.push(MierFeedEntry {
                        timestamp: std::time::Instant::now(),
                        kind: MierFeedKind::Recommendation,
                        message: format!(
                            "[{}] {}",
                            rec.recommendation_type.as_str(),
                            rec.description
                        ),
                    });
                }
                state.mier_recommendations.extend(new_recs);
                if state.mier_recommendations.len() > 20 {
                    let excess = state.mier_recommendations.len() - 20;
                    state.mier_recommendations.drain(..excess);
                }
            }
        }

        // Bound activity feed at 50
        if state.mier_activity_feed.len() > 50 {
            let excess = state.mier_activity_feed.len() - 50;
            state.mier_activity_feed.drain(..excess);
        }
    }

    // 3. Process refresh actions (inject context at appropriate tier)
    let pm = match state.pane_manager.as_ref() {
        Some(pm) => pm,
        None => return,
    };

    for action in refresh_actions {
        match action {
            crate::context_lifecycle::MonitorAction::RefreshContext { pane_id, tier } => {
                if let Some(pane) = pm.find_by_id(pane_id) {
                    let agent_kind = state
                        .context_monitor
                        .pane_states
                        .get(&pane_id)
                        .map(|s| s.agent_kind)
                        .unwrap_or(AgentKind::GenericShell);

                    let cross_insights: Vec<_> = state
                        .context_monitor
                        .pane_states
                        .values()
                        .filter(|s| s.pane_id != pane_id)
                        .flat_map(|s| s.extracted_insights.iter().cloned())
                        .take(crate::context_lifecycle::types::MAX_CROSS_PANE_INSIGHTS)
                        .collect();

                    let pane_name = pane.name.clone();
                    let msg = ContextInjector::build_refresh_message(
                        agent_kind,
                        tier,
                        &pane_name,
                        &cross_insights,
                    );

                    if pane.write_input(msg.as_bytes()).is_ok() {
                        let _ = pane.write_input(b"\n");
                        if let Some(ps) = state.context_monitor.pane_states.get_mut(&pane_id) {
                            ps.mark_injected();
                        }
                        // Emit threshold notification
                        let bus = state.notification_bus.clone();
                        let tier_str = format!("{:?}", tier);
                        let pct = state
                            .context_monitor
                            .pane_states
                            .get(&pane_id)
                            .map(|s| {
                                if window_tokens > 0 {
                                    ((s.estimated_tokens as f64 / window_tokens as f64) * 100.0)
                                        as u8
                                } else {
                                    0
                                }
                            })
                            .unwrap_or(0);
                        tokio::runtime::Handle::current().block_on(bus.publish(
                            crate::notification::NotificationEvent::ContextThresholdCrossed {
                                pane_id,
                                pane_name: pane_name.clone(),
                                threshold_pct: pct,
                                tier: tier_str.clone(),
                            },
                        ));
                        state.mier_activity_feed.push(MierFeedEntry {
                            timestamp: std::time::Instant::now(),
                            kind: MierFeedKind::ThresholdCrossed,
                            message: format!("{}% ({}) → {}", pct, tier_str, pane_name),
                        });
                    }
                }
            }
            crate::context_lifecycle::MonitorAction::CompactionDetected { pane_id } => {
                if let Some(pane) = pm.find_by_id(pane_id) {
                    let agent_kind = state
                        .context_monitor
                        .pane_states
                        .get(&pane_id)
                        .map(|s| s.agent_kind)
                        .unwrap_or(AgentKind::GenericShell);

                    let cross_insights: Vec<_> = state
                        .context_monitor
                        .pane_states
                        .values()
                        .filter(|s| s.pane_id != pane_id)
                        .flat_map(|s| s.extracted_insights.iter().cloned())
                        .take(crate::context_lifecycle::types::MAX_CROSS_PANE_INSIGHTS)
                        .collect();

                    let pane_name = pane.name.clone();
                    let msg = ContextInjector::build_refresh_message(
                        agent_kind,
                        ContextTier::PostCompaction,
                        &pane_name,
                        &cross_insights,
                    );

                    if pane.write_input(msg.as_bytes()).is_ok() {
                        let _ = pane.write_input(b"\n");
                        if let Some(ps) = state.context_monitor.pane_states.get_mut(&pane_id) {
                            ps.mark_injected();
                        }
                        // Emit compaction notification
                        let bus = state.notification_bus.clone();
                        tokio::runtime::Handle::current().block_on(bus.publish(
                            crate::notification::NotificationEvent::CompactionDetected {
                                pane_id,
                                pane_name: pane_name.clone(),
                            },
                        ));
                        state.mier_activity_feed.push(MierFeedEntry {
                            timestamp: std::time::Instant::now(),
                            kind: MierFeedKind::CompactionDetected,
                            message: format!("Compaction detected → {}", pane_name),
                        });
                    }
                }
            }
        }
    }

    // 4. Clean up monitor state for dead panes
    state.context_monitor.cleanup_dead_panes(&alive_ids);
}

fn handle_navigation(state: &mut TuiState, dir: i32) {
    let sessions = tokio::runtime::Handle::current()
        .block_on(state.state.list_sessions())
        .unwrap_or_default();

    if sessions.is_empty() {
        return;
    }

    let current_index = state
        .selected_session
        .as_ref()
        .and_then(|sel| sessions.iter().position(|s| s.id == *sel))
        .unwrap_or(0);

    let new_index = if dir < 0 {
        current_index.saturating_sub(1)
    } else {
        (current_index + 1).min(sessions.len() - 1)
    };

    state.selected_session = Some(sessions[new_index].id.clone());
}

fn handle_selection(state: &mut TuiState) {
    if let Some(ref session_id) = state.selected_session {
        state.current_session_id = Some(session_id.clone());
        state.status_message = Some(format!("Selected session: {}", session_id));
    }
}

fn ui(f: &mut Frame, state: &mut TuiState) {
    let sessions = tokio::runtime::Handle::current()
        .block_on(state.state.list_sessions())
        .unwrap_or_default();
    let history = state.state.get_history_sync().unwrap_or_default();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Menu bar
            Constraint::Length(3), // Header
            Constraint::Length(2), // Project tabs
            Constraint::Length(2), // Terminal tabs
            Constraint::Length(3), // Main tabs
            Constraint::Min(0),    // Content
            Constraint::Length(3), // Status bar
            Constraint::Length(3), // Footer
        ])
        .split(f.area());

    render_menu_bar(f, chunks[0], state);
    render_header(f, chunks[1], &sessions);
    render_project_tabs(f, chunks[2], state);
    render_terminal_tabs(f, chunks[3], state);
    render_tabs(f, chunks[4], state.active_tab);
    render_content(f, chunks[5], state, &sessions, &history);
    render_status_bar(f, chunks[6], state, &sessions);
    render_footer(f, chunks[7]);
}

fn render_menu_bar(f: &mut Frame, area: Rect, state: &TuiState) {
    let menus = vec![
        (MenuItem::File, "File"),
        (MenuItem::Session, "Session"),
        (MenuItem::View, "View"),
        (MenuItem::Help, "Help"),
    ];

    let mut spans = Vec::new();

    for (item, label) in &menus {
        let is_selected = state.menu_open && state.active_menu == *item;
        let style = if is_selected {
            Style::default()
                .fg(COLOR_TEXT_BRIGHT)
                .bg(COLOR_MENU_SELECTED)
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            Style::default().fg(COLOR_MENU)
        };

        spans.push(Span::styled(
            if is_selected {
                format!("[{}]", label)
            } else {
                format!(" {} ", label)
            },
            style,
        ));
        spans.push(Span::raw(" "));
    }

    let block = Block::default().style(Style::default().bg(Color::Rgb(30, 35, 45)));

    f.render_widget(Paragraph::new(Line::from(spans)).block(block), area);
}

fn render_header(f: &mut Frame, area: Rect, sessions: &[crate::state::Session]) {
    let time = Local::now().format("%H:%M:%S").to_string();
    let session_count = sessions.len();
    let active_count = sessions
        .iter()
        .filter(|s| s.status == SessionStatus::Active)
        .count();

    let header_text = Line::from(vec![
        Span::raw("✈ "),
        Span::styled(
            "IMPULSE",
            Style::default()
                .fg(COLOR_ACCENT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::raw(" │ "),
        Span::styled(
            format!("{} sessions", session_count),
            Style::default().fg(COLOR_TEXT_BRIGHT),
        ),
        Span::raw(" │ "),
        Span::styled(
            format!("{} active", active_count),
            Style::default().fg(COLOR_SUCCESS),
        ),
        Span::raw(" │ "),
        Span::styled(time, Style::default().fg(COLOR_TEXT)),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border::DOUBLE)
        .style(Style::default().bg(COLOR_PANEL));

    f.render_widget(Paragraph::new(header_text).block(block), area);
}

fn render_project_tabs(f: &mut Frame, area: Rect, state: &TuiState) {
    if state.projects.is_empty() {
        return;
    }

    let tab_names: Vec<String> = state
        .projects
        .iter()
        .map(|p| format!(" {} ", p.name))
        .collect();

    let tabs = Tabs::new(tab_names)
        .select(state.active_project_index)
        .style(Style::default().fg(COLOR_TEXT))
        .highlight_style(
            Style::default()
                .fg(COLOR_SUCCESS)
                .add_modifier(ratatui::style::Modifier::BOLD)
                .bg(Color::Rgb(30, 40, 50)),
        )
        .divider(Span::raw("│"));

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Rgb(30, 40, 50)));

    let inner = block.inner(area);
    f.render_widget(tabs, inner);
    f.render_widget(block, area);
}

fn render_terminal_tabs(f: &mut Frame, area: Rect, state: &TuiState) {
    if state.terminal_tabs.is_empty() {
        // Show placeholder when no terminals
        let block = Block::default()
            .title(" Terminals ")
            .borders(Borders::ALL)
            .style(Style::default().bg(COLOR_PANEL));

        let text = Paragraph::new("No terminals open. Press Ctrl+N to create a new terminal tab.")
            .style(Style::default().fg(COLOR_TEXT));

        f.render_widget(text.block(block), area);
        return;
    }

    let window_tokens = state.context_monitor.window_tokens;
    let mut spans = Vec::new();

    for (idx, tab) in state.terminal_tabs.iter().enumerate() {
        let is_selected = idx == state.active_terminal_tab;
        let indicator = if tab.is_active { "●" } else { "○" };

        // Look up context % for this tab via pane_id
        let (ctx_label, ctx_color) = if let Some(pid) = tab.pane_id {
            if let Some(ps) = state.context_monitor.pane_states.get(&pid) {
                let pct = if window_tokens > 0 {
                    ((ps.estimated_tokens as f64 / window_tokens as f64) * 100.0) as u8
                } else {
                    0
                };
                if ps.compaction_count > 0
                    && ps
                        .last_compaction_scan_at
                        .map(|t| t.elapsed().as_secs() < 30)
                        .unwrap_or(false)
                {
                    ("..Compact".to_string(), COLOR_ERROR)
                } else {
                    let color = match pct {
                        0..=44 => COLOR_SUCCESS,
                        45..=59 => COLOR_WARNING,
                        60..=79 => Color::Rgb(255, 165, 0), // orange
                        _ => COLOR_ERROR,
                    };
                    (format!("{}%", pct), color)
                }
            } else {
                ("--".to_string(), COLOR_TEXT)
            }
        } else {
            ("--".to_string(), COLOR_TEXT)
        };

        if !spans.is_empty() {
            spans.push(Span::styled("│", Style::default().fg(COLOR_TEXT)));
        }

        let style = if is_selected {
            Style::default()
                .fg(ctx_color)
                .add_modifier(ratatui::style::Modifier::BOLD)
                .bg(Color::Rgb(35, 35, 45))
        } else {
            Style::default().fg(ctx_color)
        };

        spans.push(Span::styled(
            format!(" {}{}: {} ", indicator, tab.name, ctx_label),
            style,
        ));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Rgb(35, 35, 45)));

    let inner = block.inner(area);
    f.render_widget(Paragraph::new(Line::from(spans)), inner);
    f.render_widget(block, area);
}

fn render_tabs(f: &mut Frame, area: Rect, active_tab: usize) {
    let tabs = Tabs::new(vec![
        " Dashboard ",
        " Sessions ",
        " Timeline ",
        " History ",
        " Genome ",
        " Search ",
        " Analytics ",
        " Chat ",
        " Config ",
        " Stewardship ",
    ])
    .select(active_tab)
    .style(Style::default().fg(COLOR_TEXT))
    .highlight_style(
        Style::default()
            .fg(COLOR_ACCENT)
            .add_modifier(ratatui::style::Modifier::BOLD)
            .bg(COLOR_PANEL),
    )
    .divider(Span::raw("│"));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border::DOUBLE)
        .style(Style::default().bg(COLOR_PANEL));

    let inner = block.inner(area);
    f.render_widget(tabs, inner);
    f.render_widget(block, area);
}

fn render_content(
    f: &mut Frame,
    area: Rect,
    state: &mut TuiState,
    sessions: &[crate::state::Session],
    history: &[crate::state::HistoryEntry],
) {
    match state.active_tab {
        0 => render_dashboard(f, area, state, sessions, history),
        1 => render_sessions(f, area, state, sessions),
        2 => render_timeline(f, area, sessions, history),
        3 => render_history(f, area, history),
        4 => render_genome(f, area, state),
        5 => render_search(f, area, state, sessions, history),
        6 => render_analytics(f, area, sessions, history),
        7 => render_chat(f, area, state, sessions),
        8 => render_config(f, area, state),
        9 => render_stewardship(f, area, state),
        _ => {}
    }
}

fn render_status_bar(
    f: &mut Frame,
    area: Rect,
    state: &TuiState,
    sessions: &[crate::state::Session],
) {
    // Build session info with platform details
    let session_info = if let Some(ref sid) = state.current_session_id {
        if let Some(s) = sessions.iter().find(|s| &s.id == sid) {
            let platform_str = match &s.platform {
                Some(Platform::ClaudeCode) => "Claude Code",
                Some(Platform::OpenCode) => "OpenCode",
                None => "Unknown",
            };
            // Show truncated session ID
            let sid_short = if sid.len() > 12 { &sid[..12] } else { sid };
            format!(
                "{} | Session: {} | Status: {:?}",
                platform_str, sid_short, s.status
            )
        } else {
            format!("Session: {}", &sid[..sid.len().min(8)])
        }
    } else {
        "No session selected".to_string()
    };

    // Show terminal tab info if present
    let terminal_info = if !state.terminal_tabs.is_empty() {
        let tab = &state.terminal_tabs[state.active_terminal_tab];
        let platform_str = tab
            .platform
            .as_ref()
            .map(|p| match p {
                Platform::ClaudeCode => "Claude Code",
                Platform::OpenCode => "OpenCode",
            })
            .unwrap_or("Unknown");

        let sid_info = tab
            .session_id
            .as_ref()
            .map(|sid| {
                let s = if sid.len() > 8 { &sid[..8] } else { sid };
                format!(" | Session: {}", s)
            })
            .unwrap_or_default();

        Some(format!(
            "[Terminal: {} | Platform: {}{}]",
            tab.name, platform_str, sid_info
        ))
    } else {
        None
    };

    // Use status message if set, otherwise use session info
    let status = if let Some(ref msg) = state.status_message {
        if let Some(ref term_info) = terminal_info {
            format!("{} | {}", msg, term_info)
        } else {
            msg.clone()
        }
    } else if let Some(ref term_info) = terminal_info {
        format!("{} | {}", session_info, term_info)
    } else {
        session_info
    };

    let block = Block::default().style(Style::default().bg(Color::Rgb(25, 30, 40)));

    let text = Line::from(vec![
        Span::styled("│ ", Style::default().fg(COLOR_TEXT)),
        Span::raw(status),
    ]);

    f.render_widget(Paragraph::new(text).block(block), area);
}

fn render_dashboard(
    f: &mut Frame,
    area: Rect,
    state: &mut TuiState,
    sessions: &[crate::state::Session],
    history: &[crate::state::HistoryEntry],
) {
    let retrieval_health = if let Ok(config) = state.state.config_snapshot() {
        match crate::retrieval::status(state.state.storage().base_path(), &config, false) {
            Ok(status) => {
                let summary = format!(
                    "Retrieval: db={} vec_ext={} indexed={} history={} genome={} | Injection: mode={} scope={} last={} staged={}",
                    if status.db_exists { "ok" } else { "missing" },
                    status.vector_extension_available,
                    status.index_state.indexed_at.format("%H:%M:%S"),
                    status.index_state.history_count,
                    status.index_state.genome_count,
                    status.injection.config_mode,
                    status.injection.config_scope,
                    status
                        .injection
                        .last_staged_status
                        .clone()
                        .unwrap_or_else(|| "none".to_string()),
                    status.injection.staged_artifact_count
                );
                state.retrieval_health = Some(summary.clone());
                state.last_injection_summary = Some(format!(
                    "mode={} scope={} last_status={} last_artifact={}",
                    status.injection.config_mode,
                    status.injection.config_scope,
                    status
                        .injection
                        .last_staged_status
                        .unwrap_or_else(|| "none".to_string()),
                    status
                        .injection
                        .last_staged_artifact
                        .unwrap_or_else(|| "none".to_string())
                ));
                Some(summary)
            }
            Err(e) => {
                let msg = format!("Retrieval status unavailable: {}", e);
                state.retrieval_health = Some(msg.clone());
                state.last_injection_summary = None;
                Some(msg)
            }
        }
    } else {
        None
    };

    // Derive engine state from session activity
    let engine = if sessions.iter().any(|s| s.status == SessionStatus::Active) {
        if state.input_mode {
            crate::branding::EngineState::Thinking
        } else {
            crate::branding::EngineState::Success
        }
    } else {
        crate::branding::EngineState::Idle
    };
    state.engine_state = engine;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Split left panel: engine indicator on top, stats below
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(0)])
        .split(chunks[0]);

    render_engine_indicator(f, left_chunks[0], engine);
    render_stats_panel(
        f,
        left_chunks[1],
        sessions,
        history,
        retrieval_health.as_deref(),
    );
    render_activity_panel(f, chunks[1], sessions);
}

fn render_engine_indicator(f: &mut Frame, area: Rect, engine: crate::branding::EngineState) {
    let art = crate::branding::engine_art(engine);
    let label = match engine {
        crate::branding::EngineState::Idle => "IDLE",
        crate::branding::EngineState::Thinking => "THINKING",
        crate::branding::EngineState::Success => "ENGINES LIT",
    };
    let color = match engine {
        crate::branding::EngineState::Idle => COLOR_TEXT,
        crate::branding::EngineState::Thinking => COLOR_WARNING,
        crate::branding::EngineState::Success => COLOR_SUCCESS,
    };

    let mut lines: Vec<Line> = art
        .lines()
        .map(|l| Line::from(Span::styled(format!("  {l}"), Style::default().fg(color))))
        .collect();
    lines.push(Line::from(Span::styled(
        format!("  {label}"),
        Style::default()
            .fg(color)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )));

    let block = Block::default()
        .title(" ✈ Engine ")
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .style(Style::default().bg(COLOR_PANEL));

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_stats_panel(
    f: &mut Frame,
    area: Rect,
    sessions: &[crate::state::Session],
    history: &[crate::state::HistoryEntry],
    retrieval_health: Option<&str>,
) {
    #[allow(clippy::vec_init_then_push)]
    {
        let total_sessions = sessions.len() + history.len();
        let active_sessions = sessions
            .iter()
            .filter(|s| s.status == SessionStatus::Active)
            .count();
        let total_files: usize = sessions.iter().map(|s| s.active_files.len()).sum();
        let total_tools: usize = sessions.iter().map(|s| s.recent_tools.len()).sum();

        let mut content = Vec::new();
        content.push(Line::from(""));
        content.push(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(COLOR_ACCENT)),
            Span::raw("Total Sessions: "),
            Span::styled(
                total_sessions.to_string(),
                Style::default()
                    .fg(COLOR_TEXT_BRIGHT)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
        ]));
        content.push(Line::from(""));
        content.push(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(COLOR_SUCCESS)),
            Span::raw("Active: "),
            Span::styled(
                active_sessions.to_string(),
                Style::default()
                    .fg(COLOR_SUCCESS)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
        ]));
        content.push(Line::from(""));
        content.push(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(COLOR_WARNING)),
            Span::raw("Files Tracked: "),
            Span::styled(
                total_files.to_string(),
                Style::default()
                    .fg(COLOR_TEXT_BRIGHT)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
        ]));
        content.push(Line::from(""));
        content.push(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(COLOR_ACCENT)),
            Span::raw("Tools Used: "),
            Span::styled(
                total_tools.to_string(),
                Style::default()
                    .fg(COLOR_TEXT_BRIGHT)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
        ]));
        if let Some(health) = retrieval_health {
            content.push(Line::from(""));
            content.push(Line::from(vec![
                Span::styled("▸ ", Style::default().fg(COLOR_WARNING)),
                Span::raw(health.to_string()),
            ]));
        }
        content.push(Line::from(""));
        content.push(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(COLOR_TEXT)),
            Span::raw("History: "),
            Span::styled(
                history.len().to_string(),
                Style::default()
                    .fg(COLOR_TEXT)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
        ]));
        content.push(Line::from(""));
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "Recent Activity",
            Style::default()
                .fg(COLOR_TEXT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));
        content.push(Line::from(""));

        // Add sparkline if we have history
        if !history.is_empty() {
            let activity_data: Vec<f64> = history
                .iter()
                .rev()
                .take(7)
                .map(|h| h.files_touched.len() as f64)
                .collect();
            if !activity_data.is_empty() {
                let spark = crate::ui::visualization::sparkline(&activity_data, 20);
                content.push(Line::from(vec![
                    Span::raw("  Activity: "),
                    Span::styled(spark, Style::default().fg(COLOR_ACCENT)),
                ]));
                content.push(Line::from(""));
            }
        }

        for session in sessions.iter().take(5) {
            let status_color = match session.status {
                SessionStatus::Active => COLOR_SUCCESS,
                SessionStatus::Idle => COLOR_WARNING,
                SessionStatus::Waiting => COLOR_ACCENT,
                SessionStatus::Completed => COLOR_TEXT,
                SessionStatus::Error => COLOR_ERROR,
            };

            let time = session.last_activity.format("%H:%M").to_string();
            content.push(Line::from(vec![
                Span::styled("● ", Style::default().fg(status_color)),
                Span::raw(format!("{} ", time)),
                Span::styled(
                    &session.name[..session.name.len().min(20)],
                    Style::default().fg(COLOR_TEXT_BRIGHT),
                ),
            ]));
        }

        let block = Block::default()
            .title("  Statistics  ")
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .style(Style::default().bg(COLOR_PANEL));

        f.render_widget(Paragraph::new(content).block(block), area);
    }
}

fn render_activity_panel(f: &mut Frame, area: Rect, sessions: &[crate::state::Session]) {
    let mut content = Vec::new();
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "  Session Activity  ",
        Style::default()
            .fg(COLOR_TEXT_BRIGHT)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )));
    content.push(Line::from(""));

    for session in sessions.iter().take(8) {
        let status_symbol = match session.status {
            SessionStatus::Active => "●",
            SessionStatus::Idle => "◐",
            SessionStatus::Waiting => "○",
            SessionStatus::Completed => "✓",
            SessionStatus::Error => "✗",
        };

        let status_color = match session.status {
            SessionStatus::Active => COLOR_SUCCESS,
            SessionStatus::Idle => COLOR_WARNING,
            SessionStatus::Waiting => COLOR_ACCENT,
            SessionStatus::Completed => COLOR_TEXT,
            SessionStatus::Error => COLOR_ERROR,
        };

        content.push(Line::from(vec![
            Span::styled(
                format!("{} ", status_symbol),
                Style::default()
                    .fg(status_color)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(&session.name, Style::default().fg(COLOR_TEXT_BRIGHT)),
        ]));

        if !session.active_files.is_empty() {
            let file_count = session.active_files.len();
            content.push(Line::from(vec![
                Span::raw("    📄 "),
                Span::styled(
                    format!("{} files", file_count),
                    Style::default().fg(COLOR_TEXT),
                ),
            ]));
        }

        if !session.recent_tools.is_empty() {
            let tools: Vec<String> = session.recent_tools.iter().take(3).cloned().collect();
            let tools_str = tools.join(", ");
            content.push(Line::from(vec![
                Span::raw("    🔧 "),
                Span::styled(tools_str, Style::default().fg(COLOR_TEXT)),
            ]));
        }

        content.push(Line::from(""));
    }

    if sessions.is_empty() {
        content.push(Line::from(vec![Span::styled(
            "  No active sessions  ",
            Style::default().fg(COLOR_TEXT),
        )]));
        content.push(Line::from(""));
        content.push(Line::from(vec![
            Span::raw("  Press "),
            Span::styled("n", Style::default().fg(COLOR_ACCENT)),
            Span::raw(" to create a new session"),
        ]));
    }

    let block = Block::default()
        .title("  Live  ")
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .style(Style::default().bg(COLOR_PANEL));

    f.render_widget(Paragraph::new(content).block(block), area);
}

fn render_sessions(
    f: &mut Frame,
    area: Rect,
    state: &TuiState,
    sessions: &[crate::state::Session],
) {
    // Apply filter if set
    let filtered_sessions: Vec<_> = if let Some(filter_status) = state.status_filter {
        sessions
            .iter()
            .filter(|s| s.status == filter_status)
            .collect()
    } else {
        sessions.iter().collect()
    };

    if filtered_sessions.is_empty() {
        let filter_msg = if state.status_filter.is_some() {
            "No sessions match filter\n\nPress 'f' to change filter"
        } else {
            "No active sessions\n\nPress 'n' to create a new session"
        };
        let block = Block::default()
            .title(" Sessions ")
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .style(Style::default().bg(COLOR_PANEL));

        f.render_widget(
            Paragraph::new(filter_msg)
                .block(block)
                .style(Style::default().fg(COLOR_TEXT)),
            area,
        );
        return;
    }

    let rows: Vec<Row> = filtered_sessions
        .iter()
        .map(|s| {
            let status = match s.status {
                SessionStatus::Active => "Active",
                SessionStatus::Idle => "Idle",
                SessionStatus::Waiting => "Waiting",
                SessionStatus::Completed => "Done",
                SessionStatus::Error => "Error",
            };

            let platform = match &s.platform {
                Some(Platform::ClaudeCode) => "Claude",
                Some(Platform::OpenCode) => "OpenCode",
                None => "Unknown",
            };

            let files = s.active_files.len().to_string();
            let tools = s.recent_tools.len().to_string();
            let time = s.last_activity.format("%H:%M").to_string();

            Row::new(vec![
                Cell::from(Span::raw(&s.name[..s.name.len().min(15)])),
                Cell::from(Span::raw(status)),
                Cell::from(Span::raw(platform)),
                Cell::from(Span::raw(files)),
                Cell::from(Span::raw(tools)),
                Cell::from(Span::raw(time)),
            ])
        })
        .collect();

    let block = Block::default()
        .title(" Sessions ")
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .style(Style::default().bg(COLOR_PANEL));

    let table = Table::new(
        rows,
        &[
            Constraint::Length(16),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(6),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from(Span::styled(
                "Name",
                Style::default().add_modifier(ratatui::style::Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Status",
                Style::default().add_modifier(ratatui::style::Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Platform",
                Style::default().add_modifier(ratatui::style::Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Files",
                Style::default().add_modifier(ratatui::style::Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Tools",
                Style::default().add_modifier(ratatui::style::Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Last",
                Style::default().add_modifier(ratatui::style::Modifier::BOLD),
            )),
        ])
        .style(Style::default().fg(COLOR_ACCENT).bg(COLOR_PANEL)),
    )
    .block(block)
    .highlight_style(Style::default().fg(COLOR_TEXT_BRIGHT).bg(COLOR_PANEL));

    f.render_widget(table, area);
}

fn render_timeline(
    f: &mut Frame,
    area: Rect,
    sessions: &[crate::state::Session],
    history: &[crate::state::HistoryEntry],
) {
    use crate::ui::visualization::format_duration;

    let mut content = Vec::new();
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "  Session Timeline  ",
        Style::default()
            .fg(COLOR_TEXT_BRIGHT)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )));
    content.push(Line::from(""));

    // Active sessions
    if !sessions.is_empty() {
        content.push(Line::from(Span::styled(
            "  Active Sessions  ",
            Style::default()
                .fg(COLOR_SUCCESS)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));
        content.push(Line::from(""));

        for session in sessions.iter().take(8) {
            let duration = (session.last_activity - session.created_at).num_seconds();
            let time_str = session.created_at.format("%H:%M").to_string();

            content.push(Line::from(vec![
                Span::styled("● ", Style::default().fg(COLOR_SUCCESS)),
                Span::styled(
                    &session.name[..session.name.len().min(18)],
                    Style::default().fg(COLOR_TEXT_BRIGHT),
                ),
            ]));
            content.push(Line::from(vec![
                Span::raw("    "),
                Span::raw(time_str),
                Span::raw(" - "),
                Span::styled(format_duration(duration), Style::default().fg(COLOR_TEXT)),
                Span::raw(" │ "),
                Span::styled(
                    format!("{} files", session.active_files.len()),
                    Style::default().fg(COLOR_WARNING),
                ),
                Span::raw(" │ "),
                Span::styled(
                    format!("{} tools", session.recent_tools.len()),
                    Style::default().fg(COLOR_ACCENT),
                ),
            ]));
            content.push(Line::from(""));
        }
    }

    // Recent history
    content.push(Line::from(Span::styled(
        "  Recent History  ",
        Style::default()
            .fg(COLOR_TEXT)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )));
    content.push(Line::from(""));

    for entry in history.iter().take(10) {
        let duration = (entry.ended_at - entry.started_at).num_seconds();
        let date_str = entry.started_at.format("%m/%d %H:%M").to_string();

        content.push(Line::from(vec![
            Span::styled("○ ", Style::default().fg(COLOR_TEXT)),
            Span::styled(
                &entry.session_name[..entry.session_name.len().min(18)],
                Style::default().fg(COLOR_TEXT_BRIGHT),
            ),
        ]));
        content.push(Line::from(vec![
            Span::raw("    "),
            Span::raw(date_str),
            Span::raw(" - "),
            Span::styled(format_duration(duration), Style::default().fg(COLOR_TEXT)),
        ]));

        if !entry.summary.is_empty() {
            let summary = if entry.summary.len() > 40 {
                format!("{}...", &entry.summary[..40])
            } else {
                entry.summary.clone()
            };
            content.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(summary, Style::default().fg(COLOR_TEXT)),
            ]));
        }
        content.push(Line::from(""));
    }

    if sessions.is_empty() && history.is_empty() {
        content.push(Line::from(vec![Span::styled(
            "  No session data  ",
            Style::default().fg(COLOR_TEXT),
        )]));
        content.push(Line::from(""));
        content.push(Line::from(vec![
            Span::raw("  Press "),
            Span::styled("n", Style::default().fg(COLOR_ACCENT)),
            Span::raw(" to create a session"),
        ]));
    }

    let block = Block::default()
        .title(" Timeline ")
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .style(Style::default().bg(COLOR_PANEL));

    f.render_widget(Paragraph::new(content).block(block), area);
}

fn render_history(f: &mut Frame, area: Rect, history: &[crate::state::HistoryEntry]) {
    let mut content = Vec::new();
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "  Recent Sessions  ",
        Style::default()
            .fg(COLOR_TEXT_BRIGHT)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )));
    content.push(Line::from(""));

    for entry in history.iter().take(15) {
        let time = entry.ended_at.format("%m/%d %H:%M").to_string();
        let platform = match &entry.platform {
            Some(Platform::ClaudeCode) => "Claude",
            Some(Platform::OpenCode) => "OpenCode",
            None => "-",
        };

        content.push(Line::from(vec![
            Span::styled(format!("{} ", time), Style::default().fg(COLOR_TEXT)),
            Span::styled(
                format!("{:10}", platform),
                Style::default().fg(COLOR_ACCENT),
            ),
            Span::raw(" "),
            Span::styled(
                &entry.session_name[..entry.session_name.len().min(15)],
                Style::default().fg(COLOR_TEXT_BRIGHT),
            ),
        ]));

        if !entry.summary.is_empty() {
            let summary = if entry.summary.len() > 40 {
                format!("{}...", &entry.summary[..40])
            } else {
                entry.summary.clone()
            };
            content.push(Line::from(vec![
                Span::raw("              "),
                Span::styled(summary, Style::default().fg(COLOR_TEXT)),
            ]));
        }

        content.push(Line::from(""));
    }

    if history.is_empty() {
        content.push(Line::from(vec![Span::styled(
            "  No session history  ",
            Style::default().fg(COLOR_TEXT),
        )]));
    }

    let block = Block::default()
        .title(" History ")
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .style(Style::default().bg(COLOR_PANEL));

    f.render_widget(Paragraph::new(content).block(block), area);
}

fn render_genome(f: &mut Frame, area: Rect, state: &TuiState) {
    let genome = state
        .state
        .storage()
        .read_json::<crate::memory::Genome>("GENOME.md")
        .unwrap_or_default();

    let mut content = Vec::new();
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "  GENOME.md  ",
        Style::default()
            .fg(COLOR_TEXT_BRIGHT)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )));
    content.push(Line::from(""));

    if !genome.decisions.is_empty() {
        content.push(Line::from(Span::styled(
            "  Decisions  ",
            Style::default()
                .fg(COLOR_ACCENT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));
        content.push(Line::from(""));

        for decision in genome.decisions.iter().take(10) {
            let date = decision.date.format("%Y-%m-%d").to_string();
            content.push(Line::from(vec![
                Span::styled("● ", Style::default().fg(COLOR_SUCCESS)),
                Span::styled(format!("{} ", date), Style::default().fg(COLOR_TEXT)),
                Span::raw(&decision.description[..decision.description.len().min(30)]),
            ]));
            content.push(Line::from(""));
        }
    }

    if !genome.preferences.is_empty() {
        content.push(Line::from(Span::styled(
            "  Preferences  ",
            Style::default()
                .fg(COLOR_WARNING)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));
        content.push(Line::from(""));

        for pref in genome.preferences.iter().take(5) {
            content.push(Line::from(vec![
                Span::styled("◐ ", Style::default().fg(COLOR_WARNING)),
                Span::raw(format!("[{}] ", pref.category)),
                Span::raw(&pref.description[..pref.description.len().min(25)]),
            ]));
            content.push(Line::from(""));
        }
    }

    if genome.decisions.is_empty() && genome.preferences.is_empty() {
        content.push(Line::from(vec![Span::styled(
            "  No decisions recorded  ",
            Style::default().fg(COLOR_TEXT),
        )]));
        content.push(Line::from(""));
        content.push(Line::from(vec![
            Span::raw("  Run "),
            Span::styled("impulse-rs add-decision", Style::default().fg(COLOR_ACCENT)),
            Span::raw(" to add"),
        ]));
    }

    let block = Block::default()
        .title(" Genome ")
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .style(Style::default().bg(COLOR_PANEL));

    f.render_widget(Paragraph::new(content).block(block), area);
}

fn render_chat(
    f: &mut Frame,
    area: Rect,
    state: &mut TuiState,
    sessions: &[crate::state::Session],
) {
    // Split area: MIER panel (top 45%) + Chat (bottom 55%)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    render_mier_panel(f, chunks[0], state);
    render_chat_inner(f, chunks[1], state, sessions);
}

fn render_mier_panel(f: &mut Frame, area: Rect, state: &TuiState) {
    let mut lines: Vec<Line> = Vec::new();

    // Agent status
    let status = state
        .impulse_agent
        .as_ref()
        .map(|a| a.status_summary())
        .unwrap_or_else(|| "Disabled".to_string());
    lines.push(Line::from(vec![
        Span::styled("Agent: ", Style::default().fg(COLOR_ACCENT)),
        Span::styled(
            status,
            Style::default()
                .fg(COLOR_TEXT_BRIGHT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
    ]));

    // Per-pane context gauges
    let window_tokens = state.context_monitor.window_tokens;
    if !state.context_monitor.pane_states.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Pane Context",
            Style::default()
                .fg(COLOR_TEXT_BRIGHT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));
        for (pid, ps) in &state.context_monitor.pane_states {
            let pct = if window_tokens > 0 {
                ((ps.estimated_tokens as f64 / window_tokens as f64) * 100.0) as u8
            } else {
                0
            };
            let color = match pct {
                0..=44 => COLOR_SUCCESS,
                45..=59 => COLOR_WARNING,
                60..=79 => Color::Rgb(255, 165, 0),
                _ => COLOR_ERROR,
            };
            let label = if ps.compaction_count > 0
                && ps
                    .last_compaction_scan_at
                    .map(|t| t.elapsed().as_secs() < 30)
                    .unwrap_or(false)
            {
                format!("..Compact (x{})", ps.compaction_count)
            } else {
                format!("{}%", pct)
            };
            let kind_str = match ps.agent_kind {
                AgentKind::ClaudeCode => "claude",
                AgentKind::Codex => "codex",
                AgentKind::OpenCode => "opencode",
                AgentKind::GenericShell => "shell",
            };
            // Simple gauge bar
            let bar_len = (pct as usize).min(20) / 5;
            let bar: String = "|".repeat(bar_len);
            let empty: String = ".".repeat(4usize.saturating_sub(bar_len));
            lines.push(Line::from(vec![
                Span::styled(format!("  p{} ", pid), Style::default().fg(COLOR_TEXT)),
                Span::styled(
                    format!("{:8} ", kind_str),
                    Style::default().fg(COLOR_TEXT_BRIGHT),
                ),
                Span::styled(format!("[{}{}]", bar, empty), Style::default().fg(color)),
                Span::styled(format!(" {}", label), Style::default().fg(color)),
            ]));
        }
    }

    // Recommendations (last 3)
    if !state.mier_recommendations.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Recommendations",
            Style::default()
                .fg(COLOR_TEXT_BRIGHT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));
        for rec in state.mier_recommendations.iter().rev().take(3) {
            let (icon, color) = match rec.recommendation_type {
                crate::impulse_agent::coordinator::RecommendationType::FileConflict => {
                    ("!", COLOR_ERROR)
                }
                crate::impulse_agent::coordinator::RecommendationType::ErrorAssist => {
                    ("?", COLOR_WARNING)
                }
                crate::impulse_agent::coordinator::RecommendationType::CrossPaneSync => {
                    ("~", COLOR_ACCENT)
                }
                crate::impulse_agent::coordinator::RecommendationType::TaskComplete => {
                    ("v", COLOR_SUCCESS)
                }
            };
            let desc = if rec.description.len() > 50 {
                format!("{}...", &rec.description[..50])
            } else {
                rec.description.clone()
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {} ", icon), Style::default().fg(color)),
                Span::styled(desc, Style::default().fg(color)),
            ]));
        }
    }

    // Activity feed (last 8)
    if !state.mier_activity_feed.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Activity",
            Style::default()
                .fg(COLOR_TEXT_BRIGHT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));
        for entry in state.mier_activity_feed.iter().rev().take(8) {
            let color = match entry.kind {
                MierFeedKind::Injection => COLOR_ACCENT,
                MierFeedKind::ThresholdCrossed => COLOR_WARNING,
                MierFeedKind::CompactionDetected => COLOR_ERROR,
                MierFeedKind::Recommendation => Color::Rgb(255, 165, 0),
                MierFeedKind::Extraction => COLOR_TEXT,
            };
            let age = entry.timestamp.elapsed().as_secs();
            let age_str = if age < 60 {
                format!("{}s", age)
            } else {
                format!("{}m", age / 60)
            };
            let msg = if entry.message.len() > 45 {
                format!("{}...", &entry.message[..45])
            } else {
                entry.message.clone()
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:>4} ", age_str),
                    Style::default().fg(COLOR_TEXT),
                ),
                Span::styled(msg, Style::default().fg(color)),
            ]));
        }
    }

    let block = Block::default()
        .title(" MIER Pipeline ")
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .style(Style::default().bg(COLOR_PANEL));

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_chat_inner(
    f: &mut Frame,
    area: Rect,
    state: &mut TuiState,
    sessions: &[crate::state::Session],
) {
    let mut content = Vec::new();

    // Show current session context
    if let Some(ref sid) = state.current_session_id {
        if let Some(s) = sessions.iter().find(|s| &s.id == sid) {
            content.push(Line::from(vec![
                Span::styled("Session: ", Style::default().fg(COLOR_ACCENT)),
                Span::raw(&s.name),
            ]));
            if !s.active_files.is_empty() {
                content.push(Line::from(vec![
                    Span::styled("Files: ", Style::default().fg(COLOR_WARNING)),
                    Span::raw(
                        s.active_files
                            .iter()
                            .take(3)
                            .map(|f| f.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                ]));
            }
        }
    }

    // Chat history
    for (is_user, msg) in state.chat_history.iter().rev().take(8).rev() {
        let prefix = if *is_user { "▸ " } else { "◇ " };
        let style = if *is_user {
            Style::default().fg(COLOR_ACCENT)
        } else {
            Style::default().fg(COLOR_TEXT)
        };
        content.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::raw(msg),
        ]));
    }

    // Input prompt
    content.push(Line::from(""));
    content.push(Line::from(vec![
        Span::styled("▸ ", Style::default().fg(COLOR_SUCCESS)),
        if state.input_mode {
            Span::styled(
                format!("{}{}", state.input_text, "_"),
                Style::default().fg(COLOR_TEXT_BRIGHT),
            )
        } else {
            Span::styled("Press i to type...", Style::default().fg(COLOR_TEXT))
        },
    ]));

    let block = Block::default()
        .title(" Chat ")
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .style(Style::default().bg(COLOR_PANEL));

    f.render_widget(Paragraph::new(content).block(block), area);
}

fn render_config(f: &mut Frame, area: Rect, _state: &TuiState) {
    #[allow(clippy::vec_init_then_push)]
    {
        let mut content = Vec::new();
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  Configuration & Help  ",
            Style::default()
                .fg(COLOR_TEXT_BRIGHT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));
        content.push(Line::from(""));

        // Keyboard shortcuts
        content.push(Line::from(Span::styled(
            "  Keyboard Shortcuts  ",
            Style::default()
                .fg(COLOR_ACCENT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));
        content.push(Line::from(""));

        let shortcuts = vec![
            ("n", "Create new session"),
            ("e", "End selected session"),
            ("t", "Track file (/track <path>)"),
            ("T", "Track tool (/tool <name>)"),
            ("d", "Show session details"),
            ("i", "Input mode (in chat tab)"),
            ("r", "Refresh"),
            ("f", "Filter sessions (on sessions tab)"),
            ("s", "Select session"),
            ("m", "Open menu"),
            ("/", "Search (in search tab)"),
            ("Tab", "Switch tabs"),
            ("0-9", "Go to tab (0=Dashboard, etc)"),
            ("g", "Go to Genome"),
            ("a", "Go to Analytics"),
            ("h", "Go to History"),
            ("q", "Quit"),
        ];

        // Add number shortcuts help
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  Tab Numbers  ",
            Style::default()
                .fg(COLOR_ACCENT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));
        content.push(Line::from(""));
        content.push(Line::from(vec![Span::raw(
            "  0:Dashboard 1:Sessions 2:Timeline ",
        )]));
        content.push(Line::from(vec![Span::raw(
            "  3:History 4:Genome 5:Search ",
        )]));
        content.push(Line::from(vec![Span::raw(
            "  6:Analytics 7:Chat 8:Config 9:Stewardship ",
        )]));

        for (key, desc) in shortcuts {
            content.push(Line::from(vec![
                Span::styled(format!("{:4}", key), Style::default().fg(COLOR_WARNING)),
                Span::raw("  "),
                Span::raw(desc),
            ]));
        }

        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  Tabs  ",
            Style::default()
                .fg(COLOR_ACCENT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));
        content.push(Line::from(""));

        let tabs = vec![
            ("Dashboard", "Overview of sessions and activity"),
            ("Sessions", "Manage active sessions"),
            ("Timeline", "Visual timeline of sessions"),
            ("History", "View past sessions"),
            ("Genome", "View decisions and preferences"),
            ("Search", "Search across sessions and files"),
            ("Analytics", "View session analytics and trends"),
            ("Chat", "Chat with session context"),
            ("Config", "This help and configuration"),
            ("Stewardship", "Context monitoring and cleanup"),
        ];

        for (name, desc) in tabs {
            content.push(Line::from(vec![
                Span::styled(name, Style::default().fg(COLOR_SUCCESS)),
                Span::raw("  - "),
                Span::raw(desc),
            ]));
        }

        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  Commands (in Chat)  ",
            Style::default()
                .fg(COLOR_ACCENT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));
        content.push(Line::from(""));

        let commands = vec![
            ("/track <path>", "Track a file to current session"),
            ("/tool <name>", "Track a tool to current session"),
            ("/session <name>", "Create a new session"),
            ("/search <query>", "Search across sessions"),
            ("/tag <name>", "Add tag to session"),
        ];

        for (cmd, desc) in commands {
            content.push(Line::from(vec![
                Span::styled(cmd, Style::default().fg(COLOR_WARNING)),
                Span::raw("  "),
                Span::raw(desc),
            ]));
        }

        let block = Block::default()
            .title(" Config ")
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .style(Style::default().bg(COLOR_PANEL));

        f.render_widget(Paragraph::new(content).block(block), area);
    }
}

fn render_stewardship(f: &mut Frame, area: Rect, state: &TuiState) {
    use crate::stewardship;

    let base = state.state.storage().base_path();
    let config = state.state.config_snapshot().unwrap_or_default();
    let stew_config = stewardship::StewardshipConfig::from_config(&config);

    // Load live data
    let proposals = stewardship::approval::list_pending(base).unwrap_or_default();
    let cross = stewardship::cross_project::load_cross_project(base).unwrap_or_default();

    let mut content = Vec::new();

    // Header
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "  STEWARDSHIP",
        Style::default()
            .fg(COLOR_TEXT_BRIGHT)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )));
    content.push(Line::from(""));

    // Mode and thresholds
    let mode_color = match stew_config.mode {
        stewardship::StewardshipMode::Auto => COLOR_SUCCESS,
        stewardship::StewardshipMode::Review => COLOR_WARNING,
        stewardship::StewardshipMode::Off => COLOR_ERROR,
    };
    content.push(Line::from(vec![
        Span::raw("  Mode: "),
        Span::styled(
            format!("{:?}", stew_config.mode),
            Style::default().fg(mode_color),
        ),
        Span::raw(format!(
            "  |  Thresholds: {:.0}% / {:.0}% / {:.0}% / {:.0}%",
            stew_config.monitor_threshold * 100.0,
            stew_config.surgical_threshold * 100.0,
            stew_config.thoughtful_threshold * 100.0,
            stew_config.emergency_threshold * 100.0,
        )),
    ]));
    content.push(Line::from(format!(
        "  Context window: {} tokens",
        stew_config.context_window_tokens
    )));

    // Separator
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "  ─── Pending Proposals ───",
        Style::default().fg(COLOR_ACCENT),
    )));

    if proposals.is_empty() {
        content.push(Line::from(Span::styled(
            "  No pending proposals",
            Style::default().fg(COLOR_TEXT),
        )));
    } else {
        for (i, p) in proposals.iter().enumerate() {
            let marker = if i == state.stewardship_selected_proposal {
                "▶ "
            } else {
                "  "
            };
            let thresh_color = match p.threshold {
                stewardship::ThresholdLevel::Surgical => COLOR_WARNING,
                stewardship::ThresholdLevel::Thoughtful => Color::Rgb(255, 165, 0),
                stewardship::ThresholdLevel::Emergency => COLOR_ERROR,
                _ => COLOR_TEXT,
            };
            content.push(Line::from(vec![
                Span::raw(format!("  {}", marker)),
                Span::styled(
                    format!("[{:?}]", p.threshold),
                    Style::default().fg(thresh_color),
                ),
                Span::raw(format!(
                    " {} — ~{} tokens freed",
                    p.strategy.as_str(),
                    p.estimated_tokens_freed
                )),
            ]));
            for region in &p.regions {
                content.push(Line::from(Span::styled(
                    format!(
                        "      {} ({} msgs, ~{} tok)",
                        region.description,
                        region.message_indices.len(),
                        region.estimated_tokens
                    ),
                    Style::default().fg(COLOR_TEXT),
                )));
            }
        }
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  [a]pprove  [r]eject  [↑↓] navigate",
            Style::default().fg(COLOR_TEXT),
        )));
    }

    // Cross-project insights
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "  ─── Cross-Project Insights ───",
        Style::default().fg(COLOR_ACCENT),
    )));

    if cross.patterns.is_empty() && cross.learnings.is_empty() {
        content.push(Line::from(Span::styled(
            "  No cross-project data yet. Analyze sessions to build patterns.",
            Style::default().fg(COLOR_TEXT),
        )));
    } else {
        for p in cross.patterns.iter().take(5) {
            content.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("[{}]", p.pattern_type),
                    Style::default().fg(COLOR_SUCCESS),
                ),
                Span::raw(format!(
                    " {} ({} occurrences)",
                    p.description, p.occurrences
                )),
            ]));
            content.push(Line::from(Span::styled(
                format!("      → {}", p.insight),
                Style::default().fg(COLOR_TEXT),
            )));
        }
        if !cross.learnings.is_empty() {
            content.push(Line::from(""));
            content.push(Line::from(Span::styled(
                format!("  Learnings ({}):", cross.learnings.len()),
                Style::default().fg(COLOR_TEXT_BRIGHT),
            )));
            for l in cross.learnings.iter().take(5) {
                content.push(Line::from(Span::styled(
                    format!("    • {}", l),
                    Style::default().fg(COLOR_TEXT),
                )));
            }
        }
    }

    let block = Block::default()
        .title(" Stewardship ")
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .style(Style::default().bg(COLOR_PANEL));

    f.render_widget(Paragraph::new(content).block(block), area);
}

fn render_search(
    f: &mut Frame,
    area: Rect,
    state: &mut TuiState,
    sessions: &[crate::state::Session],
    history: &[crate::state::HistoryEntry],
) {
    use crate::ui::visualization::{search_sessions, truncate};

    let mut content = Vec::new();
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "  Search Sessions  ",
        Style::default()
            .fg(COLOR_TEXT_BRIGHT)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )));
    content.push(Line::from(""));

    // Search input
    content.push(Line::from(vec![
        Span::styled("▸ ", Style::default().fg(COLOR_SUCCESS)),
        Span::raw("Search: "),
        Span::styled(
            if state.search_query.is_empty() {
                "[type to search...]".to_string()
            } else {
                state.search_query.clone()
            },
            Style::default().fg(if state.search_query.is_empty() {
                COLOR_TEXT
            } else {
                COLOR_TEXT_BRIGHT
            }),
        ),
    ]));
    content.push(Line::from(""));

    // Perform search if query exists
    if !state.search_query.is_empty() {
        if let Ok(config) = state.state.config_snapshot() {
            if let Ok(resp) = crate::retrieval::search_history(
                state.state.storage().base_path(),
                &config,
                &state.search_query,
                None,
                Some(crate::retrieval::types::SearchBackend::Auto),
                Some(10),
            ) {
                let explain = format!(
                    "Retrieval: backend={} fallback={} code={} time={}ms candidates={}",
                    resp.backend_used,
                    resp.used_fallback,
                    resp.fallback_code
                        .map(|c| format!("{:?}", c))
                        .unwrap_or_else(|| "none".to_string()),
                    resp.timing_ms,
                    resp.candidate_count
                );
                state.last_retrieval_explain = Some(explain.clone());
                content.push(Line::from(vec![
                    Span::styled("▸ ", Style::default().fg(COLOR_WARNING)),
                    Span::styled(explain, Style::default().fg(COLOR_WARNING)),
                ]));
                content.push(Line::from(""));
            }
        }

        let results = search_sessions(&state.search_query, sessions, history);
        state.search_results = results;

        if state.search_results.is_empty() {
            content.push(Line::from(vec![Span::styled(
                "  No results found  ",
                Style::default().fg(COLOR_TEXT),
            )]));
        } else {
            content.push(Line::from(Span::styled(
                "  Results  ",
                Style::default()
                    .fg(COLOR_ACCENT)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )));
            content.push(Line::from(""));

            for (i, result) in state.search_results.iter().take(10).enumerate() {
                let prefix = if i
                    == state
                        .selected_session
                        .as_ref()
                        .map(|s| {
                            state
                                .search_results
                                .iter()
                                .position(|r| r.session_id == *s)
                                .unwrap_or(0)
                        })
                        .unwrap_or(0)
                    && state.active_tab == 5
                {
                    "▸"
                } else {
                    " "
                };

                let match_type_str = match result.match_type {
                    crate::ui::visualization::MatchType::File => "[file]",
                    crate::ui::visualization::MatchType::Tool => "[tool]",
                    crate::ui::visualization::MatchType::Summary => "[summary]",
                    crate::ui::visualization::MatchType::Name => "[name]",
                };

                content.push(Line::from(vec![
                    Span::styled(format!("{} ", prefix), Style::default().fg(COLOR_SUCCESS)),
                    Span::styled(
                        truncate(&result.session_name, 20),
                        Style::default().fg(COLOR_TEXT_BRIGHT),
                    ),
                    Span::raw(" "),
                    Span::styled(match_type_str, Style::default().fg(COLOR_WARNING)),
                    Span::raw(" "),
                    Span::styled(
                        truncate(&result.snippet, 30),
                        Style::default().fg(COLOR_TEXT),
                    ),
                ]));
            }
        }
    } else {
        content.push(Line::from(vec![Span::styled(
            "  Type to search across sessions, files, and tools  ",
            Style::default().fg(COLOR_TEXT),
        )]));
        content.push(Line::from(""));
        content.push(Line::from(vec![
            Span::raw("  Press "),
            Span::styled("/", Style::default().fg(COLOR_ACCENT)),
            Span::raw(" to start searching"),
        ]));
    }

    content.push(Line::from(""));
    content.push(Line::from(vec![
        Span::styled("↑↓", Style::default().fg(COLOR_TEXT)),
        Span::raw(" Navigate "),
        Span::styled("Enter", Style::default().fg(COLOR_TEXT)),
        Span::raw(" Select "),
        Span::styled("Esc", Style::default().fg(COLOR_TEXT)),
        Span::raw(" Clear "),
    ]));

    let block = Block::default()
        .title(" Search ")
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .style(Style::default().bg(COLOR_PANEL));

    f.render_widget(Paragraph::new(content).block(block), area);
}

fn render_analytics(
    f: &mut Frame,
    area: Rect,
    sessions: &[crate::state::Session],
    history: &[crate::state::HistoryEntry],
) {
    #[allow(clippy::vec_init_then_push)]
    {
        use crate::ui::visualization::{calculate_analytics, format_duration, horizontal_bar};

        let analytics = calculate_analytics(history);

        let mut content = Vec::new();
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  Analytics  ",
            Style::default()
                .fg(COLOR_TEXT_BRIGHT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));
        content.push(Line::from(""));

        // Overview stats
        content.push(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(COLOR_ACCENT)),
            Span::raw("Total Sessions: "),
            Span::styled(
                analytics.total_sessions.to_string(),
                Style::default()
                    .fg(COLOR_TEXT_BRIGHT)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
        ]));
        content.push(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(COLOR_SUCCESS)),
            Span::raw("Active Now: "),
            Span::styled(
                sessions.len().to_string(),
                Style::default()
                    .fg(COLOR_SUCCESS)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
        ]));
        content.push(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(COLOR_WARNING)),
            Span::raw("Total Time: "),
            Span::styled(
                format_duration(analytics.total_duration_secs),
                Style::default().fg(COLOR_TEXT_BRIGHT),
            ),
        ]));
        content.push(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(COLOR_ACCENT)),
            Span::raw("Avg Duration: "),
            Span::styled(
                format_duration(analytics.avg_duration_secs),
                Style::default().fg(COLOR_TEXT_BRIGHT),
            ),
        ]));
        content.push(Line::from(""));

        // Platform breakdown
        if !analytics.platform_breakdown.is_empty() {
            content.push(Line::from(Span::styled(
                "  Platform Breakdown  ",
                Style::default()
                    .fg(COLOR_ACCENT)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )));
            content.push(Line::from(""));

            for (platform, count) in &analytics.platform_breakdown {
                let pct = if analytics.total_sessions > 0 {
                    (*count as f64 / analytics.total_sessions as f64) * 100.0
                } else {
                    0.0
                };
                content.push(Line::from(vec![
                    Span::styled(
                        format!("{} ", platform),
                        Style::default().fg(COLOR_TEXT_BRIGHT),
                    ),
                    Span::raw(horizontal_bar(
                        *count as f64,
                        analytics.total_sessions as f64,
                        15,
                    )),
                    Span::raw(" "),
                    Span::styled(
                        format!("{} ({:.0}%)", count, pct),
                        Style::default().fg(COLOR_TEXT),
                    ),
                ]));
            }
            content.push(Line::from(""));
        }

        // Recent activity sparkline
        if !analytics.daily_activity.is_empty() {
            content.push(Line::from(Span::styled(
                "  Activity Trend (Last 7 days)  ",
                Style::default()
                    .fg(COLOR_ACCENT)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )));
            content.push(Line::from(""));

            let recent: Vec<f64> = analytics
                .daily_activity
                .iter()
                .rev()
                .take(7)
                .map(|d| d.session_count as f64)
                .collect();

            if !recent.is_empty() {
                let sparkline = crate::ui::visualization::sparkline(&recent, 30);
                content.push(Line::from(vec![
                    Span::styled("  ", Style::default().fg(COLOR_TEXT)),
                    Span::raw(sparkline),
                ]));
            }
        }

        // Files and tools
        content.push(Line::from(""));
        content.push(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(COLOR_WARNING)),
            Span::raw("Files Touched: "),
            Span::styled(
                history
                    .iter()
                    .map(|e| e.files_touched.len())
                    .sum::<usize>()
                    .to_string(),
                Style::default().fg(COLOR_TEXT_BRIGHT),
            ),
        ]));
        content.push(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(COLOR_WARNING)),
            Span::raw("Tools Used: "),
            Span::styled(
                history
                    .iter()
                    .map(|e| e.tools_used.len())
                    .sum::<usize>()
                    .to_string(),
                Style::default().fg(COLOR_TEXT_BRIGHT),
            ),
        ]));

        let block = Block::default()
            .title(" Analytics ")
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .style(Style::default().bg(COLOR_PANEL));

        f.render_widget(Paragraph::new(content).block(block), area);
    }
}

fn render_footer(f: &mut Frame, area: Rect) {
    let footer = Line::from(vec![
        Span::styled("↑↓", Style::default().fg(COLOR_TEXT)),
        Span::raw(" Nav "),
        Span::styled("Tab", Style::default().fg(COLOR_TEXT)),
        Span::raw(" Switch "),
        Span::styled("n", Style::default().fg(COLOR_ACCENT)),
        Span::raw(" New "),
        Span::styled("e", Style::default().fg(COLOR_ACCENT)),
        Span::raw(" End "),
        Span::styled("t", Style::default().fg(COLOR_ACCENT)),
        Span::raw(" Track "),
        Span::styled("/", Style::default().fg(COLOR_ACCENT)),
        Span::raw(" Search "),
        Span::styled("i", Style::default().fg(COLOR_ACCENT)),
        Span::raw(" Chat "),
        Span::styled("m", Style::default().fg(COLOR_ACCENT)),
        Span::raw(" Menu "),
        Span::styled("q", Style::default().fg(COLOR_TEXT)),
        Span::raw(" Quit "),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border::DOUBLE)
        .style(Style::default().bg(COLOR_PANEL));

    f.render_widget(Paragraph::new(footer).block(block), area);
}
