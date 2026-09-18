//! Core Task: the central async actor that owns the event pipeline,
//! priority queue, log store, task tracker, and metrics.
//! See docs/ARCHITECTURE.md "Core task" and docs/SYSTEM_DESIGN.md.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use agentdesk_model::{
    AgentInfo, Body, CommandError, CommandResult, Decision, ErrorReply, EventPush,
    EventRef, GetEventLogs, Message, PipelineMode, RawLine, RespondRequest,
    Snapshot, TaskId,
};

use crate::adapter::AdapterOutput;
use crate::clock::Clock;
use crate::event_store::EventStore;
use crate::log_store::{LogStore, LogStoreConfig};
use crate::metrics::Metrics;
use crate::processor::EventProcessor;
use crate::queue::PriorityQueue;
use crate::sink::{SinkError, TransportSink};
use crate::tracker::{TaskTracker, ThresholdTable};

pub type ClientId = u64;

/// Commands sent to an adapter to resume blocked tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterCommand {
    Respond {
        task_id: TaskId,
        decision: Decision,
        now: DateTime<Utc>,
    },
}

/// Incoming messages handled by the Core task inbox.
pub enum CoreCommand {
    /// Output from an adapter (raw line or raw event).
    Adapter(AdapterOutput),
    /// Request or command from a connected client.
    Client {
        client_id: ClientId,
        message: Message,
    },
    /// Periodic tick for score re-computation and watchdog escalation.
    Tick,
    /// Attach an outbound transport sink for a client.
    Connect {
        client_id: ClientId,
        sink: Box<dyn TransportSink>,
    },
    /// Remove an outbound transport sink for a client.
    Disconnect {
        client_id: ClientId,
    },
    /// Push a fresh snapshot of all live entries and events to the client.
    SendSnapshot {
        client_id: ClientId,
    },
    /// Shutdown the core actor loop.
    Shutdown,
}

pub struct CoreTask {
    pub mode: PipelineMode,
    pub processor: EventProcessor,
    pub event_store: EventStore,
    pub queue: PriorityQueue,
    pub log_store: LogStore,
    pub tracker: TaskTracker,
    pub metrics: Metrics,
    pub clock: Arc<dyn Clock>,
    pub sinks: HashMap<ClientId, Box<dyn TransportSink>>,
    pub adapter_responder: Option<tokio::sync::mpsc::Sender<AdapterCommand>>,
}

impl CoreTask {
    pub fn new(
        mode: PipelineMode,
        clock: Arc<dyn Clock>,
        log_config: LogStoreConfig,
        thresholds: ThresholdTable,
        adapter_responder: Option<tokio::sync::mpsc::Sender<AdapterCommand>>,
    ) -> Self {
        CoreTask {
            mode,
            processor: EventProcessor::new(),
            event_store: EventStore::new(),
            queue: PriorityQueue::new(),
            log_store: LogStore::new(log_config),
            tracker: TaskTracker::new(thresholds),
            metrics: Metrics::new(),
            clock,
            sinks: HashMap::new(),
            adapter_responder,
        }
    }

    pub fn with_seed(
        mode: PipelineMode,
        clock: Arc<dyn Clock>,
        seed: u64,
        log_config: LogStoreConfig,
        thresholds: ThresholdTable,
        adapter_responder: Option<tokio::sync::mpsc::Sender<AdapterCommand>>,
    ) -> Self {
        CoreTask {
            mode,
            processor: EventProcessor::with_seed(seed),
            event_store: EventStore::new(),
            queue: PriorityQueue::new(),
            log_store: LogStore::new(log_config),
            tracker: TaskTracker::new(thresholds),
            metrics: Metrics::new(),
            clock,
            sinks: HashMap::new(),
            adapter_responder,
        }
    }

    pub fn register_agents(&mut self, agents: &[AgentInfo]) {
        self.processor.register_agents(agents);
    }

    /// Send a message to a specific client. Accounts for bytes and disconnections.
    pub fn send_to(&mut self, client_id: ClientId, message: Message) {
        let is_event_msg = matches!(message.body, Body::Event(_) | Body::RawEvent(_));
        let mut remove = false;

        if let Some(sink) = self.sinks.get_mut(&client_id) {
            match sink.send(&message) {
                Ok(bytes) => {
                    self.metrics.transmitted_bytes += bytes as u64;
                    if is_event_msg {
                        self.metrics.transmitted_events += 1;
                    }
                }
                Err(SinkError::ChannelFull) => {
                    self.metrics.slow_client_disconnects += 1;
                    remove = true;
                }
                Err(SinkError::Closed) => {
                    remove = true;
                }
                Err(SinkError::Io(_)) => {}
            }
        }

        if remove {
            self.sinks.remove(&client_id);
        }
    }

    /// Broadcast a message to all connected clients.
    pub fn broadcast(&mut self, message: Message) {
        let client_ids: Vec<ClientId> = self.sinks.keys().copied().collect();
        for cid in client_ids {
            self.send_to(cid, message.clone());
        }
    }

    /// Send a snapshot to a specific client.
    pub fn send_snapshot(&mut self, client_id: ClientId) {
        let entries = self.queue.ordered_snapshot();
        let events = entries
            .iter()
            .filter_map(|e| self.event_store.get(&e.event_id).cloned())
            .collect();
        self.send_to(
            client_id,
            Message::push(Body::Snapshot(Snapshot { entries, events })),
        );
    }

    /// Execute a single step (synchronous command transition).
    pub fn step(&mut self, command: CoreCommand) {
        let now = self.clock.now();

        match command {
            CoreCommand::Connect { client_id, sink } => {
                self.sinks.insert(client_id, sink);
            }
            CoreCommand::Disconnect { client_id } => {
                self.sinks.remove(&client_id);
            }
            CoreCommand::SendSnapshot { client_id } => {
                if self.mode == PipelineMode::Agentdesk {
                    self.send_snapshot(client_id);
                }
            }
            CoreCommand::Shutdown => {}
            CoreCommand::Tick => {
                if self.mode == PipelineMode::Agentdesk {
                    self.handle_tick(now);
                }
            }
            CoreCommand::Adapter(output) => {
                self.handle_adapter_output(output, now);
            }
            CoreCommand::Client { client_id, message } => {
                self.handle_client_message(client_id, message, now);
            }
        }
    }

    fn handle_tick(&mut self, now: DateTime<Utc>) {
        // 1. Re-score priority queue
        let score_updates = self.queue.rescore(&self.event_store, now);
        for update in score_updates {
            self.broadcast(Message::push(Body::ScoreUpdate(update)));
        }

        // 2. Watchdog escalation check
        let escalations = self.tracker.tick(now);
        for esc in escalations {
            self.metrics.escalations += 1;
            #[allow(clippy::collapsible_if)]
            if let Ok(Some(_)) = self.queue.escalate(&esc.event_id, esc.level) {
                if let Some(score_upd) =
                    self.queue.update_entry_score(&esc.event_id, &self.event_store, now)
                {
                    self.broadcast(Message::push(Body::ScoreUpdate(score_upd)));
                }
            }
        }
    }

    fn handle_adapter_output(&mut self, output: AdapterOutput, now: DateTime<Utc>) {
        match self.mode {
            PipelineMode::Agentdesk => match output {
                AdapterOutput::Line { agent_id, text } => {
                    self.processor.process_line(
                        &agent_id,
                        text,
                        now,
                        &mut self.log_store,
                        &mut self.metrics,
                    );
                }
                AdapterOutput::Event(raw) => {
                    if let Ok((event, entry)) = self.processor.process_raw_event(
                        raw,
                        now,
                        &mut self.event_store,
                        &mut self.queue,
                        &mut self.log_store,
                        &mut self.metrics,
                    ) {
                        // Observe event in task tracker
                        if let Some(superseded_ids) = self.tracker.observe_event(&event) {
                            for id in superseded_ids {
                                if let Ok(Some(update)) = self.queue.supersede(&id) {
                                    self.broadcast(Message::push(Body::StateUpdate(update)));
                                }
                            }
                        }
                        // Push event to clients
                        self.broadcast(Message::push(Body::Event(EventPush {
                            event,
                            entry,
                        })));
                    }
                }
            },
            PipelineMode::RawEvents => match output {
                AdapterOutput::Event(raw) => {
                    self.metrics.raw_events += 1;
                    self.broadcast(Message::push(Body::RawEvent(raw)));
                }
                AdapterOutput::Line { .. } => {
                    self.metrics.raw_lines += 1;
                }
            },
            PipelineMode::RawLines => match output {
                AdapterOutput::Line { agent_id, text } => {
                    self.metrics.raw_lines += 1;
                    let line = self.log_store.append(&agent_id, text, now);
                    self.broadcast(Message::push(Body::RawLine(RawLine { agent_id, line })));
                }
                AdapterOutput::Event(raw) => {
                    self.metrics.raw_events += 1;
                    for text in raw.log_lines {
                        self.metrics.raw_lines += 1;
                        let line = self.log_store.append(&raw.agent_id, text, now);
                        self.broadcast(Message::push(Body::RawLine(RawLine {
                            agent_id: raw.agent_id.clone(),
                            line,
                        })));
                    }
                }
            },
        }
    }

    fn handle_client_message(
        &mut self,
        client_id: ClientId,
        message: Message,
        now: DateTime<Utc>,
    ) {
        let req_id = message.request_id.clone();

        match message.body {
            Body::GetMetrics(_) => {
                let snap = self.metrics.snapshot();
                self.send_to(client_id, Message {
                    request_id: req_id,
                    body: Body::Metrics(snap),
                });
            }
            Body::Ack(EventRef { event_id }) => {
                let res = self.queue.ack(&event_id);
                match res {
                    Ok(Some(state_update)) => {
                        self.metrics.acks += 1;
                        self.broadcast(Message::push(Body::StateUpdate(state_update)));
                        self.send_to(client_id, Message {
                            request_id: req_id,
                            body: Body::CommandResult(CommandResult::ok()),
                        });
                    }
                    Ok(None) => {
                        self.metrics.acks += 1;
                        self.send_to(client_id, Message {
                            request_id: req_id,
                            body: Body::CommandResult(CommandResult::ok()),
                        });
                    }
                    Err(err) => {
                        self.send_to(client_id, Message {
                            request_id: req_id,
                            body: Body::CommandResult(CommandResult::err(err)),
                        });
                    }
                }
            }
            Body::Dismiss(EventRef { event_id }) => {
                let res = self.queue.dismiss(&event_id);
                match res {
                    Ok(Some(state_update)) => {
                        self.metrics.dismissals += 1;
                        self.broadcast(Message::push(Body::StateUpdate(state_update)));
                        self.send_to(client_id, Message {
                            request_id: req_id,
                            body: Body::CommandResult(CommandResult::ok()),
                        });
                    }
                    Ok(None) => {
                        self.metrics.dismissals += 1;
                        self.send_to(client_id, Message {
                            request_id: req_id,
                            body: Body::CommandResult(CommandResult::ok()),
                        });
                    }
                    Err(err) => {
                        self.send_to(client_id, Message {
                            request_id: req_id,
                            body: Body::CommandResult(CommandResult::err(err)),
                        });
                    }
                }
            }
            Body::RespondRequest(RespondRequest { event_id, decision }) => {
                let task_id = self
                    .event_store
                    .get(&event_id)
                    .and_then(|e| e.task_id.clone());
                let res = self.queue.respond_request(&event_id, decision);

                match res {
                    Ok(state_update) => {
                        self.metrics.responses += 1;
                        if let Some(sc) =
                            self.queue.update_entry_score(&event_id, &self.event_store, now)
                        {
                            self.broadcast(Message::push(Body::ScoreUpdate(sc)));
                        }
                        self.broadcast(Message::push(Body::StateUpdate(state_update)));

                        // Deliver to adapter responder so simulator unblocks (P4.7)
                        if let (Some(tid), Some(tx)) = (task_id, &self.adapter_responder) {
                            let _ = tx.try_send(AdapterCommand::Respond {
                                task_id: tid,
                                decision,
                                now,
                            });
                        }

                        self.send_to(client_id, Message {
                            request_id: req_id,
                            body: Body::CommandResult(CommandResult::ok()),
                        });
                    }
                    Err(err) => {
                        self.send_to(client_id, Message {
                            request_id: req_id,
                            body: Body::CommandResult(CommandResult::err(err)),
                        });
                    }
                }
            }
            Body::GetEventDetails(EventRef { event_id }) => {
                if let Ok(Some(state_update)) = self.queue.mark_seen(&event_id) {
                    self.metrics.detail_refetches += 1;
                    self.broadcast(Message::push(Body::StateUpdate(state_update)));
                    if let Some(sc) =
                        self.queue.update_entry_score(&event_id, &self.event_store, now)
                    {
                        self.broadcast(Message::push(Body::ScoreUpdate(sc)));
                    }
                } else if self.queue.get(&event_id).is_some() {
                    self.metrics.detail_refetches += 1;
                }

                if let (Some(ev), Some(en)) =
                    (self.event_store.get(&event_id), self.queue.get(&event_id))
                {
                    self.send_to(client_id, Message {
                        request_id: req_id,
                        body: Body::EventDetails(EventPush {
                            event: ev.clone(),
                            entry: en.clone(),
                        }),
                    });
                } else {
                    self.send_to(client_id, Message {
                        request_id: req_id,
                        body: Body::Error(ErrorReply {
                            code: "no_such_event".into(),
                            message: "event not found".into(),
                        }),
                    });
                }
            }
            Body::GetEventLogs(GetEventLogs { event_id, offset, limit }) => {
                self.metrics.log_page_requests += 1;
                if let Some(logs) = self.log_store.get_event_logs(&event_id, offset, limit) {
                    self.send_to(client_id, Message {
                        request_id: req_id,
                        body: Body::EventLogs(logs),
                    });
                } else {
                    self.send_to(client_id, Message {
                        request_id: req_id,
                        body: Body::CommandResult(CommandResult::err(CommandError::NoSuchEvent)),
                    });
                }
            }
            _ => {
                self.send_to(client_id, Message {
                    request_id: req_id,
                    body: Body::Error(ErrorReply {
                        code: "unsupported".into(),
                        message: "message type not supported as a request".into(),
                    }),
                });
            }
        }
    }

    /// Run the core task asynchronously until a Shutdown command is received or channel is closed.
    pub async fn run(mut self, mut receiver: tokio::sync::mpsc::Receiver<CoreCommand>) {
        while let Some(cmd) = receiver.recv().await {
            let is_shutdown = matches!(cmd, CoreCommand::Shutdown);
            self.step(cmd);
            if is_shutdown {
                break;
            }
        }
    }
}

/// Handle for communicating with the running Core task via channels.
#[derive(Clone)]
pub struct CoreHandle {
    sender: tokio::sync::mpsc::Sender<CoreCommand>,
}

impl CoreHandle {
    pub fn new(sender: tokio::sync::mpsc::Sender<CoreCommand>) -> Self {
        CoreHandle { sender }
    }

    pub async fn send(&self, cmd: CoreCommand) -> Result<(), tokio::sync::mpsc::error::SendError<CoreCommand>> {
        self.sender.send(cmd).await
    }

    #[allow(clippy::result_large_err)]
    pub fn try_send(&self, cmd: CoreCommand) -> Result<(), tokio::sync::mpsc::error::TrySendError<CoreCommand>> {
        self.sender.try_send(cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::VirtualClock;
    use crate::sink::VecSink;
    use agentdesk_model::{
        Details, Empty, Operation, RawAgentEvent, RequestInfo,
    };
    use chrono::Duration;

    fn make_raw(kind: &str, task_id: Option<&str>, lines: Vec<String>) -> RawAgentEvent {
        RawAgentEvent {
            agent_id: "agent-1".into(),
            agent_seq: 1,
            task_id: task_id.map(|s| s.to_string()),
            kind: kind.into(),
            operation: Operation::Build,
            message: "msg".into(),
            details: Details::new(),
            log_lines: lines,
            request: None,
        }
    }

    #[test]
    fn mode_raw_events_forwards_only_raw_events() {
        let clock = Arc::new(VirtualClock::at_epoch());
        let mut core = CoreTask::new(
            PipelineMode::RawEvents,
            clock,
            LogStoreConfig::default(),
            ThresholdTable::default(),
            None,
        );

        core.step(CoreCommand::Connect {
            client_id: 1,
            sink: Box::new(VecSink::new()),
        });

        // 1. Send an adapter raw line -> should NOT be forwarded
        core.step(CoreCommand::Adapter(AdapterOutput::Line {
            agent_id: "agent-1".into(),
            text: "noisy line".into(),
        }));

        // 2. Send an adapter event -> should be forwarded as Body::RawEvent
        let raw = make_raw("progress", None, vec![]);
        core.step(CoreCommand::Adapter(AdapterOutput::Event(raw.clone())));

        let sink = core.sinks.get_mut(&1).unwrap();
        let vsink = sink.send(&Message::push(Body::GetMetrics(Empty {}))).unwrap();
        assert!(vsink > 0);
        // Look at recorded messages: exactly 1 raw_event + 1 get_metrics we just sent
        assert_eq!(core.metrics.raw_lines, 1);
        assert_eq!(core.metrics.raw_events, 1);
        assert_eq!(core.metrics.transmitted_events, 1);
    }

    #[test]
    fn mode_raw_lines_forwards_only_lines() {
        let clock = Arc::new(VirtualClock::at_epoch());
        let mut core = CoreTask::new(
            PipelineMode::RawLines,
            clock,
            LogStoreConfig::default(),
            ThresholdTable::default(),
            None,
        );

        core.step(CoreCommand::Connect {
            client_id: 1,
            sink: Box::new(VecSink::new()),
        });

        // Send line
        core.step(CoreCommand::Adapter(AdapterOutput::Line {
            agent_id: "agent-1".into(),
            text: "line 1".into(),
        }));

        // Send event with 2 log lines
        let raw = make_raw("progress", None, vec!["event line 1".into(), "event line 2".into()]);
        core.step(CoreCommand::Adapter(AdapterOutput::Event(raw)));

        assert_eq!(core.metrics.raw_lines, 3);
        assert_eq!(core.metrics.raw_events, 1);
        assert_eq!(core.metrics.transmitted_events, 0); // lines are not events
    }

    #[test]
    fn mode_agentdesk_forwards_only_queue_derived_messages() {
        let clock = Arc::new(VirtualClock::at_epoch());
        let mut core = CoreTask::new(
            PipelineMode::Agentdesk,
            clock,
            LogStoreConfig::default(),
            ThresholdTable::default(),
            None,
        );

        core.step(CoreCommand::Connect {
            client_id: 1,
            sink: Box::new(VecSink::new()),
        });

        // Send raw line -> stored in log buffer, not pushed
        core.step(CoreCommand::Adapter(AdapterOutput::Line {
            agent_id: "agent-1".into(),
            text: "log noise".into(),
        }));

        // Send raw event -> processed and pushed as Body::Event
        let raw = make_raw("approval_required", Some("t1"), vec![]);
        core.step(CoreCommand::Adapter(AdapterOutput::Event(raw)));

        assert_eq!(core.metrics.raw_lines, 1);
        assert_eq!(core.metrics.processed_events, 1);
        assert_eq!(core.metrics.transmitted_events, 1);
        assert_eq!(core.queue.len(), 1);
    }

    #[test]
    fn tick_emits_score_update_only_for_changed_entries() {
        let clock = Arc::new(VirtualClock::at_epoch());
        let mut core = CoreTask::new(
            PipelineMode::Agentdesk,
            clock.clone(),
            LogStoreConfig::default(),
            ThresholdTable::default(),
            None,
        );

        core.step(CoreCommand::Connect {
            client_id: 1,
            sink: Box::new(VecSink::new()),
        });

        // Error event (will decay recency over time)
        let raw = make_raw("build_failed", Some("t1"), vec![]);
        core.step(CoreCommand::Adapter(AdapterOutput::Event(raw)));

        // Immediate tick at t=0: score unchanged, no ScoreUpdate pushed
        let bytes_before = core.metrics.transmitted_bytes;
        core.step(CoreCommand::Tick);
        assert_eq!(core.metrics.transmitted_bytes, bytes_before);

        // Advance clock by 15 min: recency bonus decays
        clock.advance(Duration::minutes(15));
        core.step(CoreCommand::Tick);
        assert!(core.metrics.transmitted_bytes > bytes_before);
    }

    #[tokio::test]
    async fn respond_request_delivers_decision_to_adapter_responder() {
        let clock = Arc::new(VirtualClock::at_epoch());
        let (tx_adapter, mut rx_adapter) = tokio::sync::mpsc::channel(16);

        let mut core = CoreTask::new(
            PipelineMode::Agentdesk,
            clock.clone(),
            LogStoreConfig::default(),
            ThresholdTable::default(),
            Some(tx_adapter),
        );

        core.step(CoreCommand::Connect {
            client_id: 1,
            sink: Box::new(VecSink::new()),
        });

        // Push approval_required event
        let mut raw = make_raw("approval_required", Some("task-auth"), vec![]);
        raw.request = Some(RequestInfo {
            prompt: "Allow migration?".into(),
            options: vec!["approve".into(), "deny".into()],
        });
        core.step(CoreCommand::Adapter(AdapterOutput::Event(raw)));

        let event_id = core.queue.ordered_snapshot()[0].event_id;

        // Client approves request
        let respond_msg = Message::with_request_id(
            "r-42",
            Body::RespondRequest(RespondRequest {
                event_id,
                decision: Decision::Approve,
            }),
        );
        core.step(CoreCommand::Client {
            client_id: 1,
            message: respond_msg,
        });

        // Check that adapter responder received the decision! (P4.7)
        let cmd = rx_adapter.try_recv().expect("adapter must receive command");
        match cmd {
            AdapterCommand::Respond {
                task_id,
                decision,
                now,
            } => {
                assert_eq!(task_id, "task-auth");
                assert_eq!(decision, Decision::Approve);
                assert_eq!(now, clock.now());
            }
        }

        // Check entry in queue is resolved
        let entry = core.queue.get(&event_id).unwrap();
        assert_eq!(entry.resolution, Some(agentdesk_model::Resolution::Approved));
    }

    #[test]
    fn get_metrics_handled_by_core_task() {
        let clock = Arc::new(VirtualClock::at_epoch());
        let mut core = CoreTask::new(
            PipelineMode::Agentdesk,
            clock,
            LogStoreConfig::default(),
            ThresholdTable::default(),
            None,
        );

        core.step(CoreCommand::Connect {
            client_id: 1,
            sink: Box::new(VecSink::new()),
        });

        core.step(CoreCommand::Client {
            client_id: 1,
            message: Message::with_request_id("r-1", Body::GetMetrics(Empty {})),
        });

        assert!(core.sinks.contains_key(&1));
        assert!(core.metrics.transmitted_bytes > 0);
    }
}
