//! Help overlay modal.
//! Explains AgentDesk attention architecture and keyboard shortcuts.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::tui::state::TuiState;

pub fn render_help(f: &mut Frame, area: Rect, _state: &TuiState) {
    let popup_width = 72.min(area.width.saturating_sub(4));
    let popup_height = 24.min(area.height.saturating_sub(2));

    let popup_area = Rect {
        x: (area.width.saturating_sub(popup_width)) / 2,
        y: (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" AGENTDESK — HELP & SHORTCUTS ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Cyan));

    let key_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(Color::White);

    let lines = vec![
        Line::from(vec![
            Span::styled("AgentDesk ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("is an attention and control surface for interactive coding agents.", Style::default().fg(Color::DarkGray)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("CORE NAVIGATION", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  [a]       ", key_style),
            Span::styled("Agents list and individual agent detail timeline", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  [r]       ", key_style),
            Span::styled("Requests view — answer approvals, questions & choices", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  [e]       ", key_style),
            Span::styled("Errors view — progressive disclosure (summary, details, logs)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  [w] / [t] ", key_style),
            Span::styled("Working tasks view (quiet, aggregated)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  [p]       ", key_style),
            Span::styled("Projects view — multi-agent project breakdown", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  [d]       ", key_style),
            Span::styled("Devices view — remote paired mobile supervision", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  [h]       ", key_style),
            Span::styled("History — chronological attention events", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  [P]       ", key_style),
            Span::styled("Pair new Android phone via instant terminal QR code", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  [c]       ", key_style),
            Span::styled("Connection details — relay, transport & TLS identity", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  [q]       ", key_style),
            Span::styled("Quit AgentDesk TUI", desc_style),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("INTERACTION", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  ↑ / ↓     Navigate lists, radio options, checkboxes", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Space     Toggle checkboxes in multiple-choice requests", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Enter     Submit active response / open details", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Esc       Return to previous screen / close popup", desc_style),
        ]),
    ];

    f.render_widget(Paragraph::new(lines).block(block), popup_area);
}
