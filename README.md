# AgentDesk

AgentDesk is a mobile attention and control system for coding agents. It processes agent activity on a laptop, ranks the events that need a person, and presents concise summaries on a phone. Detailed information remains on the laptop and is fetched only when needed.

> Optimize for human attention, not information volume.

The phone is an attention and control interface, not a second terminal.

## Status

**The AgentDesk MVP is complete.** Phases 0–9 of the MVP roadmap are complete, including simulator-based validation, a secure LAN transport, and the Flutter client. The next major milestone is AgentDesk v1: a real Claude Code adapter and validation against real coding-agent behaviour.

## Implemented MVP

### Architecture

```text
Simulated coding agent → Adapter → Raw events → Processor → Classifier
    → Priority queue / scoring / watchdog → secure WebSocket → Flutter client
                                              ↓
                              details, paged logs, and control commands
```

The Rust laptop-side workspace provides:

- a shared event and WebSocket message model;
- a deterministic seeded simulator and scenario format;
- classification, immutable event storage, bounded per-agent log storage, priority queueing, scoring, and duration-based escalation;
- a headless benchmark that compares `raw_lines`, `raw_events`, and `agentdesk` modes;
- a WebSocket daemon supporting event snapshots, updates, details, paged logs, acknowledgement, dismissal, and approval/denial commands.

The Flutter client provides a foreground-only four-tier ranked event list, event details, backward-paged logs, approve/deny and dismiss actions, connection/debug status, reconnect handling, and persistent connection setup.

### Security model

The normal transport is `wss://`. Each connection uses a shared token during the handshake, TLS with a self-signed certificate, and SHA-256 certificate fingerprint pinning in the Flutter client. The phone stores its token only through OS-backed secure storage; URL, device ID, and fingerprint are non-secret preferences. The development-only `--insecure-dev` mode uses plain `ws://` and is restricted to loopback on both the daemon and client.

### Validation status

The repository currently has:

- **112 passing Rust tests** covering the model, core pipeline, simulator, benchmark, server, TLS, and integration flows;
- **42 passing Flutter tests** covering models, state, UI, TLS pinning, persistent secure configuration, and end-to-end flows;
- a clean `flutter analyze` result.

Run the checks with:

```sh
cd core && cargo test --workspace
cd ../mobile && flutter test && flutter analyze
```

### Simulator benchmark result

On the committed seeded simulator scenario (seed 42), `agentdesk` surfaced **8** human-facing events, compared with **1,445** in `raw_events` and **1,455** in `raw_lines`: a **99.45% reduction** versus raw events (180.6× fewer surfaced events). The scenario retained every simulated request and error, and the benchmark exercises escalation, log retrieval, and a request response.

This is **simulator validation**, not evidence from a real coding agent. The scenario is deterministic and useful for repeatable regression measurement, but it does not yet establish performance, event coverage, or usability with Claude Code or any other real agent. See [the measurement notes](docs/measurements/README.md) for methodology and caveats.

### Current limitations

- No real Claude Code or other coding-agent adapter is implemented yet.
- State is in memory only; it does not survive daemon restarts and has no offline command/event sync.
- Pairing, multiple devices, device revocation, NAT traversal, cloud components, and background mobile notifications are out of scope.
- The Flutter client is foreground-only.
- The benchmark is simulator-based; real coding-agent, real-device battery, and real-world network validation remain to be done.

## Next: AgentDesk v1

The first v1 milestone is a **real Claude Code adapter**. It should map Claude Code's supported hooks or structured output into the existing adapter/event model, preserve the existing security and control boundaries, and be validated with representative real coding-agent sessions.

v1 work should keep simulator validation as a fast, deterministic regression suite while adding a separate real-agent validation plan. That plan should measure event coverage, false positives/negatives, task correlation, approval/control behaviour, long-running-task escalation, and the attention-filtering reduction on real sessions before claiming real-agent support.

## Layout

```text
AgentDesk/
├── docs/      Project documentation and validation evidence
├── core/      Rust workspace: model, pipeline, simulator, server, and bench
└── mobile/    Flutter client
```

## Installation and local development

### Prerequisites

- Rust and Cargo (edition 2024 toolchain)
- Flutter SDK 3.12 or newer
- An Android emulator or physical Android device for the mobile client

### Install dependencies

```sh
cd core
cargo fetch

cd ../mobile
flutter pub get
```

### Run the simulator and mobile client

Start the local daemon in development mode:

```sh
cd core
cargo run -p agentdesk-server --bin agentdesk -- \
  run \
  --scenario agentdesk-sim/scenarios/default.json \
  --seed 42 \
  --mode agentdesk \
  --insecure-dev
```

For an Android emulator, forward the daemon's loopback port before starting Flutter:

```sh
adb reverse tcp:8765 tcp:8765
cd mobile
flutter run
```

The daemon prints the connection token at startup. Configure the mobile client with
`ws://127.0.0.1:8765`, the printed token, and the development connection settings.
The `--insecure-dev` option is loopback-only and must not be used for LAN or production
connections.

### Run a real agent

The daemon can also connect to an ACP-compatible agent or the Antigravity PTY adapter:

```sh
cd core
cargo run -p agentdesk-server --bin agentdesk -- \
  run \
  --agent "gemini --skip-trust --acp" \
  --prompt "Explain the purpose of this project." \
  --insecure-dev
```

Use `--antigravity --agent "agy"` instead of `--agent` for the Antigravity adapter.

## Documentation

| Document | Purpose |
|----------|---------|
| [docs/PROJECT.md](docs/PROJECT.md) | Problem, goals, MVP, and success criteria |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Components and responsibilities |
| [docs/SYSTEM_DESIGN.md](docs/SYSTEM_DESIGN.md) | Data flow, lifecycle, and failure behaviour |
| [docs/COMMUNICATION.md](docs/COMMUNICATION.md) | WebSocket protocol |
| [docs/SECURITY.md](docs/SECURITY.md) | Threat model and security design |
| [docs/TESTING.md](docs/TESTING.md) | Test strategy |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Phase overview |
| [docs/TODO.md](docs/TODO.md) | Authoritative implementation status |
| [docs/measurements/README.md](docs/measurements/README.md) | Benchmark method and limitations |
