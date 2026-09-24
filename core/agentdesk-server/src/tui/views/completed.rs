//! Completed tasks view.
//! Displays all successfully finished work items with relative timestamps.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use crate::tui::state::TuiState;
use crate::tui::views::dashboard::{format_time_ago, truncate_str};

pub fn render_completed(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" COMPLETED TASKS ")
        .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Green));

    let completed = state.completed();
    if completed.is_empty() {
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  ○ ", Style::default().fg(Color::DarkGray)),
                Span::styled("No tasks completed yet in this session.", Style::default().fg(Color::White)),
            ]),
        ];
        f.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let header_cells = ["✓", "AGENT", "PROJECT", "COMPLETED WORK / SUMMARY", "FINISHED"]
        .into_iter()
        .map(|h| Span::styled(h, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows: Vec<Row> = completed
        .iter()
        .enumerate()
        .map(|(idx, (_, ev))| {
            let row_style = if idx == state.completed_index {
                Style::default().bg(Color::Rgb(20, 45, 30)).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            Row::new(vec![
                Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                Span::styled(ev.agent_name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(ev.project.clone(), Style::default().fg(Color::DarkGray)),
                Span::styled(truncate_str(&ev.summary, 45), Style::default()),
                Span::styled(format_time_ago(ev.ts), Style::default().fg(Color::DarkGray)),
            ])
            .style(row_style)
            .height(1)
        })
        .collect();

    let widths = [
        Constraint::Length(6),
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
