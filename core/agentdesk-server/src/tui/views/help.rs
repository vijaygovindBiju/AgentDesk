//! Help overlay modal.
//! Explains AgentDesk attention architecture and keyboard shortcuts.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::tui::state::TuiState;
use crate::tui::theme::Theme;

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
        .title_style(Theme::brand())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(Theme::CYAN));

    let key_style = Theme::shortcut_key();
    let desc_style = Theme::shortcut_desc();

    let lines = vec![
        Line::from(vec![
            Span::styled("AgentDesk ", Theme::brand()),
            Span::styled("is an attention and control surface for interactive coding agents.", Theme::text_secondary()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("CORE NAVIGATION", Theme::warning()),
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
            Span::styled("INTERACTION", Theme::warning()),
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
