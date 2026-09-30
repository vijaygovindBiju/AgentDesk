# AgentDesk — Security

## Why this matters even for a prototype

The laptop accepts `respond_request { approve }`. An open endpoint on shared Wi-Fi would let anyone on the network approve a database migration or a destructive command on behalf of the user. That is a worse failure mode than a typical dev server, so the MVP does not run unauthenticated or in plaintext.

## Threat model (MVP)

In scope:

- Passive attacker on any network path (hostel/college Wi-Fi, mobile carrier, Internet) reading traffic.
- Active attacker trying to connect to the laptop daemon and issue commands.
- Active attacker impersonating the daemon to the phone (rogue AP / ARP spoofing).
- Attacker who learns an instance id and tries to register as that laptop on the relay (hijack).
- Relay operator: must not obtain the laptop master token, raw PTY streams or shell stdin/stdout.

Out of scope for the MVP:

- Compromised laptop or phone.
- Attacker with physical access to the laptop screen while the token is displayed.
- Denial of service.
- Relay-level denial of service.

## Connectivity model (relay-only)

- The laptop daemon listens on `127.0.0.1` only (local TUI). Non-loopback binds are rejected; there is no LAN, USB/ADB or direct phone-to-laptop transport, no port forwarding and no inbound firewall rule.
- Laptop and phone each open an **outbound** WSS connection to the configured public relay (`agentdesk relay set wss://...`). They may be on completely different networks.
- The laptop authenticates its relay trunk with a per-instance host key (`relay_host.key`, `0600`), never the master token. The relay binds each instance to the first host key it sees and rejects other hosts (`4003`); a reconnecting laptop with the same key supersedes its stale trunk. An optional relay operator token (`AGENTDESK_RELAY_TOKEN` / `agentdesk-relay --token`) gates who may register hosts.
- Relay URLs must be public `wss://`; private/LAN hosts and plaintext `ws://` are rejected on both laptop and phone (loopback `ws://` is a development-only configuration).
- Only structured AgentDesk protocol messages cross the relay. See `docs/REMOTE_E2E_TEST.md`.

## MVP mechanisms

### 1. Transport encryption: TLS (`wss://`)

- The daemon generates a self-signed certificate on first run (`rcgen`), stores it under the user's config directory, and serves TLS via `rustls`.
- It prints the SHA-256 fingerprint of the DER certificate at startup, next to the address and token.
- The phone **pins** that fingerprint: it accepts the server certificate only if its SHA-256 matches the pinned value. Standard CA validation is not used (self-signed).
- The fingerprint is entered once on the phone, alongside the token. This is the seam where QR pairing plugs in later (QR = address + token + fingerprint).

This provides confidentiality and integrity on the wire and prevents daemon impersonation.

### 2. Authentication: shared token

- A random 256-bit token is generated at first run and stored with `0600` permissions in the config directory; it can be regenerated with `agentdesk token rotate`.
- The phone sends it in the first frame (`hello`), never in the URL, so it does not appear in access logs or proxies.
- The phone persists it only through OS-backed secure storage (Android Keystore-backed storage on Android and the platform equivalent elsewhere). It is never written to ordinary preferences, plaintext app files, logs, or debug output.
- Mismatch → immediate close (`4001`). Compared in constant time.
- The daemon refuses to bind to any non-loopback address (fail closed); phones reach it only via the relay.

### 3. Authorization

Single trust level in the MVP: a holder of the token can do everything. Per-device permissions are future work.

### 4. Development-only insecure mode

`agentdesk --insecure-dev`:

- plain `ws://`, **bound to `127.0.0.1` only** (the flag is rejected together with any other bind address),
- token check still enforced,
- prominent startup warning and `transport: "insecure_dev"` in `welcome` so the phone shows a banner.

Intended for local emulator / loopback testing without certificates. It is not a supported deployment mode and must never be the default.

### 5. Agent Control & Subprocess Execution Boundary

- **Zero-Terminal Scraping Guarantee**: Arbitrary terminal text or stderr streams are NEVER interpreted as commands, approval prompts, or error signals. Heuristics or regex scraping over unstructured agent output would expose the daemon to prompt-injection exploits where an untrusted agent or tool output could spoof authorization prompts. All raw terminal/stderr output is confined to bounded log stores as `AdapterOutput::Line`.
- **Strict Protocol Validation**: Attention events (`Category::Request`, `Category::Error`, `Category::Completed`, `Category::Working`) are generated exclusively from documented, strongly-typed Agent Client Protocol (ACP) JSON-RPC 2.0 frames (`session/request_permission`, `session/update`, `end_turn`).
- **Direct Subprocess Spawning**: Child processes are executed directly with explicit argument vectors (`std::process::Command::new`), avoiding shell injection risks (`/bin/sh -c`).
- **Safe Feedback Delegation & Dead-Process Rejection**: Human decisions received via `respond_request` are verified against active, pending task IDs and live child processes. If a process terminates, crashes, or disconnects, control inputs are rejected (`RespondError::NoSuchTask` or `RespondError::NotBlocked`), preventing stale decisions from affecting future tasks.

## File locations and credential lifecycle

- **Configuration directory**:
  1. `AGENTDESK_CONFIG_DIR` environment variable (if set)
  2. `$XDG_CONFIG_HOME/agentdesk` (if set)
  3. `$HOME/.config/agentdesk` (default on Unix)
  4. `.agentdesk` (fallback)
- **Token file**: `<config_dir>/token` (0600 file permissions, 0700 parent directory). Contains 64-character hex 256-bit token.
  - Inspection: `agentdesk token show [--config-dir <path>]`
  - Rotation: `agentdesk token rotate [--config-dir <path>]`
- **TLS Certificate & Key**:
  - Certificate: `<config_dir>/cert.pem` (0600 permissions, DER/X.509)
  - Private key: `<config_dir>/key.pem` (0600 permissions, PKCS#8 DER)
  - Rotation: deleting `<config_dir>/cert.pem` and `<config_dir>/key.pem` triggers automatic re-generation on daemon restart with updated SHA-256 fingerprint printed for pinning.
- **Fingerprint format**: SHA-256 digest over DER certificate bytes. Displayed as `AA:BB:CC:...` colon-separated uppercase hex; client accepts both colon-separated and continuous hex strings, case-insensitively.
- **Phone configuration**:
  - Server URL, device ID, and certificate fingerprint are persisted as non-secret app preferences.
  - The authentication token is persisted separately through the phone OS secure-storage facility. The app uses a storage abstraction so application code never handles a platform-specific keystore API directly.
  - `wss://` is the default and requires a fingerprint. Plain `ws://` is accepted by the phone only for loopback hosts (`127.0.0.1`, `::1`, or `localhost`) to match the daemon's explicit `--insecure-dev` boundary.

## Implementation rules

- Use established libraries (`rustls`, `rcgen`, `tokio-tungstenite`; Dart `SecurityContext` / `badCertificateCallback` for pinning). Do not implement cryptographic primitives or protocols by hand.
- Never log the token or full log content at info level. Debug logging of payloads must be opt-in and documented.
- Token and certificate files are never committed; `.gitignore` covers the config directory if it ever lands in the repo.
- Constant-time comparison for the token.
- Configuration validation occurs before connecting. Missing URL, token, device ID, or a required TLS fingerprint produces a configuration-required state and does not initiate a reconnect loop.

## QR Pairing Protocol & Credential Lifecycle

### 1. Ephemeral Bootstrap Secret
- The pairing QR code encodes a standard, versioned URI:
  `agentdesk://pair?v=1&inst=<instance_id>&sess=<session_id>&sec=<ephemeral_secret>&fp=<fingerprint>&relay=<relay_url>`
- **Zero Permanent Secrets in QR**: The permanent daemon master token and persistent device credentials are **never** placed into the QR payload.
- **Short-Lived & Single-Use**: Pairing sessions expire after 180 seconds (`PAIRING_DEFAULT_DURATION_SECS`). Once validated, `used` is set to `true`, and subsequent attempts with the same session or secret are immediately rejected.
- **Replay Protection**: The `PairingManager` tracks active sessions and revokes older sessions upon fresh session generation. Replay attempts fail with `PairingError::Replayed`.
- **Constant-Time Verification**: Verification of the 64-character ephemeral hex secret against the active session uses `subtle::ConstantTimeEq`.

### 2. Device Identity & Registry
- **Instance Identifier**: Generated on first initialization as `agentdesk-inst-<hex>` and persisted in `instance_id` (0600 permissions).
- **Paired Devices Registry**: Saved at `devices.json` (0600 permissions). Each paired device is assigned a cryptographically random 256-bit token.
- **Constant-Time Device Authentication**: Incoming `hello` frames identify the device via `device_id` and `token`. The token is checked against registered devices using constant-time comparison.
- **Device Revocation**: Devices can be revoked via `agentdesk devices revoke <device_id>` or interactive CLI menu 7. Revoked devices are immediately denied (`CloseCode 4001 Unauthorized`) on subsequent connection attempts.

### 3. Remote Relay Security Boundaries
- **Zero Raw PTY Exposure**: The relay only forwards authorized JSON protocol messages (`Message` envelope). Terminal streams, PTY bytes, shell execution, prompts, and filesystem operations are strictly local to the laptop.
- **Authentication**: Daemons register with the relay using their instance ID and shared token (`/ws?role=host&inst=...&token=...`). Mobile clients connect with `/ws?role=client&inst=...`.
- **Stateless/Ephemeral Routing**: The relay does not persist agent logs, task state, or source code. Disconnection of either peer cleanly tears down the paired session.
- **TLS Only**: Remote connections mandate TLS (`wss://`). Insecure mode (`--insecure-dev`) is strictly restricted to loopback interfaces (`127.0.0.1`).
