//! Footer bar rendering with keyboard shortcuts and contextual feedback.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::state::{Screen, TuiState};

pub fn render_footer(f: &mut Frame, area: Rect, state: &TuiState) {
    let right_width = if area.width >= 120 { 22 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(40),             // Shortcuts
            Constraint::Length(right_width), // Feedback or screen indicator
        ])
        .split(area);

    let key_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(Color::White);

    let shortcuts = match state.current_screen {
        Screen::Requests => vec![
            Span::styled("[Enter] ", key_style),
            Span::styled("Submit  ", desc_style),
            Span::styled("[↑/↓] ", key_style),
            Span::styled("Navigate  ", desc_style),
            Span::styled("[Space] ", key_style),
            Span::styled("Toggle  ", desc_style),
            Span::styled("[y/n] ", key_style),
            Span::styled("Approve/Deny  ", desc_style),
            Span::styled("[Esc] ", key_style),
            Span::styled("Back", desc_style),
        ],
        Screen::Errors | Screen::ErrorDetail => vec![
            Span::styled("[Tab/1-3] ", key_style),
            Span::styled("Disclosure Level  ", desc_style),
            Span::styled("[j/k] ", key_style),
            Span::styled("Scroll Logs  ", desc_style),
            Span::styled("[d] ", key_style),
            Span::styled("Dismiss  ", desc_style),
            Span::styled("[Esc] ", key_style),
            Span::styled("Back", desc_style),
        ],
        Screen::Pairing => vec![
            Span::styled("[Tab] ", key_style),
            Span::styled("Toggle Mode  ", desc_style),
            Span::styled("[g] ", key_style),
            Span::styled("Regenerate  ", desc_style),
            Span::styled("[Esc] ", key_style),
            Span::styled("Close", desc_style),
        ],
        _ => {
            if area.width < 95 {
                vec![
                    Span::styled("[a] ", key_style),
                    Span::styled("Agents ", desc_style),
                    Span::styled("[r] ", key_style),
                    Span::styled("Requests ", desc_style),
                    Span::styled("[p] ", key_style),
                    Span::styled("Projects ", desc_style),
                    Span::styled("[d] ", key_style),
                    Span::styled("Devices ", desc_style),
                    Span::styled("[q] ", key_style),
                    Span::styled("Quit", desc_style),
                ]
            } else {
                vec![
                    Span::styled("[a] ", key_style),
                    Span::styled("Agents ", desc_style),
                    Span::styled("[r] ", key_style),
                    Span::styled("Requests ", desc_style),
                    Span::styled("[p] ", key_style),
                    Span::styled("Projects ", desc_style),
                    Span::styled("[d] ", key_style),
                    Span::styled("Devices ", desc_style),
                    Span::styled("[h] ", key_style),
                    Span::styled("History ", desc_style),
                    Span::styled("[P] ", key_style),
                    Span::styled("Pair ", desc_style),
                    Span::styled("[?] ", key_style),
                    Span::styled("Help ", desc_style),
                    Span::styled("[q] ", key_style),
                    Span::styled("Quit", desc_style),
                ]
            }
        }
    };

    f.render_widget(
        Paragraph::new(Line::from(shortcuts))
            .block(Block::default().borders(Borders::NONE)),
        chunks[0],
    );

    // Right side: feedback or current view indicator
    let right_text = if let Some((ref msg, is_err)) = state.request_feedback {
        let color = if is_err { Color::Red } else { Color::Green };
        Line::from(vec![Span::styled(msg, Style::default().fg(color))])
    } else {
        let view_name = match state.current_screen {
            Screen::AgentSelection => "AGENT SELECTION",
            Screen::Workspace => "WORKSPACE",
            Screen::Dashboard => "DASHBOARD",
            Screen::Agents => "AGENTS",
            Screen::AgentDetail => "AGENT DETAIL",
            Screen::Requests => "REQUESTS",
            Screen::Errors => "ERRORS",
            Screen::ErrorDetail => "ERROR DETAIL",
            Screen::Working => "WORKING",
            Screen::Completed => "COMPLETED",
            Screen::Projects => "PROJECTS",
            Screen::ProjectDetail => "PROJECT DETAIL",
            Screen::Devices => "DEVICES",
            Screen::Connection => "CONNECTION DETAILS",
            Screen::History => "HISTORY",
            Screen::Logs => "LOGS",
            Screen::Pairing => "PAIRING",
            Screen::Help => "HELP",
        };
        Line::from(vec![
            Span::styled("View: ", Style::default().fg(Color::DarkGray)),
            Span::styled(view_name, Style::default().fg(Color::Cyan)),
            Span::raw(" "),
        ])
    };

    f.render_widget(
        Paragraph::new(right_text).alignment(ratatui::layout::Alignment::Right),
        chunks[1],
    );
}
