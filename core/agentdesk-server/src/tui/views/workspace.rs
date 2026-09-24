//! Workspace terminal screen view.
//! Dominant interactive terminal surface with minimal top bar, phone status, attention badge, and overlays.

use std::cmp::min;

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use agentdesk_core::Screen;
use crate::tui::state::{ActiveTerminalTab, TuiState, WorkspaceOverlay};

pub fn render_workspace(f: &mut Frame, area: Rect, state: &TuiState) {
    if area.width < 20 || area.height < 6 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top Bar
            Constraint::Min(4),    // Real Terminal Body
            Constraint::Length(1), // Bottom Status / Shortcut Bar
        ])
        .split(area);

    // 1. Top Bar
    render_top_bar(f, chunks[0], state);

    // 2. Real Terminal
    let term_area = chunks[1];
    if let Some(ref session) = state.shell_session {
        render_terminal_grid(f, term_area, &session.screen);
    } else {
        render_empty_terminal(f, term_area);
    }

    // 3. Bottom Status / Hints Bar
    render_bottom_bar(f, chunks[2], state);

    // 4. Overlays (if active)
    match state.workspace_overlay {
        WorkspaceOverlay::None => {}
        WorkspaceOverlay::AgentSwitch => render_agent_switch_overlay(f, area, state),
        WorkspaceOverlay::ProjectSelect => render_project_select_overlay(f, area, state),
        WorkspaceOverlay::Attention => render_attention_overlay(f, area, state),
        WorkspaceOverlay::SwitchConfirm => render_switch_confirm_overlay(f, area, state),
        WorkspaceOverlay::Menu => render_menu_overlay(f, area, state),
        WorkspaceOverlay::Pairing => crate::tui::views::pairing::render_pairing(f, area, state),
    }
}

fn render_top_bar(f: &mut Frame, area: Rect, state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(14), // ◈ AgentDesk
            Constraint::Length(22), // Agent Dropdown
            Constraint::Min(16),    // Workspace Directory
            Constraint::Length(18), // Phone Connection
            Constraint::Length(8),  // Notifications
            Constraint::Length(10), // Tab toggle
        ])
        .split(area);

    // 1. Brand
    let brand = Line::from(vec![
        Span::styled("◈ AgentDesk", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(" │", Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(brand), chunks[0]);

    // 2. Agent dropdown
    let agent_name = state
        .active_agent
        .as_ref()
        .map(|a| a.display_name.as_str())
        .unwrap_or("No Agent");

    let agent_running = state.agent_session_running;
    let (icon, color) = if agent_running {
        ("●", Color::Green)
    } else {
        ("○", Color::DarkGray)
    };

    let agent_badge = Line::from(vec![
        Span::styled(format!(" {icon} "), Style::default().fg(color)),
        Span::styled(agent_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(" ▾", Style::default().fg(Color::Yellow)),
        Span::styled(" │", Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(agent_badge), chunks[1]);

    // 3. Current Workspace Directory
    let ws_path = state.workspace_dir.to_string_lossy();
    let display_path = if let Some(home) = std::env::var_os("HOME") {
        let home_str = home.to_string_lossy();
        if ws_path.starts_with(home_str.as_ref()) {
            format!("~{}", &ws_path[home_str.len()..])
        } else {
            ws_path.to_string()
        }
    } else {
        ws_path.to_string()
    };

    let dir_badge = Line::from(vec![
        Span::styled(format!(" {display_path} ▾"), Style::default().fg(Color::Cyan)),
        Span::styled(" │", Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(dir_badge), chunks[2]);

    // 4. Phone connection
    let phone_text = if state.phone_connected {
        Line::from(vec![
            Span::styled("● ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("Connected", Style::default().fg(Color::Green)),
            Span::styled(" │", Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(vec![
            Span::styled("○ ", Style::default().fg(Color::DarkGray)),
            Span::styled("Offline", Style::default().fg(Color::DarkGray)),
            Span::styled(" │", Style::default().fg(Color::DarkGray)),
        ])
    };
    f.render_widget(Paragraph::new(phone_text), chunks[3]);

    // 5. Attention notifications badge
    let pending_requests = state.requests().len();
    let errors_count = state.errors().len();
    let total_attention = pending_requests + errors_count;

    let notif_badge = if total_attention > 0 {
        Line::from(vec![
            Span::styled("🔔 ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{total_attention}"),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("🔔 ", Style::default().fg(Color::DarkGray)),
            Span::styled("0", Style::default().fg(Color::DarkGray)),
        ])
    };
    f.render_widget(Paragraph::new(notif_badge), chunks[4]);

    // 6. Tab indicator
    let tab_text = match state.active_terminal_tab {
        ActiveTerminalTab::Shell => Line::from(Span::styled("[Shell]", Style::default().fg(Color::Green))),
        ActiveTerminalTab::Agent => Line::from(Span::styled("[Agent]", Style::default().fg(Color::Cyan))),
    };
    f.render_widget(Paragraph::new(tab_text), chunks[5]);
}

fn render_bottom_bar(f: &mut Frame, area: Rect, _state: &TuiState) {
    let shortcuts = Line::from(vec![
        Span::styled(" F1", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Agent   ", Style::default().fg(Color::DarkGray)),
        Span::styled("F2", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Project   ", Style::default().fg(Color::DarkGray)),
        Span::styled("F3", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Attention   ", Style::default().fg(Color::DarkGray)),
        Span::styled("F4", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Menu & Pair (Alt+M)   ", Style::default().fg(Color::Cyan)),
        Span::styled("F6", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Toggle   ", Style::default().fg(Color::DarkGray)),
        Span::styled("F10", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Exit", Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(shortcuts), area);
}

/// Render the VT100 screen grid directly to the terminal area.
pub fn render_terminal_grid(f: &mut Frame, area: Rect, screen: &Screen) {
    let rows_to_draw = min(area.height, screen.rows);
    let cols_to_draw = min(area.width, screen.cols);

    let buf = f.buffer_mut();

    for r in 0..rows_to_draw {
        for c in 0..cols_to_draw {
            if let Some(cell) = screen.cell(r, c) {
                let target = &mut buf[(area.x + c, area.y + r)];
                target.set_char(cell.ch);

                let mut style = Style::default();
                if cell.attrs.bold {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if cell.attrs.dim {
                    style = style.add_modifier(Modifier::DIM);
                }
                if cell.attrs.italic {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if cell.attrs.underline {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                if cell.attrs.reverse {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                if let Some(fg) = cell.attrs.fg {
                    style = style.fg(ansi_code_to_color(fg));
                }
                if let Some(bg) = cell.attrs.bg {
                    style = style.bg(ansi_code_to_color(bg));
                }
                target.set_style(style);
            }
        }
    }

    // Set physical cursor position
    if screen.cursor_visible && screen.cursor_col < area.width && screen.cursor_row < area.height {
        f.set_cursor_position(Position::new(
            area.x + screen.cursor_col,
            area.y + screen.cursor_row,
        ));
    }
}

fn render_empty_terminal(f: &mut Frame, area: Rect) {
    let p = Paragraph::new(vec![
        Line::raw(""),
        Line::from(Span::styled(
            "Terminal initializing...",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .alignment(Alignment::Center);
    f.render_widget(p, area);
}

fn ansi_code_to_color(code: u8) -> Color {
    match code {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::Gray,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        15 => Color::White,
        other => Color::Indexed(other),
    }
}

// ==========================================
// OVERLAYS
// ==========================================

fn render_agent_switch_overlay(f: &mut Frame, area: Rect, state: &TuiState) {
    let width = 60.min(area.width);
    let height = (state.detected_agents.len() as u16 + 6).min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Switch Active Coding Agent ")
        .title_alignment(Alignment::Center);
    f.render_widget(block, modal_area);

    let inner = Rect::new(
        modal_area.x + 2,
        modal_area.y + 1,
        modal_area.width.saturating_sub(4),
        modal_area.height.saturating_sub(2),
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(inner);

    let mut lines = Vec::new();
    for (idx, agent) in state.detected_agents.iter().enumerate() {
        let is_selected = idx == state.agent_switch_index;
        let bullet = if is_selected { "● " } else { "○ " };
        let bullet_style = if is_selected {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let is_active = state.active_agent.as_ref().map(|a| a.id == agent.id).unwrap_or(false);
        let status = if is_active && state.agent_session_running {
            ("Running", Color::Green)
        } else if agent.is_ready() {
            ("Ready", Color::DarkGray)
        } else {
            ("Unsupported", Color::Yellow)
        };

        lines.push(Line::from(vec![
            Span::styled(bullet, bullet_style),
            Span::styled(
                format!("{:<20}", agent.display_name),
                if is_selected {
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                },
            ),
            Span::styled(format!(" {:<12}", status.0), Style::default().fg(status.1)),
        ]));
    }
    f.render_widget(Paragraph::new(lines), chunks[0]);

    let hints = Line::from(vec![
        Span::styled("↑↓", Style::default().fg(Color::Yellow)),
        Span::raw(" Move   "),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(" Switch   "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" Cancel"),
    ]);
    f.render_widget(Paragraph::new(hints).alignment(Alignment::Center), chunks[1]);
}

fn render_project_select_overlay(f: &mut Frame, area: Rect, state: &TuiState) {
    let width = 64.min(area.width);
    let height = 8.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Change Workspace Directory ")
        .title_alignment(Alignment::Center);
    f.render_widget(block, modal_area);

    let inner = Rect::new(
        modal_area.x + 2,
        modal_area.y + 1,
        modal_area.width.saturating_sub(4),
        modal_area.height.saturating_sub(2),
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let current = Line::from(vec![
        Span::styled("Current: ", Style::default().fg(Color::DarkGray)),
        Span::styled(state.workspace_dir.to_string_lossy(), Style::default().fg(Color::White)),
    ]);
    f.render_widget(Paragraph::new(current), chunks[0]);

    let input_line = Line::from(vec![
        Span::styled("New:     ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(&state.project_input_text, Style::default().fg(Color::White)),
        Span::styled("█", Style::default().fg(Color::Cyan)),
    ]);
    f.render_widget(Paragraph::new(input_line), chunks[1]);

    let hints = Line::from(vec![
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(" Confirm   "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" Cancel"),
    ]);
    f.render_widget(Paragraph::new(hints).alignment(Alignment::Center), chunks[3]);
}

fn render_attention_overlay(f: &mut Frame, area: Rect, state: &TuiState) {
    let width = 70.min(area.width);
    let height = 18.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" Attention Required ")
        .title_alignment(Alignment::Center);
    f.render_widget(block, modal_area);

    let inner = Rect::new(
        modal_area.x + 2,
        modal_area.y + 1,
        modal_area.width.saturating_sub(4),
        modal_area.height.saturating_sub(2),
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(inner);

    let requests = state.requests();
    let errors = state.errors();

    let mut lines = Vec::new();
    if requests.is_empty() && errors.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "✓ No pending requests or active errors.",
            Style::default().fg(Color::Green),
        )));
    } else {
        for (i, req) in requests.iter().enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("[Req {}] ", i + 1), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(&req.1.summary, Style::default().fg(Color::White)),
            ]));
            if let Some(ref r_info) = req.1.request {
                lines.push(Line::from(vec![
                    Span::raw("   Type: "),
                    Span::styled(format!("{:?}", r_info.question_type), Style::default().fg(Color::Cyan)),
                ]));
            }
            lines.push(Line::raw(""));
        }
        for (i, err) in errors.iter().enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("[Err {}] ", i + 1), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(&err.1.summary, Style::default().fg(Color::White)),
            ]));
        }
    }
    f.render_widget(Paragraph::new(lines), chunks[0]);

    let hints = Line::from(vec![
        Span::styled("a / Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" Approve   "),
        Span::styled("d / c", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" Clear/Dismiss   "),
        Span::styled("Esc", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::raw(" Close"),
    ]);
    f.render_widget(Paragraph::new(hints).alignment(Alignment::Center), chunks[1]);
}

fn render_switch_confirm_overlay(f: &mut Frame, area: Rect, state: &TuiState) {
    let width = 60.min(area.width);
    let height = 8.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(" Confirm Agent Termination ")
        .title_alignment(Alignment::Center);
    f.render_widget(block, modal_area);

    let inner = Rect::new(
        modal_area.x + 2,
        modal_area.y + 1,
        modal_area.width.saturating_sub(4),
        modal_area.height.saturating_sub(2),
    );

    let agent_name = state
        .active_agent
        .as_ref()
        .map(|a| a.display_name.as_str())
        .unwrap_or("Current agent");

    let text = vec![
        Line::from(Span::styled(
            format!("An active {agent_name} session is currently running."),
            Style::default().fg(Color::Yellow),
        )),
        Line::raw("Terminate this session and switch to the new agent?"),
        Line::raw(""),
        Line::from(vec![
            Span::styled("y", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" Yes, Terminate and Switch   "),
            Span::styled("n / Esc", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" Keep Current"),
        ]),
    ];

    f.render_widget(Paragraph::new(text).alignment(Alignment::Center), inner);
}

fn render_menu_overlay(f: &mut Frame, area: Rect, state: &TuiState) {
    let width = 66.min(area.width);
    let height = 20.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .title(" ⚙ AgentDesk Configuration & Pair Menu ")
        .title_alignment(Alignment::Center);
    f.render_widget(block, modal_area);

    let inner = Rect::new(
        modal_area.x + 2,
        modal_area.y + 1,
        modal_area.width.saturating_sub(4),
        modal_area.height.saturating_sub(2),
    );

    let menu_items = [
        ("1. Pair Phone (Show QR Code)", "Generate QR code to pair your phone for remote supervision"),
        ("2. Paired Devices", "View paired mobile devices or revoke access"),
        ("3. Notifications & Attention (F3)", "View, inspect, approve, or clear pending attention items"),
        ("4. Connection & Relay Settings", "Inspect daemon port, loopback bind, and relay URL"),
        ("5. Switch Coding Agent", "Change active coding agent (Antigravity, Gemini, etc.)"),
        ("6. Exit AgentDesk", "Shut down the terminal workspace and daemon"),
    ];

    let mut lines = Vec::new();
    lines.push(Line::raw(""));

    for (i, (title, desc)) in menu_items.iter().enumerate() {
        let is_selected = state.config_menu_index == i;
        let bullet = if is_selected { "● " } else { "○ " };
        let bullet_color = if is_selected { Color::Cyan } else { Color::DarkGray };
        let title_style = if is_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        lines.push(Line::from(vec![
            Span::styled(format!("  {bullet}"), Style::default().fg(bullet_color).add_modifier(Modifier::BOLD)),
            Span::styled(*title, title_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled(format!("      {desc}"), Style::default().fg(Color::DarkGray)),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("  ↑/↓: ", Style::default().fg(Color::Yellow)),
        Span::styled("Navigate   ", Style::default().fg(Color::DarkGray)),
        Span::styled("Enter / 1-6: ", Style::default().fg(Color::Yellow)),
        Span::styled("Select   ", Style::default().fg(Color::DarkGray)),
        Span::styled("Esc: ", Style::default().fg(Color::Yellow)),
        Span::styled("Close Menu", Style::default().fg(Color::DarkGray)),
    ]));

    f.render_widget(Paragraph::new(lines), inner);
}
