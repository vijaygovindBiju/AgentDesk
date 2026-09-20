//! Benchmark runner coordinating virtual clock, simulator, core task, and fake client.
//! See docs/TODO.md P5.1, P5.3.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration, Utc};

use agentdesk_core::{
    Adapter, AdapterCommand, Clock, CoreCommand, CoreHandle, CoreTask, LogStoreConfig,
    ThresholdTable, VirtualClock,
};
use agentdesk_model::{Category, Event, PipelineMode, TaskId};
use agentdesk_sim::{Scenario, Simulator};

use crate::client::{FakeClient, TapPolicy};
use crate::report::{MultiModeReport, ObservedEscalation, SingleModeResult};

pub struct ModeRunOutput {
    pub result: SingleModeResult,
    pub surfaced_events: Vec<Event>,
    pub observed_escalations: HashMap<TaskId, u8>,
}

/// Run a single simulation mode with virtual clock and fake client.
pub fn run_bench_mode(
    scenario: &Scenario,
    seed: u64,
    mode: PipelineMode,
    policy: TapPolicy,
) -> ModeRunOutput {
    let clock = Arc::new(VirtualClock::at_epoch());
    let mut sim = Simulator::new(scenario.clone(), seed, clock.now());
    let (tx_adapter, mut rx_adapter) = tokio::sync::mpsc::channel(64);
    let (tx_core, mut rx_core) = tokio::sync::mpsc::channel(256);
    let handle = CoreHandle::new(tx_core);

    let mut core = CoreTask::with_seed(
        mode,
        clock.clone(),
        seed,
        LogStoreConfig::default(),
        ThresholdTable::default(),
        Some(tx_adapter),
    );
    core.register_agents(sim.agents());

    let (mut client, sink) = FakeClient::new(1, mode, policy);
    core.step(CoreCommand::Connect {
        client_id: 1,
        sink: Box::new(sink),
    });

    let tick_interval = Duration::seconds(10);
    let mut next_tick = clock.now() + tick_interval;

    while !sim.is_finished() {
        let now = clock.now();

        // 1. Poll simulator for newly due items
        let outputs = sim.poll(now);
        for output in outputs {
            core.step(CoreCommand::Adapter(output));
        }

        // 2. Poll client messages and execute client pending actions
        client.poll_incoming(now, &handle);
        client.execute_pending_actions(now, &handle);

        // 3. Process commands sent by fake client into core
        while let Ok(cmd) = rx_core.try_recv() {
            core.step(cmd);
        }

        // 4. Handle adapter feedback commands
        while let Ok(cmd) = rx_adapter.try_recv() {
            match cmd {
                AdapterCommand::Respond {
                    task_id,
                    decision,
                    now,
                    ..
                } => {
                    let _ = sim.respond(&task_id, decision, now);
                }
            }
        }

        // 5. In raw_* modes where client does not interact with core, approve blocked tasks directly
        if mode != PipelineMode::Agentdesk {
            let blocked = sim.blocked_tasks();
            if !blocked.is_empty() {
                clock.advance(Duration::seconds(5));
                let at = clock.now();
                for task_id in blocked {
                    let _ = sim.respond(&task_id, agentdesk_model::Decision::Approve, at);
                }
            }
        }

        // 6. Advance virtual clock
        let sim_due = sim.next_due();
        let target_time = match sim_due {
            Some(due) => due.min(next_tick),
            None => next_tick,
        };

        if target_time > now {
            clock.set(target_time);
        }

        if clock.now() >= next_tick {
            core.step(CoreCommand::Tick);
            next_tick = clock.now() + tick_interval;
        }

        if sim_due.is_none() && sim.blocked_tasks().is_empty() {
            break;
        }
    }

    // Final drain of client messages
    client.poll_incoming(clock.now(), &handle);
    client.finalize();

    let client_counters = client.counters().clone();
    let total_bytes = client.total_bytes();
    let laptop_metrics = core.metrics;
    let surfaced_events: Vec<Event> = client.surfaced_events().into_iter().cloned().collect();
    let observed_escalations = client.observed_escalations().clone();

    let surfaced_count = if mode == PipelineMode::Agentdesk {
        surfaced_events.len() as u64
    } else {
        client_counters.summaries_rendered
    };

    let result = SingleModeResult {
        mode: match mode {
            PipelineMode::RawLines => "raw_lines".to_string(),
            PipelineMode::RawEvents => "raw_events".to_string(),
            PipelineMode::Agentdesk => "agentdesk".to_string(),
        },
        transmitted_events: laptop_metrics.transmitted_events,
        laptop_metrics,
        client_counters,
        transmitted_bytes: total_bytes,
        surfaced_events: surfaced_count,
    };

    ModeRunOutput {
        result,
        surfaced_events,
        observed_escalations,
    }
}

/// Run all three modes and build the unified comparison report (P5.3).
pub fn run_all_modes(scenario: &Scenario, seed: u64, policy: TapPolicy) -> MultiModeReport {
    let thresholds = ThresholdTable::default();
    let ground_truth = scenario.ground_truth(&thresholds);

    // 1. Run raw_lines
    let raw_lines_out = run_bench_mode(scenario, seed, PipelineMode::RawLines, policy);

    // 2. Run raw_events
    let raw_events_out = run_bench_mode(scenario, seed, PipelineMode::RawEvents, policy);

    // 3. Run agentdesk
    let agentdesk_out = run_bench_mode(scenario, seed, PipelineMode::Agentdesk, policy);

    // Coverage analysis on agentdesk output
    let mut surfaced_requests = 0;
    let mut surfaced_errors = 0;
    let mut surfaced_completed_important = 0;
    let mut surfaced_completed_routine = 0;

    for ev in &agentdesk_out.surfaced_events {
        match ev.category {
            Category::Request => surfaced_requests += 1,
            Category::Error => surfaced_errors += 1,
            Category::Completed => {
                if ev.severity.as_u8() >= 2 {
                    surfaced_completed_important += 1;
                } else {
                    surfaced_completed_routine += 1;
                }
            }
            Category::Working => {}
        }
    }

    let mut observed_escalations = Vec::new();
    for (tid, lvl) in &agentdesk_out.observed_escalations {
        observed_escalations.push(ObservedEscalation {
            task_id: tid.clone(),
            highest_level: *lvl,
        });
    }
    observed_escalations.sort_by(|a, b| a.task_id.cmp(&b.task_id));

    // Check silent loss (P5.T6): every raw event mapping to request or error must be surfaced
    let clock = Arc::new(VirtualClock::at_epoch());
    let mut verification_sim = Simulator::new(scenario.clone(), seed, clock.now());
    let mut silent_loss_detected = false;

    while !verification_sim.is_finished() {
        let now = clock.now();
        let outputs = verification_sim.poll(now);
        for o in outputs {
            if let agentdesk_core::AdapterOutput::Event(raw) = o {
                let c = agentdesk_core::classify(&raw.kind);
                if c.category == Category::Request || c.category == Category::Error {
                    let has_match = agentdesk_out
                        .surfaced_events
                        .iter()
                        .any(|e| e.task_id == raw.task_id && e.category == c.category);
                    if !has_match {
                        silent_loss_detected = true;
                    }
                }
            }
        }
        let blocked = verification_sim.blocked_tasks();
        if !blocked.is_empty() {
            clock.advance(Duration::seconds(5));
            let at = clock.now();
            for tid in blocked {
                let _ = verification_sim.respond(&tid, agentdesk_model::Decision::Approve, at);
            }
        }
        let due = verification_sim.next_due();
        match due {
            Some(d) => clock.set(d),
            None => break,
        }
    }

    let timestamp = Utc::now().to_rfc3339();

    MultiModeReport::new(
        scenario.name.clone(),
        seed,
        policy,
        timestamp,
        raw_lines_out.result,
        raw_events_out.result,
        agentdesk_out.result,
        ground_truth,
        surfaced_requests,
        surfaced_errors,
        surfaced_completed_important,
        surfaced_completed_routine,
        observed_escalations,
        silent_loss_detected,
    )
}
