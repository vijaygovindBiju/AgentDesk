//! Header bar rendering.
//! Displays: AGENTDESK | Attention & Control Surface | ● X AGENTS | ● Relay Connected | ● PHONE CONNECTED

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::state::TuiState;

/// Relay trunk indicator shared by all views: "● Relay Connected" / "○ Relay Disconnected".
pub fn relay_status_spans(state: &TuiState) -> Vec<Span<'static>> {
    if state.relay_connected {
        vec![
            Span::styled("● ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("Relay Connected", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]
    } else {
        vec![
            Span::styled("○ ", Style::default().fg(Color::Red)),
            Span::styled("Relay Disconnected", Style::default().fg(Color::Red)),
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
        Span::styled(
            " AGENTDESK",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            "Attention Surface",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(brand), chunks[0]);

    // 2. Active Agents badge
    let agent_count = state.active_agents_count();
    let has_requests = !state.requests().is_empty();
    let (agent_icon, agent_style) = if has_requests {
        ("!", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else if !state.working().is_empty() {
        ("●", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    } else {
        ("✓", Style::default().fg(Color::Green))
    };

    let agents_text = Line::from(vec![
        Span::styled(format!(" {agent_icon} "), agent_style),
        Span::styled(
            format!("{agent_count} AGENT{}", if agent_count == 1 { "" } else { "S" }),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
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
            Span::styled("● ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("PHONE CONNECTED ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(format!("· {mode}"), Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(vec![
            Span::styled("○ ", Style::default().fg(Color::DarkGray)),
            Span::styled("PHONE OFFLINE", Style::default().fg(Color::DarkGray)),
        ])
    };

    f.render_widget(Paragraph::new(phone_text), chunks[4]);
}
