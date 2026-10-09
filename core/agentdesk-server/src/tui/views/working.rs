//! Quiet Working view.
//! Displays all currently active coding agents, what task they are running, and duration.
//! Never displays rapid internal agent tokens or streaming noise.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use crate::tui::state::TuiState;
use crate::tui::theme::Theme;
use crate::tui::views::dashboard::{format_duration, truncate_str};

pub fn render_working(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" WORKING / ACTIVE AGENTS ")
        .title_style(Theme::brand())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(Theme::CYAN));

    let working = state.working();
    if working.is_empty() {
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  ○ ", Theme::text_muted()),
                Span::styled("No agents currently executing work (all idle).", Theme::text_muted()),
            ]),
        ];
        f.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let header_cells = ["STATUS", "AGENT", "PROJECT", "ACTIVITY / TASK", "DURATION"]
        .into_iter()
        .map(|h| Span::styled(h, Theme::brand()));
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows: Vec<Row> = working
        .iter()
        .enumerate()
        .map(|(idx, (_, ev))| {
            let row_style = if idx == state.working_index {
                ratatui::style::Style::default().bg(Theme::SURFACE_ELEVATED).fg(Theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
            } else {
                ratatui::style::Style::default().fg(Theme::TEXT_PRIMARY)
            };

            Row::new(vec![
                Span::styled("  ● ", Theme::accent().add_modifier(Modifier::BOLD)),
                Span::styled(ev.agent_name.clone(), Theme::text_bold()),
                Span::styled(ev.project.clone(), Theme::accent()),
                Span::styled(truncate_str(&ev.summary, 45), Theme::text_secondary()),
                Span::styled(format_duration(ev.ts), Theme::text_muted()),
            ])
            .style(row_style)
            .height(1)
        })
        .collect();

    let widths = [
        Constraint::Length(8),
        Constraint::Length(22),
        Constraint::Length(20),
        Constraint::Min(35),
        Constraint::Length(12),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(block);

    f.render_widget(table, area);
}
