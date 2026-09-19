# AgentDesk — Roadmap

High-level view only. The authoritative phase definitions, checklists, and status live in TODO.md. If a phase changes there, this file is updated to match.

## Dependency order

```text
Phase 1  Foundation ─── shared model + workspace
   │
Phase 2  Classifier + seeded simulator      (both depend only on the model)
   │
Phase 3  Event pipeline (synchronous core)  (log store, event store, queue, scoring, processor, metrics)
   │
Phase 4  Runtime                            (core task, clock, task tracker/escalation, sink, pipeline modes)
   ├──────────────┐
Phase 5  Bench    │                         (headless 3-mode measurement — decision point)
   │              │
Phase 6  Transport server (token, --insecure-dev loopback)
   │
Phase 7  Flutter client
   │
Phase 8  TLS + fingerprint pinning          (MVP security architecture becomes the default path)
   │
Phase 9  Live validation + write-up         (MVP complete)
   │
Post-MVP  Claude Code adapter · persistence/offline sync · pairing/multi-device · learned baselines · notifications
```

## Phase summary

| Phase | Delivers | Exit criteria (short) |
|-------|----------|-----------------------|
| 0 ✅ | Decisions and docs | Approved architecture, DECISIONS.md complete |
| 1 ✅ | Rust workspace, `agentdesk-model`, Flutter skeleton | `cargo test` + `flutter test` green; every protocol message typed |
| 2 ✅ | Classifier, `Adapter` trait, seeded simulator + default scenario | Table-driven classifier tests; deterministic simulator |
| 3 ✅ | Log store, event store, queue/scoring/state machine, processor, metrics | All pipeline unit + property tests; bounded memory |
| 4 ✅ | Core task, clock, task tracker/escalation, `TransportSink`, modes | Golden-file headless run in all three modes |
| 5 ✅ | Bench binary, fake client, JSON comparison report | Committed reproducible measurement; go/no-go on design |
| 6 ✅ | WebSocket server, token handshake, request/reply, dev mode | Loopback client completes the full flow |
| 7 ✅ | Flutter app: home, event, logs, debug | Summary → details → logs → approve on emulator |
| 8 ✅ | Self-signed TLS + SHA-256 pinning | `wss://` is the normal path on a real LAN |
| 9 ✅ | Live device run, final bench, write-up | PROJECT.md success criteria evaluated with evidence |

## Why this order

- Model first because it is the contract for everything, including the phone.
- Simulator before the pipeline because it is the test fixture for the pipeline.
- Measurement (Phase 5) before any network or UI so the core hypothesis is checked at the cheapest possible point.
- TLS after the client exists so pinning can be tested for real, but before the MVP is declared done — plain `ws://` is never the MVP's security architecture.

## Changing the roadmap

When implementation shows the structure is wrong: explain why, update TODO.md, update this file, and add a DECISIONS.md record if the change is architectural.
