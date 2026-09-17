# AgentDesk — Simulator

The simulated coding agent (`core/agentdesk-sim`) is a deterministic, scripted `Adapter`. It exists so the pipeline can be tested and measured before any real agent is connected, and so every measurement is reproducible from a committed scenario and seed.

## Guarantees

- **Deterministic**: same scenario + same seed + same human decisions ⇒ byte-identical output. The only randomness is progress jitter, drawn from `StdRng::seed_from_u64(seed)` in emission order.
- **Polling-independent**: output is produced from `poll(now)` in due-time order (ties broken by task order in the file), so a driver that jumps to every due instant and one that polls every 37 s see the same sequence.
- **Time-injected**: the simulator never reads the wall clock. Tests and the bench use `VirtualClock`; the daemon uses `SystemClock` and sleeps until `next_due()`.
- **Blocking requests**: a `request` step blocks its task until `respond(task_id, decision, now)`. Approve resumes from `now`; deny emits the step's `on_deny_kind` (default `cancelled_by_user`) and ends the task.

## Scenario format

JSON, validated on load (`Scenario::from_json` / `Scenario::load`). Times are virtual milliseconds.

```jsonc
{
  "name": "default",
  "description": "optional",
  "agents": [ { "agent_id": "sim-backend", "name": "Backend Agent", "project": "Hybrid" } ],
  "tasks": [
    {
      "task_id": "task-auth",
      "agent_id": "sim-backend",
      "title": "Implement authentication",
      "operation": "build",            // build | test | install | analyze | edit | other
      "start_offset_ms": 0,            // relative to scenario start
      "steps": [ /* see below */ ]
    }
  ]
}
```

### Steps

| `type` | Fields | Emits |
|--------|--------|-------|
| `started` | `message?` | one `started` event (details: `task`) |
| `progress` | `count`, `interval_ms`, `jitter_ms?`, `template` (`{i}`, `{n}`) | `count` `progress` events, each with its message as a log line; next tick after `interval + rand(0..=jitter)` |
| `log` | `lines[]`, `interval_ms?` | one raw log line per entry (not events) |
| `wait` | `ms` | nothing; silence |
| `request` | `prompt`, `kind?` (default `approval_required`), `options?` (default `approve`/`deny`), `message?`, `on_deny_kind?` (default `cancelled_by_user`) | one request event with `RequestInfo`; task blocks |
| `event` | `kind`, `message`, `details?`, `log_lines?`, `delay_ms?` | one event; if `kind` classifies as `completed` or `error`, the task ends |

### Validation rules

Unique `agent_id`s and `task_id`s; every task's `agent_id` exists; tasks have at least one step; `progress.count > 0`; `request.kind` must classify as `request`; `request.options` non-empty; `on_deny_kind` must be terminal; no steps after a terminal event. A task may end without a terminal event (it simply goes silent) — useful for testing escalation of abandoned tasks.

## Default scenario

`core/agentdesk-sim/scenarios/default.json` — two agents on project "Hybrid", ~14.3 virtual minutes (with a 5 s approval delay), seed-independent event mix:

| task | agent | operation | outcome |
|------|-------|-----------|---------|
| task-auth | Backend | build | `build_failed` at `auth_service.dart:42` |
| task-deps | Frontend | install | `build_completed` |
| task-refactor | Frontend | edit | `cancelled_by_user` |
| task-db | Backend | other | `approval_required` → (approve) `task_completed` / (deny) `cancelled_by_user` |
| task-tests | Frontend | test | `test_failed` at `widget_test.dart:17` |
| task-longbuild | Backend | build | 12-minute build → `build_completed`; exceeds the 5 min expected duration and its 2× re-escalation point |
| task-lint | Frontend | analyze | `cancelled_by_agent` (conflicting edits) |

Measured with `cargo run -p agentdesk-sim --example stats` (seed 42, all requests approved):

```text
raw events: 1445   raw log lines: 1455   approx raw bytes: 277 093
by category: Working 1437 · Completed 4 · Error 3 · Request 1
```

99.4 % of raw events are Working noise. Attention-worthy ground truth: 1 request, 3 errors (`build_failed`, `test_failed`, `cancelled_by_agent`), 3 completions with severity ≥ 2, 1 low-severity completion (`cancelled_by_user`), 1 task expected to escalate twice.

## Driving it

- Tests / bench: `agentdesk_sim::run_to_end(&mut sim, &virtual_clock, response_delay, |task_id| decision)` returns a `Timeline` of `(virtual_time, AdapterOutput)`.
- Daemon (Phase 4+): the core task calls `poll(clock.now())`, sleeps until `next_due()`, and forwards `respond_request` commands to `respond`.
