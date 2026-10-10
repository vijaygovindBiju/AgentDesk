//! Workspace terminal screen view.
//! Dominant interactive terminal surface with minimal top bar, phone status, attention badge, and overlays.

use std::cmp::min;

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use agentdesk_core::Screen;
use crate::tui::state::{ActiveTerminalTab, TextSelection, TuiState, WorkspaceOverlay};
use crate::tui::theme::Theme;

pub fn render_workspace(f: &mut Frame, area: Rect, state: &TuiState) {
    if area.width < 20 || area.height < 6 {
        return;
    }

    let term_area = if state.terminal_only_mode {
        // Collapses UI chrome: Sets both top header and bottom footer heights to 0, removing all borders, tabs, status bars.
        // Allocates all available rows and columns directly to the virtual terminal emulator.
        area
    } else {
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

        // 3. Bottom Status / Hints Bar
        render_bottom_bar(f, chunks[2], state);

        chunks[1]
    };

    // 2. Real Terminal with Native Split Panes (Horizontal / Vertical) and Zoom Toggle
    match state.split_layout {
        crate::tui::state::SplitPaneLayout::Single => {
            if let Some(ref session) = state.shell_session {
                render_terminal_grid(
                    f,
                    term_area,
                    &session.screen,
                    state.terminal_scroll,
                    state.terminal_selection,
                    &state.search_matches,
                    state.search_match_index,
                    state.is_visual_mode,
                    state.visual_cursor,
                );
            } else {
                render_empty_terminal(f, term_area);
            }
        }
        _ if state.pane_zoomed => {
            // Zoomed full pane
            let session_opt = match state.pane_focus {
                crate::tui::state::PaneFocus::Primary => state.shell_session.as_ref(),
                crate::tui::state::PaneFocus::Secondary => state.secondary_session.as_ref().or(state.shell_session.as_ref()),
            };
            if let Some(session) = session_opt {
                render_terminal_grid(
                    f,
                    term_area,
                    &session.screen,
                    state.terminal_scroll,
                    state.terminal_selection,
                    &state.search_matches,
                    state.search_match_index,
                    state.is_visual_mode,
                    state.visual_cursor,
                );
            } else {
                render_empty_terminal(f, term_area);
            }
        }
        layout => {
            let dir = match layout {
                crate::tui::state::SplitPaneLayout::Horizontal => Direction::Horizontal,
                _ => Direction::Vertical,
            };
            let pane_chunks = Layout::default()
                .direction(dir)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(term_area);

            // Primary Pane
            if let Some(ref session) = state.shell_session {
                render_terminal_grid(
                    f,
                    pane_chunks[0],
                    &session.screen,
                    state.terminal_scroll,
                    state.terminal_selection,
                    &state.search_matches,
                    state.search_match_index,
                    state.is_visual_mode,
                    state.visual_cursor,
                );
            } else {
                render_empty_terminal(f, pane_chunks[0]);
            }

            // Secondary Pane
            let sec_session = state.secondary_session.as_ref().or(state.shell_session.as_ref());
            if let Some(session) = sec_session {
                render_terminal_grid(
                    f,
                    pane_chunks[1],
                    &session.screen,
                    state.secondary_scroll,
                    None,
                    &state.search_matches,
                    state.search_match_index,
                    false,
                    (0, 0),
                );
            } else {
                render_empty_terminal(f, pane_chunks[1]);
            }
        }
    }

    // 4. Overlays (if active)
    match state.workspace_overlay {
        WorkspaceOverlay::None => {}
        WorkspaceOverlay::AgentSwitch => render_agent_switch_overlay(f, area, state),
        WorkspaceOverlay::ProjectSelect => render_project_select_overlay(f, area, state),
        WorkspaceOverlay::Attention => render_attention_overlay(f, area, state),
        WorkspaceOverlay::SwitchConfirm => render_switch_confirm_overlay(f, area, state),
        WorkspaceOverlay::Menu => render_menu_overlay(f, area, state),
        WorkspaceOverlay::Pairing => crate::tui::views::pairing::render_pairing(f, area, state),
        WorkspaceOverlay::PromptInput => render_prompt_input_overlay(f, area, state),
        WorkspaceOverlay::Search => render_search_overlay(f, area, state),
        WorkspaceOverlay::UrlPicker => render_url_picker_overlay(f, area, state),
    }
}

fn render_top_bar(f: &mut Frame, area: Rect, state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(14), // ◈ AGENTDESK
            Constraint::Length(22), // Agent Dropdown
            Constraint::Min(12),    // Workspace Directory
            Constraint::Length(22), // Relay trunk
            Constraint::Length(18), // Phone (via relay)
            Constraint::Length(8),  // Notifications
            Constraint::Length(14), // Tab toggle & Mode
        ])
        .split(area);

    // 1. Brand
    let brand = Line::from(vec![
        Span::styled("◈ AgentDesk", Theme::brand()),
        Span::styled(" │", Style::default().fg(Theme::BORDER)),
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
        ("●", Theme::EMERALD)
    } else {
        ("○", Theme::TEXT_MUTED)
    };

    let agent_badge = Line::from(vec![
        Span::styled(format!(" {icon} "), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(agent_name, Theme::text_bold()),
        Span::styled(" ▾", Theme::accent()),
        Span::styled(" │", Style::default().fg(Theme::BORDER)),
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
        Span::styled(format!(" 📁 {display_path} ▾"), Theme::accent()),
        Span::styled(" │", Style::default().fg(Theme::BORDER)),
    ]);
    f.render_widget(Paragraph::new(dir_badge), chunks[2]);

    // 4. Relay trunk
    let mut relay_spans = vec![Span::raw(" ")];
    relay_spans.extend(crate::tui::views::header::relay_status_spans(state));
    relay_spans.push(Span::styled(" │", Style::default().fg(Theme::BORDER)));
    f.render_widget(Paragraph::new(Line::from(relay_spans)), chunks[3]);

    // 5. Phone connection (via relay)
    let phone_text = if state.phone_connected {
        Line::from(vec![
            Span::styled(" 📱 ", Theme::primary()),
            Span::styled("Phone Paired", Theme::primary()),
            Span::styled(" │", Style::default().fg(Theme::BORDER)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" 📱 ", Theme::text_muted()),
            Span::styled("Phone Offline", Theme::text_muted()),
            Span::styled(" │", Style::default().fg(Theme::BORDER)),
        ])
    };
    f.render_widget(Paragraph::new(phone_text), chunks[4]);

    // 6. Attention notifications badge
    let pending_requests = state.requests().len();
    let errors_count = state.errors().len();
    let total_attention = pending_requests + errors_count;

    let notif_badge = if total_attention > 0 {
        Line::from(vec![
            Span::styled(" 🔔 ", Theme::warning()),
            Span::styled(
                format!("{total_attention}"),
                Theme::warning(),
            ),
            Span::styled(" │", Style::default().fg(Theme::BORDER)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" 🔔 ", Theme::text_muted()),
            Span::styled("0", Theme::text_muted()),
            Span::styled(" │", Style::default().fg(Theme::BORDER)),
        ])
    };
    f.render_widget(Paragraph::new(notif_badge), chunks[5]);

    // 7. Tab indicator & mode indicator
    let tab_text = match state.active_terminal_tab {
        ActiveTerminalTab::Shell => Span::styled("[Shell]", Theme::primary()),
        ActiveTerminalTab::Agent => Span::styled("[Agent]", Theme::brand()),
    };
    let (mode_icon, mode_style) = if state.dangerously_skip_permissions {
        (" ⚡", Style::default().fg(Theme::AMBER).add_modifier(Modifier::BOLD))
    } else {
        ("", Style::default())
    };
    let tab_line = Line::from(vec![
        tab_text,
        Span::styled(mode_icon, mode_style),
    ]);
    f.render_widget(Paragraph::new(tab_line), chunks[6]);
}

fn render_bottom_bar(f: &mut Frame, area: Rect, state: &TuiState) {
    if let Some((ref msg, instant)) = state.clipboard_notice {
        if instant.elapsed() < std::time::Duration::from_secs(3) {
            let line = Line::from(vec![
                Span::styled(" ✓ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{} to clipboard  ", msg), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled("· Drag mouse to select & copy", Style::default().fg(Color::DarkGray)),
            ]);
            f.render_widget(Paragraph::new(line), area);
            return;
        }
    }

    let shortcuts = Line::from(vec![
        Span::styled(" F1", Theme::shortcut_key()),
        Span::styled(" Agent  ", Theme::shortcut_desc()),
        Span::styled("F2", Theme::shortcut_key()),
        Span::styled(" Folder  ", Theme::shortcut_desc()),
        Span::styled("F3", Theme::shortcut_key()),
        Span::styled(" Attention  ", Theme::shortcut_desc()),
        Span::styled("F4", Theme::shortcut_key()),
        Span::styled(" Menu  ", Theme::brand()),
        Span::styled("^I", Theme::shortcut_key()),
        Span::styled(" Prompt  ", Theme::shortcut_desc()),
        Span::styled("^F", Theme::shortcut_key()),
        Span::styled(" Full  ", Theme::shortcut_desc()),
        Span::styled("F6", Theme::shortcut_key()),
        Span::styled(" Split/Tab  ", Theme::shortcut_desc()),
        Span::styled("F8", Theme::shortcut_key()),
        Span::styled(" Mode  ", Theme::shortcut_desc()),
        Span::styled("F10", Theme::shortcut_key()),
        Span::styled(" Exit", Theme::shortcut_desc()),
    ]);
    f.render_widget(Paragraph::new(shortcuts), area);
}

/// Render the VT100 screen grid directly to the terminal area, with scrollback history and text selection support.
pub fn render_terminal_grid(
    f: &mut Frame,
    area: Rect,
    screen: &Screen,
    scroll: usize,
    selection: Option<TextSelection>,
    search_matches: &[crate::tui::state::SearchMatch],
    search_match_index: usize,
    is_visual_mode: bool,
    visual_cursor: (u16, u16),
) {
    let rows_to_draw = min(area.height, screen.rows) as usize;
    let cols_to_draw = min(area.width, screen.cols) as usize;

    let scrollback = screen.scrollback();
    let max_scroll = scrollback.len();
    let effective_scroll = min(scroll, max_scroll);

    let get_match_highlight = |loc: crate::tui::state::SearchMatchLocation, col: u16| -> Option<Style> {
        for (idx, m) in search_matches.iter().enumerate() {
            if m.location == loc && col >= m.start_col && col <= m.end_col {
                if idx == search_match_index {
                    return Some(Style::default().fg(Color::Black).bg(Color::Rgb(255, 180, 0)).add_modifier(Modifier::BOLD));
                } else {
                    return Some(Style::default().fg(Color::Black).bg(Color::Yellow));
                }
            }
        }
        None
    };

    let cursor_pos = if is_visual_mode {
        if visual_cursor.0 < area.height && visual_cursor.1 < area.width {
            Some(Position::new(
                area.x + visual_cursor.1,
                area.y + visual_cursor.0,
            ))
        } else {
            None
        }
    } else if effective_scroll == 0
        && selection.is_none()
        && screen.cursor_visible
        && screen.cursor_col < area.width
        && screen.cursor_row < area.height
    {
        Some(Position::new(
            area.x + screen.cursor_col,
            area.y + screen.cursor_row,
        ))
    } else {
        None
    };

    {
        let buf = f.buffer_mut();

        if effective_scroll == 0 {
            // Direct, live screen rendering
            for r in 0..rows_to_draw {
                for c in 0..cols_to_draw {
                    if let Some(cell) = screen.cell(r as u16, c as u16) {
                        let target = &mut buf[(area.x + c as u16, area.y + r as u16)];
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
                        if cell.attrs.strikethrough {
                            style = style.add_modifier(Modifier::CROSSED_OUT);
                        }
                        let is_selected = selection.map(|s| s.contains(r as u16, c as u16)).unwrap_or(false);
                        if cell.attrs.reverse || is_selected {
                            style = style.add_modifier(Modifier::REVERSED);
                        }
                        if let Some(match_style) = get_match_highlight(crate::tui::state::SearchMatchLocation::Screen(r as u16), c as u16) {
                            style = match_style;
                        } else if is_selected {
                            style = style.fg(Color::White).bg(Color::Blue);
                        } else {
                            if let Some((r, g, b)) = cell.attrs.fg_rgb {
                                style = style.fg(Color::Rgb(r, g, b));
                            } else if let Some(fg) = cell.attrs.fg {
                                style = style.fg(ansi_code_to_color(fg));
                            }
                            if let Some((r, g, b)) = cell.attrs.bg_rgb {
                                style = style.bg(Color::Rgb(r, g, b));
                            } else if let Some(bg) = cell.attrs.bg {
                                style = style.bg(ansi_code_to_color(bg));
                            }
                        }
                        target.set_style(style);
                    }
                }
            }
        } else {
            // Render historical viewport when scrolled up:
            for r in 0..rows_to_draw {
                let is_row_selected = |col: u16| selection.map(|s| s.contains(r as u16, col)).unwrap_or(false);
                let source_line = (r as isize) - (effective_scroll as isize);
                if source_line < 0 {
                    let sb_idx = (scrollback.len() as isize) + source_line;
                    if sb_idx >= 0 && (sb_idx as usize) < scrollback.len() {
                        let line_str = &scrollback[sb_idx as usize];
                        let chars: Vec<char> = line_str.chars().collect();
                        for c in 0..cols_to_draw {
                            let ch = chars.get(c).copied().unwrap_or(' ');
                            let target = &mut buf[(area.x + c as u16, area.y + r as u16)];
                            target.set_char(ch);
                            if let Some(match_style) = get_match_highlight(crate::tui::state::SearchMatchLocation::Scrollback(sb_idx as usize), c as u16) {
                                target.set_style(match_style);
                            } else if is_row_selected(c as u16) {
                                target.set_style(Style::default().fg(Color::White).bg(Color::Blue).add_modifier(Modifier::REVERSED));
                            } else {
                                target.set_style(Style::default().fg(Color::Gray));
                            }
                        }
                    }
                } else {
                    let screen_r = source_line as u16;
                    for c in 0..cols_to_draw {
                        if let Some(cell) = screen.cell(screen_r, c as u16) {
                            let target = &mut buf[(area.x + c as u16, area.y + r as u16)];
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
                            if cell.attrs.strikethrough {
                                style = style.add_modifier(Modifier::CROSSED_OUT);
                            }
                            let is_selected = is_row_selected(c as u16);
                            if cell.attrs.reverse || is_selected {
                                style = style.add_modifier(Modifier::REVERSED);
                            }
                            if let Some(match_style) = get_match_highlight(crate::tui::state::SearchMatchLocation::Screen(screen_r), c as u16) {
                                style = match_style;
                            } else if is_selected {
                                style = style.fg(Color::White).bg(Color::Blue);
                            } else {
                                if let Some((r, g, b)) = cell.attrs.fg_rgb {
                                    style = style.fg(Color::Rgb(r, g, b));
                                } else if let Some(fg) = cell.attrs.fg {
                                    style = style.fg(ansi_code_to_color(fg));
                                }
                                if let Some((r, g, b)) = cell.attrs.bg_rgb {
                                    style = style.bg(Color::Rgb(r, g, b));
                                } else if let Some(bg) = cell.attrs.bg {
                                    style = style.bg(ansi_code_to_color(bg));
                                }
                            }
                            target.set_style(style);
                        }
                    }
                }
            }

            // Scroll indicator badge
            if area.width > 24 {
                let badge = format!(" ▲ SCROLL: -{} | Esc to reset ", effective_scroll);
                let badge_len = badge.len() as u16;
                let start_x = area.x + area.width.saturating_sub(badge_len + 1);
                let style = Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD);
                for (i, ch) in badge.chars().enumerate() {
                    let x = start_x + i as u16;
                    if x < area.x + area.width {
                        let target = &mut buf[(x, area.y)];
                        target.set_char(ch);
                        target.set_style(style);
                    }
                }
            }
        }
    }

    if let Some(pos) = cursor_pos {
        f.set_cursor_position(pos);
    }
}

/// Extract text from highlighted viewport selection for clipboard copying.
pub fn extract_selected_text(
    screen: &Screen,
    scroll: usize,
    _rows: u16,
    cols: u16,
    selection: TextSelection,
) -> String {
    let ((r1, c1), (r2, c2)) = selection.normalized();
    let scrollback = screen.scrollback();
    let effective_scroll = std::cmp::min(scroll, scrollback.len());

    let mut result_lines = Vec::new();

    for r in r1..=r2 {
        let from_c = if r == r1 { c1 } else { 0 };
        let to_c = if r == r2 { c2 } else { cols.saturating_sub(1) };

        let mut line_chars = Vec::new();

        if effective_scroll == 0 {
            for c in from_c..=to_c {
                if let Some(cell) = screen.cell(r, c) {
                    line_chars.push(cell.ch);
                } else {
                    line_chars.push(' ');
                }
            }
        } else {
            let source_line = (r as isize) - (effective_scroll as isize);
            if source_line < 0 {
                let sb_idx = (scrollback.len() as isize) + source_line;
                if sb_idx >= 0 && (sb_idx as usize) < scrollback.len() {
                    let s = &scrollback[sb_idx as usize];
                    let chars: Vec<char> = s.chars().collect();
                    for c in from_c..=to_c {
                        let ch = chars.get(c as usize).copied().unwrap_or(' ');
                        line_chars.push(ch);
                    }
                }
            } else {
                let screen_r = source_line as u16;
                for c in from_c..=to_c {
                    if let Some(cell) = screen.cell(screen_r, c) {
                        line_chars.push(cell.ch);
                    } else {
                        line_chars.push(' ');
                    }
                }
            }
        }

        let line_str: String = line_chars.into_iter().collect();
        result_lines.push(line_str.trim_end().to_string());
    }

    result_lines.join("\n")
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
    let width = 72.min(area.width);
    let height = (state.detected_agents.len() as u16 + 8).min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::CYAN))
        .title(" Switch Active Coding Agent (F1) ")
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
    lines.push(Line::raw(""));
    for (idx, agent) in state.detected_agents.iter().enumerate() {
        let is_selected = idx == state.agent_switch_index;
        let pointer = if is_selected { " ▶ " } else { "   " };
        let is_active = state.active_agent.as_ref().map(|a| a.id == agent.id).unwrap_or(false);

        let (bullet, bullet_color) = if is_active && state.agent_session_running {
            ("●", Theme::EMERALD)
        } else if agent.is_ready() {
            ("○", Theme::CYAN)
        } else {
            ("○", Theme::TEXT_MUTED)
        };

        let (status_str, status_color) = if is_active && state.agent_session_running {
            ("[Ready - PTY Active]", Theme::EMERALD)
        } else if agent.is_ready() {
            ("[Standby]", Theme::CYAN)
        } else {
            ("[Unsupported]", Theme::TEXT_MUTED)
        };

        lines.push(Line::from(vec![
            Span::styled(pointer, if is_selected { Theme::accent() } else { Theme::text_muted() }),
            Span::styled(format!("{bullet} "), Style::default().fg(bullet_color).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{:<26}", format!("{} ({})", agent.display_name, agent.executable)),
                if is_selected {
                    Theme::text_bold().add_modifier(Modifier::UNDERLINED)
                } else {
                    Theme::text_normal()
                },
            ),
            Span::styled(format!(" {:<20}", status_str), Style::default().fg(status_color)),
        ]));
    }
    f.render_widget(Paragraph::new(lines), chunks[0]);

    let hints = Line::from(vec![
        Span::styled("Enter: ", Theme::shortcut_key()),
        Span::styled("Switch Agent   │   ", Theme::shortcut_desc()),
        Span::styled("R: ", Theme::shortcut_key()),
        Span::styled("Rescan PATH   │   ", Theme::shortcut_desc()),
        Span::styled("Esc: ", Theme::shortcut_key()),
        Span::styled("Back", Theme::shortcut_desc()),
    ]);
    f.render_widget(Paragraph::new(hints).alignment(Alignment::Center), chunks[1]);
}

pub fn render_project_select_overlay(f: &mut Frame, area: Rect, state: &TuiState) {
    let width = 72.min(area.width);
    let height = 14.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::CYAN))
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
            Constraint::Length(1), // Current
            Constraint::Length(1), // New: <input>
            Constraint::Length(1), // Separator header
            Constraint::Min(3),    // Recent list
            Constraint::Length(1), // Hints
        ])
        .split(inner);

    let current = Line::from(vec![
        Span::styled("Current: ", Theme::text_muted()),
        Span::styled(state.workspace_dir.to_string_lossy(), Theme::text_normal()),
    ]);
    f.render_widget(Paragraph::new(current), chunks[0]);

    let input_line = Line::from(vec![
        Span::styled("New:     ", Theme::brand()),
        Span::styled(&state.project_input_text, Theme::text_bold()),
        Span::styled("█", Theme::accent()),
    ]);
    f.render_widget(Paragraph::new(input_line), chunks[1]);

    let mut list_lines = Vec::new();
    if !state.project_completions.is_empty() {
        let header = Line::from(Span::styled(
            format!("─── Directory Matches ({} found, Tab to cycle) ───", state.project_completions.len()),
            Theme::brand(),
        ));
        f.render_widget(Paragraph::new(header), chunks[2]);

        for (i, p) in state.project_completions.iter().take(4).enumerate() {
            let is_selected = i == state.project_completion_index;
            let pointer = if is_selected { " ▶ " } else { "   " };
            let style = if is_selected {
                Theme::accent().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Theme::text_normal()
            };
            list_lines.push(Line::from(vec![
                Span::styled(pointer, if is_selected { Theme::accent() } else { Theme::text_muted() }),
                Span::styled(p.as_str(), style),
            ]));
        }
        if state.project_completions.len() > 4 {
            list_lines.push(Line::from(Span::styled(
                format!("   ... and {} more (press Tab to cycle)", state.project_completions.len() - 4),
                Theme::text_muted(),
            )));
        }
    } else {
        let header = Line::from(Span::styled("─── Recent / Quick Workspaces (↑/↓ to pick) ───", Theme::text_muted()));
        f.render_widget(Paragraph::new(header), chunks[2]);

        if state.recent_workspaces.is_empty() {
            list_lines.push(Line::from(Span::styled("  (No recent workspaces recorded - type path or press Tab)", Theme::text_muted())));
        } else {
            for (i, p) in state.recent_workspaces.iter().take(4).enumerate() {
                let is_selected = i == state.project_select_index;
                let pointer = if is_selected { " ▶ " } else { "   " };
                let style = if is_selected {
                    Theme::text_bold().add_modifier(Modifier::UNDERLINED)
                } else {
                    Theme::text_normal()
                };
                list_lines.push(Line::from(vec![
                    Span::styled(pointer, if is_selected { Theme::accent() } else { Theme::text_muted() }),
                    Span::styled(format!("{}. ", i + 1), Theme::shortcut_key()),
                    Span::styled(p.to_string_lossy(), style),
                ]));
            }
        }
    }
    f.render_widget(Paragraph::new(list_lines), chunks[3]);

    let hints = Line::from(vec![
        Span::styled("Enter", Theme::shortcut_key()),
        Span::styled(" Confirm   ", Theme::shortcut_desc()),
        Span::styled("Tab", Theme::shortcut_key()),
        Span::styled(" Autocomplete   ", Theme::shortcut_desc()),
        Span::styled("↑/↓", Theme::shortcut_key()),
        Span::styled(" Recent   ", Theme::shortcut_desc()),
        Span::styled("Esc", Theme::shortcut_key()),
        Span::styled(" Cancel", Theme::shortcut_desc()),
    ]);
    f.render_widget(Paragraph::new(hints).alignment(Alignment::Center), chunks[4]);
}

fn render_attention_overlay(f: &mut Frame, area: Rect, state: &TuiState) {
    let width = 78.min(area.width);
    let height = 20.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::AMBER))
        .title(" ATTENTION & APPROVALS (F3) ")
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
    let working = state.working();
    let completed = state.completed();

    let mut lines = Vec::new();
    if requests.is_empty() && errors.is_empty() && working.is_empty() && completed.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "✓ No active attention items or background tasks.",
            Theme::primary(),
        )));
    } else {
        if !requests.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("▶ [!] REQUESTS & APPROVALS ", Theme::warning()),
                Span::styled(format!("({} PENDING)", requests.len()), Theme::text_bold()),
            ]));
            for (_i, req) in requests.iter().take(3).enumerate() {
                lines.push(Line::from(vec![
                    Span::styled("  └─ ", Theme::text_muted()),
                    Span::styled(format!("[{}] ", req.1.agent_id), Theme::accent()),
                    Span::styled(&req.1.summary, Theme::text_bold()),
                ]));
                if let Some(ref r_info) = req.1.request {
                    lines.push(Line::from(vec![
                        Span::styled("     Type: ", Theme::text_muted()),
                        Span::styled(format!("{:?} ", r_info.question_type), Theme::accent()),
                        Span::styled(" │ [Y] Quick Approve  [N] Deny  [Enter] Inspect", Theme::text_secondary()),
                    ]));
                }
            }
            lines.push(Line::raw(""));
        }

        if !errors.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  [!] ERRORS ", Theme::error()),
                Span::styled(format!("({} ITEMS)", errors.len()), Theme::text_bold()),
            ]));
            for (_i, err) in errors.iter().take(2).enumerate() {
                lines.push(Line::from(vec![
                    Span::styled("  └─ ", Theme::text_muted()),
                    Span::styled(&err.1.summary, Theme::text_normal()),
                ]));
            }
            lines.push(Line::raw(""));
        }

        if !working.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  [●] ACTIVE WORKING ", Theme::accent()),
                Span::styled(format!("({} TASKS)", working.len()), Theme::text_bold()),
            ]));
            for (_i, work) in working.iter().take(2).enumerate() {
                lines.push(Line::from(vec![
                    Span::styled("  └─ ", Theme::text_muted()),
                    Span::styled(format!("[{}] ", work.1.agent_id), Theme::accent()),
                    Span::styled(&work.1.summary, Theme::text_normal()),
                ]));
            }
            lines.push(Line::raw(""));
        }

        if !completed.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  [✓] COMPLETED RECENTLY ", Theme::primary()),
                Span::styled(format!("({} TASKS)", completed.len()), Theme::text_bold()),
            ]));
            for (_i, comp) in completed.iter().take(2).enumerate() {
                lines.push(Line::from(vec![
                    Span::styled("  └─ ", Theme::text_muted()),
                    Span::styled(format!("[{}] ", comp.1.agent_id), Theme::primary()),
                    Span::styled(&comp.1.summary, Theme::text_muted()),
                ]));
            }
        }
    }
    f.render_widget(Paragraph::new(lines), chunks[0]);

    let hints = Line::from(vec![
        Span::styled("↑/↓: ", Theme::shortcut_key()),
        Span::styled("Navigate   │   ", Theme::shortcut_desc()),
        Span::styled("Enter: ", Theme::shortcut_key()),
        Span::styled("Open   │   ", Theme::shortcut_desc()),
        Span::styled("Y/N: ", Theme::shortcut_key()),
        Span::styled("Quick Action   │   ", Theme::shortcut_desc()),
        Span::styled("Esc: ", Theme::shortcut_key()),
        Span::styled("Dismiss", Theme::shortcut_desc()),
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
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::RED))
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
            Theme::warning(),
        )),
        Line::from(Span::styled("Terminate this session and switch to the new agent?", Theme::text_normal())),
        Line::raw(""),
        Line::from(vec![
            Span::styled("y", Theme::error()),
            Span::styled(" Yes, Terminate and Switch   ", Theme::shortcut_desc()),
            Span::styled("n / Esc", Theme::primary()),
            Span::styled(" Keep Current", Theme::shortcut_desc()),
        ]),
    ];

    f.render_widget(Paragraph::new(text).alignment(Alignment::Center), inner);
}

fn render_menu_overlay(f: &mut Frame, area: Rect, state: &TuiState) {
    let width = 76.min(area.width);
    let height = 24.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::CYAN).add_modifier(Modifier::BOLD))
        .title(" AGENTDESK MENU (F4) ")
        .title_alignment(Alignment::Center);
    f.render_widget(block, modal_area);

    let inner = Rect::new(
        modal_area.x + 2,
        modal_area.y + 1,
        modal_area.width.saturating_sub(4),
        modal_area.height.saturating_sub(2),
    );

    let mode_desc = if state.dangerously_skip_permissions {
        "Toggle mode (currently: ⚡ --dangerously-skip-permissions enabled)"
    } else {
        "Toggle mode (currently: 🛡 Default safe permissions mode)"
    };

    let menu_items = [
        ("1. Pair Phone (Render QR Code)", "Generate QR code to pair your phone for remote supervision"),
        ("2. Paired Devices", "View paired mobile devices or revoke access"),
        ("3. Notifications & Attention (F3)", "View, inspect, approve, or clear pending attention items"),
        ("4. Connection & Relay Settings", "Inspect daemon port, loopback bind, and relay URL"),
        ("5. Switch Coding Agent (F1)", "Change active coding agent (Antigravity, Gemini, etc.)"),
        ("6. Change Workspace Directory (F2)", "Switch or choose from recent workspace directories"),
        ("7. Toggle Execution Mode (F8)", mode_desc),
        ("8. Exit AgentDesk (F10)", "Shut down the terminal workspace and daemon"),
    ];

    let mut lines = Vec::new();
    lines.push(Line::raw(""));

    for (i, (title, desc)) in menu_items.iter().enumerate() {
        let is_selected = state.config_menu_index == i;
        let pointer = if is_selected { " ▶ " } else { "   " };
        let bullet = if is_selected { "● " } else { "○ " };
        let bullet_color = if is_selected { Theme::EMERALD } else { Theme::TEXT_MUTED };
        let title_style = if is_selected {
            Theme::text_bold().add_modifier(Modifier::UNDERLINED)
        } else {
            Theme::text_normal()
        };

        lines.push(Line::from(vec![
            Span::styled(pointer, if is_selected { Theme::accent() } else { Theme::text_muted() }),
            Span::styled(bullet, Style::default().fg(bullet_color).add_modifier(Modifier::BOLD)),
            Span::styled(*title, title_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled(format!("       {desc}"), Theme::text_secondary()),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("↑/↓: ", Theme::shortcut_key()),
        Span::styled("Navigate   │   ", Theme::shortcut_desc()),
        Span::styled("Enter / 1-8: ", Theme::shortcut_key()),
        Span::styled("Select   │   ", Theme::shortcut_desc()),
        Span::styled("Esc: ", Theme::shortcut_key()),
        Span::styled("Close", Theme::shortcut_desc()),
    ]));

    f.render_widget(Paragraph::new(lines), inner);
}

fn render_prompt_input_overlay(f: &mut Frame, area: Rect, state: &TuiState) {
    // Render at bottom-center of the workspace area
    let overlay_height = 6u16;
    let overlay_width = area.width.saturating_sub(4).min(100);
    let overlay_x = area.x + (area.width.saturating_sub(overlay_width)) / 2;
    let overlay_y = area.y + area.height.saturating_sub(overlay_height + 1);
    let overlay_area = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

    f.render_widget(Clear, overlay_area);

    let nav_hint = if state.prompt_history.index.is_some() {
        let idx = state.prompt_history.index.unwrap() + 1;
        let total = state.prompt_history.entries.len();
        format!(" ↑↓ hist [{}/{}] ", idx, total)
    } else if state.prompt_history.entries.is_empty() {
        String::new()
    } else {
        format!(" ↑ hist ({}) ", state.prompt_history.entries.len())
    };

    let mode_badge = if state.dangerously_skip_permissions {
        " [Mode: ⚡ --dangerously-skip-permissions] "
    } else {
        " [Mode: 🛡 Default] "
    };

    let title = format!("◈ Send Prompt{}{}", nav_hint, mode_badge);
    let border_color = if state.dangerously_skip_permissions { Theme::AMBER } else { Theme::CYAN };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(overlay_area);
    f.render_widget(block, overlay_area);

    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(2),
            Constraint::Length(1),
        ])
        .split(inner);

    // Display input text or placeholder hint
    let display_text = if state.prompt_input.is_empty() {
        Span::styled(
            "Type your prompt… (Enter: send, Tab/F8: toggle mode, Esc: cancel)",
            Theme::text_muted(),
        )
    } else {
        Span::styled(&state.prompt_input, Theme::text_bold())
    };

    let paragraph = Paragraph::new(Line::from(vec![display_text]))
        .wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(paragraph, inner_chunks[0]);

    // Workspace target directory & shortcut hint
    let ws_path = state.workspace_dir.to_string_lossy();
    let dir_line = Line::from(vec![
        Span::styled("📁 Target dir: ", Theme::text_muted()),
        Span::styled(ws_path, Theme::primary()),
        Span::styled("   (Tab/F8: mode, F2: change dir)", Theme::text_secondary()),
    ]);
    f.render_widget(Paragraph::new(dir_line), inner_chunks[1]);

    // Show cursor at end of text (approximate, wraps by inner_chunks[0].width columns)
    let text_len = state.prompt_input.len() as u16;
    let col_width = inner_chunks[0].width.max(1);
    let cursor_x = inner_chunks[0].x + (text_len % col_width);
    let cursor_y = inner_chunks[0].y + (text_len / col_width);
    let cursor_x = cursor_x.min(inner_chunks[0].x + inner_chunks[0].width.saturating_sub(1));
    let cursor_y = cursor_y.min(inner_chunks[0].y + inner_chunks[0].height.saturating_sub(1));
    f.set_cursor_position(Position::new(cursor_x, cursor_y));
}

fn render_search_overlay(f: &mut Frame, area: Rect, state: &TuiState) {
    let bar_height = 1;
    let bar_area = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(bar_height),
        area.width,
        bar_height,
    );

    f.render_widget(Clear, bar_area);

    let match_info = if state.search_query.is_empty() {
        Span::styled("", Style::default())
    } else if state.search_matches.is_empty() {
        Span::styled(" [0 matches]", Theme::error())
    } else {
        Span::styled(
            format!(" [{}/{} matches]", state.search_match_index + 1, state.search_matches.len()),
            Theme::primary(),
        )
    };

    let line = Line::from(vec![
        Span::styled(" / ", Theme::accent().add_modifier(Modifier::BOLD)),
        Span::styled(&state.search_query, Theme::text_bold()),
        Span::styled("█ ", Theme::accent()),
        match_info,
        Span::styled("  │  n: Next  N: Prev  Esc: Close", Theme::text_muted()),
    ]);

    let p = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    f.render_widget(p, bar_area);
}

fn render_url_picker_overlay(f: &mut Frame, area: Rect, state: &TuiState) {
    let width = 76.min(area.width);
    let height = (state.detected_urls.len() as u16 + 6).min(16).min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::CYAN))
        .title(" Interactive URL Picker (o / Ctrl+O) ")
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
            Constraint::Min(2),
            Constraint::Length(1),
        ])
        .split(inner);

    if state.detected_urls.is_empty() {
        let p = Paragraph::new(vec![
            Line::raw(""),
            Line::from(Span::styled("No HTTP/HTTPS or hyperlinks detected in terminal scrollback.", Theme::text_muted())),
        ]);
        f.render_widget(p, chunks[0]);
    } else {
        let mut lines = Vec::new();
        lines.push(Line::raw(""));
        for (i, url) in state.detected_urls.iter().enumerate().take(8) {
            let is_selected = i == state.url_picker_index;
            let pointer = if is_selected { " ▶ " } else { "   " };
            let style = if is_selected {
                Theme::accent().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Theme::text_normal()
            };
            lines.push(Line::from(vec![
                Span::styled(pointer, if is_selected { Theme::accent() } else { Theme::text_muted() }),
                Span::styled(url.as_str(), style),
            ]));
        }
        f.render_widget(Paragraph::new(lines), chunks[0]);
    }

    let hints = Line::from(vec![
        Span::styled("Enter", Theme::shortcut_key()),
        Span::styled(" Open in Browser   │   ", Theme::shortcut_desc()),
        Span::styled("c / y", Theme::shortcut_key()),
        Span::styled(" Copy URL   │   ", Theme::shortcut_desc()),
        Span::styled("Esc", Theme::shortcut_key()),
        Span::styled(" Close", Theme::shortcut_desc()),
    ]);
    f.render_widget(Paragraph::new(hints).alignment(Alignment::Center), chunks[1]);
}
