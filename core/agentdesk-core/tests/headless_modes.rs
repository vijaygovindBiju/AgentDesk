//! Phase 4 integration tests and golden-file comparison.
//! Asserts deterministic execution of CoreTask in all three modes.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use chrono::Duration;

use agentdesk_core::{
    Adapter, AdapterCommand, Clock, CoreCommand, CoreTask, LogStoreConfig, ThresholdTable, VecSink,
    VirtualClock,
};
use agentdesk_model::{Body, Category, Decision, Message, PipelineMode, RespondRequest};
use agentdesk_sim::{Scenario, Simulator};

fn run_headless_simulation(mode: PipelineMode) -> (Vec<Message>, agentdesk_core::Metrics) {
    let clock = Arc::new(VirtualClock::at_epoch());
    let scenario = Scenario::default_scenario();
    let mut sim = Simulator::new(scenario, 42, clock.now());
    let (tx_adapter, mut rx_adapter) = tokio::sync::mpsc::channel(64);

    let mut core = CoreTask::with_seed(
        mode,
        clock.clone(),
        42,
        LogStoreConfig::default(),
        ThresholdTable::default(),
        Some(tx_adapter),
    );
    core.register_agents(sim.agents());

    core.step(CoreCommand::Connect {
        client_id: 1,
        sink: Box::new(VecSink::new()),
    });

    let tick_interval = Duration::seconds(10);
    let mut next_tick = clock.now() + tick_interval;

    while !sim.is_finished() {
        let now = clock.now();
        // 1. Poll simulator
        let outputs = sim.poll(now);
        for o in outputs {
            core.step(CoreCommand::Adapter(o));
        }

        // 2. Check blocked tasks
        let blocked = sim.blocked_tasks();
        if !blocked.is_empty() {
            clock.advance(Duration::seconds(5));
            let at = clock.now();
            for task_id in blocked {
                if mode == PipelineMode::Agentdesk {
                    let req_id = core
                        .event_store
                        .iter()
                        .find(|e| {
                            e.category == Category::Request
                                && e.task_id.as_deref() == Some(&task_id)
                        })
                        .map(|e| e.event_id);

                    if let Some(eid) = req_id {
                        core.step(CoreCommand::Client {
                            client_id: 1,
                            message: Message::with_request_id(
                                "r-resp",
                                Body::RespondRequest(RespondRequest {
                                    event_id: eid,
                                    decision: Decision::Approve,
                                    selected_options: None,
                                    text_input: None,
                                }),
                            ),
                        });
                    }

                    while let Ok(cmd) = rx_adapter.try_recv() {
                        match cmd {
                            AdapterCommand::Respond {
                                task_id,
                                decision,
                                now,
                                ..
                            } => {
                                sim.respond(&task_id, decision, now).unwrap();
                            }
                        }
                    }
                } else {
                    sim.respond(&task_id, Decision::Approve, at).unwrap();
                }
            }
            continue;
        }

        // 3. Advance time to next due or next tick
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

    let sink_box = core.sinks.remove(&1).unwrap();
    let vec_sink = sink_box.as_any().downcast_ref::<VecSink>().unwrap();
    (vec_sink.messages.clone(), core.metrics)
}

fn format_messages_golden(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|m| serde_json::to_string(m).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn p4_t4_mode_test_message_types() {
    let (agentdesk_msgs, _) = run_headless_simulation(PipelineMode::Agentdesk);
    let (raw_events_msgs, _) = run_headless_simulation(PipelineMode::RawEvents);
    let (raw_lines_msgs, _) = run_headless_simulation(PipelineMode::RawLines);

    // 1. raw_events forwards only Body::RawEvent
    assert!(!raw_events_msgs.is_empty());
    assert!(
        raw_events_msgs
            .iter()
            .all(|m| matches!(m.body, Body::RawEvent(_)))
    );

    // 2. raw_lines forwards only Body::RawLine
    assert!(!raw_lines_msgs.is_empty());
    assert!(
        raw_lines_msgs
            .iter()
            .all(|m| matches!(m.body, Body::RawLine(_)))
    );

    // 3. agentdesk forwards only queue-derived messages (Event, ScoreUpdate, StateUpdate, CommandResult)
    assert!(!agentdesk_msgs.is_empty());
    assert!(agentdesk_msgs.iter().all(|m| matches!(
        m.body,
        Body::Event(_)
            | Body::ScoreUpdate(_)
            | Body::StateUpdate(_)
            | Body::CommandResult(_)
            | Body::Snapshot(_)
    )));
}

#[test]
fn p4_t5_respond_request_resumes_blocked_simulated_task() {
    let (msgs, metrics) = run_headless_simulation(PipelineMode::Agentdesk);
    assert_eq!(metrics.responses, 1);

    // Verify approval result was transmitted
    let has_approved_result = msgs.iter().any(|m| {
        matches!(
            &m.body,
            Body::CommandResult(res) if res.ok
        )
    });
    assert!(has_approved_result);

    // Verify the task subsequently completed
    let has_db_completion = msgs.iter().any(|m| match &m.body {
        Body::Event(push) => {
            push.event.task_id.as_deref() == Some("task-db")
                && push.event.category == Category::Completed
        }
        _ => false,
    });
    assert!(has_db_completion, "task-db must complete after approval");
}

#[test]
fn p4_t3_golden_file_runs_deterministically() {
    let (agentdesk_msgs, _) = run_headless_simulation(PipelineMode::Agentdesk);
    let (raw_events_msgs, _) = run_headless_simulation(PipelineMode::RawEvents);
    let (raw_lines_msgs, _) = run_headless_simulation(PipelineMode::RawLines);

    let golden_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("golden");
    fs::create_dir_all(&golden_dir).unwrap();

    let path_ag = golden_dir.join("agentdesk.golden");
    let path_re = golden_dir.join("raw_events.golden");
    let path_rl = golden_dir.join("raw_lines.golden");

    let text_ag = format_messages_golden(&agentdesk_msgs);
    let text_re = format_messages_golden(&raw_events_msgs);
    let text_rl = format_messages_golden(&raw_lines_msgs);

    if !path_ag.exists() {
        fs::write(&path_ag, &text_ag).unwrap();
    }
    if !path_re.exists() {
        fs::write(&path_re, &text_re).unwrap();
    }
    if !path_rl.exists() {
        fs::write(&path_rl, &text_rl).unwrap();
    }

    let expected_ag = fs::read_to_string(&path_ag).unwrap();
    let expected_re = fs::read_to_string(&path_re).unwrap();
    let expected_rl = fs::read_to_string(&path_rl).unwrap();

    assert_eq!(
        text_ag, expected_ag,
        "agentdesk mode must match golden file"
    );
    assert_eq!(
        text_re, expected_re,
        "raw_events mode must match golden file"
    );
    assert_eq!(
        text_rl, expected_rl,
        "raw_lines mode must match golden file"
    );
}
