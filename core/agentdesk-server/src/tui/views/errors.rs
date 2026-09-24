//! Errors list and Error Detail views.
//! Implements progressive disclosure:
//! - LEVEL 1: Summary & Message
//! - LEVEL 2: Detailed attributes (file, line, error fields)
//! - LEVEL 3: Bounded logs around the event

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::tui::state::{DisclosureLevel, TuiState};
use crate::tui::views::dashboard::{format_time_ago, truncate_str};

pub fn render_errors(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" ERRORS ")
        .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Red));

    let errors = state.errors();
    if errors.is_empty() {
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled("Zero active errors across all coding agents!", Style::default().fg(Color::White)),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("  When a build, test, tool, or syntax error fails a task,", Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(vec![
                Span::styled("  it will be captured and surfaced here for human inspection.", Style::default().fg(Color::DarkGray)),
            ]),
        ];
        f.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let header_cells = ["!", "AGENT", "PROJECT", "SEVERITY", "SUMMARY", "TIME"]
        .into_iter()
        .map(|h| Span::styled(h, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows: Vec<Row> = errors
        .iter()
        .enumerate()
        .map(|(idx, (_, ev))| {
            let sev_str = match ev.severity {
                agentdesk_model::Severity::Critical => "CRITICAL",
                agentdesk_model::Severity::Important => "ERROR",
                agentdesk_model::Severity::Notable => "NOTABLE",
                agentdesk_model::Severity::Routine => "ROUTINE",
            };
            let sev_style = if ev.severity == agentdesk_model::Severity::Critical {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::LightRed)
            };

            let row_style = if idx == state.errors_index {
                Style::default().bg(Color::Rgb(50, 20, 20)).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            Row::new(vec![
                Span::styled(" ! ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(ev.agent_name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(ev.project.clone(), Style::default().fg(Color::DarkGray)),
                Span::styled(sev_str, sev_style),
                Span::styled(truncate_str(&ev.summary, 40), Style::default()),
                Span::styled(format_time_ago(ev.ts), Style::default().fg(Color::DarkGray)),
            ])
            .style(row_style)
            .height(1)
        })
        .collect();

    let widths = [
        Constraint::Length(5),
        Constraint::Length(20),
        Constraint::Length(18),
        Constraint::Length(12),
        Constraint::Min(30),
        Constraint::Length(12),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(block);

    f.render_widget(table, area);
}

pub fn render_error_detail(f: &mut Frame, area: Rect, state: &TuiState) {
    let (_, event) = match state.active_error() {
        Some(e) => e,
        None => return,
    };

    let block = Block::default()
        .title(format!(" ERROR DETAIL — {} ", event.agent_name))
        .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Red));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Tabs for Level 1, 2, 3
            Constraint::Min(10),    // Level content
            Constraint::Length(2),  // Navigation footer
        ])
        .margin(1)
        .split(block.inner(area));

    f.render_widget(block, area);

    // 1. Disclosure Level Tabs
    let tab_style = |lvl: DisclosureLevel| {
        if state.error_disclosure_level == lvl {
            Style::default().fg(Color::Black).bg(Color::Red).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        }
    };

    let tabs = Line::from(vec![
        Span::styled(" [1] LEVEL 1: SUMMARY ", tab_style(DisclosureLevel::Summary)),
        Span::raw("   "),
        Span::styled(" [2] LEVEL 2: DETAILS ", tab_style(DisclosureLevel::Details)),
        Span::raw("   "),
        Span::styled(" [3] LEVEL 3: LOGS ", tab_style(DisclosureLevel::Logs)),
    ]);
    f.render_widget(Paragraph::new(tabs), chunks[0]);

    // 2. Active Level Content
    match state.error_disclosure_level {
        DisclosureLevel::Summary => {
            let mut lines = Vec::new();
            lines.push(Line::from(vec![
                Span::styled("Agent:     ", Style::default().fg(Color::DarkGray)),
                Span::styled(&event.agent_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("   Project: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&event.project, Style::default().fg(Color::Cyan)),
                if let Some(ref t) = event.task_id {
                    Span::styled(format!("   Task: {t}"), Style::default().fg(Color::DarkGray))
                } else {
                    Span::raw("")
                },
            ]));
            lines.push(Line::from(vec![
                Span::styled("Severity:  ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:?}", event.severity), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(format!("   Time: {}", event.ts.format("%Y-%m-%d %H:%M:%S UTC")), Style::default().fg(Color::DarkGray)),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("Summary:", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(vec![
                Span::styled(format!("  {}", event.summary), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("Error Message:", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(vec![
                Span::styled(format!("  {}", event.message), Style::default().fg(Color::LightRed)),
            ]));

            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), chunks[1]);
        }
        DisclosureLevel::Details => {
            let mut lines = Vec::new();
            lines.push(Line::from(vec![
                Span::styled("Operation: ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:?}", event.operation), Style::default().fg(Color::White)),
                Span::styled("   Kind: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&event.kind, Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Log Range: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}-{} (pinned: {})", event.log_range.start, event.log_range.end, event.log_range.pinned),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("Structured Attributes (Details):", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("─".repeat(50), Style::default().fg(Color::DarkGray)),
            ]));

            if event.details.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("  No additional structured attributes attached", Style::default().fg(Color::DarkGray)),
                ]));
            } else {
                for (key, val) in &event.details {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {:<20} : ", key), Style::default().fg(Color::DarkGray)),
                        Span::styled(val.to_string(), Style::default().fg(Color::White)),
                    ]));
                }
            }

            f.render_widget(Paragraph::new(lines), chunks[1]);
        }
        DisclosureLevel::Logs => {
            let log_block = Block::default()
                .title(" Paged Bounded Logs ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray));

            if state.logs_lines.is_empty() {
                let p = Paragraph::new("No cached log lines available for this error window.\n(Logs are retained boundedly on laptop)")
                    .style(Style::default().fg(Color::DarkGray))
                    .block(log_block);
                f.render_widget(p, chunks[1]);
            } else {
                let mut lines = Vec::new();
                let skip = state.error_log_scroll;
                for l in state.logs_lines.iter().skip(skip).take(30) {
                    lines.push(Line::from(vec![
                        Span::styled(format!("{:>6} ", l.offset), Style::default().fg(Color::DarkGray)),
                        Span::styled(l.ts.format("%H:%M:%S ").to_string(), Style::default().fg(Color::DarkGray)),
                        Span::styled(&l.text, Style::default().fg(Color::White)),
                    ]));
                }
                f.render_widget(Paragraph::new(lines).block(log_block), chunks[1]);
            }
        }
    }

    // 3. Navigation hints
    let nav = Line::from(vec![
        Span::styled("Tab/1/2/3 = switch disclosure level    d = dismiss error    Esc = back to errors", Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(nav), chunks[2]);
}
