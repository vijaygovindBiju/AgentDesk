//! Quiet Working view.
//! Displays all currently active coding agents, what task they are running, and duration.
//! Never displays rapid internal agent tokens or streaming noise.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use crate::tui::state::TuiState;
use crate::tui::views::dashboard::{format_duration, truncate_str};

pub fn render_working(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" WORKING / ACTIVE AGENTS ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let working = state.working();
    if working.is_empty() {
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  ○ ", Style::default().fg(Color::DarkGray)),
                Span::styled("No agents currently executing work (all idle).", Style::default().fg(Color::White)),
            ]),
        ];
        f.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let header_cells = ["STATUS", "AGENT", "PROJECT", "ACTIVITY / TASK", "DURATION"]
        .into_iter()
        .map(|h| Span::styled(h, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows: Vec<Row> = working
        .iter()
        .enumerate()
        .map(|(idx, (_, ev))| {
            let row_style = if idx == state.working_index {
                Style::default().bg(Color::Rgb(20, 35, 50)).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            Row::new(vec![
                Span::styled("  ● ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(ev.agent_name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(ev.project.clone(), Style::default().fg(Color::Cyan)),
                Span::styled(truncate_str(&ev.summary, 45), Style::default()),
                Span::styled(format_duration(ev.ts), Style::default().fg(Color::DarkGray)),
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
