//! Agent selection screen view.
//! Displayed on initial `agentdesk` launch to choose the coding agent to run in the workspace.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::discovery::AgentAvailability;
use crate::tui::state::TuiState;

pub fn render_agent_selection(f: &mut Frame, area: Rect, state: &TuiState) {
    // Fill entire screen with clean dark background
    let bg_block = Block::default().style(Style::default().bg(Color::Reset));
    f.render_widget(bg_block, area);

    // Center container (max 72 width, 24 height)
    let container_width = area.width.min(74);
    let container_height = area.height.min(26);
    let x = area.x + (area.width.saturating_sub(container_width)) / 2;
    let y = area.y + (area.height.saturating_sub(container_height)) / 2;
    let center_area = Rect::new(x, y, container_width, container_height);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("◈ AgentDesk", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
        ]))
        .title_alignment(Alignment::Center);

    f.render_widget(Clear, center_area);
    f.render_widget(block, center_area);

    let inner = Rect::new(
        center_area.x + 2,
        center_area.y + 1,
        center_area.width.saturating_sub(4),
        center_area.height.saturating_sub(2),
    );

    if state.detected_agents.is_empty() {
        render_empty_state(f, inner);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Subtitle
            Constraint::Min(8),    // Agent list
            Constraint::Length(3), // Workspace path & last used
            Constraint::Length(1), // Footer keys
        ])
        .split(inner);

    // 1. Header / Subtitle
    let subtitle = Paragraph::new(vec![
        Line::from(Span::styled(
            "Select a coding agent",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "AgentDesk detected the following coding agents on your machine:",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .alignment(Alignment::Center);
    f.render_widget(subtitle, chunks[0]);

    // 2. Agent List
    let mut list_lines = Vec::new();
    for (idx, agent) in state.detected_agents.iter().enumerate() {
        let is_selected = idx == state.selected_agent_index;
        let bullet = if is_selected { "● " } else { "○ " };
        let bullet_style = if is_selected {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let name_style = if is_selected {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };

        // Name line
        list_lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(bullet, bullet_style),
            Span::styled(&agent.display_name, name_style),
            Span::raw(" "),
            if let Some(ref ver) = agent.version {
                Span::styled(format!("({ver})"), Style::default().fg(Color::DarkGray))
            } else {
                Span::raw("")
            },
        ]));

        // Executable line
        list_lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(&agent.executable, Style::default().fg(Color::DarkGray)),
        ]));

        // Path line
        list_lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(
                agent.executable_path.to_string_lossy(),
                Style::default().fg(Color::DarkGray),
            ),
        ]));

        // Status line
        let (status_icon, status_label, status_style) = match &agent.availability {
            AgentAvailability::Ready => (
                "✓ ",
                "Ready",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            AgentAvailability::Unsupported { reason } => (
                "✗ ",
                reason.as_str(),
                Style::default().fg(Color::Yellow),
            ),
        };

        list_lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(status_icon, status_style),
            Span::styled(status_label, status_style),
        ]));

        // Spacing between agents
        list_lines.push(Line::raw(""));
    }

    let agent_list = Paragraph::new(list_lines);
    f.render_widget(agent_list, chunks[1]);

    // 3. Workspace / Directory Info
    let mut ws_lines = Vec::new();
    let ws_path = state.workspace_dir.to_string_lossy();
    ws_lines.push(Line::from(vec![
        Span::styled("Workspace: ", Style::default().fg(Color::DarkGray)),
        Span::styled(ws_path, Style::default().fg(Color::White)),
    ]));
    if let Some(ref last_id) = state.last_used_agent_id {
        ws_lines.push(Line::from(vec![
            Span::styled("Last used:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(last_id, Style::default().fg(Color::Cyan)),
        ]));
    }
    f.render_widget(Paragraph::new(ws_lines), chunks[2]);

    // 4. Footer navigation
    let footer_text = Line::from(vec![
        Span::styled("↑ / ↓", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Select   ", Style::default().fg(Color::White)),
        Span::styled("Enter", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Launch   ", Style::default().fg(Color::White)),
        Span::styled("p", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Change Dir   ", Style::default().fg(Color::White)),
        Span::styled("Esc", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Exit", Style::default().fg(Color::White)),
    ]);
    f.render_widget(
        Paragraph::new(footer_text).alignment(Alignment::Center),
        chunks[3],
    );
}

fn render_empty_state(f: &mut Frame, area: Rect) {
    let text = vec![
        Line::raw(""),
        Line::raw(""),
        Line::from(Span::styled(
            "No supported coding agents detected.",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            "Install a supported coding agent (such as agy) and restart AgentDesk.",
            Style::default().fg(Color::White),
        )),
        Line::raw(""),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Esc", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Exit", Style::default().fg(Color::White)),
        ]),
    ];

    let p = Paragraph::new(text).alignment(Alignment::Center);
    f.render_widget(p, area);
}
