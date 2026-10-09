//! Structured Requests view.
//! Supports all AgentDesk interactive question types:
//! - Binary Approval / Denial
//! - Yes / No
//! - Single Choice (Radio selection)
//! - Multiple Choice (Checkboxes)
//! - Free-form Text Input
//! - Choice + Write-in ("Other")

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;

use agentdesk_model::{Decision, QuestionType};

use crate::tui::state::TuiState;
use crate::tui::theme::Theme;

pub fn render_requests(f: &mut Frame, area: Rect, state: &TuiState) {
    let requests = state.requests();

    let block = Block::default()
        .title(" REQUESTS / HUMAN SUPERVISION REQUIRED ")
        .title_style(Theme::warning())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(Theme::AMBER));

    if requests.is_empty() {
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  ✓ ", Theme::primary()),
                Span::styled(
                    "No pending requests — all agents are running autonomously!",
                    Theme::text_bold(),
                ),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("  When an agent asks for permission, confirmation, or answers,", Theme::text_secondary()),
            ]),
            Line::from(vec![
                Span::styled("  the structured decision card will appear here and on your phone.", Theme::text_secondary()),
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
            .border_style(ratatui::style::Style::default().fg(Theme::BORDER));

        let mut lines = Vec::new();
        for (idx, (_, ev)) in requests.iter().enumerate() {
            let is_sel = idx == state.requests_index;
            let marker = if is_sel { "▶ " } else { "  " };
            let style = if is_sel {
                Theme::warning()
            } else {
                Theme::text_normal()
            };

            lines.push(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(&ev.agent_name, style),
            ]));
            lines.push(Line::from(vec![
                Span::styled(format!("   {}", ev.project), Theme::text_muted()),
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
            Span::styled("Agent:   ", Theme::text_muted()),
            Span::styled(&event.agent_name, Theme::text_bold()),
            Span::styled("   Project: ", Theme::text_muted()),
            Span::styled(&event.project, Theme::accent()),
            if let Some(ref t) = event.task_id {
                Span::styled(format!("   Task: {t}"), Theme::text_muted())
            } else {
                Span::raw("")
            },
        ]),
    ];
    f.render_widget(Paragraph::new(header_lines), card_chunks[0]);

    // 2. Prompt Box
    let prompt_block = Block::default()
        .title(card_title)
        .title_style(Theme::warning())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(Theme::AMBER));

    let prompt_text = Line::from(vec![
        Span::styled(
            format!("  {}", req_info.prompt),
            Theme::text_bold(),
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
                ratatui::style::Style::default().fg(ratatui::style::Color::Black).bg(Theme::EMERALD).add_modifier(Modifier::BOLD)
            } else {
                ratatui::style::Style::default().fg(Theme::EMERALD)
            };
            let deny_style = if !approve_sel {
                ratatui::style::Style::default().fg(ratatui::style::Color::Black).bg(Theme::RED).add_modifier(Modifier::BOLD)
            } else {
                ratatui::style::Style::default().fg(Theme::RED)
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
                    Theme::brand()
                } else if is_selected {
                    Theme::text_bold()
                } else {
                    Theme::text_secondary()
                };

                control_lines.push(Line::from(vec![
                    Span::styled(prefix, Theme::accent()),
                    Span::styled(format!("{radio} {opt}"), style),
                ]));

                // Choice + Write-in ("Other")
                if opt.eq_ignore_ascii_case("other") && is_selected {
                    let cursor_char = if is_cursor { "█" } else { " " };
                    control_lines.push(Line::from(vec![
                        Span::raw("        "),
                        Span::styled("Custom: ", Theme::text_muted()),
                        Span::styled(&state.request_text_input, Theme::text_bold().add_modifier(Modifier::UNDERLINED)),
                        Span::styled(cursor_char, Theme::accent()),
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
                    Theme::brand()
                } else if is_checked {
                    Theme::primary()
                } else {
                    Theme::text_secondary()
                };

                control_lines.push(Line::from(vec![
                    Span::styled(prefix, Theme::accent()),
                    Span::styled(format!("{check} {opt}"), style),
                ]));
            }
        }
        QuestionType::FreeText => {
            // Free-form input box
            control_lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("Enter response:", Theme::text_muted()),
            ]));
            control_lines.push(Line::raw(""));
            control_lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("> ", Theme::brand()),
                Span::styled(&state.request_text_input, Theme::text_bold()),
                Span::styled("█", Theme::accent()),
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
        Span::styled(hint_text, Theme::text_muted()),
    ]);
    f.render_widget(Paragraph::new(hint), card_chunks[3]);
}
