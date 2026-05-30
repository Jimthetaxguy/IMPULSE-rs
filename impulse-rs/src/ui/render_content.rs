use super::*;
use crate::state::Platform;
use crate::state::SessionStatus;

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
                Some(Platform::Codex) => "Codex",
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
            Some(Platform::Codex) => "Codex",
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
