//! Agent selection screen view.
//! Displayed on initial `agentdesk` launch to choose the coding agent to run in the workspace.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::discovery::AgentAvailability;
use crate::tui::state::TuiState;
use crate::tui::theme::Theme;

pub fn render_agent_selection(f: &mut Frame, area: Rect, state: &TuiState) {
    // Fill entire screen with clean dark background
    let bg_block = Block::default().style(ratatui::style::Style::default().bg(ratatui::style::Color::Reset));
    f.render_widget(bg_block, area);

    // Center container (max 72 width, 24 height)
    let container_width = area.width.min(74);
    let container_height = area.height.min(26);
    let x = area.x + (area.width.saturating_sub(container_width)) / 2;
    let y = area.y + (area.height.saturating_sub(container_height)) / 2;
    let center_area = Rect::new(x, y, container_width, container_height);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(Theme::CYAN))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("◈ AgentDesk", Theme::brand()),
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
            Constraint::Min(6),    // Agent list
            Constraint::Length(4), // Workspace path & execution mode & last used
            Constraint::Length(1), // Footer keys
        ])
        .split(inner);

    // 1. Header / Subtitle
    let subtitle = Paragraph::new(vec![
        Line::from(Span::styled(
            "Select a coding agent",
            Theme::text_bold(),
        )),
        Line::from(Span::styled(
            "AgentDesk detected the following coding agents on your machine:",
            Theme::text_secondary(),
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
            Theme::primary()
        } else {
            Theme::text_muted()
        };

        let name_style = if is_selected {
            Theme::text_bold()
        } else {
            Theme::text_secondary()
        };

        // Name line
        list_lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(bullet, bullet_style),
            Span::styled(&agent.display_name, name_style),
            Span::raw(" "),
            if let Some(ref ver) = agent.version {
                Span::styled(format!("({ver})"), Theme::text_muted())
            } else {
                Span::raw("")
            },
        ]));

        // Executable line
        list_lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(&agent.executable, Theme::text_muted()),
        ]));

        // Path line
        list_lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(
                agent.executable_path.to_string_lossy(),
                Theme::text_muted(),
            ),
        ]));

        // Status line
        let (status_icon, status_label, status_style) = match &agent.availability {
            AgentAvailability::Ready => (
                "✓ ",
                "Ready",
                Theme::primary(),
            ),
            AgentAvailability::Unsupported { reason } => (
                "✗ ",
                reason.as_str(),
                Theme::warning(),
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

    // 3. Workspace / Directory Info & Execution Mode
    let mut ws_lines = Vec::new();
    let ws_path = state.workspace_dir.to_string_lossy();
    ws_lines.push(Line::from(vec![
        Span::styled("Workspace:  ", Theme::text_muted()),
        Span::styled(ws_path, Theme::text_bold()),
        Span::styled("  [d/p: Change Dir]", Theme::accent()),
    ]));

    let (mode_icon, mode_label, mode_color) = if state.dangerously_skip_permissions {
        ("⚡", "--dangerously-skip-permissions", Theme::AMBER)
    } else {
        ("🛡", "Default (Prompt on action)", Theme::EMERALD)
    };
    ws_lines.push(Line::from(vec![
        Span::styled("Mode:       ", Theme::text_muted()),
        Span::styled(format!("{mode_icon} {mode_label}"), ratatui::style::Style::default().fg(mode_color).add_modifier(ratatui::style::Modifier::BOLD)),
        Span::styled("  [m/Tab: Toggle]", Theme::primary()),
    ]));

    if let Some(ref last_id) = state.last_used_agent_id {
        ws_lines.push(Line::from(vec![
            Span::styled("Last used:  ", Theme::text_muted()),
            Span::styled(last_id, Theme::accent()),
        ]));
    }
    f.render_widget(Paragraph::new(ws_lines), chunks[2]);

    // 4. Footer navigation
    let footer_text = Line::from(vec![
        Span::styled("↑ / ↓", Theme::shortcut_key()),
        Span::styled(" Select   ", Theme::shortcut_desc()),
        Span::styled("d/p", Theme::shortcut_key()),
        Span::styled(" Change Dir   ", Theme::shortcut_desc()),
        Span::styled("m/Tab", Theme::shortcut_key()),
        Span::styled(" Mode   ", Theme::shortcut_desc()),
        Span::styled("Enter", Theme::shortcut_key()),
        Span::styled(" Launch   ", Theme::brand()),
        Span::styled("Esc", Theme::shortcut_key()),
        Span::styled(" Exit", Theme::shortcut_desc()),
    ]);
    f.render_widget(
        Paragraph::new(footer_text).alignment(Alignment::Center),
        chunks[3],
    );

    // If ProjectSelect overlay is open, render it over the agent selection screen
    if state.workspace_overlay == crate::tui::state::WorkspaceOverlay::ProjectSelect {
        crate::tui::views::workspace::render_project_select_overlay(f, area, state);
    }
}

fn render_empty_state(f: &mut Frame, area: Rect) {
    let text = vec![
        Line::raw(""),
        Line::raw(""),
        Line::from(Span::styled(
            "No supported coding agents detected.",
            Theme::error(),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            "Install a supported coding agent (such as agy) and restart AgentDesk.",
            Theme::text_normal(),
        )),
        Line::raw(""),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Esc", Theme::shortcut_key()),
            Span::styled(" Exit", Theme::shortcut_desc()),
        ]),
    ];

    let p = Paragraph::new(text).alignment(Alignment::Center);
    f.render_widget(p, area);
}
