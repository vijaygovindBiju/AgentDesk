# AgentDesk — Remote E2E Test (Different Networks)

This is the release-gating end-to-end test for AgentDesk's relay-only
architecture. It proves the laptop and phone work when they are on
**completely different networks** and can never reach each other directly.

```
Laptop (hostel/college Wi-Fi, behind NAT) ──outbound WSS──► Public Relay ◄──outbound WSS── Phone (4G/5G, Wi-Fi OFF)
```

## Network requirements (do not skip)

| Endpoint | Network | Must NOT |
|----------|---------|----------|
| Laptop   | Hostel/college/home Wi-Fi behind NAT | have port forwarding, a public inbound port, or an inbound firewall rule |
| Phone    | **Wi-Fi disabled**, 4G/5G mobile data enabled | be on the laptop's Wi-Fi/LAN, be connected by USB, use `adb reverse` |
| Relay    | Public Internet host, `wss://` with a CA-trusted certificate | run on the laptop's LAN address |

Do **not** perform this validation with laptop and phone on the same router:
that can hide LAN dependencies. A USB cable may be attached only to install
the APK; unplug it before step 5.

## What the architecture guarantees (and what to look for)

- Laptop daemon listens only on `127.0.0.1` (local TUI). Any non-loopback
  bind is rejected at startup; `--bind`, `--lan`, `--no-lan`, `--usb` are removed.
- Laptop authenticates its trunk to the relay with a per-instance host key
  (`relay_host.key`), never the master token. The relay binds the instance to
  that key; a second host presenting another key is rejected (close `4003`).
- The QR contains only `v, inst, sess, sec (single-use, 180s), fp, relay`.
  No LAN IP, port, USB or ADB data, no master token.
- The phone accepts only public `wss://` relays; private/LAN relay hosts
  (`10.*`, `172.16-31.*`, `192.168.*`, `*.local`) and plaintext `ws://` are rejected.
- Only structured AgentDesk protocol messages cross the relay. Raw PTY bytes,
  shell stdin/stdout and the master token never do.

## Procedure

### 1. Start the public relay (VPS)

TLS is terminated either by `agentdesk-relay` itself or by a reverse proxy
(Caddy/nginx/Cloudflare) in front of a loopback-bound relay:

```bash
# Behind a TLS-terminating proxy that forwards wss://relay.example.com → 127.0.0.1:8766
agentdesk-relay --bind 127.0.0.1 --port 8766 --insecure-dev --token "$RELAY_OPERATOR_TOKEN"
```

Verify from any machine: `curl -si https://relay.example.com/ws` returns an HTTP
`400` (missing role) — proving the relay is publicly reachable.

### 2. Configure and start the laptop daemon (hostel/college Wi-Fi)

```bash
agentdesk relay set wss://relay.example.com
# only if the relay requires an operator token:
echo "AGENTDESK_RELAY_TOKEN=$RELAY_OPERATOR_TOKEN" >> ~/.config/agentdesk/service.env
agentdesk            # select coding agent → workspace
```

### 3. Verify the laptop's outbound WSS connection

- TUI header shows **`● Relay Connected`**.
- `jq .relay_connected ~/.config/agentdesk/runtime_status.json` → `true`.
- Relay log: `Host connected: instance '<id>' from <hostel public IP>`.
- `ss -ltnp | grep agentdesk` shows only `127.0.0.1:8765` (no `0.0.0.0`, no LAN IP).

### 4. Generate the pairing QR

In the workspace press the pairing shortcut (or run `agentdesk pair`). Decode the
QR and confirm it contains `relay=wss://relay.example.com` and no `lan=`/IP address.

### 5. Scan the QR on the phone (Wi-Fi OFF, 4G/5G ON)

Settings → **Scan QR Code to Pair**.

### 6. Verify the phone's outbound WSS connection

- Phone Settings shows **Relay Connected** / "Connected via Relay".
- Relay log: `Client connected for instance '<id>' ... from <carrier public IP>`;
  its IP differs from the laptop's.

### 7. Verify the phone is an authorized device

`agentdesk devices list` shows the phone as `ACTIVE`; TUI header shows `● PHONE CONNECTED · RELAY`.

### 8–9. Start a coding-agent session and trigger an attention event

In the Agent tab ask the agent to do something that needs approval, e.g.
*"create a file named remote_e2e.txt, ask me before running any command"*.
Antigravity raises a command-approval / structured question.

### 10. Event reaches the phone through the relay

The phone shows the request (yes/no, single/multi choice, free text, write-in,
command approval or workspace trust) within a few seconds.

### 11–14. Respond from the phone

Answer on the phone. Verify:

- The response reaches the laptop (TUI request clears; relay forwarded-message count grows).
- The coding-agent PTY receives the controlled keystrokes (visible in the Agent tab).
- The agent continues and later emits Working / Completion events on the phone.

### 15–17. Disconnect and reconnect the phone

Toggle airplane mode for ~30s (or switch 4G → 5G / another carrier), then restore.

- Phone shows *Reconnecting…* then **Relay Connected** (exponential backoff, 1s→30s with jitter).
- The request list/snapshot matches the laptop (no duplicates, outstanding requests preserved).

Also restart the relay process: both laptop (`○ Relay Disconnected` → `● Relay Connected`)
and phone must recover automatically without re-pairing and without duplicate sessions.
Changing the laptop's Wi-Fi network must likewise recover automatically.

### 18–19. Revoke the device

```bash
agentdesk devices revoke <device_id>
```

- The phone is disconnected immediately (close `4001 device revoked`).
- Re-opening the app / reconnecting fails with *device revoked*; it cannot reconnect
  until it pairs again with a fresh QR.

## Automated coverage (loopback relay, CI)

These run with a development relay on `127.0.0.1` and exercise the same protocol:

```bash
cd core && cargo test --workspace      # relay_tests, relay_integration_tests, pairing, TUI
cd mobile && flutter test              # relay-only client, pairing, reconnect, revocation
```

The loopback relay is a development configuration only; it does not replace the
different-networks validation above.

## Result record

| Step | Result | Notes (IPs, timings) |
|------|--------|----------------------|
| 1–3 Laptop → relay | | |
| 4–7 Pair over 4G/5G | | |
| 8–14 Event + response round-trip | | |
| 15–17 Reconnect / relay restart | | |
| 18–19 Revocation | | |
