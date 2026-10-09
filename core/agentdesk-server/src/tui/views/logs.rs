//! Logs view.
//! Secondary paged, bounded log inspection.
//! The dashboard never acts as a raw terminal log dump; logs are progressive disclosure.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::state::TuiState;
use crate::tui::theme::Theme;

pub fn render_logs(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" BOUNDED LOG VIEWER (PROGRESSIVE DISCLOSURE) ")
        .title_style(Theme::brand())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(Theme::BORDER));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if state.logs_lines.is_empty() {
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  ○ ", Theme::text_muted()),
                Span::styled("No log window currently loaded.", Theme::text_bold()),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("  In AgentDesk, raw logs stay boundedly stored on the laptop.", Theme::text_secondary()),
            ]),
            Line::from(vec![
                Span::styled("  To inspect logs, select an Error from [e] Errors and choose Level 3,", Theme::text_secondary()),
            ]),
            Line::from(vec![
                Span::styled("  or inspect an agent task from [a] Agents.", Theme::text_secondary()),
            ]),
        ];
        f.render_widget(Paragraph::new(text), inner);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),  // Header stats
            Constraint::Min(10),    // Log lines
            Constraint::Length(1),  // Scroll hint
        ])
        .split(inner);

    let stats = Line::from(vec![
        Span::styled(format!("Retrieved {} lines   ", state.logs_lines.len()), Theme::brand()),
        Span::styled(format!("Offset: {} ", state.logs_scroll), Theme::text_muted()),
    ]);
    f.render_widget(Paragraph::new(stats), chunks[0]);

    let mut lines = Vec::new();
    let skip = state.logs_scroll;
    for l in state.logs_lines.iter().skip(skip).take(chunks[1].height as usize) {
        lines.push(Line::from(vec![
            Span::styled(format!("{:>6} ", l.offset), Style::default().fg(Color::DarkGray)),
            Span::styled(l.ts.format("%H:%M:%S ").to_string(), Style::default().fg(Color::DarkGray)),
            Span::styled(&l.text, Style::default().fg(Color::White)),
        ]));
    }
    f.render_widget(Paragraph::new(lines), chunks[1]);

    let hint = Line::from(vec![
        Span::styled("j/k = scroll    PgUp/PgDn = page scroll    Esc = back", Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(hint), chunks[2]);
}
