# AgentDesk — Testing

## Principles

- Core logic (classifier, queue, log store, tracker, processor) is pure or single-task and tested without any network.
- The simulator is seeded, so every scenario is reproducible; tests assert on exact sequences where practical.
- Time is injected (`Clock` trait) so escalation and re-scoring are tested in milliseconds.
- Nothing is marked done in TODO.md until its tests exist and pass.

## Unit tests

### Classifier (`agentdesk-core`)
- Every `kind` in the rules table maps to the documented category and severity.
- Prefix fallbacks: `foo_failed → error/3`, `foo_completed → completed/2`.
- Unknown kind → `working/0` and `unclassified_events` incremented; never panics, never drops.
- Table-driven: adding a rule requires adding a row to the test table.

### Scoring and Priority Queue
- Ordering is `(tier asc, score desc, seq desc)`; ties on tier and score fall back to newest `seq`.
- Property test: for any two entries in different tiers, the lower tier always sorts first regardless of score, escalation, or state.
- Property test: within Working, any entry with `escalation_level ≥ 1` outscores any entry with level 0 of the same age; level 2 outscores level 1.
- Recency bonus decays to 0 at 30 min and never goes negative.
- `seen` lowers score; `resolved` lowers it further; scores clamp to `0..=100`.
- State machine: `new → seen → dismissed`; idempotent acks; `dismiss` from `new` allowed; `resolution` independent of `state`; `respond_request` on non-request → `not_a_request`; twice → `already_resolved`.
- Dismissed and superseded entries are excluded from snapshots but events remain fetchable.
- Re-score tick emits `score_update` only for entries whose score or escalation changed.

### Log Store
- Ring buffer evicts oldest lines at capacity; offsets keep increasing across eviction.
- Page by `offset/limit`; tail (`offset: -1`) returns the last `limit` lines with the true start offset.
- `evicted: true` when any requested line is gone and not pinned.
- Pinned windows survive eviction; window bounds clamp at buffer start (no negative offsets) and at current end.
- `limit` capped at the configured maximum.

### Task Tracker / Watchdog
- Task opens on first event with `task_id`; `operation` taken from that event.
- Escalation at `expected` and `2 × expected`, never a third time.
- Closing event (`completed | error | cancelled`) supersedes all Working entries of the task and stops escalation.
- Task with no Working event never escalates (nothing to raise).
- Threshold table is injected; tests use millisecond thresholds.

### Event Processor
- Assigns strictly increasing `seq`; preserves adapter `agent_seq`; stamps `log_range` from the current ring offsets.
- Pins logs only for `request` and `error`.
- Malformed raw event → dropped and counted.

### Model (`agentdesk-model`)
- Serde round-trip for every message type; unknown fields ignored (forward compatibility); missing required fields rejected.
- `schema_version` present on `Event`.

## Communication tests (`agentdesk-server`)

- `hello` with correct token → `welcome` then `snapshot`; wrong token → close `4001`; non-hello first frame → `4001`; schema mismatch → `4002`.
- Request/reply correlation: each request gets exactly one reply with the same `request_id`.
- `get_event_details` marks `seen` and triggers a `state_update` to *other* clients (single-client MVP still asserts the emission).
- Malformed JSON → `error` reply, connection remains open.
- Slow client: fill the outbound channel → close `4003`, core continues processing.
- TLS: client with the pinned fingerprint connects; client with a different fingerprint fails the handshake. `--insecure-dev` refuses to bind to non-loopback.
- Token never appears in any log line at default log level (grep the captured log output).

## Real-Agent Adapter tests (`agentdesk-core/tests/acp_adapter_test.rs`)

- **Deterministic mock transport tests**:
  - `test_happy_path_initialization_and_turn`: Handshake (`initialize` → `session/new` → `session/prompt`), streaming updates, and turn completion.
  - `test_permission_approval_flow`: `session/request_permission` yields `approval_required` (`Category::Request`), `respond(Approve)` emits structured JSON-RPC selection (`outcome: "selected"`), and turn completes.
  - `test_permission_denial_flow`: `respond(Deny)` emits structured JSON-RPC cancellation (`outcome: "cancelled"`).
  - `test_dead_process_control_rejection`: Control signals dispatched after process termination or crash return `RespondError` and emit an `adapter_error` event.
  - `test_invalid_task_and_not_blocked_rejections`: Calls to `respond()` with nonexistent task IDs or unblocked states are safely rejected.
  - `test_process_crash_and_disconnect`: Subprocess EOF / unexpected termination generates `command_failed` (`Category::Error`).
  - `test_jsonrpc_error_handling`: Incoming JSON-RPC errors generate `command_failed` with structured details.
  - `test_arbitrary_terminal_text_logged_only_never_scraped`: Stderr and arbitrary terminal text containing misleading keywords ("ERROR:", "FATAL", "Permission denied", "Approve? [y/N]") are piped strictly as `AdapterOutput::Line` and NEVER promote to `Request` or `Error`.
  - `test_acp_adapter_through_core_pipeline`: End-to-end integration through `CoreTask`, log store ring buffer, classifier, and attention queue.
- **Process lifecycle and live tests**:
  - `test_real_process_transport_stdio_lifecycle`: Validates non-blocking child process stdout/stderr capture and stdin piping using a live Python mock child process.
  - `test_gemini_live_e2e`: Live integration test against `/usr/bin/gemini --skip-trust --acp` with real LLM inference. Runs automatically when the binary is installed.

## Detail retrieval tests

- Tail request on a pinned Error event returns lines from the pinned window even after the ring has fully rotated.
- Paging backwards from the tail reaches offset 0 or hits `evicted: true` at the window boundary — never both silently wrong.
- Log request for an unknown `event_id` → `command_result { ok: false, error: "no_such_event" }`.

## Offline / reconnection tests

MVP scope (no server-side queue):
- Disconnect, produce events, reconnect → `snapshot` contains the new live entries and omits dismissed/superseded ones.
- Phone applies `snapshot` wholesale (unit test in Flutter): stale entries disappear, states are replaced.

Future (when offline queues exist): replay ordering, individual command failure not blocking later commands, duplicate suppression by `(client_id, client_seq)`.

## Integration tests

- **Scenario replay**: run the simulator with a fixed seed through the full core pipeline with a fake transport; assert the exact ordered set of transmitted messages against a golden file. Any behavioural change updates the golden file deliberately.
- **Three-mode bench**: same seed through `raw_lines`, `raw_events`, `agentdesk`; assert `agentdesk.transmitted_events < raw_events.transmitted_events` and that every simulated Request and Error appears exactly once in `agentdesk` output. This is the automated form of the success criteria.
- **End-to-end (manual for MVP)**: daemon + Flutter app on a device over `wss://`; checklist in TODO.md before a live measurement run.

## Flutter tests

- Model decoding for every laptop → phone message.
- `QueueView` ordering matches the laptop's `(tier, score desc, seq desc)`.
- Reducer tests: `event`, `score_update`, `state_update`, `snapshot` produce the expected view.
- Widget tests: Request that is `dismissed` but `unresolved` renders the "still blocking" indicator; escalated Working renders the badge.
- Persistent configuration tests use in-memory implementations of the configuration-preferences and secure-token-store interfaces. They cover save/load and startup restoration, missing URL/token/fingerprint states, secure `wss://` validation, loopback-only `ws://`, configuration replacement, and secure-token read/write/delete without requiring a physical keystore.

## Edge cases to keep in the suite

- Event with no `task_id` (never escalates, never superseded).
- Two agents with interleaved `agent_seq`; global `seq` still monotonic.
- Request resolved after the task was already closed by an Error → `already_resolved` or `no_such_event` per state, never a panic.
- Ring buffer capacity smaller than the pin window.
- Zero-length log page (`limit: 0`) → empty page, `ok`.
- Score exactly on tier boundary values does not reorder across tiers.

## Measurement

`agentdesk-bench` output is JSON; a run is only cited if it is produced headlessly from a committed seed and scenario. See PROJECT.md success criteria and SYSTEM_DESIGN.md "Pipeline modes".
