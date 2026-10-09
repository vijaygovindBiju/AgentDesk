//! Completed tasks view.
//! Displays all successfully finished work items with relative timestamps.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use crate::tui::state::TuiState;
use crate::tui::theme::Theme;
use crate::tui::views::dashboard::{format_time_ago, truncate_str};

pub fn render_completed(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" COMPLETED TASKS ")
        .title_style(Theme::primary())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(Theme::EMERALD));

    let completed = state.completed();
    if completed.is_empty() {
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  ○ ", Theme::text_muted()),
                Span::styled("No tasks completed yet in this session.", Theme::text_muted()),
            ]),
        ];
        f.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let header_cells = ["✓", "AGENT", "PROJECT", "COMPLETED WORK / SUMMARY", "FINISHED"]
        .into_iter()
        .map(|h| Span::styled(h, Theme::primary()));
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows: Vec<Row> = completed
        .iter()
        .enumerate()
        .map(|(idx, (_, ev))| {
            let row_style = if idx == state.completed_index {
                ratatui::style::Style::default().bg(Theme::SURFACE_ELEVATED).fg(Theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
            } else {
                ratatui::style::Style::default().fg(Theme::TEXT_PRIMARY)
            };

            Row::new(vec![
                Span::styled("  ✓ ", Theme::primary()),
                Span::styled(ev.agent_name.clone(), Theme::text_bold()),
                Span::styled(ev.project.clone(), Theme::text_muted()),
                Span::styled(truncate_str(&ev.summary, 45), Theme::text_normal()),
                Span::styled(format_time_ago(ev.ts), Theme::text_muted()),
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
