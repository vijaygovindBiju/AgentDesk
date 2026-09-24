//! Pairing flow view.
//! Generates and displays the terminal QR code for instant Android phone pairing.
//! Uses existing PairingManager and renders Unicode block QR code.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::tui::state::TuiState;

pub fn render_pairing(f: &mut Frame, area: Rect, state: &TuiState) {
    let qr_line_count = state.pairing_qr.as_ref().map(|q| q.lines().count()).unwrap_or(22);

    // Success screen
    if let Some((ref dev_name, ref dev_id)) = state.pairing_paired_device {
        let popup_width = 70.min(area.width.saturating_sub(4));
        let popup_height = 12.min(area.height.saturating_sub(2));
        let popup_area = Rect {
            x: (area.width.saturating_sub(popup_width)) / 2,
            y: (area.height.saturating_sub(popup_height)) / 2,
            width: popup_width,
            height: popup_height,
        };
        f.render_widget(Clear, popup_area);
        let block = Block::default()
            .title(" PAIRING SUCCESS ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Green));
        let inner = block.inner(popup_area);
        f.render_widget(block, popup_area);

        let mut lines = Vec::new();
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled("  ✓ SUCCESS! Phone paired successfully!", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled("  Device Name: ", Style::default().fg(Color::DarkGray)),
            Span::styled(dev_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Device ID:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(dev_id, Style::default().fg(Color::Cyan)),
        ]));
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled("  Remote supervision is now active. Press [Esc] to continue.", Style::default().fg(Color::White)),
        ]));
        f.render_widget(Paragraph::new(lines), inner);
        return;
    }

    let compact = area.height < (qr_line_count as u16 + 5);

    if compact {
        // Compact full-area rendering: zero border loss, all QR lines guaranteed visible
        f.render_widget(Clear, area);
        let mut lines = Vec::new();

        lines.push(Line::from(vec![
            Span::styled(
                "── PAIR AGENTDESK PHONE [Relay Only] ──",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
        ]));

        if let Some(ref qr_str) = state.pairing_qr {
            for qline in qr_str.lines() {
                lines.push(Line::from(vec![
                    Span::styled(qline, Style::default().fg(Color::White)),
                ]));
            }
        } else {
            lines.push(Line::from(vec![
                Span::styled("Generating pairing session and QR code...", Style::default().fg(Color::Yellow)),
            ]));
        }

        if area.height >= lines.len() as u16 + 1 {
            lines.push(Line::from(vec![
                Span::styled("Scan with AgentDesk app | Esc to close", Style::default().fg(Color::DarkGray)),
            ]));
        }

        if let Some(ref err) = state.pairing_error {
            lines.push(Line::from(vec![
                Span::styled(format!("Error: {err}"), Style::default().fg(Color::Red)),
            ]));
        }

        f.render_widget(Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center), area);
    } else {
        // Standard modal with borders when screen has ample height
        let popup_height = (qr_line_count as u16 + 8).min(area.height.saturating_sub(1));
        let popup_width = 76.min(area.width.saturating_sub(2));

        let popup_area = Rect {
            x: (area.width.saturating_sub(popup_width)) / 2,
            y: (area.height.saturating_sub(popup_height)) / 2,
            width: popup_width,
            height: popup_height,
        };

        f.render_widget(Clear, popup_area);

        let block = Block::default()
            .title(" PAIR AGENTDESK SUPERVISION PHONE ")
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(popup_area);
        f.render_widget(block, popup_area);

        let mut lines = Vec::new();

        let relay_display = state.relay_url.as_deref().unwrap_or(crate::config::DEFAULT_RELAY_URL);
        lines.push(Line::from(vec![
            Span::styled("Connection: ", Style::default().fg(Color::DarkGray)),
            Span::styled("Public Relay (Outbound WSS/TLS)", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(format!("({relay_display})"), Style::default().fg(Color::White)),
        ]));
        lines.push(Line::raw(""));

        if let Some(ref qr_str) = state.pairing_qr {
            for qline in qr_str.lines() {
                lines.push(Line::from(vec![
                    Span::styled(qline, Style::default().fg(Color::White)),
                ]));
            }

            lines.push(Line::raw(""));
            if let Some(ref inst) = state.pairing_instance_id {
                lines.push(Line::from(vec![
                    Span::styled("  Instance: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(inst, Style::default().fg(Color::Cyan)),
                ]));
            }
            lines.push(Line::from(vec![
                Span::styled("  Scan with AgentDesk Android app. Expires in 3 minutes.", Style::default().fg(Color::DarkGray)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("  Generating pairing session and QR code...", Style::default().fg(Color::Yellow)),
            ]));
        }

        if let Some(ref err) = state.pairing_error {
            lines.push(Line::from(vec![
                Span::styled(format!("  Error: {err}"), Style::default().fg(Color::Red)),
            ]));
        }

        f.render_widget(Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center), inner);
    }
}
