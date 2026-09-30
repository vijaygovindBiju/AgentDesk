# AgentDesk — Agent Notes

## Verification

- Rust: `cd core && cargo test --workspace --no-fail-fast`
  - `tui::terminal_session::tests::test_spawn_shell_and_pump_output` spawns `$SHELL`; it fails under fish. Run with `SHELL=/bin/bash` if needed.
- Flutter: `cd mobile && dart analyze lib && flutter test`

## Architecture invariants

- Relay is the ONLY remote transport. The laptop daemon binds `127.0.0.1` only (enforced in `token::validate_bind_security`); no LAN/USB/ADB runtime paths, no LAN fallback.
- Relay URL is first-class config (`agentdesk relay set <wss-url>` → `AGENTDESK_RELAY` in `service.env`); validated by `config::validate_relay_url` (Rust) and `mobile/lib/services/relay_url.dart`.
- Laptop authenticates to the relay with `relay_host.key`, never the master token.
- Mobile `ConnectionService(allowLoopbackTestServer: true)` is a test-harness-only direct loopback mode; the app never enables it.
- Different-networks E2E procedure: `docs/REMOTE_E2E_TEST.md`.
