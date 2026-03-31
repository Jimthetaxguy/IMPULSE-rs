use super::*;
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
