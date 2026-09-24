//! Projects view.
//! Groups running agents and attention states by project.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::state::TuiState;
use crate::tui::views::dashboard::truncate_str;

pub fn render_projects(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" PROJECTS ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));

    let projects_map = state.projects();
    if projects_map.is_empty() {
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  ○ ", Style::default().fg(Color::DarkGray)),
                Span::styled("No active projects found in queue.", Style::default().fg(Color::White)),
            ]),
        ];
        f.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let mut lines = Vec::new();
    let project_names: Vec<String> = projects_map.keys().cloned().collect();

    for (p_idx, proj_name) in project_names.iter().enumerate() {
        let is_sel_project = p_idx == state.projects_index;
        let proj_header_style = if is_sel_project {
            Style::default().fg(Color::Cyan).bg(Color::Rgb(20, 35, 50)).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        };

        let marker = if is_sel_project { "▶ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(Color::Cyan)),
            Span::styled(format!("Project: {proj_name}"), proj_header_style),
        ]));

        if let Some(entries) = projects_map.get(proj_name) {
            for (_, ev) in entries {
                let (icon, color) = match ev.category {
                    agentdesk_model::Category::Request => ("!", Color::Yellow),
                    agentdesk_model::Category::Error => ("!", Color::Red),
                    agentdesk_model::Category::Working => ("●", Color::Cyan),
                    agentdesk_model::Category::Completed => ("✓", Color::Green),
                };

                let cat_str = match ev.category {
                    agentdesk_model::Category::Request => "Needs attention",
                    agentdesk_model::Category::Error => "Error",
                    agentdesk_model::Category::Working => "Working",
                    agentdesk_model::Category::Completed => "Completed",
                };

                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(format!("{icon} "), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format!("{:<18}", ev.agent_name),
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<18}", cat_str),
                        Style::default().fg(color),
                    ),
                    Span::styled(
                        truncate_str(&ev.summary, 40),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }

        lines.push(Line::raw(""));
    }

    f.render_widget(Paragraph::new(lines).block(block), area);
}
