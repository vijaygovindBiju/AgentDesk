# AgentDesk — Communication

## Mechanism

- **Transport**: WebSocket over TLS (`wss://`), one persistent connection per phone.
- **Encoding**: JSON text frames. One message per frame.
- **Security**: shared token in the first message, server certificate fingerprint pinned by the client. See SECURITY.md.
- **Development only**: `--insecure-dev` serves plain `ws://` on `127.0.0.1` for local testing. Not part of the MVP architecture.

Why WebSocket + JSON for the MVP: bidirectional push and request/response on one connection, trivially debuggable, natively supported in both Rust (`tokio-tungstenite`) and Flutter (`web_socket_channel`). Binary encodings and message brokers are future considerations recorded in DECISIONS.md.

## Envelope

Every frame:

```jsonc
{
  "type": "event",            // message type, snake_case
  "request_id": "r-17",       // present on phone requests and their single reply
  "payload": { ... }
}
```

## Message catalogue

### Handshake

| Direction | Type | Payload |
|-----------|------|---------|
| phone → laptop | `hello` | `{ token, device_id, client_version, schema_version }` |
| laptop → phone | `welcome` | `{ daemon_version, schema_version, mode, server_time }` |
| laptop → phone | `snapshot` | `{ entries: [QueueEntry…], events: [Event…] }` |

`hello` must be the first frame. Anything else before it, or a wrong token, closes the socket with code `4001`. Schema mismatch closes with `4002`. `snapshot` follows `welcome` immediately and contains all live (non-dismissed, non-superseded) entries and their events.

### Laptop → phone push

| Type | Payload | When |
|------|---------|------|
| `event` | `{ event: Event, entry: QueueEntry }` | New event enters the queue |
| `score_update` | `{ event_id, score, escalation_level }` | Tick changed score or escalation |
| `state_update` | `{ event_id, state, resolution?, superseded }` | Ack/dismiss/respond/task-closed |
| `raw_event` | `RawAgentEvent` | `raw_events` mode only |
| `raw_line` | `{ agent_id, offset, ts, text }` | `raw_lines` mode only |

### Phone → laptop requests (replied with the same `request_id`)

| Type | Payload | Reply |
|------|---------|-------|
| `get_event_details` | `{ event_id }` | `event_details { event, entry }` — also marks entry `seen` |
| `get_event_logs` | `{ event_id, offset, limit }` | `event_logs { event_id, offset, total, evicted, lines: [{offset, ts, text}] }` |
| `ack` | `{ event_id }` | `command_result` |
| `dismiss` | `{ event_id }` | `command_result` |
| `respond_request` | `{ event_id, decision: "approve" \| "deny" }` | `command_result` |
| `get_metrics` | `{}` | `metrics { … counters … }` |

`command_result`: `{ ok: true }` or `{ ok: false, error: "no_such_event" | "not_a_request" | "already_resolved" | "invalid" }`.

`error` (any request that cannot be parsed or dispatched): `{ code, message }`.

### Log paging

- `offset >= 0`: return up to `limit` lines starting at that per-agent line offset.
- `offset = -1`: tail — return the last `limit` lines within the event's pinned window if pinned, else within the ring buffer; the reply's `offset` is the real start offset so the phone can page backwards with `offset - limit`.
- `total` is the number of lines currently retrievable for this event (pinned window length, or the ring's live span).
- `evicted: true` when part of the requested range is no longer in the ring and not pinned.
- `limit` is capped server-side (default 500).

## Ordering

- Push messages are sent in the order the core task emits them; a single client's outbound channel is FIFO.
- The phone orders its list by `(tier, score desc, seq desc)` using the `QueueEntry`, never by arrival time or `event_id`.
- `seq` and `agent_seq` on the event let the phone (and future sync) detect gaps.

## Duplicate handling

- Phone ignores the `event` payload for an already-known `event_id` but applies its `entry`.
- `score_update` / `state_update` for unknown ids are ignored.
- Laptop treats repeated `ack`/`dismiss` as idempotent successes; repeated `respond_request` on a resolved request returns `already_resolved`.

## Reconnection

- Phone reconnects with exponential backoff (1s → 30s cap, with jitter).
- On every successful `hello`, the laptop sends a fresh `snapshot`, which the phone applies wholesale (replacing its queue view, merging events).
- No server-side queueing for offline phones in the MVP. Future design in SYSTEM_DESIGN.md.

## Failure behaviour

| Situation | Behaviour |
|-----------|-----------|
| Wrong token / no hello | close `4001` |
| Schema mismatch | close `4002` |
| Malformed frame | `error` reply, connection kept |
| Client outbound channel full | close `4003` (`slow_client`), counted |
| Laptop restarts | phone reconnects; all `event_id`s are new (in-memory store) |
| Request timeout on phone | shown inline; retry is manual |

## Byte accounting

`transmitted_bytes` counts UTF-8 payload bytes of every frame actually written to a socket (or to the fake client in the bench), per mode. TLS overhead is not counted; the comparison is between modes on the same transport.

## Future messaging architecture

Not for the MVP; recorded so the envelope does not preclude them:

- Broker-based transport (NATS/MQTT) for multi-device fan-out and store-and-forward.
- Binary encoding (CBOR/MessagePack) if JSON size becomes the measured bottleneck.
- Per-message sequence numbers for phone commands (`client_seq`) for offline replay.
