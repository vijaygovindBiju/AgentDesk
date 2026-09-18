# AgentDesk — Changelog

## 2026-09-18

### Added

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
