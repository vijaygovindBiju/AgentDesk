use std::collections::{BTreeMap, HashMap};

use chrono::Duration;

use agentdesk_core::{classify, Adapter, AdapterOutput, Clock, RespondError, VirtualClock};
use agentdesk_model::{Category, Decision, RawAgentEvent};
use agentdesk_sim::{run_to_end, Scenario, Simulator, Step, Timeline};

const SEED: u64 = 42;

fn default_sim(seed: u64) -> (Simulator, VirtualClock) {
    let clock = VirtualClock::at_epoch();
    (Simulator::new(Scenario::default_scenario(), seed, clock.now()), clock)
}

fn events(t: &Timeline) -> Vec<&RawAgentEvent> {
    t.iter().filter_map(|(_, o)| match o {
        AdapterOutput::Event(e) => Some(e),
        _ => None,
    }).collect()
}

fn serialize(t: &Timeline) -> String {
    t.iter()
        .map(|(at, o)| match o {
            AdapterOutput::Event(e) => format!("{at} E {}", serde_json::to_string(e).unwrap()),
            AdapterOutput::Line { agent_id, text } => format!("{at} L {agent_id} {text}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---- P2.4: scenario format ----

#[test]
fn default_scenario_parses_and_validates() {
    let s = Scenario::default_scenario();
    assert_eq!(s.name, "default");
    assert_eq!(s.agents.len(), 2);
    assert!(s.tasks.len() >= 7);
    assert!(s.validate().is_ok());
}

#[test]
fn scenario_validation_rejects_bad_files() {
    let base = r#"{"name":"x","agents":[{"agent_id":"a","name":"A","project":"P"}],"tasks":[TASKS]}"#;
    let with = |tasks: &str| Scenario::from_json(&base.replace("TASKS", tasks));

    assert!(with(r#"{"task_id":"t","agent_id":"ghost","title":"T","operation":"build","steps":[{"type":"started"}]}"#).is_err(), "unknown agent");
    assert!(with(r#"{"task_id":"t","agent_id":"a","title":"T","operation":"build","steps":[]}"#).is_err(), "empty task");
    assert!(with(r#"{"task_id":"t","agent_id":"a","title":"T","operation":"build","steps":[{"type":"progress","count":0,"interval_ms":1,"template":"x"}]}"#).is_err(), "zero count");
    assert!(with(r#"{"task_id":"t","agent_id":"a","title":"T","operation":"build","steps":[{"type":"event","kind":"build_completed","message":"m"},{"type":"started"}]}"#).is_err(), "step after terminal");
    assert!(with(r#"{"task_id":"t","agent_id":"a","title":"T","operation":"build","steps":[{"type":"request","kind":"progress","prompt":"p"}]}"#).is_err(), "request kind must be a request");
    assert!(with(r#"{"task_id":"t","agent_id":"a","title":"T","operation":"build","steps":[{"type":"request","prompt":"p","on_deny_kind":"progress"}]}"#).is_err(), "on_deny must be terminal");
    assert!(with(r#"{"task_id":"t","agent_id":"a","title":"T","operation":"build","steps":[{"type":"started"}]},{"task_id":"t","agent_id":"a","title":"T","operation":"build","steps":[{"type":"started"}]}"#).is_err(), "duplicate task");
    assert!(with(r#"{"task_id":"t","agent_id":"a","title":"T","operation":"build","steps":[{"type":"request","prompt":"p"},{"type":"event","kind":"task_completed","message":"m"}]}"#).is_ok());
}

#[test]
fn request_step_defaults() {
    let s: Step = serde_json::from_str(r#"{"type":"request","prompt":"ok?"}"#).unwrap();
    match s {
        Step::Request { kind, options, on_deny_kind, .. } => {
            assert_eq!(kind, "approval_required");
            assert_eq!(options, ["approve", "deny"]);
            assert_eq!(on_deny_kind, "cancelled_by_user");
        }
        _ => panic!(),
    }
}

// ---- P2.T3: determinism ----

#[test]
fn same_seed_produces_identical_output() {
    let (mut a, ca) = default_sim(SEED);
    let (mut b, cb) = default_sim(SEED);
    let ta = run_to_end(&mut a, &ca, Duration::seconds(5), |_| Decision::Approve);
    let tb = run_to_end(&mut b, &cb, Duration::seconds(5), |_| Decision::Approve);
    assert!(!ta.is_empty());
    assert_eq!(serialize(&ta), serialize(&tb));
}

#[test]
fn different_seed_changes_only_jitter_not_event_sequence() {
    let (mut a, ca) = default_sim(1);
    let (mut b, cb) = default_sim(2);
    let ta = run_to_end(&mut a, &ca, Duration::seconds(5), |_| Decision::Approve);
    let tb = run_to_end(&mut b, &cb, Duration::seconds(5), |_| Decision::Approve);
    assert_ne!(serialize(&ta), serialize(&tb), "jitter should differ between seeds");
    // Interleaving across tasks may differ, but each task's own sequence must not.
    let per_task = |t: &Timeline| {
        let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for e in events(t) {
            m.entry(e.task_id.clone().unwrap()).or_default().push(e.kind.clone());
        }
        m
    };
    assert_eq!(per_task(&ta), per_task(&tb));
    assert_eq!(events(&ta).len(), events(&tb).len());
}

#[test]
fn output_is_independent_of_polling_granularity() {
    // Human response timing is an input, so use the scenario without its request.
    let mut scenario = Scenario::default_scenario();
    scenario.tasks.retain(|t| t.task_id != "task-db");
    scenario.validate().unwrap();

    // Fine-grained: jump to every due instant.
    let ca = VirtualClock::at_epoch();
    let mut a = Simulator::new(scenario.clone(), SEED, ca.now());
    let fine = run_to_end(&mut a, &ca, Duration::seconds(5), |_| unreachable!());

    // Coarse: poll every 37 seconds.
    let cb = VirtualClock::at_epoch();
    let mut b = Simulator::new(scenario, SEED, cb.now());
    let mut coarse = Timeline::new();
    while !b.is_finished() {
        let now = cb.now();
        for o in b.poll(now) {
            coarse.push((now, o));
        }
        cb.advance(Duration::seconds(37));
    }
    let strip = |t: &Timeline| t.iter().map(|(_, o)| o.clone()).collect::<Vec<_>>();
    assert_eq!(fine.len(), coarse.len());
    assert_eq!(strip(&fine), strip(&coarse));
}

// ---- P2.T4: sequence numbers ----

#[test]
fn agent_seq_is_strictly_increasing_per_agent() {
    let (mut s, c) = default_sim(SEED);
    let t = run_to_end(&mut s, &c, Duration::seconds(5), |_| Decision::Approve);
    let mut last: HashMap<&str, u64> = HashMap::new();
    for e in events(&t) {
        let prev = last.insert(&e.agent_id, e.agent_seq).unwrap_or(0);
        assert_eq!(e.agent_seq, prev + 1, "agent {} jumped {} -> {}", e.agent_id, prev, e.agent_seq);
    }
    assert_eq!(last.len(), 2);
}

#[test]
fn timeline_is_monotonic_in_time() {
    let (mut s, c) = default_sim(SEED);
    let t = run_to_end(&mut s, &c, Duration::seconds(5), |_| Decision::Approve);
    assert!(t.windows(2).all(|w| w[0].0 <= w[1].0));
}

// ---- P2.T5: default scenario mix ----

#[test]
fn default_scenario_has_the_documented_minimum_mix() {
    let (mut s, c) = default_sim(SEED);
    let start = c.now();
    let t = run_to_end(&mut s, &c, Duration::seconds(5), |_| Decision::Approve);
    let evs = events(&t);

    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for e in &evs {
        *by_kind.entry(e.kind.as_str()).or_default() += 1;
    }
    let by_cat = |c: Category| evs.iter().filter(|e| classify(&e.kind).category == c).count();

    assert_eq!(by_cat(Category::Request), 1);
    assert!(by_cat(Category::Error) >= 2, "{by_kind:?}");
    assert!(evs.iter().filter(|e| classify(&e.kind).category == Category::Completed && classify(&e.kind).severity.as_u8() >= 2).count() >= 3, "{by_kind:?}");
    assert_eq!(by_kind["cancelled_by_user"], 1);
    assert_eq!(by_kind["cancelled_by_agent"], 1);
    assert_eq!(by_kind["build_failed"], 1);
    assert_eq!(by_kind["test_failed"], 1);

    // Realistic Working density: noise dominates.
    let working = by_cat(Category::Working);
    assert!(working > 1000, "working events = {working}");
    assert!(working as f64 / evs.len() as f64 > 0.98);
    let lines = t.iter().filter(|(_, o)| matches!(o, AdapterOutput::Line { .. })).count();
    assert!(lines >= 10);

    // One long-running build (> 10 min = 2x the expected 5 min for `build`).
    let long: Vec<_> = evs.iter().filter(|e| e.task_id.as_deref() == Some("task-longbuild")).collect();
    let first = t.iter().find(|(_, o)| matches!(o, AdapterOutput::Event(e) if e.task_id.as_deref() == Some("task-longbuild"))).unwrap().0;
    let last = t.iter().rev().find(|(_, o)| matches!(o, AdapterOutput::Event(e) if e.task_id.as_deref() == Some("task-longbuild"))).unwrap().0;
    assert!(last - first > Duration::minutes(10), "long build ran {}", last - first);
    assert_eq!(long.last().unwrap().kind, "build_completed");

    // Whole scenario fits in a compressed virtual run.
    assert!(t.last().unwrap().0 - start < Duration::minutes(20));
}

#[test]
fn error_events_carry_actionable_details_and_logs() {
    let (mut s, c) = default_sim(SEED);
    let t = run_to_end(&mut s, &c, Duration::seconds(5), |_| Decision::Approve);
    let bf = events(&t).into_iter().find(|e| e.kind == "build_failed").unwrap();
    assert_eq!(bf.details["file"], "auth_service.dart");
    assert_eq!(bf.details["line"], 42);
    assert!(!bf.log_lines.is_empty());
    assert!(bf.message.contains("auth_service.dart:42"));
}

// ---- P2.7: respond ----

#[test]
fn approve_resumes_and_completes_the_task() {
    let (mut s, c) = default_sim(SEED);
    let t = run_to_end(&mut s, &c, Duration::seconds(5), |_| Decision::Approve);
    let db: Vec<_> = events(&t).into_iter().filter(|e| e.task_id.as_deref() == Some("task-db")).map(|e| e.kind.as_str()).collect();
    let req = db.iter().position(|k| *k == "approval_required").unwrap();
    assert!(db[req + 1..].contains(&"progress"), "work resumes after approve");
    assert_eq!(*db.last().unwrap(), "task_completed");
}

#[test]
fn deny_emits_on_deny_kind_and_ends_the_task() {
    let (mut s, c) = default_sim(SEED);
    let t = run_to_end(&mut s, &c, Duration::seconds(5), |id| if id == "task-db" { Decision::Deny } else { Decision::Approve });
    let db: Vec<_> = events(&t).into_iter().filter(|e| e.task_id.as_deref() == Some("task-db")).collect();
    let req = db.iter().position(|e| e.kind == "approval_required").unwrap();
    assert_eq!(db.len(), req + 2, "exactly one event after the request");
    let last = db.last().unwrap();
    assert_eq!(last.kind, "cancelled_by_user");
    assert!(last.message.starts_with("Denied: "));
    assert_eq!(classify(&last.kind).summary, "Cancelled");
}

#[test]
fn respond_errors() {
    let (mut s, c) = default_sim(SEED);
    let now = c.now();
    assert_eq!(s.respond(&"nope".to_string(), Decision::Approve, now), Err(RespondError::NoSuchTask));
    assert_eq!(s.respond(&"task-db".to_string(), Decision::Approve, now), Err(RespondError::NotBlocked));

    // Run until the request appears, then respond twice.
    while s.blocked_tasks().is_empty() {
        let due = s.next_due().expect("scenario should reach the request");
        c.set(due);
        s.poll(due);
    }
    assert_eq!(s.blocked_tasks(), ["task-db"]);
    assert!(s.next_due().is_none() || s.next_due().is_some(), "other tasks may still be due");
    assert_eq!(s.respond(&"task-db".to_string(), Decision::Approve, c.now()), Ok(()));
    assert_eq!(s.respond(&"task-db".to_string(), Decision::Approve, c.now()), Err(RespondError::NotBlocked));
}

#[test]
fn blocked_task_produces_nothing_until_answered() {
    let (mut s, c) = default_sim(SEED);
    while s.blocked_tasks().is_empty() {
        let due = s.next_due().unwrap();
        c.set(due);
        s.poll(due);
    }
    c.advance(Duration::hours(1));
    let out = s.poll(c.now());
    assert!(out.iter().all(|o| !matches!(o, AdapterOutput::Event(e) if e.task_id.as_deref() == Some("task-db"))));
    assert_eq!(s.blocked_tasks(), ["task-db"]);
}

#[test]
fn agents_are_exposed_with_names_and_project() {
    let (s, _) = default_sim(SEED);
    let a = s.agents();
    assert_eq!(a.len(), 2);
    assert_eq!(a[0].agent_id, "sim-backend");
    assert_eq!(a[0].name, "Backend Agent");
    assert_eq!(a[0].project, "Hybrid");
}

#[test]
fn scenario_ground_truth_matches_default_scenario_expectations() {
    let scenario = Scenario::default_scenario();
    let thresholds = agentdesk_core::ThresholdTable::default();
    let gt = scenario.ground_truth(&thresholds);

    // 1 request: task-db (approval_required)
    assert_eq!(gt.requests.len(), 1);
    assert_eq!(gt.requests[0].task_id, "task-db");
    assert_eq!(gt.requests[0].kind, "approval_required");
    assert_eq!(gt.requests[0].category, Category::Request);
    assert_eq!(gt.requests[0].severity, 3);

    // 3 errors: task-auth (build_failed), task-tests (test_failed), task-lint (cancelled_by_agent)
    assert_eq!(gt.errors.len(), 3);
    let error_tasks: Vec<_> = gt.errors.iter().map(|e| e.task_id.as_str()).collect();
    assert_eq!(error_tasks, vec!["task-auth", "task-tests", "task-lint"]);
    assert!(gt.errors.iter().all(|e| e.category == Category::Error));

    // 3 important completions (severity >= 2): task-deps, task-db, task-longbuild
    assert_eq!(gt.completed_important.len(), 3);
    let completed_tasks: Vec<_> = gt.completed_important.iter().map(|e| e.task_id.as_str()).collect();
    assert_eq!(completed_tasks, vec!["task-deps", "task-db", "task-longbuild"]);
    assert!(gt.completed_important.iter().all(|e| e.category == Category::Completed && e.severity >= 2));

    // 1 routine completion (severity < 2): task-refactor (cancelled_by_user, severity 1)
    assert_eq!(gt.completed_routine.len(), 1);
    assert_eq!(gt.completed_routine[0].task_id, "task-refactor");
    assert_eq!(gt.completed_routine[0].kind, "cancelled_by_user");
    assert_eq!(gt.completed_routine[0].severity, 1);

    // Escalations: task-longbuild (12m > 5m expected) reaches level 1 and level 2
    assert_eq!(gt.escalations.len(), 2);
    assert_eq!(gt.escalations[0].task_id, "task-longbuild");
    assert_eq!(gt.escalations[0].level, 1);
    assert_eq!(gt.escalations[1].task_id, "task-longbuild");
    assert_eq!(gt.escalations[1].level, 2);
}

