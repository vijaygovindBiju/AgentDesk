//! Connection Details view.
//! Displays remote relay status, local bind, TLS fingerprint, and active connection metrics.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::state::TuiState;
use crate::tui::theme::Theme;

pub fn render_connection(f: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" CONNECTION & SUPERVISION DETAILS ")
        .title_style(Theme::brand())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(Theme::CYAN));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let phone_status_line = if state.phone_connected {
        Line::from(vec![
            Span::styled("Remote Status:     ", Theme::text_muted()),
            Span::styled("● PHONE CONNECTED", Theme::primary()),
            Span::styled(
                format!(" ({})", state.phone_transport.as_deref().unwrap_or("Relay")),
                Theme::accent(),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("Remote Status:     ", Theme::text_muted()),
            Span::styled("○ PHONE OFFLINE", Theme::text_muted()),
            Span::styled(" (waiting for mobile client to connect)", Theme::text_muted()),
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

    let mut relay_line = vec![Span::styled("Relay Trunk:       ", Theme::text_muted())];
    relay_line.extend(crate::tui::views::header::relay_status_spans(state));

    let lines = vec![
        Line::from(relay_line),
        phone_status_line,
        Line::raw(""),
        Line::from(vec![
            Span::styled("Public Relay:      ", Theme::text_muted()),
            Span::styled(relay_str, Theme::brand()),
            Span::styled(" (Outbound WSS/TLS)", Theme::text_secondary()),
        ]),
        Line::from(vec![
            Span::styled("Architecture:      ", Theme::text_muted()),
            Span::styled("Relay-Only (Outbound WSS; No Inbound Ports)", Theme::primary()),
        ]),
        Line::from(vec![
            Span::styled("Local IPC:         ", Theme::text_muted()),
            Span::styled(
                format!("127.0.0.1:{} (Loopback IPC only)", state.daemon_port),
                Theme::text_normal(),
            ),
        ]),
        Line::from(vec![
            Span::styled("TLS Fingerprint:   ", Theme::text_muted()),
            Span::styled(fp_str, Theme::text_normal()),
        ]),
        Line::from(vec![
            Span::styled("Device Name:       ", Theme::text_muted()),
            Span::styled(
                state.phone_device_name.as_deref().unwrap_or("None active"),
                Theme::text_normal(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Last Seen:         ", Theme::text_muted()),
            Span::styled(last_seen_str, Theme::text_secondary()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Security Model:    ", Theme::text_muted()),
            Span::styled(
                "Zero-Trust Token Auth + End-to-End Pinning (Unstructured PTY text retained laptop-side only)",
                Theme::text_secondary(),
            ),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Press [P] to open pairing QR code. Press [Esc] to return.", Theme::shortcut_desc()),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}
