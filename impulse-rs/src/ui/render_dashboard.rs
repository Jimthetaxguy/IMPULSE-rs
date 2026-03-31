use super::*;
use crate::state::SessionStatus;

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
