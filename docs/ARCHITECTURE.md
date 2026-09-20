# AgentDesk — Architecture

## Overview

```text
                    ┌──────────────────────── LAPTOP (Rust) ─────────────────────────┐
                    │                                                                │
  Simulated Agent   │  raw lines ───────────────────────────► Log Store (ring/agent) │
  (seeded) ─────────┼──► Adapter ──► RawAgentEvent                 ▲    ▲            │
                    │                    │                         │    │ pin        │
                    │                    ▼                         │    │            │
                    │             Event Processor ── id, seq, ts, log_range          │
                    │                    │                                           │
                    │                    ├──► Classifier (pure) ──► category/severity│
                    │                    │                                           │
                    │                    ▼                                           │
                    │               Event (immutable) ──► Event Store                │
                    │                    │                                           │
                    │        ┌───────────┼───────────────┐                           │
                    │        ▼           ▼               ▼                           │
                    │   Task Tracker  Priority Queue   Metrics                       │
                    │   + Watchdog ──►(tier, score,                                  │
                    │   (raises       state,          ◄── tick re-score              │
                    │    escalation   escalation)     ◄── ack / dismiss / respond    │
                    │    level)           │                                          │
                    │                     ▼                                          │
                    │              Transport (wss + token) ◄── pipeline mode         │
                    └─────────────────────┬──────────────────────────────────────────┘
                                          │
              snapshot / event / score_update / state_update          (push)
              event_details / event_logs / command_result             (reply)
                                          │
              hello{token} / ack / dismiss / respond_request          (command)
              get_event_details / get_event_logs{offset,limit}        (request)
                                          │
                    ┌─────────────────────▼─────────────── PHONE (Flutter) ──────────┐
                    │  Connection ──► EventStore + QueueView ──► App shell          │
                    │                         Home / Requests / Working / History   │
                    │                                              │ tap            │
                    │                                              ▼                │
                    │                                    Event screen (Level 2)     │
                    │                                     approve/deny · dismiss    │
                    │                                              │ "View logs"    │
                    │                                              ▼                │
                    │                                    Log viewer (paged tail)    │
                    └────────────────────────────────────────────────────────────────┘
```

## Repository layout

```text
AgentDesk/
├── docs/
├── core/                      Rust workspace
│   ├── agentdesk-model/       Event model, message envelope. No I/O, no async.
│   ├── agentdesk-core/        Processor, classifier, queue, log store, watchdog, metrics.
│   ├── agentdesk-sim/         Seeded simulated agent adapter.
│   ├── agentdesk-server/      TLS WebSocket transport + `agentdesk` binary.
│   └── agentdesk-bench/       Headless measurement runner (3 modes → JSON report).
└── mobile/                    Flutter app
```

Why separate crates: `model` is pure data and is mirrored into Dart; `core` is fully testable with no network; `sim` and `server` are the swappable ends; `bench` links `core + sim` with a fake client and never touches a socket.

## Laptop components

### Adapter (trait) and Simulator

- **Responsibility**: turn agent-specific activity into `RawAgentEvent`s and raw log lines.
- **Inputs**: Simulator — a seed and a scenario script (`docs/SIMULATOR.md`). Real agents — standard Agent Client Protocol (ACP) over stdio.
- **Outputs**: `AdapterOutput::Event(RawAgentEvent)` or `AdapterOutput::Line { agent_id, text }` (pure log noise that is not an event).
- **Interface** (`agentdesk-core::adapter::Adapter`): adapters are **passive and time-driven**. The owner calls `poll(now)` to collect everything due at or before `now` (in due-time order), `next_due()` to learn how long to sleep or how far to advance a virtual clock, `respond(task_id, decision, now)` to deliver a human decision, and `agents()` for identity (`AgentInfo`). No threads or timers inside an adapter, so the same scenario yields the same output regardless of polling granularity.
- **Dependencies**: none on the rest of core (it only produces).
- **State**: per-agent `agent_seq` counter; simulator per-task cursor, blocked state and seeded RNG (`StdRng::seed_from_u64`).
- **Failure**: runs in its own task. A panic or error is converted into an Error event about the adapter itself; the daemon keeps running.

### Real-Agent Adapter (Agent Client Protocol - ACP)

- **Responsibility**: Integrate real, interactive coding agents into AgentDesk using the open Agent Client Protocol (ACP v1 over JSON-RPC 2.0 stdio).
- **Architecture**:
  - `AcpTransport` trait: Pluggable I/O layer with `send_line`, `try_recv`, `is_alive`, and `close`.
  - `ProcessTransport`: Spawns the target agent CLI process (e.g. `gemini --skip-trust --acp`), captures child stdout/stderr onto non-blocking mpsc channels, and manages stdin for newline-delimited JSON-RPC communication.
  - `MockAcpTransport`: Fully deterministic in-memory fake transport for hermetic, offline testing of all protocol edge cases and error states.
  - `AcpAdapter<T: AcpTransport>`: Implements the `Adapter` trait:
    - On startup, executes the ACP handshake: `initialize` → `session/new` → `session/prompt`.
    - **Zero-Terminal-Scraping Guarantee**: Arbitrary terminal text, stderr lines, and non-protocol stdout chunks are strictly piped as `AdapterOutput::Line` into the bounded ring buffer log store. Terminal text is *never* parsed or heuristically scraped for attention events.
    - **Documented Structured Signals**:
      - `session/request_permission` RPC request → `approval_required` raw event (`Category::Request`, `Severity::Critical`, `RequestInfo`).
      - `session/update` tool execution notification → `progress` raw event (`Category::Working`, `Severity::Info`).
      - `stopReason: "end_turn"` in prompt response → `task_completed` raw event (`Category::Completed`, `Severity::Info`).
      - JSON-RPC error or child process exit/crash → `adapter_error` or `command_failed` raw event (`Category::Error`, `Severity::Critical`).
    - **Safe Human Feedback**: `respond(task_id, decision, now)` checks whether the task exists and is blocked. If valid, maps `Decision::Approve` to option selection (e.g. `allow_once`) and `Decision::Deny` to cancellation/rejection, dispatching the JSON-RPC response to the child process.
    - **Dead-Process Rejection**: If `respond` is invoked when the child process has exited or crashed, the call safely rejects the command with `RespondError::NotBlocked` or `RespondError::NoSuchTask` and queues an `adapter_error` raw event for the core pipeline.

### Log Store

- **Responsibility**: bounded ring buffer of raw lines per agent; pinned copies of log windows for attention events.
- **Inputs**: raw lines (with agent id); pin requests `(event_id, range)` from the processor.
- **Outputs**: `LogPage { lines, offset, total, evicted }`.
- **State**: `HashMap<AgentId, RingBuffer<Line>>`, `HashMap<EventId, PinnedWindow>`. Memory is bounded by configuration.
- **Failure**: requests for evicted ranges return `evicted: true` with whatever remains; never a crash. Pinned windows survive eviction.

### Event Processor

- **Responsibility**: the single place where a `RawAgentEvent` becomes an `Event`. Assigns `event_id` (UUID v4) and global `seq`, stamps time, records `log_range`, calls the Classifier, pins logs for Request/Error events, stores the event, notifies Queue, Task Tracker and Metrics.
- **Inputs**: `RawAgentEvent`.
- **Outputs**: `Event`.
- **State**: global `seq` counter.
- **Failure**: a malformed raw event is dropped and counted in `dropped_events`.

### Classifier

- **Responsibility**: `RawAgentEvent → Classification { category, severity, base_score }`. A pure function over a rules table.
- **Categories**: exactly `request`, `error`, `completed`, `working` (see EVENT_MODEL.md).
- **State**: none. Fully unit-tested.

### Event Store

- **Responsibility**: `HashMap<EventId, Event>` — immutable events with Level 2 details and `log_range`.
- Events are never mutated. Mutable per-event data (state, score, escalation) lives in the Queue.

### Task Tracker and Watchdog

- **Responsibility**: derived view of *open tasks*; on a tick, compare elapsed time to the per-operation expected duration and raise the Queue's `escalation_level` for the task's current Working event (0 → 1 at threshold, 1 → 2 at 2× threshold, then stop).
- **Inputs**: every `Event` (observed after storage); a periodic tick.
- **Outputs**: `Escalate { event_id, level }` to the Queue.
- **State**: `HashMap<TaskId, OpenTask { started_at, operation, latest_working_event, escalation_level }>`. A task is closed by a `completed`, `error`, or `cancelled` event with the same `task_id`; closing it also marks its Working events' queue entries as `superseded` so they drop out of snapshots.
- **Failure**: escalation is capped at level 2, so an orphaned task cannot generate unbounded noise.
- **Does not** create events or categories. Escalation is expressed purely in queue metadata.

### Priority Queue

- **Responsibility**: hold one `QueueEntry` per live event: `{ event_id, tier, score, state, escalation_level }`. Order by `(tier asc, score desc, seq desc)`. Re-score on tick. Apply state transitions from phone commands and from the tracker.
- **Inputs**: new events; `ack`, `dismiss`, `respond_request` commands; `Escalate`; tick.
- **Outputs**: ordered snapshot; `ScoreUpdate` and `StateUpdate` deltas for the transport.
- **State**: the entries. Small — events themselves are in the Event Store.
- **Scoring**: see EVENT_MODEL.md. Tier is fixed by category; escalation can never move an entry across tiers.
- **Failure**: unknown `event_id` in a command → `command_result { ok: false, error }`; queue unaffected.

### Metrics

- **Responsibility**: counters for the measurement story. `raw_lines, raw_events, processed_events, dropped_events, transmitted_events, transmitted_bytes, surfaced_summaries, escalations, log_page_requests, detail_refetches, acks, dismissals, responses`.
- Snapshottable to JSON. Counted in every pipeline mode.

### Transport (Server)

- **Responsibility**: `wss://` listener; `hello { token }` handshake; on success send `snapshot`; stream `event`, `score_update`, `state_update`; answer `get_event_details`, `get_event_logs`; accept `ack`, `dismiss`, `respond_request`.
- **Inputs/outputs**: JSON envelopes (COMMUNICATION.md).
- **State**: connected clients (one in the MVP; held in a `Vec` so nothing assumes one). Each client has a bounded outbound channel.
- **Failure**: bad or missing token → close with a defined code. Malformed message → `error` reply, socket kept. Slow client whose channel fills → disconnected rather than blocking core. Core panics never propagate into the network task.
- **Development-only insecure mode**: `--insecure-dev` serves plain `ws://`, binds only to `127.0.0.1`, and logs a prominent warning. It is not part of the MVP security architecture (SECURITY.md).

### Pipeline mode switch

`raw_lines | raw_events | agentdesk`, chosen at startup.

- `raw_lines`: adapter log lines → transport as `raw_line` messages. No processing.
- `raw_events`: `RawAgentEvent`s → transport in arrival order as `raw_event`. No classifier, queue, or escalation.
- `agentdesk`: the full pipeline.

Metrics are collected in all three so the bench can compare them on the same seeded run.

## Mobile components

### Connection

`wss://` client with certificate-fingerprint pinning and token hello. At startup it restores URL, device ID, and fingerprint from ordinary app preferences and restores the token through an OS-backed secure-storage abstraction before attempting a connection. Android secure storage explicitly enables algorithm migration with backup and disables destructive reset-on-error, so a key/decryption failure cannot silently erase a stored token. It refuses incomplete configuration, remote `ws://`, and any TLS connection without a fingerprint; it never downgrades `wss://` to `ws://`. Reconnects with backoff; on reconnect it receives a fresh `snapshot` and replaces local queue state.

### State

- `EventStore`: `event_id → Event` (with Level 2 details).
- `QueueView`: ordered entries `{ event_id, tier, score, state, escalation_level }` updated by `snapshot`, `event`, `score_update`, `state_update`.

### UI

- **App shell**: page-level Home, Requests, Working, Completed, New Work, and Settings destinations with a persistent bottom navigation bar. Home is an overview; it is not a raw terminal view.
- **Requests**: unresolved attention events are presented as cards and open a dedicated interaction screen.
- **Event screen**: structured single-choice, multiple-choice, free-text, write-in, and approval controls. Responses remain bound to the event ID through the existing `respond_request` protocol. Opening this screen sends `get_event_details`, which marks the event `seen` on the laptop.
- **Themes**: shared light and dark Material 3 themes use the same spacing, contrast, and status semantics.
- **Onboarding**: first launch presents a five-page explanation of the laptop/phone attention model. Completion is persisted in the existing `SharedPreferences` store; Settings can reopen it without creating a second storage system.
- **Branding**: the original AgentDesk geometric connection mark is stored as SVG source plus PNG fallback, used by Flutter onboarding/home, Android launch splash, and launcher resources.
- **New Work**: the current daemon has one adapter/session created at startup and does not yet expose a generic start-task command. The mobile page states this capability boundary rather than pretending to create a task.
- **Log viewer**: tail-first paged log view driven by `get_event_logs { offset, limit }`.
- **Debug screen**: client-side metrics (`summaries_rendered`, `taps`, `log_pages_requested`), insecure-dev warning banner, and laptop daemon metrics.
- **Settings screen**: connection URL, auth token, device ID, and certificate fingerprint. URL/device ID/fingerprint persist in ordinary preferences; the token is stored only through the secure-storage abstraction. Incomplete configuration is shown as `configurationRequired`, rather than as a failed network connection.

## Things deliberately not built in the MVP

See PROJECT.md "Non-goals" and SYSTEM_DESIGN.md "Offline behaviour (future)".
