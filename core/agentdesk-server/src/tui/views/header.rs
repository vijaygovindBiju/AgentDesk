//! Header bar rendering.
//! Displays: AGENTDESK | Attention & Control Surface | ● X AGENTS | ● Relay Connected | ● PHONE CONNECTED

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::state::TuiState;
use crate::tui::theme::Theme;

/// Relay trunk indicator shared by all views: "● Relay Connected" / "○ Relay Disconnected".
pub fn relay_status_spans(state: &TuiState) -> Vec<Span<'static>> {
    if state.relay_connected {
        vec![
            Span::styled("● ", Theme::primary()),
            Span::styled("Relay Connected", Theme::primary()),
        ]
    } else {
        vec![
            Span::styled("○ ", Theme::error()),
            Span::styled("Relay Disconnected", Theme::error()),
        ]
    }
}

pub fn render_header(f: &mut Frame, area: Rect, state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(30), // Title & Subtitle
            Constraint::Min(20),    // Center spacing
            Constraint::Length(16), // Agent count badge
            Constraint::Length(22), // Relay trunk status badge
            Constraint::Length(28), // Phone connection status badge
        ])
        .split(area);

    // 1. Title & Brand
    let brand = Line::from(vec![
        Span::styled("◈ AGENTDESK", Theme::brand()),
        Span::raw(" "),
        Span::styled("Attention Surface", Theme::text_secondary()),
    ]);
    f.render_widget(Paragraph::new(brand), chunks[0]);

    // 2. Active Agents badge
    let agent_count = state.active_agents_count();
    let has_requests = !state.requests().is_empty();
    let (agent_icon, agent_style) = if has_requests {
        ("!", Theme::warning())
    } else if !state.working().is_empty() {
        ("●", Theme::accent().add_modifier(Modifier::BOLD))
    } else {
        ("✓", Theme::primary())
    };

    let agents_text = Line::from(vec![
        Span::styled(format!(" {agent_icon} "), agent_style),
        Span::styled(
            format!("{agent_count} AGENT{}", if agent_count == 1 { "" } else { "S" }),
            Theme::text_bold(),
        ),
    ]);
    f.render_widget(
        Paragraph::new(agents_text)
            .block(Block::default().borders(Borders::NONE)),
        chunks[2],
    );

    // 3. Relay trunk badge
    f.render_widget(Paragraph::new(Line::from(relay_status_spans(state))), chunks[3]);

    // 4. Phone Connection badge
    let phone_text = if state.phone_connected {
        let mode = state
            .phone_transport
            .as_deref()
            .unwrap_or("CONNECTED")
            .to_uppercase();
        Line::from(vec![
            Span::styled("● ", Theme::primary()),
            Span::styled("PHONE CONNECTED ", Theme::primary()),
            Span::styled(format!("· {mode}"), Theme::text_secondary()),
        ])
    } else {
        Line::from(vec![
            Span::styled("○ ", Theme::text_muted()),
            Span::styled("PHONE OFFLINE", Theme::text_muted()),
        ])
    };

    f.render_widget(Paragraph::new(phone_text), chunks[4]);
}
