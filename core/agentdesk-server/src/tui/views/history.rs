//! History view.
//! Displays chronological meaningful attention events surfaced by AgentDesk.
//! Never pollutes with internal agent log lines.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use crate::tui::state::TuiState;
use crate::tui::views::dashboard::truncate_str;

pub fn render_history(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" ATTENTION EVENT HISTORY ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));

    if state.history.is_empty() {
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  ○ ", Style::default().fg(Color::DarkGray)),
                Span::styled("No attention events recorded yet in this session.", Style::default().fg(Color::White)),
            ]),
        ];
        f.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let header_cells = ["TIME", "!", "AGENT", "PROJECT", "CATEGORY", "SUMMARY / EVENT"]
        .into_iter()
        .map(|h| Span::styled(h, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows: Vec<Row> = state
        .history
        .iter()
        .enumerate()
        .map(|(idx, ev)| {
            let (icon, color, cat_str) = match ev.category {
                agentdesk_model::Category::Request => ("!", Color::Yellow, "Request"),
                agentdesk_model::Category::Error => ("!", Color::Red, "Error"),
                agentdesk_model::Category::Working => ("●", Color::Cyan, "Working"),
                agentdesk_model::Category::Completed => ("✓", Color::Green, "Completed"),
            };

            let row_style = if idx == state.history_index {
                Style::default().bg(Color::Rgb(25, 35, 50)).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            Row::new(vec![
                Span::styled(ev.ts.format("%H:%M:%S").to_string(), Style::default().fg(Color::DarkGray)),
                Span::styled(format!(" {icon} "), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::styled(ev.agent_name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(ev.project.clone(), Style::default().fg(Color::DarkGray)),
                Span::styled(cat_str, Style::default().fg(color)),
                Span::styled(truncate_str(&ev.summary, 45), Style::default()),
            ])
            .style(row_style)
            .height(1)
        })
        .collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Length(5),
        Constraint::Length(20),
        Constraint::Length(18),
        Constraint::Length(14),
        Constraint::Min(35),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(block);

    f.render_widget(table, area);
}
