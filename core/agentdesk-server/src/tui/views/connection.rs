//! Connection Details view.
//! Displays remote relay status, local bind, TLS fingerprint, and active connection metrics.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::state::TuiState;

pub fn render_connection(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" CONNECTION & SUPERVISION DETAILS ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let phone_status_line = if state.phone_connected {
        Line::from(vec![
            Span::styled("Remote Status:     ", Style::default().fg(Color::DarkGray)),
            Span::styled("● PHONE CONNECTED", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!(" ({})", state.phone_transport.as_deref().unwrap_or("Relay")),
                Style::default().fg(Color::Cyan),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("Remote Status:     ", Style::default().fg(Color::DarkGray)),
            Span::styled("○ PHONE OFFLINE", Style::default().fg(Color::DarkGray)),
            Span::styled(" (waiting for mobile client to connect)", Style::default().fg(Color::DarkGray)),
        ])
    };

    let relay_str = state
        .relay_url
        .as_deref()
        .unwrap_or("not configured (agentdesk relay set wss://relay.example.com)");

    let fp_str = state
        .fingerprint
        .as_deref()
        .unwrap_or("Auto-generated self-signed identity active");

    let last_seen_str = if let Some(ls) = state.phone_last_seen {
        ls.format("%Y-%m-%d %H:%M:%S UTC").to_string()
    } else {
        "Never in this session".to_string()
    };

    let mut relay_line = vec![Span::styled("Relay Trunk:       ", Style::default().fg(Color::DarkGray))];
    relay_line.extend(crate::tui::views::header::relay_status_spans(state));

    let lines = vec![
        Line::from(relay_line),
        phone_status_line,
        Line::raw(""),
        Line::from(vec![
            Span::styled("Public Relay:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(relay_str, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(" (Outbound WSS/TLS)", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled("Architecture:      ", Style::default().fg(Color::DarkGray)),
            Span::styled("Relay-Only (Outbound WSS; No Inbound Ports; No LAN/USB)", Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("Local IPC:         ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("127.0.0.1:{} (Loopback IPC only)", state.daemon_port),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("TLS Fingerprint:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(fp_str, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Device Name:       ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                state.phone_device_name.as_deref().unwrap_or("None active"),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Last Seen:         ", Style::default().fg(Color::DarkGray)),
            Span::styled(last_seen_str, Style::default().fg(Color::DarkGray)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Security Model:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "Zero-Trust Token Auth + End-to-End Pinning (Unstructured PTY text retained laptop-side only)",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Press [P] to open pairing QR code. Press [Esc] to return.", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}
