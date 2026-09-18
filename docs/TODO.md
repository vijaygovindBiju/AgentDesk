# AgentDesk — TODO

Single source of truth for implementation progress. Phases are derived from the dependency order of the approved architecture (ARCHITECTURE.md, SYSTEM_DESIGN.md, EVENT_MODEL.md, COMMUNICATION.md, SECURITY.md, DECISIONS.md).

Legend: `[ ]` not started · `[~]` in progress · `[x]` completed (implementation + tests + docs) · `[!]` blocked / decision required

---

## Git Checkpoint Rule (applies to all phases)

Git commits are part of the development workflow, not an afterthought.

After every meaningful completed task or task group:

1. Run the relevant tests and validation.
2. Confirm the implementation is in a known-good state.
3. Update `TODO.md` with the current status, checklist, and next action.
4. Update relevant documentation if the change affects documented behaviour or architecture.
5. Create a Git commit with a meaningful message.
6. Record the Git commit (hash and summary) in `TODO.md` under the Status section and completed phase.

Commit messages must describe the actual change, using `type(scope): summary`:

- Good: `feat(model): add event schemas` · `test(model): add serialization tests` · `feat(core): implement classifier` · `test(sim): add deterministic scenario coverage` · `docs: define escalation behaviour`
- Not acceptable: `update` · `changes` · `work` · `fix` · `phase 1`

Do not commit broken or knowingly incomplete states unless there is a specific reason to preserve that checkpoint (say so in the message body).

Prefer small, logical commits that represent one coherent change and leave the repository easy to understand and revert.

**Milestone sequence**: tests/validation → documentation → `TODO.md` status/checklist → Git commit → record Git commit in `TODO.md`.

The Git history should read as a readable development history of AgentDesk.

---

## Status

| | |
|---|---|
| **Current Phase** | Phase 6 — Transport server (token, development mode) (awaiting approval to start) |
| **Current Task** | — |
| **Next Action** | On approval, start Phase 6 with P6.1: `agentdesk-server`: `tokio-tungstenite` listener; per-connection task with a bounded outbound channel implementing `TransportSink`; registers with the core task. |
| **Last Commit** | `048222d` `docs(todo): record commit c6d7dde for phase 4 completion` |
| **Overall MVP progress** | Phases 0–5 complete · 5 / 9 implementation phases · 60 % of checklist items |
| **Blocked / Needs Decision** | None |

---

## Phase 0 — Design (complete · commit efbf19e)

Goal: agree problem, stack, architecture, and decisions before writing code.

- [x] Project understanding confirmed
- [x] Stack chosen: Flutter, Rust, WebSocket + JSON, in-memory, seeded simulator, Claude Code as first real target
- [x] Seven architecture decisions + amendments recorded in DECISIONS.md
- [x] Initial `docs/` set written and cross-checked

Exit criteria met: developer approved architecture with amendments; all decisions recorded.

---

## Phase 1 — Foundation: workspace and shared model (complete · commit ec18dfc)

**Goal**: a buildable monorepo and the shared data contract (`agentdesk-model`) that every other component depends on.

**Why now**: the event schema and message envelope are the protocol between laptop and phone; nothing downstream can be written or tested without them. Getting serde behaviour (unknown-field tolerance, required fields, `schema_version`) right here avoids a protocol rewrite later.

**Depends on**: Phase 0.

### Tasks

- [x] P1.1 Rust workspace at `core/` with crates `agentdesk-model`, `agentdesk-core`, `agentdesk-sim`, `agentdesk-server`, `agentdesk-bench` (empty `lib.rs`/`main.rs`). Done when `cargo build && cargo test` succeed from `core/`.
- [x] P1.2 `.gitignore` covering `core/target`, Flutter build artefacts, and the daemon config dir pattern. Done when `git status` after a build shows no artefacts.
- [x] P1.3 `agentdesk-model`: `RawAgentEvent`, `Event`, `Details`, `LogRange`, `RequestInfo`, `Category`, `Severity`, `Operation` as in EVENT_MODEL.md. Done when types compile with serde derives and `schema_version` is a constant.
- [x] P1.4 `agentdesk-model`: `QueueEntry`, `EntryState`, `Resolution` as in DATA_MODEL.md. Done when types compile with serde.
- [x] P1.5 `agentdesk-model`: envelope `Message { type, request_id, payload }` and every message type in COMMUNICATION.md as a tagged enum (`hello`, `welcome`, `snapshot`, `event`, `score_update`, `state_update`, `raw_event`, `raw_line`, `get_event_details`, `event_details`, `get_event_logs`, `event_logs`, `ack`, `dismiss`, `respond_request`, `command_result`, `get_metrics`, `metrics`, `error`). Done when a JSON sample of each round-trips.
- [x] P1.6 Flutter project skeleton at `mobile/` (`flutter create`, package name `agentdesk`), placeholder home screen. Done when `flutter analyze` and `flutter test` pass.
- [x] P1.7 README: how to build/test both halves.

### Validation

- [x] P1.T1 Serde round-trip test for every message type and for `Event` / `RawAgentEvent` / `QueueEntry`.
- [x] P1.T2 Unknown fields are ignored on decode; missing required fields are rejected (one test each on `Event`).
- [x] P1.T3 `Category` serialises to exactly `request | error | completed | working`; `Operation` to the six documented values.

### Exit criteria

`cargo test` and `flutter test` green; every message in COMMUNICATION.md has a Rust type; docs unchanged or updated to match any schema adjustment made while typing it.

---

## Phase 2 — Classification and simulated agent (complete · commit 818e320)

**Goal**: the first hypothesis-bearing logic (classifier) and a realistic, reproducible input source (simulator).

**Why now**: both depend only on the model. The simulator is needed as a test fixture for every later phase, so it must exist before the pipeline, not after. The classifier is pure and can be fully verified in isolation.

**Depends on**: Phase 1.

### Tasks

- [x] P2.1 `agentdesk-core::classifier`: rules table keyed on `kind` → `(Category, Severity)`; prefix fallbacks (`*_failed`, `*_completed`); unknown → `working/0`. Done when `classify()` is a pure function over a data table.
- [x] P2.2 Document the initial rules table in EVENT_MODEL.md (kinds used by the simulator).
- [x] P2.3 `agentdesk-sim`: `Adapter` trait (`next() -> Option<AdapterOutput>` where output is a raw event or a log line), defined in `agentdesk-core` so future adapters implement the same trait.
- [x] P2.4 `agentdesk-sim`: scenario format (agents, tasks with `operation`, steps: progress bursts, completions, errors with file/line, approval requests that block until answered, cancellations, long-running tasks). Done when a scenario file parses and validates.
- [x] P2.5 `agentdesk-sim`: seeded RNG (`rand` with `StdRng::seed_from_u64`) and a virtual clock (`Clock` trait) so time is compressed in tests. Done when two runs with the same seed produce byte-identical output.
- [x] P2.6 `agentdesk-sim`: default scenario with realistic Working density (per-line progress ticks) and at least: 2 agents, 1 request, 2 errors, 3 completions, 1 long-running build, 1 `cancelled_by_user`, 1 `cancelled_by_agent`. Committed under `core/agentdesk-sim/scenarios/`.
- [x] P2.7 `agentdesk-sim`: `respond(task_id, decision)` unblocks a waiting request. Done when a blocked task resumes after approve and terminates after deny.

### Validation

- [x] P2.T1 Table-driven classifier test: every rule row asserted.
- [x] P2.T2 Fallback tests: `foo_failed`, `foo_completed`, unknown kind (no panic, `working/0`).
- [x] P2.T3 Simulator determinism test (same seed ⇒ identical sequence).
- [x] P2.T4 Simulator emits strictly increasing `agent_seq` per agent.
- [x] P2.T5 Default scenario contains the documented minimum mix (counted by kind).

### Exit criteria

Classifier and simulator fully tested; default scenario committed; EVENT_MODEL.md rules table matches code.

---

## Phase 3 — Event pipeline (synchronous core) (complete · commit 8f4cd89)

**Goal**: turn `RawAgentEvent`s into stored, classified, queued, scored events with log references — the heart of the hypothesis.

**Why now**: needs model, classifier and (for tests) the simulator. It is deliberately synchronous and clock-injected so it can be exhaustively unit-tested before any async runtime or network is involved.

**Depends on**: Phase 2.

### Tasks

- [x] P3.1 Log Store: per-agent ring buffer with monotonically increasing offsets, `append`, `page(offset, limit)`, `tail(limit)`, `pin(event_id, range)`, `evicted` detection. Configurable capacity (default 10 000) and pin window (200 before / 50 after), page cap 500.
- [x] P3.2 Event Store: `insert`, `get`, immutable after insert.
- [x] P3.3 Scoring: `score(entry, event, now) -> u16` with the constants in EVENT_MODEL.md in one module.
- [x] P3.4 Priority Queue: `QueueEntry` map; `insert`, `ordered_snapshot()` by `(tier, score desc, seq desc)`; `rescore(now) -> Vec<ScoreUpdate>` returning only changed entries; state machine `ack/dismiss/respond/supersede/escalate` returning `StateUpdate` or a typed error (`no_such_event`, `not_a_request`, `already_resolved`).
- [x] P3.5 Event Processor: assigns `event_id`, global `seq`, `ts`, `log_range`; calls classifier; pins logs for `request`/`error`; inserts into Event Store and Queue; returns the `Event` and side-effects.
- [x] P3.6 Metrics struct with the counters listed in ARCHITECTURE.md, incremented by the processor/queue/log store; `snapshot() -> serde_json::Value`.
- [x] P3.7 Update DATA_MODEL.md / EVENT_MODEL.md if any field changed while implementing.

### Validation

- [x] P3.T1 Log Store: eviction keeps offsets increasing; tail returns true start offset; `evicted: true` when appropriate; pinned window survives full rotation; window clamps at buffer start; `limit` capped; `limit: 0` ⇒ empty ok.
- [x] P3.T2 Queue ordering test + property test: different tiers ⇒ lower tier first regardless of score/escalation/state.
- [x] P3.T3 Property test: within Working, escalation level 1 outscores level 0 of equal age; level 2 outscores level 1.
- [x] P3.T4 Scoring: recency decays to 0 at 30 min and not below; seen and resolved penalties applied; clamp 0..=100.
- [x] P3.T5 State machine: all transitions in SYSTEM_DESIGN.md incl. idempotent ack, dismiss-from-new, `resolution` independent of `state`, error variants.
- [x] P3.T6 Dismissed/superseded excluded from snapshot but fetchable from Event Store.
- [x] P3.T7 Processor: strictly increasing `seq`; `agent_seq` preserved; `log_range` matches ring offsets; pins only for request/error; malformed raw event dropped and counted.
- [x] P3.T8 Pipeline test driven by the simulator: every Request/Error in the scenario appears exactly once in the queue.

### Exit criteria

All Phase 3 tests green; memory bounded by configuration (asserted by a test that appends 3× capacity); docs match code.

---

## Phase 4 — Runtime: core task, time, escalation, modes (complete · commit c6d7dde)

**Goal**: run the pipeline as a single async core task with a clock, tick-driven re-scoring and escalation, a transport-sink abstraction, and the three pipeline modes.

**Why now**: the Task Tracker/Watchdog needs the queue and a clock; the bench and the server both need a core they can drive through channels and a sink they can observe. Splitting this from Phase 3 keeps synchronous logic separate from concurrency.

**Depends on**: Phase 3.

### Tasks

- [x] P4.1 `Clock` trait with real and virtual implementations; `tokio` runtime added to `agentdesk-core`.
- [x] P4.2 Task Tracker: open/close tasks by `task_id`; per-operation threshold table (EVENT_MODEL.md) injected; on tick, escalate latest Working entry 0→1→2 and stop; on close, supersede the task's Working entries.
- [x] P4.3 `TransportSink` trait: `send(&Message)` returning bytes written; `CountingSink`/`VecSink` for tests; all outbound frames go through it (this is where `transmitted_events`/`transmitted_bytes` are counted).
- [x] P4.4 Core task: owns Event Store, Queue, Log Store, Tracker, Metrics; `mpsc` inbox for `AdapterOutput`, `ClientCommand { client_id, Message }`, `Tick`; dispatches replies/pushes to sinks by `client_id` / broadcast. No shared mutexes on core state.
- [x] P4.5 Pipeline mode enum `raw_lines | raw_events | agentdesk` selected at core construction; `raw_*` modes bypass processor/queue and forward directly to sinks while still counting metrics.
- [x] P4.6 `get_metrics` handled by the core task.
- [x] P4.7 Request response path: `respond_request` reaches the adapter (`respond(task_id, decision)`) so the simulator unblocks.
- [x] P4.8 SYSTEM_DESIGN.md concurrency section updated to match the implemented channel layout.

### Validation

- [x] P4.T1 Tracker: escalates at `expected` and `2×expected`, never a third time; closing event supersedes and stops escalation; task without Working never escalates; event without `task_id` never tracked.
- [x] P4.T2 Tick emits `score_update` only for changed entries.
- [x] P4.T3 Core task end-to-end with virtual clock and `VecSink`: simulator scenario ⇒ ordered outbound messages; golden-file comparison.
- [x] P4.T4 Mode test: `raw_events` forwards every raw event; `raw_lines` forwards every line; `agentdesk` forwards only queue-derived messages.
- [x] P4.T5 A `respond_request` on a blocked simulated task resumes it and produces the subsequent completion event.

### Exit criteria

Core runs headless from a simulator to a sink, deterministically, in all three modes; golden file committed.

---

## Phase 5 — Measurement bench (complete)

**Goal**: answer the core question numerically before building any UI: does `agentdesk` mode transmit and surface materially less than `raw_events` and `raw_lines` on the same seeded run?

**Why now**: everything it needs exists after Phase 4; it costs little and de-risks the project — if the numbers are unconvincing, the design is revisited before investing in transport and mobile.

**Depends on**: Phase 4.

### Tasks

- [x] P5.1 `agentdesk-bench` binary: args `--scenario --seed --mode --tap-policy`, runs the core task with a fake client sink.
- [x] P5.2 Fake client with scripted tap policy (default: open every Request and Error, request one tail log page per Error, approve every Request after N virtual seconds). Counts `summaries_rendered`, `taps`, `log_pages_requested`.
- [x] P5.3 JSON report combining laptop Metrics and client counters; `--all-modes` runs all three and emits one comparison document with two sections: **reduction** (raw lines, raw events, AgentDesk surfaced events, reduction ratio vs each baseline, bytes per mode) and **coverage** (per-category counts expected vs surfaced, duplicates, escalations expected vs observed).
- [x] P5.4 Scenario ground truth: the simulator exports the expected set of important events (every Request, every Error, every Completed with severity ≥ 2, every task expected to escalate) so coverage is checked against the scenario, not against AgentDesk's own output.
- [x] P5.5 Commit the first comparison to `docs/measurements/<date>-<scenario>-<seed>.json` with a short human summary in `docs/measurements/README.md` (what was measured, how to reproduce, caveats about simulator density).
- [x] P5.6 Update PROJECT.md success criteria status with the measured numbers (no claims beyond the numbers).

### Validation

Success is **reduction AND preservation**; a run that reduces volume by losing important events fails.

- [x] P5.T1 Reduction: `agentdesk.surfaced_events < raw_events.transmitted_events < raw_lines.transmitted_events` for the default scenario; reduction ratios reported.
- [x] P5.T2 Coverage — Requests: every scenario Request surfaced exactly once.
- [x] P5.T3 Coverage — Errors: every scenario Error surfaced exactly once.
- [x] P5.T4 Coverage — Completed: every scenario Completed with severity ≥ 2 surfaced; lower-severity completions reported but not required.
- [x] P5.T5 Escalation: every task the scenario marks as over-long reaches `escalation_level` 1 and then 2 at the designed times, stays in the Working tier, and never outranks any Request/Error/Completed entry; no task escalates a third time.
- [x] P5.T6 No silent loss: every raw event that the classifier maps to `request` or `error` has a corresponding surfaced entry (checked from the raw stream, independent of the scenario ground truth).
- [x] P5.T7 Duplicate control: no `event_id` is pushed as `event` more than once per connection; `score_update`/`state_update` counts are reported and bounded (≤ ticks × live entries).
- [x] P5.T8 Bench run is reproducible: same args ⇒ identical report (excluding wall-clock fields).

### Exit criteria

Committed, reproducible measurement showing both reduction and full important-event coverage; success criteria 1–2 and 4 of PROJECT.md checked headlessly. **Decision point**: proceed to transport, or revisit classifier/scoring first.

---

## Phase 6 — Transport server (token, development mode)

**Goal**: expose the core over WebSocket with the full message catalogue and token authentication, using the loopback-only `--insecure-dev` mode so the Flutter client can be developed before TLS lands.

**Why now**: first phase that needs a network; the protocol is already fixed by the model crate, and the core is already driven by channels, so this is mostly wiring. TLS is deferred to its own phase so certificate work cannot block UI development.

**Depends on**: Phase 4 (Phase 5 recommended first).

### Tasks

- [ ] P6.1 `agentdesk-server`: `tokio-tungstenite` listener; per-connection task with a bounded outbound channel implementing `TransportSink`; registers with the core task.
- [ ] P6.2 Handshake: first frame must be `hello`; constant-time token compare; `welcome` (with `pipeline_mode` and `transport`) then `snapshot`; close `4001` on failure, `4002` on schema mismatch.
- [ ] P6.3 Request/reply dispatch with `request_id` echo; `error` reply for malformed frames; `get_event_details` marks `seen`.
- [ ] P6.4 Slow-client handling: full outbound channel ⇒ close `4003`, counted in `slow_client_disconnects`.
- [ ] P6.5 Token lifecycle: generate 256-bit token on first run into the config dir (`0600`); `agentdesk token show|rotate`; refuse non-loopback bind without a token.
- [ ] P6.6 `--insecure-dev`: plain `ws://`, forced `127.0.0.1`, loud warning, `transport: "insecure_dev"` in `welcome`. Rejected in combination with any other bind address.
- [ ] P6.7 `agentdesk` binary: `run --scenario --seed --mode [--insecure-dev]` wiring simulator + core + server; startup prints address and token.
- [ ] P6.8 Logging: token and payload contents never at info level; document the debug flag.

### Validation

- [ ] P6.T1 Handshake tests: correct token, wrong token, non-hello first frame, schema mismatch.
- [ ] P6.T2 Every request type gets exactly one reply with the same `request_id`.
- [ ] P6.T3 Malformed JSON ⇒ `error`, connection stays open.
- [ ] P6.T4 Slow client ⇒ `4003`, core keeps processing (assert later events still reach a second client).
- [ ] P6.T5 `--insecure-dev` with a non-loopback bind is rejected at startup.
- [ ] P6.T6 Captured logs at default level contain no token.

### Exit criteria

A test client can connect over loopback, receive snapshot and pushes, page logs, approve a request, and observe the simulator continue.

---

## Phase 7 — Flutter client

**Goal**: the attention interface: ranked four-tier home, event details with actions, paged log viewer, debug metrics.

**Why now**: needs a running server (Phase 6) to develop against; uses `--insecure-dev` via `adb reverse`/emulator until Phase 8.

**Depends on**: Phase 6.

### Tasks

- [ ] P7.1 Dart models mirroring `agentdesk-model` (hand-written) with `fromJson`/`toJson`.
- [ ] P7.2 Connection service: `web_socket_channel`, `hello`, request/reply correlation with timeouts, reconnect with jittered exponential backoff (1s → 30s), status stream. Settings screen for address + token (fingerprint field added in Phase 8).
- [ ] P7.3 State: `EventStore` and `QueueView` reducers for `snapshot` (wholesale replace), `event`, `score_update`, `state_update`; ordering `(tier, score desc, seq desc)`; duplicate handling per COMMUNICATION.md.
- [ ] P7.4 Home screen: four sections in tier order; entry shows category colour, agent, project, summary, message; escalation badge for `escalation_level ≥ 1`; "still blocking" indicator for dismissed-but-unresolved requests; empty-state copy.
- [ ] P7.5 Event screen: Level 2 details; sends `get_event_details` on open; `Approve`/`Deny` for requests; `Dismiss`; `View logs`; shows `command_result` errors inline.
- [ ] P7.6 Log viewer: tail-first page, "load earlier" paging backwards, "earlier logs no longer available" on `evicted`.
- [ ] P7.7 Debug screen: connection status, `transport` banner (red for `insecure_dev`), client counters `summaries_rendered`, `taps`, `log_pages_requested`, laptop `get_metrics` result.
- [ ] P7.8 Update ARCHITECTURE.md mobile section if the screen structure changed.

### Validation

- [ ] P7.T1 Model decode tests for every laptop → phone message.
- [ ] P7.T2 Reducer tests: each message type produces the expected view; snapshot replaces stale entries; duplicate `event` ignored but entry applied; unknown-id updates ignored.
- [ ] P7.T3 `QueueView` ordering matches the laptop rule (shared fixture with Rust golden output).
- [ ] P7.T4 Widget tests: escalation badge; dismissed-but-unresolved request indicator; insecure-dev banner.
- [ ] P7.T5 Manual: full flow on emulator against `--insecure-dev` server (checklist recorded here when executed).

### Exit criteria

Summary → details → logs → approve works end to end on loopback; `flutter test` green.

---

## Phase 8 — TLS and certificate pinning

**Goal**: make `wss://` with a self-signed certificate and SHA-256 fingerprint pinning the normal path, per SECURITY.md.

**Why now**: both endpoints exist, so pinning can be tested for real. Deferred until here so certificate work never blocked pipeline or UI progress. The MVP is not complete without this phase.

**Depends on**: Phases 6 and 7.

### Tasks

- [ ] P8.1 Server: `rcgen` self-signed certificate generated on first run into the config dir; `rustls` acceptor; startup prints SHA-256 fingerprint next to address and token.
- [ ] P8.2 Server default is `wss://`; `--insecure-dev` remains loopback-only.
- [ ] P8.3 Flutter: `SecurityContext` with `badCertificateCallback` accepting only the pinned SHA-256 (DER); fingerprint field in settings; clear error when mismatch.
- [ ] P8.4 SECURITY.md updated with actual file locations, rotation command, and any deviations.

### Validation

- [ ] P8.T1 Rust test: client with pinned fingerprint connects; client with different fingerprint fails the handshake.
- [ ] P8.T2 Flutter unit test for the fingerprint comparison (positive/negative).
- [ ] P8.T3 Manual: phone on LAN connects over `wss://`; wrong fingerprint is rejected with a readable message.

### Exit criteria

Normal operation is `wss://` + token + pinning on a real LAN; `--insecure-dev` is only used for emulator work.

---

## Phase 9 — Live validation and write-up (MVP complete)

**Goal**: run the demo path on a real device, re-run the headless bench, and record honest results and known limitations.

**Why now**: the last integration step; produces the artefact that decides whether AgentDesk proceeds beyond prototype.

**Depends on**: Phase 8.

### Tasks

- [ ] P9.1 End-to-end checklist on a real Android device over LAN `wss://`: connect, receive snapshot, see new events, badge on long-running task, open details, page logs, approve request, see completion, dismiss, reconnect after airplane mode.
- [ ] P9.2 Re-run bench (`--all-modes`) from a committed seed; commit report.
- [ ] P9.3 `docs/measurements/README.md`: results, method, caveats (simulator density, no real agent yet), and an explicit statement of what has *not* been shown.
- [ ] P9.4 Review the within-tier escalation limitation against the live run; record findings and, if needed, a new decision in DECISIONS.md.
- [ ] P9.5 CHANGELOG, ROADMAP, PROJECT.md success-criteria status updated.

### Validation

- [ ] P9.T1 All Rust and Flutter tests green on the tagged commit.
- [ ] P9.T2 Checklist P9.1 executed and recorded with date and device.

### Exit criteria

Success criteria 1–5 in PROJECT.md evaluated with evidence; MVP tagged.

---

## Post-MVP (not scheduled)

- [ ] Claude Code adapter: investigate hooks / structured output vs PTY parsing; decision record; adapter + classifier rows.
- [ ] SQLite persistence and offline event/command queues with sequence-based sync (SYSTEM_DESIGN.md "Offline behaviour").
- [ ] QR pairing (address + token + fingerprint), multiple devices, permissions, revocation.
- [ ] Learned per-operation duration baselines.
- [ ] Background notifications.
- [ ] Revisit: tick re-scoring vs re-score-on-snapshot if the phone list reorders too often.
