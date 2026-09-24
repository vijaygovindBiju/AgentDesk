//! Devices view.
//! Displays paired devices for remote supervision, platform, connection type, and status.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use crate::tui::state::TuiState;

pub fn render_devices(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" SUPERVISION DEVICES ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));

    if state.paired_devices.is_empty() {
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  ○ ", Style::default().fg(Color::DarkGray)),
                Span::styled("No paired mobile devices found.", Style::default().fg(Color::White)),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("  Press ", Style::default().fg(Color::DarkGray)),
                Span::styled("[P]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" to generate a QR code and pair an Android phone.", Style::default().fg(Color::DarkGray)),
            ]),
        ];
        f.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let header_cells = ["DEVICE NAME", "PLATFORM", "CONNECTION", "STATUS", "PAIRED AT"]
        .into_iter()
        .map(|h| Span::styled(h, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows: Vec<Row> = state
        .paired_devices
        .iter()
        .enumerate()
        .map(|(idx, dev)| {
            let is_revoked = dev.revoked;
            let is_connected = state.phone_connected
                && state.phone_device_name.as_deref().map(|n| n == dev.device_name || n == dev.device_id).unwrap_or(false);

            let (status_text, status_style) = if is_revoked {
                ("○ Revoked", Style::default().fg(Color::Red))
            } else if is_connected {
                ("● Connected", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            } else {
                ("○ Offline", Style::default().fg(Color::DarkGray))
            };

            let conn_type = if let Some(ref t) = state.phone_transport {
                if is_connected {
                    t.clone()
                } else {
                    "Relay/WSS".into()
                }
            } else {
                "Relay/WSS".into()
            };

            let row_style = if idx == state.devices_index {
                Style::default().bg(Color::Rgb(20, 35, 50)).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            Row::new(vec![
                Span::styled(dev.device_name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled("Android", Style::default().fg(Color::DarkGray)),
                Span::styled(conn_type, Style::default().fg(Color::Cyan)),
                Span::styled(status_text, status_style),
                Span::styled(
                    dev.paired_at.format("%Y-%m-%d %H:%M").to_string(),
                    Style::default().fg(Color::DarkGray),
                ),
            ])
            .style(row_style)
            .height(1)
        })
        .collect();

    let widths = [
        Constraint::Length(25),
        Constraint::Length(14),
        Constraint::Length(18),
        Constraint::Length(16),
        Constraint::Min(20),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(block);

    f.render_widget(table, area);
}
