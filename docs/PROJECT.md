# AgentDesk — Project

## Problem

Coding agents emit large amounts of terminal output and can run for long periods without needing a human. When several agents run in parallel, a developer has two bad options:

1. Watch every terminal continuously and waste attention on `Compiling 213/500`.
2. Stop watching and miss the one moment an agent is blocked on an approval or has failed.

Existing tooling optimizes for information *volume*. Nothing optimizes for *when a human actually needs to look*.

## Target user

A developer running one or more coding agents on a laptop who wants to step away (other room, commute, meeting) and still be told when — and only when — an agent needs them.

## Proposed solution

A laptop-side daemon that:

- ingests agent activity through adapters into a common event model,
- classifies each event into an attention category and scores it,
- keeps a priority queue of attention-worthy events,
- pushes concise summaries to a phone over an authenticated, encrypted WebSocket,
- keeps all detailed information (event details, raw logs) on the laptop and serves it on demand,
- accepts control commands from the phone (approve/deny, acknowledge, dismiss).

## Core concept

```text
Coding Agent → Adapter → Raw Events → Processor → Classifier → Priority Queue
    → Transport → Phone: summary → tap → details → "View logs" → paged logs from laptop
```

Progressive disclosure:

| Level | Content | Delivery |
|-------|---------|----------|
| 1 — Summary | Category, agent, project, one-line message | Pushed |
| 2 — Details | Task, error, file, line, time | Pushed with the summary (small) |
| 3 — Full logs | Raw agent output around the event | Fetched on demand, paged |

## Principles

- **Optimize for human attention, not information volume.**
- The laptop is the source of truth for detailed information; the phone receives a smaller representation.
- Events are immutable and separate; correlation happens via `task_id`.
- Distinguish *information* from *information requiring human attention*.
- Measure the effect; do not claim reductions without numbers.

## Goals

- Demonstrate, with measurements, that AgentDesk reduces the volume of agent information a human must watch compared to (a) mirroring the terminal and (b) an unfiltered event feed.
- Keep the design extensible toward severity, duration, blocking state, acknowledgement state, and task importance without rewriting the core.
- Provide a clean adapter seam so real agents (first target: Claude Code) can be connected later.

## Non-goals (for the MVP)

- Being a remote terminal.
- Supporting every coding agent.
- Multiple phones, device pairing UX, revocation, production authentication.
- Offline queueing and synchronization (documented as future work in SYSTEM_DESIGN.md).
- Persistence across daemon restarts.
- Background execution / push notifications on the phone.
- Machine-learning classification.
- NATS or other messaging infrastructure; NAT traversal; cloud components.

## MVP

```text
1 seeded simulated agent
    → Rust laptop daemon (adapter, log store, processor, classifier, queue, watchdog, metrics)
    → wss:// + shared token, JSON messages
    → Flutter client (foreground only): ranked list by tier → event details → paged logs
    → approve / deny / acknowledge / dismiss commands back to the laptop
    → headless bench comparing raw_lines | raw_events | agentdesk on the same seeded run
```

## Future scope

- Claude Code adapter (hooks / structured output), then other agents.
- Persistent store (SQLite) and offline event/command queues with sequence-based sync.
- Device pairing via QR (address + token + cert fingerprint), multiple devices, revocation.
- Learned per-operation duration baselines for escalation.
- Background notifications.

## Success criteria

The MVP is successful if, on the same seeded simulation:

1. `agentdesk` mode transmits materially fewer events than `raw_events` mode, and the difference is reported honestly alongside `raw_lines`.
   - **Measured & Verified (2026-09-19, seed 42)**: Human-surfaced events reduced from 1,445 (`raw_events`) and 1,455 (`raw_lines`) to **8 events** in `agentdesk` mode (**180.6× reduction, 99.45% reduction**). Transmitted bytes are 1,067,291 B (`agentdesk`) vs 321,478 B (`raw_events`) and 192,974 B (`raw_lines`), honestly documenting the wire cost of structured JSON envelopes, Level 2 details, and tick score updates.
2. Every simulated Request and Error is surfaced on the phone (no attention-worthy event is lost to filtering).
   - **Measured & Verified (2026-09-19, seed 42)**: **100% preserved** (1/1 Requests, 3/3 Errors, 3/3 important completions with severity ≥ 2; 0 duplicate events, 0 silent loss).
3. The user can go from summary → details → paged logs, and approve/deny a request, from the phone.
   - **Verified**: Fully verified through headless bench (`taps: 8`, `log_pages_requested: 3`, `responses: 1`), Flutter UI widget tests (`phase7_exit_criteria_test.dart`, `home_screen_test.dart`), and secure LAN integration tests (`connection_tls_test.dart`, `phase9_e2e_test.dart`) exercising summary → details → backward-paged logs → approval flow over `wss://`.
4. A task that runs longer than its expected duration is visibly escalated on the phone without producing additional events.
   - **Measured & Verified**: `task-longbuild` escalated 0 → 1 at 5m and 1 → 2 at 10m via `score_update` metadata, moving to top of Working tier with visible badge without generating new event records. Verified in bench, unit tests, and UI widget tests.
5. All of the above is covered by tests described in TESTING.md.
   - **Verified**: 112 Rust tests in `core/` (`cargo test`) and 42 Flutter tests in `mobile/` (`flutter test`) passing with 100% success across the repository.

