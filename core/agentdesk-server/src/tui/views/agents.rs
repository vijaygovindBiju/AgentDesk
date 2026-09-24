//! Agents list and Agent Detail views.
//! Displays all agents, their project association, and their current activity.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use crate::tui::state::TuiState;
use crate::tui::views::dashboard::truncate_str;

pub fn render_agents(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" AGENTS ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));

    if state.agents.is_empty() {
        let text = Line::from(vec![
            Span::styled("  ○ ", Style::default().fg(Color::DarkGray)),
            Span::styled("No agents registered yet", Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let header_cells = ["STATUS", "AGENT", "PROJECT", "ACTIVITY"]
        .into_iter()
        .map(|h| Span::styled(h, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows: Vec<Row> = state
        .agents
        .iter()
        .enumerate()
        .map(|(idx, agent)| {
            let (cat, status_text, event) = state.get_agent_status(&agent.agent_id);
            let (icon, icon_style) = match cat {
                agentdesk_model::Category::Request => {
                    ("!", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                }
                agentdesk_model::Category::Error => {
                    ("!", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                }
                agentdesk_model::Category::Working => {
                    ("●", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                }
                agentdesk_model::Category::Completed => {
                    ("✓", Style::default().fg(Color::Green))
                }
            };

            let activity = if let Some(ev) = event {
                ev.summary.clone()
            } else {
                status_text.to_string()
            };

            let row_style = if idx == state.agents_index {
                Style::default().bg(Color::Rgb(30, 40, 55)).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            Row::new(vec![
                Span::styled(format!(" {icon} "), icon_style),
                Span::styled(agent.name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(agent.project.clone(), Style::default().fg(Color::DarkGray)),
                Span::styled(truncate_str(&activity, 45), Style::default()),
            ])
            .style(row_style)
            .height(1)
        })
        .collect();

    let widths = [
        Constraint::Length(8),
        Constraint::Length(22),
        Constraint::Length(20),
        Constraint::Min(30),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(block);

    f.render_widget(table, area);
}

pub fn render_agent_detail(f: &mut Frame, area: Rect, state: &TuiState) {
    let agent = match state.agents.get(state.agents_index) {
        Some(a) => a,
        None => return,
    };

    let block = Block::default()
        .title(format!(" AGENT DETAIL — {} ", agent.name))
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let (cat, status_text, current_ev) = state.get_agent_status(&agent.agent_id);
    let recent = state.get_agent_recent_events(&agent.agent_id);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Status, Project, Adapter
            Constraint::Length(4),  // Current Task
            Constraint::Length(4),  // Current Attention
            Constraint::Min(6),     // Recent Activity Timeline
        ])
        .margin(1)
        .split(block.inner(area));

    f.render_widget(block, area);

    // 1. Overview header
    let (icon, color) = match cat {
        agentdesk_model::Category::Request => ("!", Color::Yellow),
        agentdesk_model::Category::Error => ("!", Color::Red),
        agentdesk_model::Category::Working => ("●", Color::Cyan),
        agentdesk_model::Category::Completed => ("✓", Color::Green),
    };

    let overview = vec![
        Line::from(vec![
            Span::styled(format!("Status:   {icon} "), Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Span::styled(status_text.to_uppercase(), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Project:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(&agent.project, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Adapter:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:?}", agent.adapter_kind), Style::default().fg(Color::White)),
        ]),
    ];
    f.render_widget(Paragraph::new(overview), chunks[0]);

    // 2. Current Task
    let mut task_lines = vec![
        Line::from(vec![
            Span::styled("Current Task", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("─".repeat(50), Style::default().fg(Color::DarkGray)),
        ]),
    ];
    if let Some(ev) = current_ev {
        task_lines.push(Line::from(vec![
            Span::styled(&ev.summary, Style::default().fg(Color::White)),
            if let Some(ref t) = ev.task_id {
                Span::styled(format!(" (ID: {t})"), Style::default().fg(Color::DarkGray))
            } else {
                Span::raw("")
            },
        ]));
    } else {
        task_lines.push(Line::from(vec![
            Span::styled("None (agent is idle)", Style::default().fg(Color::DarkGray)),
        ]));
    }
    f.render_widget(Paragraph::new(task_lines), chunks[1]);

    // 3. Current Attention
    let mut att_lines = vec![
        Line::from(vec![
            Span::styled("Current Attention", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("─".repeat(50), Style::default().fg(Color::DarkGray)),
        ]),
    ];
    if cat == agentdesk_model::Category::Request {
        if let Some(ev) = current_ev {
            if let Some(ref req) = ev.request {
                att_lines.push(Line::from(vec![
                    Span::styled("! Input Required: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled(&req.prompt, Style::default().fg(Color::White)),
                ]));
            }
        }
    } else if cat == agentdesk_model::Category::Error {
        if let Some(ev) = current_ev {
            att_lines.push(Line::from(vec![
                Span::styled("! Error: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(&ev.message, Style::default().fg(Color::White)),
            ]));
        }
    } else {
        att_lines.push(Line::from(vec![
            Span::styled("None (operating normally)", Style::default().fg(Color::DarkGray)),
        ]));
    }
    f.render_widget(Paragraph::new(att_lines), chunks[2]);

    // 4. Recent Activity Timeline
    let mut hist_lines = vec![
        Line::from(vec![
            Span::styled("Recent Activity", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("─".repeat(50), Style::default().fg(Color::DarkGray)),
        ]),
    ];
    if recent.is_empty() {
        hist_lines.push(Line::from(vec![
            Span::styled("No activity recorded yet", Style::default().fg(Color::DarkGray)),
        ]));
    } else {
        for ev in recent {
            let time_str = ev.ts.format("%H:%M:%S").to_string();
            let (icon, color) = match ev.category {
                agentdesk_model::Category::Request => ("!", Color::Yellow),
                agentdesk_model::Category::Error => ("!", Color::Red),
                agentdesk_model::Category::Working => ("●", Color::Cyan),
                agentdesk_model::Category::Completed => ("✓", Color::Green),
            };
            hist_lines.push(Line::from(vec![
                Span::styled(format!("{time_str}  "), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{icon} "), Style::default().fg(color)),
                Span::styled(&ev.summary, Style::default().fg(Color::White)),
            ]));
        }
    }
    f.render_widget(Paragraph::new(hist_lines), chunks[3]);
}
