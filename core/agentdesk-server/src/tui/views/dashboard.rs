//! Main Dashboard: the primary screen of AgentDesk.
//! Strictly orders content by priority:
//! 1. NEEDS YOUR ATTENTION (Tier 0: Requests)
//! 2. ERRORS (Tier 1: Unresolved errors)
//! 3. WORKING (Tier 3: Active tasks, quiet & aggregated)
//! 4. COMPLETED (Tier 2: Recently finished work)

use chrono::Utc;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::state::TuiState;

pub fn render_dashboard(f: &mut Frame, area: Rect, state: &TuiState) {
    let requests = state.requests();
    let errors = state.errors();
    let working = state.working();
    let completed = state.completed();

    // Responsive height allocation based on content
    let req_height = if requests.is_empty() {
        3
    } else {
        // Expanded to show prompt + options
        8
    };

    let err_height = if errors.is_empty() {
        3
    } else {
        let count = errors.len().min(4);
        (count as u16) + 3
    };

    let work_height = if working.is_empty() {
        3
    } else {
        let count = working.len().min(5);
        (count as u16) + 3
    };

    // Calculate layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(req_height), // NEEDS ATTENTION
            Constraint::Length(err_height), // ERRORS
            Constraint::Length(work_height), // WORKING
            Constraint::Min(4),             // COMPLETED
        ])
        .split(area);

    // =========================================================================
    // 1. NEEDS YOUR ATTENTION (Priority 1 - High visual dominance)
    // =========================================================================
    let (att_border_color, att_title_style) = if !requests.is_empty() {
        (
            Color::Yellow,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (Color::DarkGray, Style::default().fg(Color::DarkGray))
    };

    let att_block = Block::default()
        .title(" NEEDS YOUR ATTENTION ")
        .title_style(att_title_style)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(att_border_color));

    if requests.is_empty() {
        let text = Line::from(vec![
            Span::styled("  ✓ ", Style::default().fg(Color::Green)),
            Span::styled(
                "All agents running autonomously (no pending approvals or input)",
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        f.render_widget(Paragraph::new(text).block(att_block), chunks[0]);
    } else {
        let (_entry, event) = requests[0];
        let mut lines = Vec::new();

        // Agent & Project tag line
        lines.push(Line::from(vec![
            Span::styled("  ! ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(
                &event.agent_name,
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("    {} ", event.project), Style::default().fg(Color::Cyan)),
            if let Some(ref t) = event.task_id {
                Span::styled(format!("/ {t} "), Style::default().fg(Color::DarkGray))
            } else {
                Span::raw("")
            },
            if requests.len() > 1 {
                Span::styled(
                    format!(" (+{} more requests waiting, press 'r')", requests.len() - 1),
                    Style::default().fg(Color::Yellow),
                )
            } else {
                Span::raw("")
            },
        ]));

        lines.push(Line::raw(""));

        // Prompt text
        if let Some(ref req) = event.request {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    &req.prompt,
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
            ]));

            lines.push(Line::raw(""));

            // Option pills
            if req.options.is_empty() {
                // Approval buttons
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(
                        " [1] Approve ",
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("   "),
                    Span::styled(
                        " [2] Deny ",
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Red)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("   "),
                    Span::styled("(Press 'r' for detailed response)", Style::default().fg(Color::DarkGray)),
                ]));
            } else {
                let mut opt_spans = vec![Span::raw("    ")];
                for (idx, opt) in req.options.iter().enumerate().take(5) {
                    opt_spans.push(Span::styled(
                        format!("[{}] {}", idx + 1, opt),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ));
                    opt_spans.push(Span::raw("     "));
                }
                if req.options.len() > 5 {
                    opt_spans.push(Span::styled("...", Style::default().fg(Color::DarkGray)));
                }
                lines.push(Line::from(opt_spans));
            }
        } else {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(&event.message, Style::default().fg(Color::White)),
            ]));
        }

        f.render_widget(Paragraph::new(lines).block(att_block), chunks[0]);
    }

    // =========================================================================
    // 2. ERRORS (Priority 2 - Red visual prominence)
    // =========================================================================
    let (err_border_color, err_title_style) = if !errors.is_empty() {
        (
            Color::Red,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        (Color::DarkGray, Style::default().fg(Color::DarkGray))
    };

    let err_block = Block::default()
        .title(" ERRORS ")
        .title_style(err_title_style)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(err_border_color));

    if errors.is_empty() {
        let text = Line::from(vec![
            Span::styled("  ✓ ", Style::default().fg(Color::Green)),
            Span::styled(
                "Zero active errors across all agents",
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        f.render_widget(Paragraph::new(text).block(err_block), chunks[1]);
    } else {
        let mut lines = Vec::new();
        for (_, event) in errors.iter().take(4) {
            let sev_str = match event.severity {
                agentdesk_model::Severity::Critical => "CRITICAL",
                agentdesk_model::Severity::Important => "ERROR",
                _ => "NOTABLE",
            };
            let sev_style = if event.severity == agentdesk_model::Severity::Critical {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::LightRed)
            };

            lines.push(Line::from(vec![
                Span::styled("  ! ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("{:<15}", event.agent_name),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<16}", event.project),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<8}", sev_str),
                    sev_style,
                ),
                Span::styled(
                    format!("{:<35}", truncate_str(&event.summary, 35)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format_time_ago(event.ts),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
        f.render_widget(Paragraph::new(lines).block(err_block), chunks[1]);
    }

    // =========================================================================
    // 3. WORKING (Priority 3 - Quiet & Aggregated)
    // =========================================================================
    let work_block = Block::default()
        .title(" WORKING ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));

    if working.is_empty() {
        let text = Line::from(vec![
            Span::styled("  ○ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "No agents currently executing tasks (all idle)",
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        f.render_widget(Paragraph::new(text).block(work_block), chunks[2]);
    } else {
        let mut lines = Vec::new();
        for (_, event) in working.iter().take(5) {
            lines.push(Line::from(vec![
                Span::styled("  ● ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{:<15}", event.agent_name),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<16}", event.project),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("{:<38}", truncate_str(&event.summary, 38)),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    format_duration(event.ts),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
        f.render_widget(Paragraph::new(lines).block(work_block), chunks[2]);
    }

    // =========================================================================
    // 4. COMPLETED (Priority 4 - Confirmation of done tasks)
    // =========================================================================
    let comp_block = Block::default()
        .title(" COMPLETED ")
        .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));

    if completed.is_empty() {
        let text = Line::from(vec![
            Span::styled("  ○ ", Style::default().fg(Color::DarkGray)),
            Span::styled("No completed tasks yet in this session", Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(text).block(comp_block), chunks[3]);
    } else {
        let mut lines = Vec::new();
        for (_, event) in completed.iter().take(6) {
            lines.push(Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                Span::styled(
                    format!("{:<15}", event.agent_name),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{:<16}", event.project),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<38}", truncate_str(&event.summary, 38)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format_time_ago(event.ts),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
        f.render_widget(Paragraph::new(lines).block(comp_block), chunks[3]);
    }
}

pub fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() > max_len {
        let truncated: String = s.chars().take(max_len - 3).collect();
        format!("{truncated}...")
    } else {
        s.to_string()
    }
}

pub fn format_duration(ts: chrono::DateTime<Utc>) -> String {
    let elapsed = Utc::now().signed_duration_since(ts);
    let total_secs = elapsed.num_seconds().max(0);
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}", mins, secs)
}

pub fn format_time_ago(ts: chrono::DateTime<Utc>) -> String {
    let elapsed = Utc::now().signed_duration_since(ts);
    let secs = elapsed.num_seconds().max(0);
    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}
