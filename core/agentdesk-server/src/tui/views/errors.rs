//! Errors list and Error Detail views.
//! Implements progressive disclosure:
//! - LEVEL 1: Summary & Message
//! - LEVEL 2: Detailed attributes (file, line, error fields)
//! - LEVEL 3: Bounded logs around the event

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::tui::state::{DisclosureLevel, TuiState};
use crate::tui::theme::Theme;
use crate::tui::views::dashboard::{format_time_ago, truncate_str};

pub fn render_errors(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" ERRORS ")
        .title_style(Theme::error())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(Theme::RED));

    let errors = state.errors();
    if errors.is_empty() {
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  ✓ ", Theme::primary()),
                Span::styled("Zero active errors across all coding agents!", Theme::text_bold()),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("  When a build, test, tool, or syntax error fails a task,", Theme::text_secondary()),
            ]),
            Line::from(vec![
                Span::styled("  it will be captured and surfaced here for human inspection.", Theme::text_secondary()),
            ]),
        ];
        f.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let header_cells = ["!", "AGENT", "PROJECT", "SEVERITY", "SUMMARY", "TIME"]
        .into_iter()
        .map(|h| Span::styled(h, Theme::error()));
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
                Theme::error()
            } else {
                ratatui::style::Style::default().fg(Theme::RED)
            };

            let row_style = if idx == state.errors_index {
                ratatui::style::Style::default().bg(Theme::SURFACE_ELEVATED).fg(Theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
            } else {
                ratatui::style::Style::default().fg(Theme::TEXT_PRIMARY)
            };

            Row::new(vec![
                Span::styled(" ! ", Theme::error()),
                Span::styled(ev.agent_name.clone(), Theme::text_bold()),
                Span::styled(ev.project.clone(), Theme::text_muted()),
                Span::styled(sev_str, sev_style),
                Span::styled(truncate_str(&ev.summary, 40), Theme::text_normal()),
                Span::styled(format_time_ago(ev.ts), Theme::text_muted()),
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
        .title_style(Theme::error())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(Theme::RED));

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
            ratatui::style::Style::default().fg(ratatui::style::Color::Black).bg(Theme::RED).add_modifier(Modifier::BOLD)
        } else {
            Theme::text_muted()
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
                Span::styled("Agent:     ", Theme::text_muted()),
                Span::styled(&event.agent_name, Theme::text_bold()),
                Span::styled("   Project: ", Theme::text_muted()),
                Span::styled(&event.project, Theme::accent()),
                if let Some(ref t) = event.task_id {
                    Span::styled(format!("   Task: {t}"), Theme::text_muted())
                } else {
                    Span::raw("")
                },
            ]));
            lines.push(Line::from(vec![
                Span::styled("Severity:  ", Theme::text_muted()),
                Span::styled(format!("{:?}", event.severity), Theme::error()),
                Span::styled(format!("   Time: {}", event.ts.format("%Y-%m-%d %H:%M:%S UTC")), Theme::text_muted()),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("Summary:", Theme::brand()),
            ]));
            lines.push(Line::from(vec![
                Span::styled(format!("  {}", event.summary), Theme::text_bold()),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("Error Message:", Theme::brand()),
            ]));
            lines.push(Line::from(vec![
                Span::styled(format!("  {}", event.message), ratatui::style::Style::default().fg(Theme::RED)),
            ]));

            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), chunks[1]);
        }
        DisclosureLevel::Details => {
            let mut lines = Vec::new();
            lines.push(Line::from(vec![
                Span::styled("Operation: ", Theme::text_muted()),
                Span::styled(format!("{:?}", event.operation), Theme::text_normal()),
                Span::styled("   Kind: ", Theme::text_muted()),
                Span::styled(&event.kind, Theme::text_normal()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Log Range: ", Theme::text_muted()),
                Span::styled(
                    format!("{}-{} (pinned: {})", event.log_range.start, event.log_range.end, event.log_range.pinned),
                    Theme::text_muted(),
                ),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("Structured Attributes (Details):", Theme::brand()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("─".repeat(50), Theme::text_muted()),
            ]));

            if event.details.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("  No additional structured attributes attached", Theme::text_muted()),
                ]));
            } else {
                for (key, val) in &event.details {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {:<20} : ", key), Theme::text_muted()),
                        Span::styled(val.to_string(), Theme::text_normal()),
                    ]));
                }
            }

            f.render_widget(Paragraph::new(lines), chunks[1]);
        }
        DisclosureLevel::Logs => {
            let log_block = Block::default()
                .title(" Paged Bounded Logs ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(ratatui::style::Style::default().fg(Theme::BORDER));

            if state.logs_lines.is_empty() {
                let p = Paragraph::new("No cached log lines available for this error window.\n(Logs are retained boundedly on laptop)")
                    .style(Theme::text_muted())
                    .block(log_block);
                f.render_widget(p, chunks[1]);
            } else {
                let mut lines = Vec::new();
                let skip = state.error_log_scroll;
                for l in state.logs_lines.iter().skip(skip).take(30) {
                    lines.push(Line::from(vec![
                        Span::styled(format!("{:>6} ", l.offset), Theme::text_muted()),
                        Span::styled(l.ts.format("%H:%M:%S ").to_string(), Theme::text_muted()),
                        Span::styled(&l.text, Theme::text_normal()),
                    ]));
                }
                f.render_widget(Paragraph::new(lines).block(log_block), chunks[1]);
            }
        }
    }

    // 3. Navigation hints
    let nav = Line::from(vec![
        Span::styled("Tab/1/2/3 = switch disclosure level    d = dismiss error    Esc = back to errors", Theme::text_muted()),
    ]);
    f.render_widget(Paragraph::new(nav), chunks[2]);
}
