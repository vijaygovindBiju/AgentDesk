# AgentDesk — Event Model

## Design rules

1. Events are **immutable** and **separate**. A task's progress is a sequence of events sharing a `task_id`, not one mutable record.
2. Identity (`event_id`, UUID v4) is never used for ordering. Ordering uses `seq` (global) and `agent_seq` (per agent), both strictly monotonic integers.
3. Exactly four categories in the MVP. Anything finer-grained is expressed through `severity`, `kind`, and queue metadata (score, escalation), not by adding categories.
4. Level 2 details travel with the event. Level 3 (logs) is referenced by `log_range` and fetched on demand.
5. The schema is versioned (`schema_version`) so the phone can refuse or adapt to incompatible daemons.

## RawAgentEvent (adapter → processor)

What an adapter produces. Agent-specific adapters normalize into this; nothing downstream sees agent-specific data.

```jsonc
{
  "agent_id": "sim-backend",
  "agent_seq": 87,
  "task_id": "task-auth-01",           // optional but strongly encouraged
  "kind": "build_failed",              // free-form but stable per adapter; classifier keys on it
  "operation": "build",                // build | test | install | analyze | edit | other
  "message": "Undefined variable: token",
  "details": {                         // adapter-supplied Level 2 fields, all optional
    "task": "Implement authentication",
    "file": "auth_service.dart",
    "line": 42,
    "error": "Undefined variable `token`"
  },
  "log_lines": ["..."],                // raw lines emitted alongside this event (may be empty)
  "request": {                         // only for kinds that block on a human decision
    "prompt": "Allow database migration on `users`?",
    "options": ["approve", "deny"]
  }
}
```

`task_id`, `details`, `log_lines`, and `request` are optional on the wire (absent ⇒ none/empty). `details` values are JSON scalars; keys are serialised in sorted order so output is deterministic for golden-file tests.

## Event (processor output, stored, pushed)

```jsonc
{
  "schema_version": 1,
  "event_id": "b1c2…",                 // uuid v4, identity only
  "seq": 1042,                         // global, monotonic per daemon run
  "agent_seq": 87,                     // per agent, monotonic
  "agent_id": "sim-backend",
  "agent_name": "Backend Agent",
  "project": "Hybrid",
  "task_id": "task-auth-01",
  "ts": "2026-09-17T10:32:05.123Z",
  "category": "error",                 // request | error | completed | working
  "severity": 3,                       // 0..3, see below
  "kind": "build_failed",              // passed through from the adapter
  "operation": "build",
  "summary": "Build Failed",           // Level 1: one line
  "message": "auth_service.dart:42 Undefined variable: token",  // Level 1: one line
  "details": {                         // Level 2
    "task": "Implement authentication",
    "error": "Undefined variable `token`",
    "file": "auth_service.dart",
    "line": 42
  },
  "log_range": { "start": 9310, "end": 9412, "pinned": true },  // Level 3 reference
  "request": {                         // present only when category = request
    "prompt": "Allow database migration on `users`?",
    "options": ["approve", "deny"]
  }
}
```

Field notes:

- `summary` and `message` are what the phone shows in the list. Together they must be enough to understand the event without opening it (PROJECT.md "Error handling" principle).
- `details` is a flat map of short strings/numbers. It is deliberately not a nested document; large content belongs in logs.
- `log_range` offsets are per-agent line offsets into the Log Store. `pinned: true` means the window was copied and will survive eviction.

## Categories and tiers

| Category | Tier | Meaning | Examples of `kind` |
|----------|------|---------|-------------------|
| `request` | 0 | Agent is blocked waiting for a human decision. | `approval_required`, `input_required`, `credential_required` |
| `error` | 1 | Something failed and the agent stopped or degraded. | `build_failed`, `test_failed`, `command_failed`, `adapter_error` |
| `completed` | 2 | A unit of work reached a terminal state (success, or an expected stop). | `build_completed`, `task_completed`, `tests_passed`, `cancelled_by_user`, `cancelled`* |
| `working` | 3 | Progress information; normally no attention. | `progress`, `started`, `waiting`, `installing` |

\* **Cancellation is classified by cause**, not by label. `cancelled_by_user` → `completed`/1 (expected, low attention, no log pin). `cancelled_by_agent` and `aborted` → `error`/2 (unexpected stop; logs pinned so "why did it stop?" is answerable). A plain `cancelled` whose cause the adapter cannot determine defaults to `completed`/1. All of these close the task in the Task Tracker. The summary text always says "Cancelled", never "Completed", so the Completed section cannot misread as success.

Tier is fixed by category. **Nothing moves an entry across tiers** — not severity, not escalation.

## Severity

`0..3`, assigned by the classifier from `kind`:

| Severity | Meaning |
|----------|---------|
| 0 | Routine (`progress`) |
| 1 | Notable (`started`, `cancelled_by_user`, `tests_passed`) |
| 2 | Important (`build_completed`, `test_failed`, `cancelled_by_agent`, `aborted`) |
| 3 | Critical (`build_failed`, `approval_required`, `credential_required`) |

## Classification

Pure function `classify(raw) -> Classification { category, severity }` driven by a rules table keyed on `kind`, with a prefix/fallback rule (`*_failed → error/3`, `*_completed → completed/2`, unknown → `working/0` + `unclassified_events` metric). The rules table is data, so a future Claude Code adapter can extend it without touching the classifier.

## Scoring (queue metadata, not part of the Event)

Score orders entries **within a tier** only. It is recomputed on each tick.

```text
score = base(severity)                   // 0:10, 1:25, 2:45, 3:70
      + recency_bonus(age)               // +20 at 0 min, linearly to 0 at 30 min  (request/error/completed)
      + escalation_bonus(level)          // 0:0, 1:+30, 2:+60                       (working only)
      - seen_penalty                     // −15 if state = seen
      - resolved_penalty                 // −40 if resolution ∈ {approved, denied}
clamped to 0..=100
```

Rationale: fresh critical items first; things you have already looked at sink; escalated Working tasks rise to the top of their tier and gain a visible badge. The exact constants are tunable and live in one place; TESTING.md requires property tests that ordinary Working entries never outscore escalated ones and that no term is large enough to matter across tiers (because tiers are compared first, this is structural, but the test documents the intent).

## Escalation

Handled by the Task Tracker; see SYSTEM_DESIGN.md. Effects on the model:

- `QueueEntry.escalation_level` on the task's latest Working event goes 0 → 1 → 2.
- A `score_update { event_id, score, escalation_level }` is pushed.
- No new event, no new category, no change to the stored Event.

Per-operation expected durations for the MVP (configurable):

| operation | expected |
|-----------|----------|
| `build` | 5 min |
| `test` | 3 min |
| `install` | 10 min |
| `analyze` | 2 min |
| `edit` | 2 min |
| `other` | 5 min |

The simulator uses compressed time so these are exercised in seconds during tests; the tracker takes the threshold table and clock as inputs.

## Extensibility hooks (not implemented in MVP)

- `impact`, `blocking`, `dependencies` can be added to `details` first and promoted to first-class fields when a consumer needs them.
- Learned duration baselines replace the static table behind the same `expected(operation) -> Duration` interface.
- Additional adapters add rows to the classification table.
