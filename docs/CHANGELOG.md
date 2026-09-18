# AgentDesk — Changelog

## 2026-09-18

### Added

- Phase 6 complete: Transport server (token authentication, development mode) in `agentdesk-server`:
  - `Server` & `ServerConfig`: WebSocket listener using `tokio-tungstenite` exposing `CoreTask` over WebSocket (`ws://`). Supports configurable bind address, port, and `--insecure-dev` mode strictly locked to `127.0.0.1` (P6.1, P6.6).
  - Token authentication & lifecycle: 256-bit cryptographically secure token generated on first run and stored with `0600` permissions (`0700` parent dir) under config directory; CLI commands `agentdesk token show` and `agentdesk token rotate`; constant-time token verification using `subtle::ConstantTimeEq`; non-loopback bind without token rejected (P6.2, P6.5).
  - Handshake protocol: first frame validation requiring `hello`; unauthorized or non-hello first frame closed with `4001` (`close_code::UNAUTHORIZED`); schema mismatch closed with `4002` (`close_code::SCHEMA_MISMATCH`); on success, returns `welcome` followed immediately by core `snapshot` (P6.2).
  - Request/reply dispatch & error handling: bidirectional routing between WebSocket clients and `CoreTask` actor via `CoreCommand::Client`, echoing `request_id` on replies; malformed JSON frames return `error` replies while keeping the socket connection open; `get_event_details` marks entries `seen` (P6.3).
  - Slow-client protection: bounded outbound channel (`ServerConnectionSink` implementing `TransportSink`); channel overflow terminates slow connection with close code `4003` (`close_code::SLOW_CLIENT`) and increments `slow_client_disconnects` in metrics while core continues processing (P6.4).
  - Main binary CLI: `agentdesk run --scenario <path> --seed <u64> --mode <mode> [--insecure-dev] [--bind <addr>] [--port <port>] [--debug]` wiring simulator, core task, and WebSocket server; startup prints bind address, mode, and token (P6.7).
  - Safe logging: tokens and raw payload contents never logged at `info` level; opt-in `--debug` flag for payload inspection (P6.8).
  - Test coverage: 15 unit and integration tests verifying handshake security, all request/reply pairs, malformed JSON recovery, slow-client overflow termination, `--insecure-dev` bind restrictions, token privacy in logs, and end-to-end exit criteria (P6.T1–P6.T6).
- Phase 5 complete: Measurement bench in `agentdesk-bench`:
  - `ScenarioGroundTruth`: Ground truth event and escalation export directly from scenario definition in `agentdesk-sim` (P5.4).
  - `FakeClient`: Scripted client tap policy (`TapPolicy::Default`) tracking `summaries_rendered`, `taps`, `log_pages_requested`, `duplicates`, and observed escalations (P5.2).
  - `agentdesk-bench` CLI & Runner: Multi-mode execution across `raw_lines`, `raw_events`, and `agentdesk` generating unified JSON reports (P5.1, P5.3).
  - Committed measurement report: `docs/measurements/2026-09-18-default-42.json` and `docs/measurements/README.md` (P5.5) proving 180.6× (99.45%) attention reduction with 100% preservation of attention-worthy events.
  - Success criteria updated in `docs/PROJECT.md` with measured numbers (P5.6).
  - Test coverage: 8 automated integration tests in `agentdesk-bench` verifying P5.T1 through P5.T8.
- Phase 4 complete: Runtime (core task, clock, task tracker/escalation, transport sinks, pipeline modes) in `agentdesk-core`:
  - `TaskTracker`: open/close tasks by `task_id`, per-operation duration thresholds table (`ThresholdTable`), watchdog escalation 0→1→2 on tick, task closing on `Completed`/`Error` superseding prior working entries.
  - `TransportSink`: trait with `send(&Message) -> Result<usize, SinkError>`, `as_any()`, `as_any_mut()`, implemented by `VecSink`, `CountingSink`, and `ChannelSink`. All outbound frames pass through sinks, updating `Metrics.transmitted_events` and `Metrics.transmitted_bytes`.
  - `CoreTask` & `CoreHandle`: single-threaded actor owning event store, queue, log store, tracker, and metrics. Receives `CoreCommand` via `tokio::sync::mpsc` without mutexes on core state. Handles adapter ingestion, client protocol requests (`respond_request`, `ack_event`, `dismiss_event`, `get_event_details`, `get_log_page`, `get_metrics`), ticks, snapshot dispatch, and sink registry.
  - Pipeline Modes: `PipelineMode::RawLines`, `PipelineMode::RawEvents`, and `PipelineMode::Agentdesk` selected at core initialization. `raw_*` modes bypass processor/queue and forward directly to sinks while updating metrics.
  - Interactive Request Resolution: `respond_request` communicates decisions back to adapters (`AdapterCommand::Respond`) unblocking simulator/agent tasks.
  - Concurrency model in `docs/SYSTEM_DESIGN.md` updated to match implemented channel layout.
  - Headless Integration & Golden Tests: deterministic execution across all three modes verified with virtual clock, simulator scenario, and committed golden files.
- Phase 3 complete: synchronous event pipeline in `agentdesk-core`:
  - `LogStore`: bounded per-agent ring buffer with monotonic line offsets, tailing, paging, pin windows (`[start-200, end+50]`) surviving ring eviction, and eviction detection.
  - `EventStore`: immutable in-memory storage for classified events.
  - `scoring`: pure scoring function calculating severity base, recency decay (30 min window), escalation bonus, and seen/resolved penalties clamped to 0..=100.
  - `PriorityQueue`: ordered snapshot by `(tier asc, score desc, seq desc)`, periodic tick re-scoring emitting deltas, and state machine (`ack`, `dismiss`, `respond_request`, `escalate`, `supersede`).
  - `EventProcessor`: strictly increasing global `seq`, `agent_seq` preservation, `log_range` attachment, classification, log pinning for Request/Error, and metrics tracking.
  - `Metrics`: flat diagnostic and measurement counters with JSON snapshotting.
  - `Pipeline`: synchronous composite binding processor, event store, queue, log store, and metrics.
  - Test coverage: 35 unit/property tests in `agentdesk-core` and end-to-end scenario pipeline test in `agentdesk-sim`.

## 2026-09-17

### Added

- Phase 2 complete: `agentdesk-core` classifier (19-row rules table, suffix fallbacks, cause-split cancellation), `Adapter` trait and `Clock` (`SystemClock`/`VirtualClock`), `agentdesk-sim` with validated JSON scenario format, seeded deterministic simulator, blocking requests with approve/deny, headless `run_to_end` driver, default scenario (1445 raw events, 99.4 % Working), `stats` example, and `docs/SIMULATOR.md`.
- Phase 1 complete: Rust workspace (`agentdesk-model`, `-core`, `-sim`, `-server`, `-bench`); `agentdesk-model` with event schemas, queue metadata, and the full 19-message wire protocol, all with serde round-trip tests; Flutter skeleton with four-tier placeholder home and widget test.

- Repository skeleton: `README.md`, `docs/`, empty `core/` and `mobile/` directories.
- Initial documentation set: PROJECT, ARCHITECTURE, SYSTEM_DESIGN, DATA_MODEL, EVENT_MODEL, COMMUNICATION, SECURITY, DECISIONS, TESTING, ROADMAP, TODO, CHANGELOG.

### Changed

- `welcome` now carries `pipeline_mode` and `transport` separately (previously a single ambiguous `mode`).
- `RawAgentEvent` gained an optional `request` field so adapters can supply approval prompt/options.

### Architecture

- Technology stack agreed: Flutter (mobile, foreground-only), Rust (laptop daemon), WebSocket + JSON, in-memory storage with bounded ring buffers, seeded simulator, Claude Code as the first real-agent target.
- Event model: immutable, separate events correlated by `task_id`; global `seq` and per-agent `agent_seq`; four categories (`request`, `error`, `completed`, `working`).
- Priority: tier by category, score within tier, periodic re-score; escalation raises score within the Working tier only and is surfaced via `escalation_level`.
- Acknowledgement tracked on the laptop (`new → seen → dismissed`), independent of request `resolution`.
- Level 2 details pushed with the event; Level 3 logs paged on demand; per-agent ring buffer with pinned windows for Request/Error events.
- Measurement: `raw_lines | raw_events | agentdesk` pipeline modes on one seeded run; headless bench results are the cited numbers.
- Security: shared token in the first frame plus `wss://` with self-signed certificate and SHA-256 fingerprint pinning; `--insecure-dev` loopback-only mode marked development-only.
