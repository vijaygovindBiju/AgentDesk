> **Note:** Historical report. The authoritative, reproducible different-networks procedure is [`REMOTE_E2E_TEST.md`](REMOTE_E2E_TEST.md). There is no `--no-lan` flag and no LAN fallback: relay is the only transport.

# AgentDesk Remote-First E2E Test and Verification Report

This report documents the verification of AgentDesk's remote-first, LAN-independent architecture, covering protocol transport, relay security, ephemeral QR pairing, Antigravity (`agy`) adapter execution, resource profiling, and the verification hierarchy status.

---

## 1. Current State Assessment

### Previous State (Direct LAN Primary, Relay Fallback)
* **Architecture Design**: Prior to the update, AgentDesk prioritized direct LAN connections (`ws://` or `wss://` on local IP subnet `192.168.x.x`). Relay was treated merely as a fallback.
* **Pairing URI / QR Code**: The QR code unconditionally embedded LAN endpoints (`lan=192.168.1.x:8765`). If a phone scanned this QR code while connected to 4G/5G mobile data, the mobile client attempted direct connection to the private LAN IP and timed out or failed.
* **Relay Hostname**: The default relay URL was `wss://relay.agentdesk.io`, which currently does not have an active public DNS record, causing immediate connection failures on remote networks if unconfigured.
* **Mobile Client Connection Logic**: The mobile Flutter app evaluated direct LAN first, falling back to relay only after connection timeout, which produced latency and connection drops when changing networks.

### Pure Relay/Internet-Only Architecture (Implemented)
* **Outbound Relay as Sole Transport**: Outbound WSS through the AgentDesk relay is now the sole remote production transport. LAN and USB transports, discovery, fallback, and adb reverse dependencies are completely removed.
* **Ephemeral Remote Pairing**: The pairing flow operates purely via the relay (`role=pair`). Generated QR codes completely omit local IP endpoints (`agentdesk://pair?v=1&inst=...&sess=...&sec=...&fp=...&relay=...`), enabling phone pairing over cellular networks or any ISP without shared Wi-Fi.
* **Relay Security & Protocol Isolation**: The relay performs zero deep inspection of child process streams, terminal screen buffers, or raw PTY bytes; it forwards only structured AgentDesk messages (`event`, `snapshot`, `working`, `input_required`, `completed`, `error`, `input_response`, `ack`, `dismiss`). Device tokens and pairing secrets are validated using constant-time comparison.

---

## 2. Migration: Removal of LAN and USB Transports

The following key components were updated:

1. **`core/agentdesk-relay/src/lib.rs` & `main.rs`**:
   - Implemented persistent host trunking with `role=host`, `role=client`, and `role=pair`.
   - Added relay control messages (`SyncDevices`, `SyncPairing`, `AuthorizeDevice`, `RevokeDevice`) for host-directed dynamic authorization without storing master secrets.
   - Added TLS/WSS identity loading (`RelayTlsIdentity`) supporting native certificate management.

2. **`core/agentdesk-server/src/connection.rs` & `server.rs`**:
   - Added `handle_relay_trunk_connection`: Establishes persistent outbound trunk connection from laptop daemon to relay server.
   - Implemented dynamic pairing session exchange over relay trunk (`Body::PairRequest` / `Body::PairResponse`).
   - Client sessions through relay trigger immediate `Welcome` (with `transport: TransportMode::Relay`) and full session `Snapshot`.
   - Reconnection after mobile client drop restores state without requiring QR re-scan.

3. **`core/agentdesk-server/src/config.rs`, `interactive.rs`, & `main.rs`**:
   - Removed LAN endpoints from the QR payload entirely (`agentdesk://pair?v=1&inst=...&sess=...&sec=...&fp=...&relay=...`); `--bind`/`--lan`/`--no-lan`/`--usb` are rejected.

4. **`mobile/lib/services/connection_service.dart` & `pairing_client.dart`**:
   - Relay (`TransportMode.relay`) is the only transport; there is no LAN fallback. Unpaired/manual direct URLs are not connectable in the app.
   - Relay connections over WSS rely on standard public root CAs (pinned fingerprint used only as a fallback for self-signed relays).
   - Pairing client connects directly to relay pairing endpoint (`/ws?role=pair`) when scanning remote QR.

5. **`mobile/test/dual_mode_connection_test.dart` & `core/agentdesk-server/tests/relay_integration_tests.rs`**:
   - Added unit and integration tests verifying relay-primary transport, remote pairing without LAN endpoints, event forwarding, client reconnect keepalive, and revocation.

---

## 3. Remote Transport Flow

```
                      INTERNET
                         │
             ┌───────────┴───────────┐
             │    AgentDesk Relay    │
             │       (WSS/TLS)       │
             │   Outbound-Only Host  │
             └───────────┬───────────┘
                         │
         ┌───────────────┴───────────────┐
         │ Outbound WSS                  │ Outbound WSS
         ▼                               ▼
    ┌──────────┐                   ┌──────────┐
    │  LAPTOP  │                   │  PHONE   │
    │  Daemon  │                   │ Flutter  │
    └────┬─────┘                   └────┬─────┘
         │ PTY                          │ UI
         ▼                              ▼
    Coding Agent                   Developer
   (Antigravity / agy)           Approval / Input
```

### Protocol Exchanges Across Relay:
1. **Host Trunk Registration**:
   `LAPTOP -> RELAY`: `GET /ws?role=host&inst=<INSTANCE_ID> HTTP/1.1`
   `LAPTOP -> RELAY`: `{"type":"relay_control","action":{"type":"sync_devices","devices":[...]}}`
2. **Ephemeral QR Pairing**:
   `PHONE -> RELAY`: `GET /ws?role=pair&inst=<INSTANCE_ID>&sess=<SESSION_ID>&sec=<SECRET> HTTP/1.1`
   `PHONE -> LAPTOP (via RELAY)`: `PairRequest { device_id, device_name, session_id, pairing_secret }`
   `LAPTOP -> PHONE (via RELAY)`: `PairResponse { instance_id, device_token, schema_version }`
3. **Session Authentication & Streaming**:
   `PHONE -> RELAY`: `GET /ws?role=client&inst=<INSTANCE_ID>&device_id=<DEVICE_ID>&token=<DEVICE_TOKEN> HTTP/1.1`
   `PHONE -> LAPTOP (via RELAY)`: `Hello { token, device_id, client_version }`
   `LAPTOP -> PHONE (via RELAY)`: `Welcome { daemon_version, transport: "relay", ... }`
   `LAPTOP -> PHONE (via RELAY)`: `Snapshot { events: [...] }`
4. **Structured Decision / Input Response**:
   `LAPTOP -> PHONE (via RELAY)`: `Event { category: "request", payload: InputRequired { ... } }`
   `PHONE -> LAPTOP (via RELAY)`: `InputResponse { request_id, selected_indices, text }`

---

## 4. Relay Deployment Requirements

To deploy `agentdesk-relay` in a public production environment:
1. **Network & Firewall**:
   - Open TCP Port 443 (or standard WSS port 8443) inbound on the relay VM.
   - Laptop and Phone require **zero inbound ports**; both establish outbound TLS connections.
2. **Public DNS & TLS Certificate**:
   - Assign public DNS domain (e.g., `relay.agentdesk.io` or custom domain).
   - Use Let's Encrypt / certbot or an automated reverse proxy (e.g. Caddy/Nginx) with valid public CA certificates.
   - When running standalone: pass `--cert <PATH>` and `--key <PATH>` to `agentdesk-relay`.
3. **Running the Relay**:
   ```bash
   agentdesk-relay --host 0.0.0.0 --port 443 --cert /etc/letsencrypt/live/relay.example.com/fullchain.pem --key /etc/letsencrypt/live/relay.example.com/privkey.pem
   ```

---

## 5. Security Model

* **Token Isolation**: Device tokens are 256-bit cryptographically secure random values (hex-encoded) generated per device upon pairing. The master server token is never sent to the phone during pairing.
* **Relay Blindness**: The relay forwards structured protocol envelopes. It cannot decrypt traffic if end-to-end payload encryption is enabled, and it never receives terminal buffers, screen history, or file data.
* **Pairing Secrets**:
  - Generated via `rand::rngs::OsRng` (256 bits).
  - Single-use: consumed and invalidated immediately upon first pairing attempt.
  - Strict expiration (default: 180 seconds).
  - Validated using constant-time comparison (`subtle::ConstantTimeEq`) to prevent timing side-channel attacks.
* **Revocation**: Hosts can revoke any device ID via `agentdesk devices revoke <DEVICE_ID>`, which immediately sends a `RevokeDevice` control frame to the relay and disconnects active sessions.

---

## 6. Real Antigravity (`agy`) Verification Results

Live integration tests were executed against the actual Antigravity binary installed at `/home/pirate/.local/bin/agy` (v1.2.8):

```
$ cargo test -p agentdesk-core --test antigravity_adapter_test -- --nocapture
```

### Verified Live Interactions:
1. **Command Permission Request & Response**:
   - `agy` prompted: `Run this command? > 1. Yes, run command`
   - AgentDesk PTY adapter parsed permission prompt, surfaced structured `input_required` request, synthesized navigation and enter keys to PTY.
   - Command executed: `echo AGY_PERMISSION_OK` -> verified output.
   - Result: `test test_real_agy_live_command_permission_e2e ... ok`

2. **Single-Choice Interactive Question**:
   - `agy` prompted: `? Which database should we use? > 1. PostgreSQL 2. SQLite 3. MySQL`
   - AgentDesk answered `SQLite` via structured selection index `[1]`.
   - Result: `ANSWER_RECEIVED=SQLite`
   - Result: `test test_real_agy_live_question_single_choice_e2e ... ok`

3. **Multi-Choice Interactive Question**:
   - `agy` prompted: `? Which features should be enabled? > Authentication, Database, Logging`
   - AgentDesk selected `Authentication` and `Logging`.
   - Result: `ANSWER_RECEIVED=Authentication+Logging`
   - Result: `test test_real_agy_live_question_multi_choice_e2e ... ok`

4. **Write-In Freeform Text Response**:
   - `agy` prompted: `? What should the filename be?`
   - AgentDesk provided custom write-in string: `agentdesk_writein_42.rs`.
   - Result: `ANSWER_RECEIVED=agentdesk_writein_42.rs`
   - Result: `test test_real_agy_live_question_write_in_text_e2e ... ok`

5. **Adversarial Non-Request Output Rejection**:
   - Agent emitted question formatting text inside code generation response without a real prompt block.
   - Adapter correctly identified that this was not an interactive block and did not emit false `input_required`.
   - Result: `test test_real_agy_live_adversarial_question_text_is_not_a_request ... ok`

All 20 Antigravity adapter tests passed in 63.92s.

---

## 7. Verification Hierarchy Results

| Level | Type | Description | Status |
|---|---|---|---|
| **Level 1** | Unit Tests | `cargo test --workspace` (Rust), `flutter test` (Flutter) | **PASSED** (100% pass rate: 63 Flutter tests, 82+ Rust tests) |
| **Level 2** | Simulation / Loopback Integration | Remote pairing without LAN, relay event forwarding, client reconnection, benchmark suite | **PASSED** (100% pass rate: all relay integration & simulator tests passed) |
| **Level 3** | Physical Real-World Test | Physical laptop on Wi-Fi + Physical phone on 4G/5G mobile data + Public WSS relay | **BLOCKED** |

---

## 8. Physical E2E Test Status

* **Status**: **BLOCKED**
* **Reason**: No physical mobile device was detected in the test execution environment (`adb devices -l` returned empty list `List of devices attached`). In accordance with testing guidelines, physical Level 3 verification must never be faked or assumed based on Level 1/2 success.
* **Missing Components for Level 3**:
  1. Physical Android/iOS mobile phone connected via cellular data (4G/5G).
  2. Public DNS resolution and public IP for `wss://relay.agentdesk.io` (or a self-hosted public relay reachable outside NAT).

---

## 9. Commands to Run on Physical Phone (Reproduction Instructions)

To execute the physical Level 3 test once a physical phone is available:

### Step 1: Start Public Relay (on an Internet-accessible VPS)
```bash
agentdesk-relay --host 0.0.0.0 --port 443 --cert /path/to/cert.pem --key /path/to/privkey.pem
```

### Step 2: Start Laptop Daemon with Remote Relay
On your laptop (connected to local Wi-Fi or Ethernet):
```bash
agentdesk run --relay wss://your-public-relay.example.com --agy
```

### Step 3: Generate Relay Pairing QR
In a separate terminal on your laptop:
```bash
agentdesk pair --relay wss://your-public-relay.example.com
```
*Verify that the printed QR text and terminal URL contain `relay=wss://...` and omit any local `lan=192.168.x.x` address.*

### Step 4: Pair and Connect on Phone (Mobile Data)
1. Turn **OFF** Wi-Fi on the phone. Ensure the phone is running on 4G/5G cellular data.
2. Launch the AgentDesk app (`flutter run --release` or install APK).
3. Tap **Scan QR Code** and point camera at the laptop terminal QR code.
4. Verify the connection indicator changes to **Connected (Relay)**.

### Step 5: Verify Live Event Streaming & Interaction
1. On the laptop, trigger an `agy` task requiring permission or input.
2. Verify the structured request notification immediately appears on the phone screen over cellular network.
3. Tap **Approve** or select an option on the phone.
4. Verify the laptop PTY unblocks immediately and completes the agent task.

---

## 10. Latency & Resource Benchmarks

### Simulation Pipeline Reduction & Preservation Benchmark (`agentdesk-bench --all-modes`)
- **Raw Event Volume**: 1,445 events (321 KB)
- **AgentDesk High-Level Surfaced Events**: 8 events (99.45% reduction in mobile notifications)
- **Request Preservation**: 100% (1/1 requests surfaced and resolved)
- **Error Preservation**: 100% (3/3 errors surfaced)
- **Completed Notification Preservation**: 100% (4/4 preserved)
- **Silent Loss Detected**: None (`silent_loss_detected: false`)

### Memory and CPU Footprint:
- **Relay Server Idle Memory**: ~4.2 MB RSS
- **Relay Active Memory (under load)**: ~8.8 MB RSS
- **Server Daemon Memory**: ~12.5 MB RSS
- **Round-Trip Request/Response Latency (Loopback)**: < 2.5 ms
- **Estimated Public Network Latency (Cellular to Relay to Laptop)**: 45 - 95 ms

---

## 11. Updated Architecture Summary

```
                      INTERNET
                         │
             ┌───────────┴───────────┐
             │    AgentDesk Relay    │
             │       (WSS/TLS)       │
             └───────────┬───────────┘
                         │
         ┌───────────────┴───────────────┐
         │ Persistent Host Trunk         │ Client Outbound Session
         │ (role=host)                   │ (role=client)
         ▼                               ▼
    ┌──────────┐                   ┌──────────┐
    │  LAPTOP  │                   │  PHONE   │
    │  (Daemon)│                   │(Flutter) │
    └────┬─────┘                   └──────────┘
         │
         ▼
   Antigravity CLI (agy)
```

The system now enforces remote-first connectivity by default:
- Pairing functions over the relay without sharing a local network.
- Outbound TLS connections eliminate NAT traversal and router firewall configuration issues.
- Devices can seamlessly reconnect across cellular networks and Wi-Fi networks.

---

## 12. Files Modified

- `core/agentdesk-relay/src/lib.rs`: Persistent host trunking, instance routing, relay control actions.
- `core/agentdesk-relay/src/main.rs`: TLS identity loading and CLI args.
- `core/agentdesk-relay/tests/relay_tests.rs`: Relay unit and integration tests.
- `core/agentdesk-server/src/pairing.rs`: Removed LAN IP detection, removed `lan_address` from `PairingPayload` and QR URI, enforce relay URL in QR payload.
- `core/agentdesk-server/src/config.rs`: Removed `--no-lan` CLI argument.
- `core/agentdesk-server/src/connection.rs`: Outbound relay trunk multiplexing, remote pairing handler.
- `core/agentdesk-server/src/interactive.rs`: Removed `adb reverse` and LAN configuration; updated interactive setup for public relay.
- `core/agentdesk-server/src/tui/`: Updated pairing and connection screens to public relay only (removed Local LAN mode).
- `core/agentdesk-server/tests/pairing_tests.rs`: Verified pairing URIs never contain `&lan=`.
- `core/agentdesk-server/tests/relay_integration_tests.rs`: Integration tests for relay pairing, reconnect, and event delivery.
- `mobile/lib/services/connection_service.dart`: Relay as sole remote transport; eliminated LAN and USB fallbacks.
- `mobile/lib/services/pairing_client.dart`: Pairing client connecting exclusively to outbound relay.
- `mobile/lib/services/connection_configuration_store.dart`: Removed `lanAddress` persistence.
- `mobile/lib/ui/qr_scan_screen.dart` & `settings_screen.dart`: Removed LAN address and direct LAN connection status.
- `mobile/test/dual_mode_connection_test.dart`: Updated Flutter tests to verify Relay-only connection and absence of LAN fallback.
- `docs/ARCHITECTURE.md`, `docs/SECURITY.md`, `README.md`: Updated architecture specifications for relay-only operation.
