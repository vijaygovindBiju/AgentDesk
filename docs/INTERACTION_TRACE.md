# AgentDesk Interaction Pipeline Trace

**Date:** 2026-09-21  
**Purpose:** Complete trace of the Antigravity → AgentDesk → phone → response → Antigravity pipeline, verified through code analysis, unit tests, and fixture replay. The live end-to-end test is infrastructure-ready but blocked by a Gemini API quota (resets ~149h from 2026-09-21T09:00 IST).

---

## The 10-Stage Pipeline

```
Stage 1   Antigravity TUI
Stage 2   PTY → VT100 screen parser
Stage 3   Screen snapshot → state machine → AntigravityState
Stage 4   State → RawAgentEvent (adapter.rs)
Stage 5   RawAgentEvent → CoreTask → Event + QueueEntry
Stage 6   Event → WebSocket → JSON frame → phone
Stage 7   Phone renders structured request UI
Stage 8   User responds → phone sends RespondRequest
Stage 9   RespondRequest → CoreTask → queue.respond_request → AdapterCommand::Respond
Stage 10  AdapterCommand → adapter.respond_with_response → PTY keystrokes → Antigravity continues
```

---

## Stage 1: Antigravity TUI

**File:** PTY child process spawned by `AntigravityPtyAdapter::spawn` (adapter.rs:162–187)  
**Mechanism:** `PtySession::spawn(&config.command, &config.args, cwd, cols, rows)` using POSIX `openpty` + `fork/exec`. The child runs `agy -i "<prompt>"`.

When Antigravity decides to call `run_command`, it enters its command-confirmation TUI: a Bubbletea interactive menu rendered as VT100 escape sequences. For example:

```
Command: echo hello
> Yes, run command
  Yes, always allow in this conversation
  Yes, allow all commands in this conversation
  No, cancel
```

The full frame includes VT100 SGR reverse-video attributes (`\e[7m`) on the selected row.

**Evidence:** fixture `fixtures/antigravity/command_confirmation.json` contains captured PTY bytes from this exact state. `test_numbered_command_confirmation_menu` (antigravity_adapter_test.rs) replays this fixture and asserts `approval_required` is detected — **passes deterministically**.

---

## Stage 2: PTY → VT100 Screen Parser

**File:** `core/agentdesk-core/src/antigravity/pty.rs` + `screen.rs`  
**Mechanism:**
- `PtySession::try_recv()` reads available bytes from the PTY master fd (non-blocking).
- `AntigravityPtyAdapter::drain_transport()` (adapter.rs:361–369) calls `try_recv()` in a loop until no bytes remain.
- Each chunk is passed to `Screen::process_bytes(&chunk.bytes)`.
- `Screen` implements a VT100 state machine: handles `\e[H` (cursor home), `\e[2J` (erase screen), `\e[xm` (SGR attributes including `\e[7m` for reverse video), cursor positioning, and all printable characters.
- The result is a 2D character grid (120×40 default) with per-cell attributes.

**Key security guarantee:** `drain_transport` is called inside the Rust process only. No PTY bytes or escape sequences are ever written to `AdapterOutput`, and no raw bytes reach the JSON serialization path.

**Evidence:** `screen.rs` unit tests — `test_plain_text_and_cursor_movement`, `test_ansi_color_and_reverse_attributes`, `test_alternate_screen_buffer_switching`, `test_screen_resize`, `test_erase_line_and_display` — all pass.

---

## Stage 3: Screen Snapshot → State Machine → AntigravityState

**File:** `core/agentdesk-core/src/antigravity/state_machine.rs`  
**Mechanism:**
- `adapter.poll()` calls `screen.snapshot()` → `ScreenSnapshot` (a captured 2D grid with attributes).
- `detect_state(&snapshot)` analyzes the snapshot using structural VT100 attributes:
  - **CommandConfirmation**: finds a line containing `Command:` text, followed by menu rows where at least one row has `reverse_video=true` (the selected option) and the first option starts with `Yes`.
  - **FileEditConfirmation**: similar but identifies file edit dialogs with `File:` prefix.
  - **WorkspaceTrust**: specific header text + trust options.
  - **UserQuestion**: detects Antigravity's `ask_question` TUI format (`Question N/N:` header or `esc Skip` footer).
  - **Working**: spinner glyphs (⣾⣽⣻⢿⡿⣟⣯⣷) or "Loading..." in status area.
  - **IdlePrompt**: bottom row matches `>` prompt pattern without spinner.
  - **Completed**: specific completion message in response area.
  - **FatalError**: error text in response area.

**Deduplication:** `poll()` only calls `handle_state_transition` when `detected != self.current_state`. Working events are further deduplicated: only emitted when the details string changes (`handle_state_transition` checks `lifecycle == Working { details: prev } && prev != details`).

**Evidence:** `test_detect_command_confirmation`, `test_state_machine_emits_request_event`, `spinner_redraws_share_one_working_state`, `classifies_explicit_yes_no_controls_without_relabeling_options` — all pass.

---

## Stage 4: State → RawAgentEvent

**File:** `core/agentdesk-core/src/antigravity/adapter.rs` → `request_event()`  
**Mechanism:** When `open_request()` is called with a blocking state:

```rust
// CommandConfirmation produces:
RawAgentEvent {
    kind: "approval_required",
    operation: classify_command_operation(&command),  // Build/Test/Install/etc
    message: format!("Command requires approval: {command}"),
    details: { "command": command, "selected_option": idx, "request_generation": generation },
    log_lines: [ format!("Command: {command}") ],
    request: Some(RequestInfo {
        prompt: command.clone(),
        options: vec!["approve".into(), "deny".into()],
        question_type: None,  // None → phone renders Approve/Deny buttons
    }),
}
```

**Request ID tracking:** Each unique request gets a monotonically increasing `request_generation`. The `PendingRequest` struct holds `generation`, `agent_seq` (the `RawAgentEvent.agent_seq`), and the live `AntigravityState`. Redraw of the same dialog (same `same_request()` identity) refreshes `pending.state` but does NOT emit a new event.

**Evidence:** `test_command_confirmation_approval_flow`, `test_command_confirmation_denial_flow`, `test_screen_redraw_does_not_duplicate_request_events` — all pass.

---

## Stage 5: RawAgentEvent → CoreTask → Event + QueueEntry

**File:** `core/agentdesk-core/src/core_task.rs` → `handle_adapter_output()`  
**File:** `core/agentdesk-core/src/processor.rs` → `process_raw_event()`  
**Mechanism:**

```
AdapterOutput::Event(raw) 
  → processor.process_raw_event(raw, now, event_store, queue, log_store, metrics)
    → assigns event_id (UUID), seq (global monotonic), timestamp
    → classifier assigns: category=Request, severity=Critical (for approval_required)
    → QueueEntry: tier=0, resolution=Unresolved, state=New, score=computed
    → event_store.insert(event)
    → queue.insert(entry)
  → tracker.observe_event(&event)  — opens task, records working event
  → broadcast(Message::push(Body::Event(EventPush { event, entry })))
```

The event broadcast happens synchronously within the CoreTask actor. At this point, the request is live in the queue with `resolution: Unresolved`.

**Evidence:** `mode_agentdesk_forwards_only_queue_derived_messages`, `respond_request_delivers_decision_to_adapter_responder` — pass.

---

## Stage 6: Event → WebSocket → JSON → Phone

**File:** `core/agentdesk-server/src/connection.rs`  
**Mechanism:**
- `broadcast()` calls `sink.send(&message)` for each connected client.
- `ServerConnectionSink::send()` serializes the `Message` to JSON and sends it to the bounded channel.
- The connection task's `select!` loop drains the channel and writes `WsMessage::Text(json)` over the TLS WebSocket.
- The phone receives a JSON frame of type `"event"` with the full `Event` + `QueueEntry`.

**What the phone receives (for command confirmation):**
```json
{
  "type": "event",
  "payload": {
    "event": {
      "event_id": "...",
      "category": "request",
      "kind": "approval_required",
      "message": "Command requires approval: echo hello",
      "request": {
        "prompt": "echo hello",
        "options": ["approve", "deny"]
      }
    },
    "entry": {
      "tier": 0,
      "resolution": "unresolved",
      "state": "new"
    }
  }
}
```

Note: `question_type` is absent (None → omitted by `skip_serializing_if = "Option::is_none"`). This is intentional.

**No raw PTY data in this frame:** The `Event` struct contains only: `event_id`, `seq`, `agent_seq`, `agent_id`, `agent_name`, `project`, `task_id`, `ts`, `category`, `severity`, `kind`, `operation`, `summary`, `message`, `details`, `log_range`, `request`. No PTY bytes, no escape sequences, no terminal coordinates.

**Evidence:** `server_tests.rs` integration tests test the full connection → snapshot → event push path. TLS tests confirm `wss://` transport.

---

## Stage 7: Phone Renders Structured Request UI

**File:** `mobile/lib/ui/event_screen.dart` → `_buildRequestControls()`  
**Mechanism:**

The Flutter client receives the `event` message, applies it to `AgentDeskState`, and renders it in the queue view. When the user taps the event, `EventScreen` opens.

`_buildRequestControls(event)` logic:
```dart
final isApprovalOptions = options.length == 2 &&
    options.map((o) => o.toLowerCase()).toSet()
           .containsAll({'approve', 'deny'});
final questionType = request.questionType ??
    (options.isNotEmpty && !isApprovalOptions
        ? QuestionType.singleChoice
        : null);
// For CommandConfirmation: questionType == null (None + isApprovalOptions == true)
// → renders Approve / Deny buttons
```

For `options: ["approve", "deny"]` and `question_type: None`:
- `isApprovalOptions = true`
- `questionType = null`
- Phone shows: green **Approve** button + red **Deny** button
- Prompt text: `"echo hello"` (the actual command)
- Header: "Approval Request"

For `UserQuestion` with `question_type: single_choice`:
- `questionType = QuestionType.singleChoice`
- Phone shows radio buttons with the exact option labels from the agent
- A "Send response" button

**Evidence:** `question_input_test.dart` — `renders exact single-choice options`, `renders explicit yes/no options as structured choices`, `renders multi-select options`, `renders free text input`, `choice plus write-in exposes both supplied choices and text` — **all 6 tests pass**.

---

## Stage 8: User Responds → Phone Sends RespondRequest

**File:** `mobile/lib/ui/event_screen.dart` → `_respond()` / `_respondWithInput()`  
**File:** `mobile/lib/services/connection_service.dart` → `respondRequest()`  

When the user taps **Approve**:
```dart
await connection.respondRequest(
  widget.eventId,
  Decision.approve,
  selectedOptions: null,
  textInput: null,
);
```

This sends over WebSocket:
```json
{
  "type": "respond_request",
  "request_id": "r-xyz",
  "payload": {
    "event_id": "...",
    "decision": "approve"
  }
}
```

For a structured question (user selected "PostgreSQL"):
```json
{
  "type": "respond_request",
  "payload": {
    "event_id": "...",
    "decision": "approve",
    "selected_options": ["PostgreSQL"]
  }
}
```

**Evidence:** `phase7_exit_criteria_test.dart` tests the full approve flow end-to-end through a mock server. `question_input_test.dart` verifies the correct payload is sent for each question type.

---

## Stage 9: RespondRequest → Core → queue → AdapterCommand

**File:** `core/agentdesk-core/src/core_task.rs` → `handle_client_message()` → `Body::RespondRequest`

**Ordering (Task 4 analysis):**

```rust
// 1. Parse response from wire
let req_response = RequestResponse::from_respond_request(&req);

// 2. Look up task_id and agent_seq from event store
let (task_id, request_seq) = self.event_store.get(&req.event_id)
    .map(|e| (e.task_id.clone(), Some(e.agent_seq)))
    .unwrap_or((None, None));

// 3. Mark queue entry as resolved (BEFORE adapter is notified)
let res = self.queue.respond_request(&req.event_id, req.decision);

match res {
    Ok(state_update) => {
        // 4. Broadcast StateUpdate to phone (shows "resolved: approved")
        self.broadcast(Message::push(Body::StateUpdate(state_update)));
        
        // 5. Send AdapterCommand to adapter loop (AFTER queue update)
        if let (Some(tid), Some(tx)) = (task_id, &self.adapter_responder) {
            let _ = tx.try_send(AdapterCommand::Respond { ... });
        }
        
        // 6. Reply to phone with CommandResult::ok()
        self.send_to(client_id, Message { body: Body::CommandResult(ok) });
    }
}
```

**Lifecycle ordering conclusion:** The queue marks the request as `resolved` BEFORE the adapter processes the response. This is by design: the core queue is the canonical resolution authority. The adapter is notified asynchronously through a bounded mpsc channel (`tx_adapter`). The `try_send` is non-blocking — if the adapter channel is full, the command is dropped silently with a warning.

**Implication for correctness:** If the adapter fails to deliver keystrokes (PTY closed, transport error), the queue still shows `approved`. The adapter returns `RespondError::Disconnected` which is logged as a warning but does not undo the queue resolution. This is acceptable for the current experimental adapter but is a known gap for production hardening.

**Duplicate response rejection:** `queue.respond_request` checks `entry.resolution`:
- `None` → `NotARequest` error → phone gets `CommandResult::err("not_a_request")`
- `Approved | Denied` → `AlreadyResolved` error → phone gets `CommandResult::err("already_resolved")`
- `Unresolved` → success → proceeds to adapter delivery

**Evidence:** `respond_request_delivers_decision_to_adapter_responder` test verifies the adapter channel receives the command. Queue state machine tests verify `AlreadyResolved` rejection.

---

## Stage 10: AdapterCommand → Adapter → PTY Keystrokes → Antigravity Continues

**File:** `core/agentdesk-server/src/main.rs` → adapter driving loop  
**File:** `core/agentdesk-core/src/antigravity/adapter.rs` → `respond_with_response()`

**Adapter loop** (main.rs):
```rust
loop {
    // Poll outputs every 50ms
    let outputs = adapter.poll(now);
    for out in outputs { adapter_sender.send(CoreCommand::Adapter(out)).await; }
    
    tokio::select! {
        adapter_cmd = rx_adapter.recv() => {
            if let Some(AdapterCommand::Respond { task_id, decision, request_seq, response, now }) = adapter_cmd {
                let result = adapter.respond_with_response(&task_id, request_seq, &resp, now);
                if let Err(error) = result {
                    eprintln!("[WARN] Antigravity response delivery failed: {}", error);
                }
            }
        }
        _ = tokio::time::sleep(Duration::from_millis(50)) => {}
    }
}
```

**`respond_with_response()` validation chain:**
1. Task identity: `task_id == self.task_id` → else `NoSuchTask`
2. Transport alive: `transport.is_alive()` → else `Disconnected`
3. Pending request exists: `self.pending.is_some()` → else `NotBlocked | AlreadyConsumed | StateMismatch`
4. Request generation: `request_seq == pending.agent_seq` → else `WrongRequest`
5. Live state check: `drain_transport(); detect_state(snapshot)` → `same_request(live)` → else `StateMismatch`
6. Response encoding: `encode_for_state(&live, response)` → keystrokes or `InvalidResponse`
7. PTY send: `transport.send_input(&bytes)` → else `Disconnected`
8. Mark consumed + grace period: `pending = None; last_consumed = Some(ConsumedRequest { grace_until: now + 5s })`
9. Lifecycle update: `Working { details: "Response delivered" }`

**For Approve on CommandConfirmation:**
- `live.selected_option_index = 0` (cursor at "Yes, run command")
- `encode_decision(Approve, &live)` → `navigate_and_select(0, 0)` → `[b'\r']` (just Enter)
- PTY receives `\r` → Antigravity processes it → command executes → screen updates
- Next `poll()` detects new Working state → emits progress events
- Eventually: screen shows task completion → `Completed` state emitted

**For Deny on CommandConfirmation:**
- `encode_decision(Deny, &live)` → `navigate_and_select(current, deny_index)` → e.g. `\e[B\e[B\e[B\r` (3 down + Enter)

**Evidence:** `test_command_confirmation_approval_flow` asserts `sent_inputs[0] == b"\r"`. `test_command_confirmation_denial_flow` asserts correct navigation bytes. `test_antigravity_adapter_through_core_pipeline` tests the full adapter → core → respond path end-to-end.

---

## Task 2: No Raw PTY Bytes Reach the Phone

**Verified by construction:**

The data boundary is enforced at the `AdapterOutput` type level:

```rust
pub enum AdapterOutput {
    Event(RawAgentEvent),   // Structured, sanitized event only
    Line { agent_id, text } // Pure text log line (no escape sequences)
}
```

`drain_transport()` reads PTY bytes into `self.screen` (in-process VT100 emulator) and returns `bool`. No `AdapterOutput` is produced from this path.

`detect_state()` produces an `AntigravityState` enum — a structured Rust value with typed fields (strings, numbers, vectors). No bytes, no escape sequences.

`request_event()` / `handle_state_transition()` produce `AdapterOutput::Event(RawAgentEvent)` with only:
- `kind`: a fixed string like `"approval_required"`
- `operation`: an enum value
- `message`: a sanitized summary
- `details`: a `BTreeMap<String, serde_json::Value>` of short structured fields
- `log_lines`: short sanitized text strings
- `request`: `RequestInfo { prompt, options, question_type }`

`RawAgentEvent` → JSON → WebSocket → phone: all fields are typed Rust values serialized to JSON. There is no `Vec<u8>` or terminal-byte-bearing field anywhere in `RawAgentEvent`, `Event`, or any protocol message.

**Security boundary validated by `test_terminal_noise_and_adversarial_chat_ignored` and `test_real_agy_live_adversarial_question_text_is_not_a_request`** — terminal text never produces an `approval_required` event without the verified structural VT100 attributes.

---

## Task 3: Response Validation

The adapter enforces 7 validation checks before delivering any keystrokes (Stage 10 above). The queue enforces resolution state before the adapter is even notified.

**Duplicate response:** `queue.respond_request` → `AlreadyResolved` → phone receives `CommandResult::err("already_resolved")`. The adapter is NOT notified.

**Stale response:** `respond_with_response` step 4 checks `request_seq == pending.agent_seq`. If the dialog was replaced by a new request with a new `agent_seq`, `WrongRequest` is returned.

**Invalid option:** `encode_for_state(&live, response)` → `encode_question_response()` → `resolve_option_index(label, details)` checks that the label exists in the menu rows (exact match, case-preserved). Unknown labels → `InvalidResponse`.

**Already consumed:** If the adapter's `pending == None` and `last_outcome == Consumed`, `respond_with_response` returns `AlreadyConsumed`.

**State mismatch:** If the dialog disappeared between the phone's response and the adapter receiving it (e.g. user manually dismissed in the terminal), `drain_transport(); detect_state()` → `!pending.state.same_request(live)` → `StateMismatch`.

---

## Task 4: Request Lifecycle Ordering (confirmed gap)

```
phone submits RespondRequest
  ↓
CoreTask: queue.respond_request() marks Resolved (BEFORE adapter)
  ↓
CoreTask: tx.try_send(AdapterCommand) — non-blocking, may drop if full
  ↓
CoreTask: phone receives CommandResult::ok()
  ↓  (asynchronous)
Adapter loop: receives AdapterCommand
  ↓
Adapter: respond_with_response() validates + sends PTY keystrokes
  ↓
PTY: Antigravity processes keystrokes + changes state
```

The gap: **the queue is marked resolved before the adapter confirms PTY delivery**. If `try_send` drops the command (channel full) or the PTY transport fails, the queue shows "approved" but Antigravity never received the keystroke. This is not a failure in normal operation (the adapter channel has capacity 64, the adapter loop polls every 50ms), but it is a known architectural gap for production hardening.

Mitigation already in place: `[WARN]` log in main.rs when adapter response fails. The adapter has a grace period: if the dialog reappears (Antigravity rejected the keystroke), the adapter re-opens the request after 5 seconds.

---

## Task 5: Working Events

**Deduplication mechanism:**

```rust
// In handle_state_transition:
AntigravityState::Working { ref details } => {
    let should_emit = match &self.lifecycle {
        AntigravityLifecycle::Working { details: prev } => prev != details,
        _ => true,
    };
    if should_emit {
        self.lifecycle = AntigravityLifecycle::Working { details: details.clone() };
        out.push(self.event("progress", ...));
    }
}
```

Spinner redraws at 50ms polling produce `detect_state()` → `Working { details: "Loading..." }`. Since `detected == self.current_state`, `handle_state_transition` is never called. No event is produced.

When the details string changes (e.g. new tool call is shown), a new Working event is emitted.

**Priority:** Working events are classified as `Category::Working` (tier 3). Request events are `Category::Request` (tier 0). The queue always orders tier 0 above tier 3 — requests always appear above working events in the phone UI, regardless of score.

**Evidence:** `spinner_redraws_share_one_working_state` asserts zero Working events from 10 identical spinner frames. Pass.

---

## Summary Table

| Stage | Verification | Status |
|---|---|---|
| 1. Antigravity TUI renders command confirmation | `test_numbered_command_confirmation_menu` (fixture replay) | ✅ Passes |
| 2. PTY → VT100 screen parser | Screen unit tests (5 tests) | ✅ Passes |
| 3. Screen → state machine | State machine unit tests | ✅ Passes |
| 4. State → RawAgentEvent | Adapter unit tests | ✅ Passes |
| 5. RawAgentEvent → Event + Queue | Core pipeline test | ✅ Passes |
| 6. Event → WebSocket → phone | Server integration tests | ✅ Passes |
| 7. Phone renders correct UI | question_input_test.dart (6 tests) | ✅ Passes |
| 8. Phone sends RespondRequest | phase7_exit_criteria_test.dart | ✅ Passes |
| 9. Core validates + routes response | Core unit + integration tests | ✅ Passes |
| 10. Adapter delivers PTY keystrokes | Adapter unit + core pipeline tests | ✅ Passes |
| **Live end-to-end with real API** | Blocked: Gemini quota (~149h) | ⏸ Blocked |

---

## Live Test Command (ready to run when quota resets)

```bash
cd /media/pirate/Shared/currently working/AgentDesk/core

cargo run -p agentdesk-server --bin agentdesk -- run \
  --antigravity \
  --agent "agy" \
  --prompt "Create a hello world Rust project in /tmp/agentdesk-interaction-test/hello-rust/ and run it with cargo run." \
  --bind 0.0.0.0 \
  --port 8765
```

Phone configuration:
- URL: `wss://192.168.1.8:8765`
- Token: see `~/.config/agentdesk/token`
- Fingerprint: `bd0dcbec88656846a1678ebcd8cb2725c9eb2880c4a79753455e47bf4855c47d`
