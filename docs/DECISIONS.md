# AgentDesk — Decisions

Decision records for choices that shape the system. Newest at the bottom. Reopen a decision by adding a new record that supersedes it; do not edit history.

---

## Decision: Mobile framework — Flutter

Date: 2026-09-17

Problem: Need a mobile client quickly, Android first, foreground only for the MVP.

Options: Flutter; native Android (Kotlin/Compose); React Native/Expo.

Decision: Flutter.

Reason: Fast UI iteration, single codebase if iOS is wanted later, mature WebSocket and TLS-pinning support, and the developer's preference.

Trade-offs: Larger binary and a second toolchain vs. native; weaker background-service story than native (irrelevant while foreground-only).

Consequences: Event model is mirrored in Dart (hand-written for the MVP; generation considered later).

---

## Decision: Laptop daemon language — Rust

Date: 2026-09-17

Problem: Long-running daemon that will eventually wrap agent processes/PTYs and whose resource use we intend to measure.

Options: Rust; Dart (share code with Flutter); Go/Python/TypeScript.

Decision: Rust.

Reason: Predictable resource usage, strong typing for the event model, good async/WebSocket/TLS ecosystem, excellent fit for future PTY/process work.

Trade-offs: Slower iteration and a steeper curve than Dart/Python; no code sharing with the Flutter client.

Consequences: Model crate kept free of I/O so it stays simple to mirror; bench and tests live in Rust.

---

## Decision: Transport — WebSocket + JSON

Date: 2026-09-17

Problem: Bidirectional push and request/response between laptop and phone on a LAN.

Options: WebSocket; HTTP + SSE/polling; NATS/MQTT; gRPC.

Decision: WebSocket with a JSON envelope `{ type, request_id?, payload }`.

Reason: One persistent connection covers push and request/response; human-readable for debugging; first-class support on both ends; no infrastructure.

Trade-offs: JSON is verbose (measured, see bench); no built-in store-and-forward (future broker option kept open by the envelope design).

Consequences: Message catalogue in COMMUNICATION.md; `request_id` correlation; `transmitted_bytes` counted at the frame level.

---

## Decision: Storage — in-memory for the MVP

Date: 2026-09-17

Problem: Where events, queue state, and logs live.

Options: In-memory; SQLite; JSONL files.

Decision: In-memory, with bounded per-agent ring buffers for logs.

Reason: The MVP validates a filtering hypothesis, not durability. Bounded memory is required because we measure resource use.

Trade-offs: All state lost on daemon restart; no offline sync.

Consequences: `event_id`s are not stable across restarts; SQLite is the planned next step (ROADMAP).

---

## Decision: Events are immutable and separate; `task_id` correlates

Date: 2026-09-17

Problem: Model a task's progress as one mutable record or as a stream of events.

Options: Mutable task lifecycle; immutable separate events.

Decision: Immutable separate events with `task_id`.

Reason: Simpler reasoning, trivial deduplication, natural fit for an append-only future store. Mutable per-event data (state, score, escalation) is kept in the queue entry, not the event.

Trade-offs: A task's progress is several list items unless the UI groups them; `superseded` flag needed to hide stale Working entries.

Consequences: Watchdog cannot "update" an event; it updates queue metadata. Task Tracker is derived state.

---

## Decision: Global and per-agent sequence numbers

Date: 2026-09-17

Problem: Ordering and gap detection without relying on UUIDs.

Decision: Every event carries `seq` (global, monotonic per daemon run) and `agent_seq` (monotonic per agent). `event_id` is identity only.

Trade-offs: Two counters to maintain; `seq` resets on restart until persistence exists.

---

## Decision: Priority = tier first, score within tier; re-scored periodically

Date: 2026-09-17

Problem: How the queue orders events, and whether priority can change over time.

Options: (A) category-only ordering; (B) single numeric score with category as a term; (C) category tier first, score within tier.

Decision: (C), with scores recomputed on a tick. Score is queue metadata, not an event field.

Reason: Stable UI grouping by the four categories; still allows recency, acknowledgement, and escalation to affect order; extensible by adding score terms.

Trade-offs: Nothing can cross tiers via score (see escalation decision). Ticking re-score means the phone list can reorder; mitigated by pushing small `score_update` deltas and, if it proves noisy, re-scoring only on snapshot.

---

## Decision: Laptop-tracked acknowledgement; `seen` ≠ `resolved`

Date: 2026-09-17

Problem: Whether the laptop needs to know what the user has seen.

Options: (A) phone-local only; (B) laptop tracks `new/seen/dismissed` (+ `resolved` for requests); (C) requests only.

Decision: (B). `get_event_details` implies `seen`; explicit action sets `dismissed`; `respond_request` sets `resolution` independently.

Reason: Laptop is the source of truth; enables honest "surfaced vs seen" metrics and future multi-device sync.

Trade-offs: One more state machine and command type. The attention state and the outcome state are orthogonal — a dismissed request is still blocking the agent, and the UI must show that.

---

## Decision: Level 2 pushed with summary; Level 3 paged on demand

Date: 2026-09-17

Problem: Shape of detail/log retrieval.

Options: single `get_event` with a `level` field; separate typed messages; separate messages with paged logs.

Decision: Separate typed messages; `get_event_logs { offset, limit }` with `offset: -1` = tail; Level 2 `details` included in the pushed event.

Reason: Level 2 is small and saves a round trip on every tap; logs are the part that is actually large and benefits from paging. Distinct message types give precise schemas and separate metrics.

Trade-offs: "Detail requests" metric becomes primarily log-page requests; `get_event_details` is a re-fetch path. Paging adds client state.

---

## Decision: Log retention — per-agent ring buffer + pinned windows

Date: 2026-09-17

Problem: How much raw output to keep and how to attach it to events.

Options: unbounded per-event slices; bounded ring per agent with offsets; ring + pinned windows for attention events.

Decision: Ring buffer per agent (default 10 000 lines); events store `log_range` offsets; Request/Error events pin a copied window (default 200 lines before, 50 after).

Reason: Bounded memory, "what happened before the failure" is free, and the logs you actually want survive eviction.

Trade-offs: Two storage paths; evicted ranges for Working/Completed return `evicted: true`.

---

## Decision: Time-based escalation via score within the Working tier

Date: 2026-09-17

Problem: How "running longer than expected" becomes visible, given immutable events and only four categories.

Options considered across two rounds:
- Emit a new escalation event in a fifth `Attention` category (initial proposal — rejected by the developer to keep four categories).
- Score-first ordering so escalated Working entries overtake Completed.
- Promote an "effective tier" while keeping the category label.
- Strict tiers; escalation raises score within Working only.

Decision: Strict tiers. The Task Tracker raises `escalation_level` (0 → 1 at the per-operation expected duration, 1 → 2 at 2×, then stops) on the task's latest Working queue entry; this adds to its score within the Working tier and is pushed via `score_update`. The phone shows a badge. No new event, no new category.

Reason: Preserves the four-category model and predictable sectioning; keeps events immutable; escalation still surfaces (top of Working + badge) without generating notification noise.

Trade-offs: A stuck agent never outranks a Completed event by position; visibility depends on the badge. Recorded as a known limitation to revisit after measurement.

Consequences: Per-operation threshold table in EVENT_MODEL.md; simulator must declare `operation` per task.

---

## Decision: Measurement baseline — three modes on one seeded run

Date: 2026-09-17

Problem: An honest comparison target for "AgentDesk reduces information".

Options: raw log lines only; unfiltered adapter events only; both side by side.

Decision: Pipeline mode switch `raw_lines | raw_events | agentdesk`; the bench runs the same seed/scenario through all three with a scripted fake client and reports counters as JSON. Headless numbers are cited; live phone runs are demos.

Reason: `raw_lines` shows the cost of mirroring the terminal; `raw_events` is the fair "naive notification app" baseline; reporting both avoids quoting a strawman.

Trade-offs: Simulator event density influences the result; documented and kept realistic.

---

## Decision: MVP security — shared token + TLS with fingerprint pinning

Date: 2026-09-17

Problem: The daemon accepts approve commands; it must not be an open LAN endpoint.

Options: none; shared token over `ws://`; token + self-signed TLS with pinning; loopback + tunnel.

Decision: Token in the first frame plus `wss://` with a self-signed certificate pinned by SHA-256 fingerprint. Plain `ws://` exists only behind `--insecure-dev`, loopback-only, clearly marked.

Reason: Confidentiality, integrity, and authentication on shared networks with established libraries only; the token+fingerprint pair is exactly what QR pairing will carry later.

Trade-offs: Manual entry of fingerprint and token; certificate generation and Flutter pinning add a phase. Sequenced after the core pipeline so it cannot block UI work.

---

## Decision: `cancelled` is classified by cause

Date: 2026-09-17

Problem: A cancellation closes a task but is neither a success nor necessarily a failure. With exactly four categories it must land somewhere, and the initial draft (`working`/1) would hide unexpected stops at the bottom of the queue.

Options: (A) `working`/1; (B) `error`/1–2; (C) `completed`/1; (D) split by cause via `kind`.

Decision: (D). `cancelled_by_user` → `completed`/1. `cancelled_by_agent`, `aborted` → `error`/2 with logs pinned. Plain `cancelled` (cause unknown) → `completed`/1. All close the task in the Task Tracker. Summary text always reads "Cancelled".

Reason: Attention-worthiness depends on who stopped the task, not on the word "cancelled". The classifier is already keyed on `kind`, so this is table rows, not a model change, and preserves the four categories.

Trade-offs: Relies on adapters distinguishing cause; the unknown-cause default is deliberately the low-attention bucket because real agents usually surface unexpected stops as explicit errors anyway. The Completed section may contain non-successes, mitigated by explicit summary text.

Consequences: Simulator emits both `cancelled_by_user` and `cancelled_by_agent`; bench coverage counts `cancelled_by_agent`/`aborted` as Errors.
