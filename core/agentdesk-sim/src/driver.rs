//! Headless driver: runs a simulator to completion on a virtual clock,
//! answering requests with a caller-supplied policy. Used by tests and the
//! bench; the live daemon drives adapters from its core task instead.

use chrono::{DateTime, Duration, Utc};

use agentdesk_core::{Adapter, AdapterOutput, Clock, VirtualClock};
use agentdesk_model::{Decision, TaskId};

use crate::simulator::Simulator;

/// One emitted item with the virtual time it was emitted at.
pub type Timeline = Vec<(DateTime<Utc>, AdapterOutput)>;

/// Advance `clock` through every due instant until the simulator finishes.
/// Blocked requests are answered by `decide` after `response_delay`.
pub fn run_to_end(sim: &mut Simulator, clock: &VirtualClock, response_delay: Duration, mut decide: impl FnMut(&TaskId) -> Decision) -> Timeline {
    let mut timeline = Timeline::new();
    loop {
        let now = clock.now();
        for o in sim.poll(now) {
            timeline.push((now, o));
        }
        let blocked = sim.blocked_tasks();
        if !blocked.is_empty() {
            clock.advance(response_delay);
            let at = clock.now();
            for task_id in blocked {
                let d = decide(&task_id);
                sim.respond(&task_id, d, at).expect("task was reported blocked");
            }
            continue;
        }
        match sim.next_due() {
            Some(due) => clock.set(due.max(now)),
            None => break,
        }
    }
    debug_assert!(sim.is_finished());
    timeline
}
