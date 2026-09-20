//! Phase 5 validation tests: P5.T1 through P5.T8.
//! See docs/TODO.md Phase 5 Validation.

use std::collections::HashSet;

use agentdesk_bench::{TapPolicy, run_all_modes, run_bench_mode};
use agentdesk_model::{Category, PipelineMode};
use agentdesk_sim::Scenario;

const SEED: u64 = 42;

#[test]
fn p5_t1_reduction_asserted() {
    let scenario = Scenario::default_scenario();
    let report = run_all_modes(&scenario, SEED, TapPolicy::Default);

    let red = &report.reduction;
    // agentdesk.surfaced_events < raw_events.transmitted_events <= raw_lines.transmitted_events
    assert!(red.agentdesk_surfaced_events < red.raw_events_events);
    assert!(red.raw_events_events <= red.raw_lines_events);
    assert_eq!(red.agentdesk_surfaced_events, 8);
    assert_eq!(red.raw_events_events, 1445);
    assert!(red.raw_lines_events >= 1445);

    // Reduction ratio is at least 100x (> 99%)
    assert!(red.event_reduction_vs_raw_events_ratio > 100.0);
    assert!(red.event_reduction_vs_raw_events_pct > 99.0);
    assert!(red.event_reduction_vs_raw_lines_ratio > 100.0);
    assert!(red.event_reduction_vs_raw_lines_pct > 99.0);
}

#[test]
fn p5_t2_coverage_requests() {
    let scenario = Scenario::default_scenario();
    let report = run_all_modes(&scenario, SEED, TapPolicy::Default);

    assert_eq!(report.coverage.requests.expected, 1);
    assert_eq!(report.coverage.requests.surfaced, 1);
    assert!(report.coverage.requests.preserved);
}

#[test]
fn p5_t3_coverage_errors() {
    let scenario = Scenario::default_scenario();
    let report = run_all_modes(&scenario, SEED, TapPolicy::Default);

    assert_eq!(report.coverage.errors.expected, 3);
    assert_eq!(report.coverage.errors.surfaced, 3);
    assert!(report.coverage.errors.preserved);
}

#[test]
fn p5_t4_coverage_completed() {
    let scenario = Scenario::default_scenario();
    let report = run_all_modes(&scenario, SEED, TapPolicy::Default);

    // All severity >= 2 completions surfaced
    assert_eq!(report.coverage.completed_important.expected, 3);
    assert_eq!(report.coverage.completed_important.surfaced, 3);
    assert!(report.coverage.completed_important.preserved);

    // Routine completion (cancelled_by_user) reported
    assert_eq!(report.coverage.completed_routine.expected, 1);
    assert_eq!(report.coverage.completed_routine.surfaced, 1);
}

#[test]
fn p5_t5_escalation() {
    let scenario = Scenario::default_scenario();
    let out = run_bench_mode(&scenario, SEED, PipelineMode::Agentdesk, TapPolicy::Default);

    // task-longbuild must reach level 1 and then level 2, but never level 3
    let longbuild_level = out
        .observed_escalations
        .get("task-longbuild")
        .copied()
        .unwrap_or(0);
    assert_eq!(
        longbuild_level, 2,
        "task-longbuild must reach escalation level 2"
    );

    // Check that working tier entries never outrank request, error, or completed entries
    let requests: Vec<_> = out
        .surfaced_events
        .iter()
        .filter(|e| e.category == Category::Request)
        .collect();
    let errors: Vec<_> = out
        .surfaced_events
        .iter()
        .filter(|e| e.category == Category::Error)
        .collect();
    let completed: Vec<_> = out
        .surfaced_events
        .iter()
        .filter(|e| e.category == Category::Completed)
        .collect();

    assert!(!requests.is_empty());
    assert!(!errors.is_empty());
    assert!(!completed.is_empty());

    for r in &requests {
        assert_eq!(r.category.tier(), 0);
    }
    for err in &errors {
        assert_eq!(err.category.tier(), 1);
    }
    for comp in &completed {
        assert_eq!(comp.category.tier(), 2);
    }

    // Working tier is tier 3, which is strictly > tier 0, 1, 2
    assert!(Category::Working.tier() > Category::Request.tier());
    assert!(Category::Working.tier() > Category::Error.tier());
    assert!(Category::Working.tier() > Category::Completed.tier());

    // In laptop metrics, exactly 2 escalations occurred
    assert_eq!(out.result.laptop_metrics.escalations, 2);
}

#[test]
fn p5_t6_no_silent_loss() {
    let scenario = Scenario::default_scenario();
    let report = run_all_modes(&scenario, SEED, TapPolicy::Default);

    assert!(
        !report.coverage.silent_loss_detected,
        "No request or error event should be silently lost"
    );
}

#[test]
fn p5_t7_duplicate_control() {
    let scenario = Scenario::default_scenario();
    let out = run_bench_mode(&scenario, SEED, PipelineMode::Agentdesk, TapPolicy::Default);

    assert_eq!(out.result.client_counters.duplicates, 0);

    // Verify all surfaced events have unique event_ids
    let mut ids = HashSet::new();
    for ev in &out.surfaced_events {
        assert!(
            ids.insert(ev.event_id),
            "Duplicate event_id detected in surfaced events"
        );
    }

    // score_updates and state_updates are bounded
    assert!(out.result.client_counters.score_updates > 0);
    assert!(out.result.client_counters.score_updates < 200);
    assert!(out.result.client_counters.state_updates > 0);
}

#[test]
fn p5_t8_bench_run_is_reproducible() {
    let scenario = Scenario::default_scenario();
    let report1 = run_all_modes(&scenario, SEED, TapPolicy::Default);
    let report2 = run_all_modes(&scenario, SEED, TapPolicy::Default);

    let mut json1 = serde_json::to_value(&report1).unwrap();
    let mut json2 = serde_json::to_value(&report2).unwrap();

    // Ignore wall-clock timestamp field
    json1["timestamp"] = serde_json::json!("");
    json2["timestamp"] = serde_json::json!("");

    assert_eq!(
        json1, json2,
        "Repeated bench runs with identical args must produce identical reports"
    );
}
