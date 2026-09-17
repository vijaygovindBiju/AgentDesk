# AgentDesk

A mobile attention and control system for coding agents.

Coding agents produce large volumes of terminal output and can run unattended for long periods. AgentDesk processes that activity on the laptop, classifies and prioritizes it, and sends only concise, human-relevant summaries to a phone. Detailed information stays on the laptop and is retrieved on demand.

> Optimize for human attention, not information volume.

The phone is **not** a second terminal. It is an attention and control interface.

## Status

Pre-implementation. Architecture and documentation are agreed; see `docs/TODO.md` for the current state.

## Layout

```text
AgentDesk/
├── docs/      Project documentation (start with PROJECT.md and ARCHITECTURE.md)
├── core/      Rust workspace: laptop daemon, event processing, simulator, bench
└── mobile/    Flutter client
```

## Documentation

| Document | Purpose |
|----------|---------|
| [docs/PROJECT.md](docs/PROJECT.md) | Problem, goals, MVP, success criteria |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Components and their responsibilities |
| [docs/SYSTEM_DESIGN.md](docs/SYSTEM_DESIGN.md) | Data flow, lifecycle, state transitions, failure behaviour |
| [docs/DATA_MODEL.md](docs/DATA_MODEL.md) | Entities |
| [docs/EVENT_MODEL.md](docs/EVENT_MODEL.md) | Event schema, categories, scoring |
| [docs/COMMUNICATION.md](docs/COMMUNICATION.md) | WebSocket protocol |
| [docs/SECURITY.md](docs/SECURITY.md) | Threat model and MVP security |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Engineering decision records |
| [docs/TESTING.md](docs/TESTING.md) | Test strategy |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Phases |
| [docs/TODO.md](docs/TODO.md) | Live status |
| [docs/CHANGELOG.md](docs/CHANGELOG.md) | Meaningful changes |
