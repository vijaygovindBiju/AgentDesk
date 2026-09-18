# AgentDesk — System Design

Component responsibilities are in ARCHITECTURE.md. This document covers how they interact over time.

## Data flow (agentdesk mode)

```text
1. Adapter emits raw log lines            → Log Store appends to agent's ring buffer
2. Adapter emits RawAgentEvent            → Processor
3. Processor assigns event_id, seq, ts, log_range (current ring offsets)
4. Processor → Classifier                 → category, severity, base_score
5. If category ∈ {request, error}         → Log Store pins [start−200, end+50] lines
6. Processor stores Event                 → Event Store
7. Processor notifies                     → Queue (new entry, state=new)
                                          → Task Tracker (open/close task)
                                          → Metrics
8. Queue → Transport                      → `event` pushed to connected clients
9. Tick (every N seconds)                 → Queue re-scores; Watchdog checks open tasks
                                          → `score_update` deltas pushed if changed
10. Phone opens event                     → get_event_details → state=seen → `state_update`
11. Phone requests logs                   → get_event_logs → Log Store page → `event_logs`
12. Phone approves/denies                 → respond_request → state=resolved → `state_update`
                                          → adapter is informed (simulator unblocks the task)
```

## Event lifecycle

An `Event` is immutable once created. Its *queue entry* has a lifecycle.

```text
                 ┌─────────────────────────────────────────────────┐
                 │                  QueueEntry                     │
                 │                                                 │
 created ──► new ──(get_event_details / ack)──► seen               │
                 │     │                          │                │
                 │     └────────(dismiss)─────────┴──► dismissed   │
                 │                                                 │
                 │  request only, orthogonal to the above:         │
                 │     unresolved ──(respond_request)──► resolved{approved|denied}
                 │                                                 │
                 │  working only, set by Task Tracker:             │
                 │     live ──(task closed by later event)──► superseded
                 └─────────────────────────────────────────────────┘
```

Rules:

- `seen` and `dismissed` describe **user attention**. `resolved` describes **task outcome**. They are independent: a request can be `dismissed` yet `unresolved`, and the agent is still blocked. The phone must show this distinction; the laptop must never treat `dismissed` as `resolved`.
- `dismissed` and `superseded` entries are excluded from `snapshot` and from re-scoring, but remain in the Event Store and can still be fetched by id.
- `resolved` requests remain in the snapshot until dismissed, with reduced score.
- Transitions are idempotent: acking a `seen` event is a no-op success.

## Task lifecycle (derived, Task Tracker)

```text
first event with task_id ──► open { started_at, operation }
    working events   → update latest_working_event
    tick             → elapsed > expected(operation)      ⇒ escalation_level 1
                       elapsed > 2 × expected(operation)  ⇒ escalation_level 2 (final)
    completed | error | cancelled with same task_id ──► closed
                       ⇒ working entries of the task → superseded
```

Expected durations for the MVP come from a small per-operation table (`build`, `test`, `install`, `analyze`, `edit`, `other`). The simulator declares each task's `operation`. Learned baselines are future work.

## Request / response flow

Every phone-initiated message carries a `request_id`. The laptop echoes it in exactly one reply (`event_details`, `event_logs`, `command_result`, or `error`). The phone times out unanswered requests after a configurable interval and surfaces the failure locally.

```text
Phone                                     Laptop
  │  get_event_logs {id, offset:-1, limit:200, request_id}  │
  │────────────────────────────────────────────────────────►│
  │                                                          │ Log Store page (tail)
  │  event_logs {request_id, lines, offset, total, evicted}  │
  │◄────────────────────────────────────────────────────────│
```

`offset: -1` means "tail": return the last `limit` lines and their real offset so the phone can page backwards.

## Ordering and duplicates

- Global `seq` is strictly monotonic per daemon run; `agent_seq` is strictly monotonic per agent. Both are integers. `event_id` (UUID) is identity only and never used for ordering.
- The phone treats an `event` with an already-known `event_id` as a duplicate and ignores the payload (it may still apply the entry's score/state).
- `score_update` / `state_update` for unknown ids are ignored; a `snapshot` always replaces the phone's queue view wholesale.

## Reconnection

```text
disconnect ──► phone backs off (1s, 2s, 4s … max 30s) ──► hello{token}
           ──► snapshot (all non-dismissed, non-superseded entries + events)
           ──► phone replaces QueueView and merges EventStore
```

Nothing is queued for offline phones in the MVP. Events produced during a disconnect are simply present in the next snapshot if still live.

## Pipeline modes (measurement)

| Mode | Path | What is transmitted |
|------|------|---------------------|
| `raw_lines` | adapter lines → transport | every raw log line as `raw_line` |
| `raw_events` | adapter events → transport | every `RawAgentEvent` as `raw_event`, arrival order |
| `agentdesk` | full pipeline | `event`, `score_update`, `state_update`, replies |

The same seed and scenario are run in all three; the bench (`agentdesk-bench`) uses a fake client with a scripted tap policy (e.g. open every Request and Error, request one log page per Error) and writes a JSON report with the Metrics counters plus client-side counts. Headless results are the ones cited; live runs are demos.

## Error handling

- Adapter failure → Error event about the adapter (agent_id of the adapter, `operation: adapter`).
- Classifier cannot classify → falls back to `working` with severity 0 and increments `unclassified_events`. It never drops.
- Log Store eviction → `evicted: true` in the page; UI shows "earlier logs no longer available".
- Unknown `event_id` in a command → `command_result { ok: false }`.
- Transport: bad token → close code `4001`; malformed JSON → `error` reply, socket kept; outbound channel full → client disconnected, counted in `slow_client_disconnects`.
- Every error the phone shows includes agent, project, and the failing message; never a bare "failed".

## Concurrency model (laptop)

Single `tokio` runtime. Core state (`EventStore`, `PriorityQueue`, `LogStore`, `TaskTracker`, `Metrics`) is entirely owned by one **core task** that runs a single-threaded event loop processing commands from an `mpsc::Receiver<CoreCommand>`.

### Channel Layout & Commands

1. **Inbox (`mpsc::Sender<CoreCommand>` / `CoreHandle`)**:
   - `CoreCommand::Adapter(AdapterOutput)`: Ingestion of adapter output (`AdapterOutput::Line` or `AdapterOutput::Event`).
   - `CoreCommand::Client { client_id, message }`: Client requests (`RespondRequest`, `AckEvent`, `DismissEvent`, `GetEventDetails`, `GetLogPage`, `GetMetrics`).
   - `CoreCommand::Tick`: Periodic timer event triggering watchdog escalation checks and queue re-scoring.
   - `CoreCommand::Connect { client_id, sink }`: Registers a new client `TransportSink`.
   - `CoreCommand::Disconnect { client_id }`: Removes a disconnected client sink.
   - `CoreCommand::SendSnapshot { client_id }`: Sends initial snapshot frame containing agents and live queue entries.
   - `CoreCommand::Shutdown`: Signals graceful termination of the event loop.

2. **Outbound Sinks (`TransportSink`)**:
   - Each connected client registers a `Box<dyn TransportSink>` (e.g. `ChannelSink` in server mode, `VecSink`/`CountingSink` in bench/test mode).
   - Point-to-point messages (command replies, requested log pages, details, snapshots, metrics) are dispatched directly to `client_id`.
   - Live queue changes (`Event` push, `ScoreUpdate`, `StateUpdate`) are broadcast to all registered sinks.
   - Sink writes update `Metrics.transmitted_events` and `Metrics.transmitted_bytes`.

3. **Adapter Feedback (`mpsc::Sender<AdapterCommand>`)**:
   - For interactive requests requiring approval, `respond_request` routes an `AdapterCommand::Respond { task_id, decision, now }` back to the adapter/simulator to unblock paused agent tasks.

Zero shared mutexes are used around core state. This design guarantees deterministic replay during benchmarking and ensures slow client sockets never stall pipeline event processing.


## Offline behaviour (future, not MVP)

When implemented:

- Laptop persists live queue entries and events (SQLite); a disconnected phone receives, on reconnect, everything with `seq` greater than its last-acknowledged `seq`, ordered by tier then score.
- Phone queues commands while offline with its own monotonic `client_seq`; on reconnect they replay in order. A command referring to a missing/superseded event fails individually (`command_result { ok: false, error: "no_such_event" }`) without stopping later commands.
- Duplicate suppression uses `(client_id, client_seq)`.
