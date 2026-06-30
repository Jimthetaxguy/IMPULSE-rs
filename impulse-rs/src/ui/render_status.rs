use super::*;
use crate::context_lifecycle::AgentKind;

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
                AgentKind::Gemini => "gemini",
                AgentKind::Cursor => "cursor",
                AgentKind::GenericShell => "shell",
            };
            // Simple gauge bar: 10-char wide, each segment = 10% of context used.
            let bar_len = (pct as usize).min(100) / 10;
            let bar: String = "|".repeat(bar_len);
            let empty: String = ".".repeat(10usize.saturating_sub(bar_len));
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
                crate::agent::coordinator::RecommendationType::DelegationReady => {
                    ("D", COLOR_ACCENT)
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
                MierFeedKind::PaneSummary => Color::Rgb(100, 149, 237), // cornflower blue
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
