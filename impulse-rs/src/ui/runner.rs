use super::*;
use crate::state::Platform;
use crate::state::SessionStatus;
use crossterm::event::{self, Event, KeyCode};
use std::io;

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

pub(crate) fn run_app(
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
                        spawn_agent_in_terminal(&mut state, "codex", Platform::Codex);
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
                    KeyCode::Char('c') => {
                        // Toggle conflicts panel
                        state.conflicts_panel_open = !state.conflicts_panel_open;
                        if state.conflicts_panel_open {
                            state.status_message = Some(
                                "Conflicts panel (m=merge, t=theirs, y=mine, r=rebase)".to_string(),
                            );
                        } else {
                            state.status_message = Some("Dashboard".to_string());
                        }
                    }
                    #[allow(unreachable_patterns)]
                    KeyCode::Char('m') => {
                        // Merge conflict resolution
                        if state.conflicts_panel_open {
                            handle_conflict_resolution(
                                &mut state,
                                crate::agent::coordinator::ConflictResolution::Merge,
                            );
                        }
                    }
                    #[allow(unreachable_patterns)]
                    KeyCode::Char('t') => {
                        // Accept theirs conflict resolution
                        if state.conflicts_panel_open {
                            handle_conflict_resolution(
                                &mut state,
                                crate::agent::coordinator::ConflictResolution::AcceptTheirs,
                            );
                        }
                    }
                    KeyCode::Char('y') => {
                        // Accept mine conflict resolution
                        if state.conflicts_panel_open {
                            handle_conflict_resolution(
                                &mut state,
                                crate::agent::coordinator::ConflictResolution::AcceptMine,
                            );
                        }
                    }
                    #[allow(unreachable_patterns)]
                    KeyCode::Char('r') => {
                        // Rebase or Refresh (context-dependent)
                        if state.conflicts_panel_open {
                            handle_conflict_resolution(
                                &mut state,
                                crate::agent::coordinator::ConflictResolution::Rebase,
                            );
                        } else {
                            // Refresh
                            state.last_refresh = std::time::Instant::now();
                            state.status_message = Some("Refreshed".to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

pub(crate) fn handle_navigation(state: &mut TuiState, dir: i32) {
    // Handle conflict panel navigation if open
    if state.conflicts_panel_open {
        let conflict_count = state
            .mier_recommendations
            .iter()
            .filter(|r| {
                matches!(r.recommendation_type, RecommendationType::FileConflict)
                    && !r.description.contains("RESOLVED")
            })
            .count();

        if conflict_count > 0 {
            if dir < 0 {
                state.selected_conflict_index = state.selected_conflict_index.saturating_sub(1);
            } else {
                state.selected_conflict_index =
                    (state.selected_conflict_index + 1).min(conflict_count - 1);
            }
            state.status_message = Some(format!(
                "Selected conflict {}/{}",
                state.selected_conflict_index + 1,
                conflict_count
            ));
        }
        return;
    }

    // Default: session navigation
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

pub(crate) fn handle_selection(state: &mut TuiState) {
    if let Some(ref session_id) = state.selected_session {
        state.current_session_id = Some(session_id.clone());
        state.status_message = Some(format!("Selected session: {}", session_id));
    }
}

pub(crate) fn ui(f: &mut Frame, state: &mut TuiState) {
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
