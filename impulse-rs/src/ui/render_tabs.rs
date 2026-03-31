use super::*;
use crate::state::Platform;

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
