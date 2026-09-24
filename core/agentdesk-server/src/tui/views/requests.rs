//! Structured Requests view.
//! Supports all AgentDesk interactive question types:
//! - Binary Approval / Denial
//! - Yes / No
//! - Single Choice (Radio selection)
//! - Multiple Choice (Checkboxes)
//! - Free-form Text Input
//! - Choice + Write-in ("Other")

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;

use agentdesk_model::{Decision, QuestionType};

use crate::tui::state::TuiState;

pub fn render_requests(f: &mut Frame, area: Rect, state: &TuiState) {
    let requests = state.requests();

    let block = Block::default()
        .title(" REQUESTS / HUMAN SUPERVISION REQUIRED ")
        .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Yellow));

    if requests.is_empty() {
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(
                    "No pending requests — all agents are running autonomously!",
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("  When an agent asks for permission, confirmation, or answers,", Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(vec![
                Span::styled("  the structured decision card will appear here and on your phone.", Style::default().fg(Color::DarkGray)),
            ]),
        ];
        f.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    // If multiple requests, show list on left and active card on right
    let (left_area, right_area) = if requests.len() > 1 && area.width > 90 {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(40)])
            .split(block.inner(area));
        (Some(chunks[0]), chunks[1])
    } else {
        (None, block.inner(area))
    };

    f.render_widget(block, area);

    // Render left list if applicable
    if let Some(la) = left_area {
        let list_block = Block::default()
            .title(" Queue ")
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(Color::DarkGray));

        let mut lines = Vec::new();
        for (idx, (_, ev)) in requests.iter().enumerate() {
            let is_sel = idx == state.requests_index;
            let marker = if is_sel { "▶ " } else { "  " };
            let style = if is_sel {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            lines.push(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(&ev.agent_name, style),
            ]));
            lines.push(Line::from(vec![
                Span::styled(format!("   {}", ev.project), Style::default().fg(Color::DarkGray)),
            ]));
            lines.push(Line::raw(""));
        }
        f.render_widget(Paragraph::new(lines).block(list_block), la);
    }

    // Active request details & interactive controls
    let (_, event) = match state.active_request() {
        Some(r) => r,
        None => return,
    };

    let req_info = match event.request {
        Some(ref r) => r,
        None => {
            let p = Paragraph::new(format!("Request: {}", event.message));
            f.render_widget(p, right_area);
            return;
        }
    };

    let qtype = req_info.question_type.unwrap_or_else(|| {
        if req_info.options.is_empty() {
            QuestionType::YesNo
        } else if req_info.options.len() == 2
            && req_info.options[0].eq_ignore_ascii_case("yes")
            && req_info.options[1].eq_ignore_ascii_case("no")
        {
            QuestionType::YesNo
        } else {
            QuestionType::SingleChoice
        }
    });

    let card_title = match qtype {
        QuestionType::YesNo if req_info.options.is_empty() => " APPROVAL REQUIRED ",
        _ => " INPUT REQUIRED ",
    };

    let card_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Agent, project, task
            Constraint::Length(4),  // Prompt box
            Constraint::Min(8),     // Interactive options
            Constraint::Length(2),  // Navigation hint
        ])
        .margin(1)
        .split(right_area);

    // 1. Header Info
    let header_lines = vec![
        Line::from(vec![
            Span::styled("Agent:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(&event.agent_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("   Project: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&event.project, Style::default().fg(Color::Cyan)),
            if let Some(ref t) = event.task_id {
                Span::styled(format!("   Task: {t}"), Style::default().fg(Color::DarkGray))
            } else {
                Span::raw("")
            },
        ]),
    ];
    f.render_widget(Paragraph::new(header_lines), card_chunks[0]);

    // 2. Prompt Box
    let prompt_block = Block::default()
        .title(card_title)
        .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Yellow));

    let prompt_text = Line::from(vec![
        Span::styled(
            format!("  {}", req_info.prompt),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(
        Paragraph::new(prompt_text).block(prompt_block).wrap(Wrap { trim: true }),
        card_chunks[1],
    );

    // 3. Interactive Controls based on QuestionType
    let mut control_lines = Vec::new();
    control_lines.push(Line::raw(""));

    match qtype {
        QuestionType::YesNo => {
            // Approval buttons
            let approve_sel = state.request_approval_decision == Decision::Approve;
            let approve_style = if approve_sel {
                Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            let deny_style = if !approve_sel {
                Style::default().fg(Color::Black).bg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Red)
            };

            let label_approve = if req_info.options.len() >= 2 {
                format!(" [ {} ] ", req_info.options[0])
            } else {
                " [ Approve ] ".to_string()
            };
            let label_deny = if req_info.options.len() >= 2 {
                format!(" [ {} ] ", req_info.options[1])
            } else {
                " [ Deny ] ".to_string()
            };

            control_lines.push(Line::from(vec![
                Span::raw("        "),
                Span::styled(label_approve, approve_style),
                Span::raw("        "),
                Span::styled(label_deny, deny_style),
            ]));
        }
        QuestionType::SingleChoice => {
            // Radio list
            for (idx, opt) in req_info.options.iter().enumerate() {
                let is_cursor = idx == state.request_option_cursor;
                let is_selected = state.request_selected_single == Some(idx);
                let radio = if is_selected { "●" } else { "○" };

                let prefix = if is_cursor { "  ▶ " } else { "    " };
                let style = if is_cursor {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };

                control_lines.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(Color::Cyan)),
                    Span::styled(format!("{radio} {opt}"), style),
                ]));

                // Choice + Write-in ("Other")
                if opt.eq_ignore_ascii_case("other") && is_selected {
                    let cursor_char = if is_cursor { "█" } else { " " };
                    control_lines.push(Line::from(vec![
                        Span::raw("        "),
                        Span::styled("Custom: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(&state.request_text_input, Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED)),
                        Span::styled(cursor_char, Style::default().fg(Color::Cyan)),
                    ]));
                }
            }
        }
        QuestionType::MultipleChoice => {
            // Checkboxes
            for (idx, opt) in req_info.options.iter().enumerate() {
                let is_cursor = idx == state.request_option_cursor;
                let is_checked = state.request_selected_multi.contains(&idx);
                let check = if is_checked { "[✓]" } else { "[ ]" };

                let prefix = if is_cursor { "  ▶ " } else { "    " };
                let style = if is_cursor {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else if is_checked {
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };

                control_lines.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(Color::Cyan)),
                    Span::styled(format!("{check} {opt}"), style),
                ]));
            }
        }
        QuestionType::FreeText => {
            // Free-form input box
            control_lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("Enter response:", Style::default().fg(Color::DarkGray)),
            ]));
            control_lines.push(Line::raw(""));
            control_lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(&state.request_text_input, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("█", Style::default().fg(Color::Cyan)),
            ]));
        }
    }

    f.render_widget(Paragraph::new(control_lines), card_chunks[2]);

    // 4. Instructions footer
    let hint_text = match qtype {
        QuestionType::YesNo => "Enter = submit    ←/→ = toggle Approve/Deny    y = approve    n = deny    Esc = back",
        QuestionType::SingleChoice => "Enter = submit    ↑/↓ = select option    Space = pick    Esc = back",
        QuestionType::MultipleChoice => "Enter = submit    ↑/↓ = navigate    Space = toggle checkbox    Esc = back",
        QuestionType::FreeText => "Enter = submit    Type response    Backspace = delete    Esc = back",
    };
    let hint = Line::from(vec![
        Span::styled(hint_text, Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(hint), card_chunks[3]);
}
