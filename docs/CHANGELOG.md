# AgentDesk — Changelog

## 2026-09-17

### Added

- Repository skeleton: `README.md`, `docs/`, empty `core/` and `mobile/` directories.
- Initial documentation set: PROJECT, ARCHITECTURE, SYSTEM_DESIGN, DATA_MODEL, EVENT_MODEL, COMMUNICATION, SECURITY, DECISIONS, TESTING, ROADMAP, TODO, CHANGELOG.

### Architecture

- Technology stack agreed: Flutter (mobile, foreground-only), Rust (laptop daemon), WebSocket + JSON, in-memory storage with bounded ring buffers, seeded simulator, Claude Code as the first real-agent target.
- Event model: immutable, separate events correlated by `task_id`; global `seq` and per-agent `agent_seq`; four categories (`request`, `error`, `completed`, `working`).
- Priority: tier by category, score within tier, periodic re-score; escalation raises score within the Working tier only and is surfaced via `escalation_level`.
- Acknowledgement tracked on the laptop (`new → seen → dismissed`), independent of request `resolution`.
- Level 2 details pushed with the event; Level 3 logs paged on demand; per-agent ring buffer with pinned windows for Request/Error events.
- Measurement: `raw_lines | raw_events | agentdesk` pipeline modes on one seeded run; headless bench results are the cited numbers.
- Security: shared token in the first frame plus `wss://` with self-signed certificate and SHA-256 fingerprint pinning; `--insecure-dev` loopback-only mode marked development-only.
