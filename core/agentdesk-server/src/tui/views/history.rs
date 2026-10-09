//! History view.
//! Displays chronological meaningful attention events surfaced by AgentDesk.
//! Never pollutes with internal agent log lines.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use crate::tui::state::TuiState;
use crate::tui::theme::Theme;
use crate::tui::views::dashboard::truncate_str;

pub fn render_history(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" ATTENTION EVENT HISTORY ")
        .title_style(Theme::brand())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(Theme::BORDER));

    if state.history.is_empty() {
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  ○ ", Theme::text_muted()),
                Span::styled("No attention events recorded yet in this session.", Theme::text_muted()),
            ]),
        ];
        f.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let header_cells = ["TIME", "!", "AGENT", "PROJECT", "CATEGORY", "SUMMARY / EVENT"]
        .into_iter()
        .map(|h| Span::styled(h, Theme::brand()));
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows: Vec<Row> = state
        .history
        .iter()
        .enumerate()
        .map(|(idx, ev)| {
            let (icon, color, cat_str) = match ev.category {
                agentdesk_model::Category::Request => ("!", Theme::AMBER, "Request"),
                agentdesk_model::Category::Error => ("!", Theme::RED, "Error"),
                agentdesk_model::Category::Working => ("●", Theme::CYAN, "Working"),
                agentdesk_model::Category::Completed => ("✓", Theme::EMERALD, "Completed"),
            };

            let row_style = if idx == state.history_index {
                ratatui::style::Style::default().bg(Theme::SURFACE_ELEVATED).fg(Theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
            } else {
                ratatui::style::Style::default().fg(Theme::TEXT_PRIMARY)
            };

            Row::new(vec![
                Span::styled(ev.ts.format("%H:%M:%S").to_string(), Theme::text_muted()),
                Span::styled(format!(" {icon} "), ratatui::style::Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::styled(ev.agent_name.clone(), Theme::text_bold()),
                Span::styled(ev.project.clone(), Theme::text_muted()),
                Span::styled(cat_str, ratatui::style::Style::default().fg(color)),
                Span::styled(truncate_str(&ev.summary, 45), Theme::text_normal()),
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
