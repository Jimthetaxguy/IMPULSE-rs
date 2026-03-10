use super::*;
use crate::state::Platform;
use crate::state::SessionStatus;

pub(crate) fn render_menu_bar(f: &mut Frame, area: Rect, state: &TuiState) {
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

pub(crate) fn render_header(f: &mut Frame, area: Rect, sessions: &[crate::state::Session]) {
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

pub(crate) fn render_project_tabs(f: &mut Frame, area: Rect, state: &TuiState) {
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

pub(crate) fn render_terminal_tabs(f: &mut Frame, area: Rect, state: &TuiState) {
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

pub(crate) fn render_tabs(f: &mut Frame, area: Rect, active_tab: usize) {
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

pub(crate) fn render_content(
    f: &mut Frame,
    area: Rect,
    state: &mut TuiState,
    sessions: &[crate::state::Session],
    history: &[crate::state::HistoryEntry],
) {
    // Render the base tab content
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

    // Render conflicts panel as overlay if open
    if state.conflicts_panel_open {
        // Use a centered panel with 60% width and 70% height
        let panel_width = (area.width as f32 * 0.6) as u16;
        let panel_height = (area.height as f32 * 0.7) as u16;
        let x = area.x + (area.width - panel_width) / 2;
        let y = area.y + (area.height - panel_height) / 2;

        let panel_area = Rect::new(x, y, panel_width, panel_height);
        render_conflicts_panel(f, panel_area, state);
    }
}

pub(crate) fn render_status_bar(
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

    // Show conflict count if any
    let active_conflicts: usize = state
        .mier_recommendations
        .iter()
        .filter(|r| {
            matches!(r.recommendation_type, RecommendationType::FileConflict)
                && !r.description.contains("RESOLVED")
        })
        .count();

    let conflict_info = if active_conflicts > 0 {
        Some(format!(
            "[{} CONFLICT{}]",
            active_conflicts,
            if active_conflicts > 1 { "S" } else { "" }
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
    } else {
        let mut parts = Vec::new();
        if let Some(ref term_info) = terminal_info {
            parts.push(term_info.clone());
        }
        if let Some(ref conflict) = conflict_info {
            parts.push(conflict.clone());
        }

        if parts.is_empty() {
            session_info
        } else {
            format!("{} | {}", session_info, parts.join(" | "))
        }
    };

    let block = Block::default().style(Style::default().bg(Color::Rgb(25, 30, 40)));

    let text = Line::from(vec![
        Span::styled("│ ", Style::default().fg(COLOR_TEXT)),
        Span::raw(status),
    ]);

    f.render_widget(Paragraph::new(text).block(block), area);
}

pub(crate) fn render_dashboard(
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

pub(crate) fn render_engine_indicator(
    f: &mut Frame,
    area: Rect,
    engine: crate::branding::EngineState,
) {
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

pub(crate) fn render_stats_panel(
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

pub(crate) fn render_activity_panel(f: &mut Frame, area: Rect, sessions: &[crate::state::Session]) {
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

pub(crate) fn render_sessions(
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

pub(crate) fn render_timeline(
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

pub(crate) fn render_history(f: &mut Frame, area: Rect, history: &[crate::state::HistoryEntry]) {
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

pub(crate) fn render_genome(f: &mut Frame, area: Rect, state: &TuiState) {
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

pub(crate) fn render_chat(
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

pub(crate) fn render_mier_panel(f: &mut Frame, area: Rect, state: &TuiState) {
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

    // Conflict notification banner (show for 30 seconds after detection)
    if let Some(notified_at) = state.last_conflict_notification {
        let elapsed = notified_at.elapsed().as_secs();
        if elapsed < 30 {
            let has_conflict = state.mier_recommendations.iter().any(|r| {
                matches!(
                    r.recommendation_type,
                    crate::agent::coordinator::RecommendationType::FileConflict
                )
            });
            if has_conflict {
                let banner_text = format!(" ⚠ CONFLICT DETECTED ({}s ago) ", elapsed);
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    banner_text,
                    Style::default()
                        .bg(COLOR_ERROR)
                        .fg(Color::White)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                )));
                lines.push(Line::from(""));
            }
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
                crate::agent::coordinator::RecommendationType::FileConflict => ("!", COLOR_ERROR),
                crate::agent::coordinator::RecommendationType::ErrorAssist => ("?", COLOR_WARNING),
                crate::agent::coordinator::RecommendationType::CrossPaneSync => ("~", COLOR_ACCENT),
                crate::agent::coordinator::RecommendationType::TaskComplete => ("v", COLOR_SUCCESS),
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

pub(crate) fn render_chat_inner(
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

pub(crate) fn render_config(f: &mut Frame, area: Rect, _state: &TuiState) {
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

pub(crate) fn render_stewardship(f: &mut Frame, area: Rect, state: &TuiState) {
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
            stew_config.mode.as_str().to_string(),
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

pub(crate) fn render_conflicts_panel(f: &mut Frame, area: Rect, state: &mut TuiState) {
    let mut content = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  CONFLICT RESOLUTION",
            Style::default()
                .fg(COLOR_ERROR)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Shortcuts: ", Style::default().fg(COLOR_ACCENT)),
            Span::raw("m=merge  "),
            Span::styled("t", Style::default().fg(COLOR_WARNING)),
            Span::raw("=theirs  "),
            Span::styled("y", Style::default().fg(COLOR_SUCCESS)),
            Span::raw("=mine  "),
            Span::styled("r", Style::default().fg(COLOR_ACCENT)),
            Span::raw("=rebase  "),
            Span::styled("c", Style::default().fg(COLOR_TEXT)),
            Span::raw("=close"),
        ]),
        Line::from(""),
    ];

    // Active conflicts
    let conflicts: Vec<_> = state
        .mier_recommendations
        .iter()
        .filter(|r| matches!(r.recommendation_type, RecommendationType::FileConflict))
        .filter(|r| !r.description.contains("RESOLVED"))
        .collect();

    content.push(Line::from(Span::styled(
        "  ─── Active Conflicts ───",
        Style::default().fg(COLOR_ERROR),
    )));

    if conflicts.is_empty() {
        content.push(Line::from(Span::styled(
            "  No active conflicts",
            Style::default().fg(COLOR_SUCCESS),
        )));
    } else {
        for (i, rec) in conflicts.iter().enumerate() {
            let is_selected = i == state.selected_conflict_index;
            let marker = if is_selected { "▶ " } else { "  " };

            // Extract file path from description
            let file_path = rec
                .description
                .strip_prefix("Multiple agents modifying: ")
                .unwrap_or(&rec.description)
                .to_string();

            // Color based on severity (critical = red)
            let color = COLOR_ERROR;

            content.push(Line::from(vec![
                Span::styled(marker, Style::default().fg(COLOR_WARNING)),
                Span::styled(
                    format!("[{}]", if is_selected { "SELECTED" } else { "      " }),
                    Style::default().fg(if is_selected {
                        COLOR_WARNING
                    } else {
                        COLOR_TEXT
                    }),
                ),
                Span::styled(
                    format!(" {}", file_path),
                    Style::default()
                        .fg(color)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
            ]));

            // Show panes involved
            if !rec.panes_involved.is_empty() {
                content.push(Line::from(vec![
                    Span::raw("      "),
                    Span::styled("Panes: ", Style::default().fg(COLOR_TEXT)),
                    Span::raw(rec.panes_involved.join(", ")),
                ]));
            }

            // Show action hint
            content.push(Line::from(vec![
                Span::raw("      "),
                Span::styled("Action: ", Style::default().fg(COLOR_TEXT)),
                Span::raw(&rec.action),
            ]));
        }

        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  Use ↑/↓ to select, then press resolution key",
            Style::default().fg(COLOR_TEXT),
        )));
    }

    // Resolved conflicts (recent)
    let resolved: Vec<_> = state
        .mier_recommendations
        .iter()
        .filter(|r| matches!(r.recommendation_type, RecommendationType::FileConflict))
        .filter(|r| r.description.contains("RESOLVED"))
        .take(3)
        .collect();

    if !resolved.is_empty() {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  ─── Recently Resolved ───",
            Style::default().fg(COLOR_SUCCESS),
        )));

        for rec in resolved {
            let file_path = rec
                .description
                .strip_prefix("Multiple agents modifying: ")
                .unwrap_or(&rec.description)
                .replace(" (RESOLVED)", "");

            content.push(Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(COLOR_SUCCESS)),
                Span::raw(file_path),
            ]));
        }
    }

    let block = Block::default()
        .title(" Conflicts ")
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .style(Style::default().bg(COLOR_PANEL));

    f.render_widget(Paragraph::new(content).block(block), area);
}

pub(crate) fn render_search(
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
                None,
            ) {
                let explain = format!(
                    "Retrieval: backend={} fallback={} code={} time={}ms candidates={}",
                    resp.backend_used,
                    resp.used_fallback,
                    resp.fallback_code
                        .map(|c| c.as_str().to_string())
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

pub(crate) fn render_analytics(
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

pub(crate) fn render_footer(f: &mut Frame, area: Rect) {
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
