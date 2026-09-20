//! P3.T8: Pipeline test driven by the simulator.
//! Asserts that every Request and Error in the scenario appears exactly once
//! in the queue and event store, with logs appropriately pinned.

use std::collections::HashSet;

use chrono::Duration;

use agentdesk_core::{Adapter, Clock, Pipeline, VirtualClock};
use agentdesk_model::{Category, Decision};
use agentdesk_sim::{Scenario, Simulator, run_to_end};

const SEED: u64 = 42;

#[test]
fn simulator_driven_pipeline_every_request_and_error_appears_exactly_once() {
    let scenario = Scenario::default_scenario();
    let clock = VirtualClock::at_epoch();
    let start_time = clock.now();
    let mut sim = Simulator::new(scenario, SEED, start_time);

    let mut pipeline = Pipeline::default();
    pipeline.register_agents(sim.agents());

    // Run the simulator to completion, collecting timeline
    let timeline = run_to_end(&mut sim, &clock, Duration::seconds(5), |_| {
        Decision::Approve
    });
    assert!(!timeline.is_empty());

    let mut processed_event_ids = Vec::new();
    for (at, output) in timeline {
        if let Some((event, entry)) = pipeline.handle_output(output, at).unwrap() {
            assert_eq!(event.event_id, entry.event_id);
            processed_event_ids.push(event.event_id);
        }
    }

    // Assert no duplicate event_id was emitted
    let mut unique_ids = HashSet::new();
    for id in &processed_event_ids {
        assert!(unique_ids.insert(*id), "duplicate event_id: {id}");
    }

    // Count categories in the event store and queue
    let mut request_events = Vec::new();
    let mut error_events = Vec::new();

    for ev in pipeline.event_store.iter() {
        match ev.category {
            Category::Request => request_events.push(ev.clone()),
            Category::Error => error_events.push(ev.clone()),
            _ => {}
        }
    }

    // From default scenario: exactly 1 request (approval_required)
    assert_eq!(
        request_events.len(),
        1,
        "expected exactly 1 request event, got: {:?}",
        request_events.iter().map(|e| &e.kind).collect::<Vec<_>>()
    );

    // From default scenario: at least 3 errors (build_failed, test_failed, cancelled_by_agent)
    assert!(
        error_events.len() >= 3,
        "expected at least 3 error events, got: {:?}",
        error_events.iter().map(|e| &e.kind).collect::<Vec<_>>()
    );

    // Every Request appears in the queue with tier 0 and is pinned
    for req in &request_events {
        let entry = pipeline
            .queue
            .get(&req.event_id)
            .expect("request must be in queue");
        assert_eq!(entry.tier, 0, "request must have tier 0");
        assert!(req.log_range.pinned, "request log_range must be pinned");
        assert!(
            pipeline.log_store.is_pinned(&req.event_id),
            "request logs must be pinned in log store"
        );
    }

    // Every Error appears in the queue with tier 1 and is pinned
    for err in &error_events {
        let entry = pipeline
            .queue
            .get(&err.event_id)
            .expect("error must be in queue");
        assert_eq!(entry.tier, 1, "error must have tier 1");
        assert!(err.log_range.pinned, "error log_range must be pinned");
        assert!(
            pipeline.log_store.is_pinned(&err.event_id),
            "error logs must be pinned in log store"
        );
    }

    // Check that get_event_logs returns valid logs for the pinned errors
    for err in &error_events {
        let logs = pipeline
            .log_store
            .get_event_logs(&err.event_id, -1, 10)
            .expect("pinned error logs retrievable");
        assert_eq!(logs.event_id, err.event_id);
    }

    // Check Metrics counters
    assert_eq!(
        pipeline.metrics.processed_events,
        processed_event_ids.len() as u64
    );
    assert_eq!(
        pipeline.metrics.surfaced_summaries,
        processed_event_ids.len() as u64
    );
    assert!(pipeline.metrics.raw_lines > 0);
    assert_eq!(pipeline.metrics.dropped_events, 0);

    // Ensure snapshots can be cleanly taken
    let snapshot = pipeline.queue.ordered_snapshot();
    assert!(!snapshot.is_empty());
    // Ordering check: the snapshot must be sorted by order_key()
    for window in snapshot.windows(2) {
        assert!(window[0].order_key() <= window[1].order_key());
    }
}
