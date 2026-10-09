//! Projects view.
//! Groups running agents and attention states by project.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::state::TuiState;
use crate::tui::theme::Theme;
use crate::tui::views::dashboard::truncate_str;

pub fn render_projects(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" PROJECTS ")
        .title_style(Theme::brand())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(Theme::BORDER));

    let projects_map = state.projects();
    if projects_map.is_empty() {
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  ○ ", Theme::text_muted()),
                Span::styled("No active projects found in queue.", Theme::text_muted()),
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
            ratatui::style::Style::default().fg(Theme::CYAN).bg(Theme::SURFACE_ELEVATED).add_modifier(Modifier::BOLD)
        } else {
            Theme::brand()
        };

        let marker = if is_sel_project { "▶ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(marker, Theme::accent()),
            Span::styled(format!("Project: {proj_name}"), proj_header_style),
        ]));

        if let Some(entries) = projects_map.get(proj_name) {
            for (_, ev) in entries {
                let (icon, color) = match ev.category {
                    agentdesk_model::Category::Request => ("!", Theme::AMBER),
                    agentdesk_model::Category::Error => ("!", Theme::RED),
                    agentdesk_model::Category::Working => ("●", Theme::CYAN),
                    agentdesk_model::Category::Completed => ("✓", Theme::EMERALD),
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
