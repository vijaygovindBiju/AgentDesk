//! Core Task: the central async actor that owns the event pipeline,
//! priority queue, log store, task tracker, and metrics.
//! See docs/ARCHITECTURE.md "Core task" and docs/SYSTEM_DESIGN.md.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use agentdesk_model::{
    AgentInfo, Body, CommandError, CommandResult, Decision, ErrorReply, EventPush,
    EventRef, GetEventLogs, Message, PipelineMode, RawLine, RequestResponse, Snapshot, TaskId,
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
        /// The queue event_id this response answers — needed to report the outcome back.
        event_id: agentdesk_model::EventId,
        task_id: TaskId,
        decision: Decision,
        /// `agent_seq` of the request event being answered.
        request_seq: Option<u64>,
        response: Option<RequestResponse>,
        now: DateTime<Utc>,
    },
    StartTask {
        prompt: String,
        workspace: Option<String>,
        agent_id: Option<String>,
        mode: Option<String>,
        now: DateTime<Utc>,
    },
}

/// Outcome reported back from the adapter loop after attempting PTY delivery.
pub use agentdesk_model::AdapterOutcome;

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
    Disconnect { client_id: ClientId },
    /// Push a fresh snapshot of all live entries and events to the client.
    SendSnapshot { client_id: ClientId },
    /// Delivery outcome from the adapter loop: confirms or rolls back PendingDelivery.
    AdapterResponse(AdapterOutcome),
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
                } else {
                    self.send_to(
                        client_id,
                        Message::push(Body::Snapshot(Snapshot {
                            entries: Vec::new(),
                            events: Vec::new(),
                        })),
                    );
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
            CoreCommand::AdapterResponse(outcome) => {
                self.handle_adapter_response(outcome, now);
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
                    self.queue
                        .update_entry_score(&esc.event_id, &self.event_store, now)
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
                        self.broadcast(Message::push(Body::Event(EventPush { event, entry })));
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

    fn handle_client_message(&mut self, client_id: ClientId, message: Message, now: DateTime<Utc>) {
        let req_id = message.request_id.clone();

        match message.body {
            Body::GetMetrics(_) => {
                let snap = self.metrics.snapshot();
                self.send_to(
                    client_id,
                    Message {
                        request_id: req_id,
                        body: Body::Metrics(snap),
                    },
                );
            }
            Body::Ack(EventRef { event_id }) => {
                let res = self.queue.ack(&event_id);
                match res {
                    Ok(Some(state_update)) => {
                        self.metrics.acks += 1;
                        self.broadcast(Message::push(Body::StateUpdate(state_update)));
                        self.send_to(
                            client_id,
                            Message {
                                request_id: req_id,
                                body: Body::CommandResult(CommandResult::ok()),
                            },
                        );
                    }
                    Ok(None) => {
                        self.metrics.acks += 1;
                        self.send_to(
                            client_id,
                            Message {
                                request_id: req_id,
                                body: Body::CommandResult(CommandResult::ok()),
                            },
                        );
                    }
                    Err(err) => {
                        self.send_to(
                            client_id,
                            Message {
                                request_id: req_id,
                                body: Body::CommandResult(CommandResult::err(err)),
                            },
                        );
                    }
                }
            }
            Body::Dismiss(EventRef { event_id }) => {
                let res = self.queue.dismiss(&event_id);
                match res {
                    Ok(Some(state_update)) => {
                        self.metrics.dismissals += 1;
                        self.broadcast(Message::push(Body::StateUpdate(state_update)));
                        self.send_to(
                            client_id,
                            Message {
                                request_id: req_id,
                                body: Body::CommandResult(CommandResult::ok()),
                            },
                        );
                    }
                    Ok(None) => {
                        self.metrics.dismissals += 1;
                        self.send_to(
                            client_id,
                            Message {
                                request_id: req_id,
                                body: Body::CommandResult(CommandResult::ok()),
                            },
                        );
                    }
                    Err(err) => {
                        self.send_to(
                            client_id,
                            Message {
                                request_id: req_id,
                                body: Body::CommandResult(CommandResult::err(err)),
                            },
                        );
                    }
                }
            }
            Body::RespondRequest(req) => {
                let req_response = RequestResponse::from_respond_request(&req);
                let (task_id, request_seq) = self
                    .event_store
                    .get(&req.event_id)
                    .map(|e| (e.task_id.clone(), Some(e.agent_seq)))
                    .unwrap_or((None, None));

                // Transition Unresolved → PendingDelivery.
                // AlreadyResolved, PendingDelivery, NotARequest all returned to client.
                let res = self.queue.respond_request(&req.event_id);

                match res {
                    Ok(state_update) => {
                        self.metrics.responses += 1;
                        if let Some(sc) =
                            self.queue
                                .update_entry_score(&req.event_id, &self.event_store, now)
                        {
                            self.broadcast(Message::push(Body::ScoreUpdate(sc)));
                        }
                        // Broadcast PendingDelivery state only when adapter responder is present (awaiting PTY confirmation).
                        if self.adapter_responder.is_some() {
                            self.broadcast(Message::push(Body::StateUpdate(state_update)));
                        }
                        // Forward the response command to other connected clients (e.g. attached TUI sessions)
                        let respond_msg = Message::push(Body::RespondRequest(req.clone()));
                        for (&cid, sink) in self.sinks.iter_mut() {
                            if cid != client_id {
                                let _ = sink.send(&respond_msg);
                            }
                        }

                        // Record human response into pinned request window so it appears in request logs
                        let response_desc = match &req_response {
                            RequestResponse::SelectOption { option } => format!("> User selected: {option}"),
                            RequestResponse::SelectMultiple { options } => format!("> User selected: {}", options.join(", ")),
                            RequestResponse::TextInput { text } => format!("> User input: {text}"),
                            RequestResponse::Approve => "> User approved".to_string(),
                            RequestResponse::Deny => "> User denied".to_string(),
                        };
                        self.log_store.append_to_pinned(&req.event_id, response_desc, now);

                        // Deliver to adapter. Use the blocking send path so a full channel
                        // returns an explicit error rather than silently dropping the command.
                        let event_id = req.event_id;
                        let decision = req.decision;
                        let delivery_result = if let Some(tx) = &self.adapter_responder {
                            let tid = task_id.unwrap_or_else(|| "antigravity-session".to_string());
                            match tx.try_send(AdapterCommand::Respond {
                                event_id,
                                task_id: tid,
                                decision,
                                request_seq,
                                response: Some(req_response),
                                now,
                            }) {
                                Ok(()) => Ok(()),
                                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                    Err("adapter command channel full")
                                }
                                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                    Err("adapter command channel closed")
                                }
                            }
                        } else {
                            // No adapter responder (e.g. simulator mode): treat as immediate success.
                            if let Ok(update) = self.queue.mark_delivered(&event_id, decision) {
                                if let Some(sc) = self.queue.update_entry_score(
                                    &event_id,
                                    &self.event_store,
                                    now,
                                ) {
                                    self.broadcast(Message::push(Body::ScoreUpdate(sc)));
                                }
                                self.broadcast(Message::push(Body::StateUpdate(update)));
                            }
                            Ok(())
                        };

                        match delivery_result {
                            Ok(()) => {
                                // AdapterResponse will complete the transition; tell the phone
                                // the command was accepted (PendingDelivery).
                                self.send_to(
                                    client_id,
                                    Message {
                                        request_id: req_id,
                                        body: Body::CommandResult(CommandResult::ok()),
                                    },
                                );
                            }
                            Err(_reason) => {
                                // Channel full / closed: roll back to Unresolved and tell the phone.
                                if let Ok(reopen) = self.queue.mark_delivery_failed(&event_id) {
                                    if let Some(sc) = self.queue.update_entry_score(
                                        &event_id,
                                        &self.event_store,
                                        now,
                                    ) {
                                        self.broadcast(Message::push(Body::ScoreUpdate(sc)));
                                    }
                                    self.broadcast(Message::push(Body::StateUpdate(reopen)));
                                }
                                self.send_to(
                                    client_id,
                                    Message {
                                        request_id: req_id,
                                        body: Body::CommandResult(CommandResult::err(
                                            CommandError::DeliveryFailed,
                                        )),
                                    },
                                );
                            }
                        }
                    }
                    Err(err) => {
                        self.send_to(
                            client_id,
                            Message {
                                request_id: req_id,
                                body: Body::CommandResult(CommandResult::err(err)),
                            },
                        );
                    }
                }
            }
            Body::GetEventDetails(EventRef { event_id }) => {
                if let Ok(Some(state_update)) = self.queue.mark_seen(&event_id) {
                    self.metrics.detail_refetches += 1;
                    self.broadcast(Message::push(Body::StateUpdate(state_update)));
                    if let Some(sc) =
                        self.queue
                            .update_entry_score(&event_id, &self.event_store, now)
                    {
                        self.broadcast(Message::push(Body::ScoreUpdate(sc)));
                    }
                } else if self.queue.get(&event_id).is_some() {
                    self.metrics.detail_refetches += 1;
                }

                if let (Some(ev), Some(en)) =
                    (self.event_store.get(&event_id), self.queue.get(&event_id))
                {
                    self.send_to(
                        client_id,
                        Message {
                            request_id: req_id,
                            body: Body::EventDetails(EventPush {
                                event: ev.clone(),
                                entry: en.clone(),
                            }),
                        },
                    );
                } else {
                    self.send_to(
                        client_id,
                        Message {
                            request_id: req_id,
                            body: Body::Error(ErrorReply {
                                code: "no_such_event".into(),
                                message: "event not found".into(),
                            }),
                        },
                    );
                }
            }
            Body::GetEventLogs(GetEventLogs {
                event_id,
                offset,
                limit,
            }) => {
                self.metrics.log_page_requests += 1;
                if let Some(logs) = self.log_store.get_event_logs(&event_id, offset, limit) {
                    self.send_to(
                        client_id,
                        Message {
                            request_id: req_id,
                            body: Body::EventLogs(logs),
                        },
                    );
                } else {
                    self.send_to(
                        client_id,
                        Message {
                            request_id: req_id,
                            body: Body::CommandResult(CommandResult::err(
                                CommandError::NoSuchEvent,
                            )),
                        },
                    );
                }
            }
            Body::StartTask(ref task) => {
                let agent_id = task.agent_id.as_deref().unwrap_or("antigravity-session");
                self.log_store.append(agent_id, format!("> Prompt: {}", task.prompt), now);
                for line in task.prompt.lines() {
                    self.log_store.append(agent_id, format!("User: {}", line), now);
                }
                if let Some(tx) = &self.adapter_responder {
                    let _ = tx.try_send(AdapterCommand::StartTask {
                        prompt: task.prompt.clone(),
                        workspace: task.workspace.clone(),
                        agent_id: task.agent_id.clone(),
                        mode: task.mode.clone(),
                        now,
                    });
                }
                self.broadcast(Message::push(Body::StartTask(task.clone())));
                self.send_to(
                    client_id,
                    Message {
                        request_id: req_id,
                        body: Body::CommandResult(CommandResult::ok()),
                    },
                );
            }
            Body::RawEvent(_) | Body::AdapterOutcome(_) => {
                // These are internal bridge messages. Reject them from WebSocket
                // clients to prevent phone clients from spoofing adapter events
                // or falsely marking requests as delivered.
                self.send_to(
                    client_id,
                    Message {
                        request_id: req_id,
                        body: Body::Error(ErrorReply {
                            code: "unsupported".into(),
                            message: "adapter bridge messages are not accepted from clients".into(),
                        }),
                    },
                );
            }
            _ => {
                self.send_to(
                    client_id,
                    Message {
                        request_id: req_id,
                        body: Body::Error(ErrorReply {
                            code: "unsupported".into(),
                            message: "message type not supported as a request".into(),
                        }),
                    },
                );
            }
        }
    }

    /// Handle delivery outcome from the adapter loop.
    /// Transitions PendingDelivery → Approved/Denied (on success) or back to Unresolved (on failure).
    fn handle_adapter_response(&mut self, outcome: AdapterOutcome, now: DateTime<Utc>) {
        match outcome {
            AdapterOutcome::Delivered { event_id, decision } => {
                if let Ok(update) = self.queue.mark_delivered(&event_id, decision) {
                    if let Some(sc) =
                        self.queue
                            .update_entry_score(&event_id, &self.event_store, now)
                    {
                        self.broadcast(Message::push(Body::ScoreUpdate(sc)));
                    }
                    self.broadcast(Message::push(Body::StateUpdate(update)));
                }
            }
            AdapterOutcome::DeliveryFailed { event_id, reason } => {
                eprintln!(
                    "[WARN] Adapter delivery failed for event {}: {}; reopening request",
                    event_id, reason
                );
                if let Ok(update) = self.queue.mark_delivery_failed(&event_id) {
                    if let Some(sc) =
                        self.queue
                            .update_entry_score(&event_id, &self.event_store, now)
                    {
                        self.broadcast(Message::push(Body::ScoreUpdate(sc)));
                    }
                    self.broadcast(Message::push(Body::StateUpdate(update)));
                }
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

    pub async fn send(
        &self,
        cmd: CoreCommand,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<CoreCommand>> {
        self.sender.send(cmd).await
    }

    #[allow(clippy::result_large_err)]
    pub fn try_send(
        &self,
        cmd: CoreCommand,
    ) -> Result<(), tokio::sync::mpsc::error::TrySendError<CoreCommand>> {
        self.sender.try_send(cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::VirtualClock;
    use crate::sink::VecSink;
    use agentdesk_model::RespondRequest;
    use agentdesk_model::{Details, Empty, Operation, RawAgentEvent, RequestInfo};
    use chrono::Duration;
    use uuid::Uuid as TestUuid;

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
        let vsink = sink
            .send(&Message::push(Body::GetMetrics(Empty {})))
            .unwrap();
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
        let raw = make_raw(
            "progress",
            None,
            vec!["event line 1".into(), "event line 2".into()],
        );
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
            question_type: None,
        });
        core.step(CoreCommand::Adapter(AdapterOutput::Event(raw)));

        let event_id = core.queue.ordered_snapshot()[0].event_id;

        // Client approves request
        let respond_msg = Message::with_request_id(
            "r-42",
            Body::RespondRequest(RespondRequest {
                event_id,
                decision: Decision::Approve,
                selected_options: None,
                text_input: None,
            }),
        );
        core.step(CoreCommand::Client {
            client_id: 1,
            message: respond_msg,
        });

        // After respond_request, state is PendingDelivery (not yet Approved)
        let entry = core.queue.get(&event_id).unwrap();
        assert_eq!(
            entry.resolution,
            Some(agentdesk_model::Resolution::PendingDelivery),
            "Must be PendingDelivery until adapter confirms"
        );

        // Adapter must have received the command
        let cmd = rx_adapter.try_recv().expect("adapter must receive command");
        let (cmd_event_id, cmd_decision) = match cmd {
            AdapterCommand::Respond {
                event_id,
                task_id,
                decision,
                now,
                ..
            } => {
                assert_eq!(task_id, "task-auth");
                assert_eq!(decision, Decision::Approve);
                assert_eq!(now, clock.now());
                (event_id, decision)
            }
            _ => panic!("expected AdapterCommand::Respond"),
        };
        assert_eq!(cmd_event_id, event_id);

        // Simulate adapter loop confirming delivery
        core.step(CoreCommand::AdapterResponse(AdapterOutcome::Delivered {
            event_id: cmd_event_id,
            decision: cmd_decision,
        }));

        // Now the entry should be Approved
        let entry = core.queue.get(&event_id).unwrap();
        assert_eq!(
            entry.resolution,
            Some(agentdesk_model::Resolution::Approved),
            "Must be Approved after adapter confirms delivery"
        );
    }

    #[tokio::test]
    async fn delivery_failure_reopens_request() {
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

        let mut raw = make_raw("approval_required", Some("task-fail"), vec![]);
        raw.request = Some(RequestInfo {
            prompt: "Allow?".into(),
            options: vec!["approve".into(), "deny".into()],
            question_type: None,
        });
        core.step(CoreCommand::Adapter(AdapterOutput::Event(raw)));
        let event_id = core.queue.ordered_snapshot()[0].event_id;

        // Phone responds
        core.step(CoreCommand::Client {
            client_id: 1,
            message: Message::with_request_id(
                "r-1",
                Body::RespondRequest(RespondRequest {
                    event_id,
                    decision: Decision::Approve,
                    selected_options: None,
                    text_input: None,
                }),
            ),
        });

        // PendingDelivery before adapter confirms
        assert_eq!(
            core.queue.get(&event_id).unwrap().resolution,
            Some(agentdesk_model::Resolution::PendingDelivery)
        );

        let _ = rx_adapter.try_recv().unwrap(); // drain

        // Adapter reports PTY write failed
        core.step(CoreCommand::AdapterResponse(AdapterOutcome::DeliveryFailed {
            event_id,
            reason: "PTY transport closed".into(),
        }));

        // Request is reopened: back to Unresolved
        assert_eq!(
            core.queue.get(&event_id).unwrap().resolution,
            Some(agentdesk_model::Resolution::Unresolved),
            "Request must be reopened (Unresolved) after delivery failure"
        );
    }

    #[tokio::test]
    async fn adapter_channel_full_returns_delivery_failed_and_reopens() {
        let clock = Arc::new(VirtualClock::at_epoch());
        // Channel of capacity 1 with 1 item already queued guarantees full immediately
        let (tx_adapter, _rx_adapter) = tokio::sync::mpsc::channel(1);
        let _ = tx_adapter.try_send(AdapterCommand::Respond {
            event_id: TestUuid::new_v4(),
            task_id: "blocking-item".into(),
            decision: Decision::Approve,
            request_seq: None,
            response: None,
            now: clock.now(),
        });

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

        let mut raw = make_raw("approval_required", Some("task-full"), vec![]);
        raw.request = Some(RequestInfo {
            prompt: "Allow?".into(),
            options: vec!["approve".into(), "deny".into()],
            question_type: None,
        });
        core.step(CoreCommand::Adapter(AdapterOutput::Event(raw)));
        let event_id = core.queue.ordered_snapshot()[0].event_id;

        // Phone responds — channel is full immediately
        core.step(CoreCommand::Client {
            client_id: 1,
            message: Message::with_request_id(
                "r-full",
                Body::RespondRequest(RespondRequest {
                    event_id,
                    decision: Decision::Approve,
                    selected_options: None,
                    text_input: None,
                }),
            ),
        });

        // Request must NOT be falsely approved — must be Unresolved (rolled back)
        assert_eq!(
            core.queue.get(&event_id).unwrap().resolution,
            Some(agentdesk_model::Resolution::Unresolved),
            "Channel-full must roll back to Unresolved, not falsely Approved"
        );
    }

    #[tokio::test]
    async fn duplicate_response_while_pending_is_rejected() {
        let clock = Arc::new(VirtualClock::at_epoch());
        let (tx_adapter, _rx_adapter) = tokio::sync::mpsc::channel(16);

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

        let mut raw = make_raw("approval_required", Some("task-dup"), vec![]);
        raw.request = Some(RequestInfo {
            prompt: "Allow?".into(),
            options: vec!["approve".into(), "deny".into()],
            question_type: None,
        });
        core.step(CoreCommand::Adapter(AdapterOutput::Event(raw)));
        let event_id = core.queue.ordered_snapshot()[0].event_id;

        let respond = || Message::with_request_id(
            "r-dup",
            Body::RespondRequest(RespondRequest {
                event_id,
                decision: Decision::Approve,
                selected_options: None,
                text_input: None,
            }),
        );

        // First response: accepted → PendingDelivery
        core.step(CoreCommand::Client { client_id: 1, message: respond() });
        assert_eq!(
            core.queue.get(&event_id).unwrap().resolution,
            Some(agentdesk_model::Resolution::PendingDelivery)
        );

        // Duplicate while still pending: must be rejected
        core.step(CoreCommand::Client { client_id: 1, message: respond() });
        // Still PendingDelivery, not double-delivered
        assert_eq!(
            core.queue.get(&event_id).unwrap().resolution,
            Some(agentdesk_model::Resolution::PendingDelivery),
            "Duplicate response while pending must be rejected"
        );
    }

    #[tokio::test]
    async fn stale_response_on_resolved_request_rejected() {
        let clock = Arc::new(VirtualClock::at_epoch());
        let (tx_adapter, _rx_adapter) = tokio::sync::mpsc::channel(16);

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

        let mut raw = make_raw("approval_required", Some("task-stale"), vec![]);
        raw.request = Some(RequestInfo {
            prompt: "Allow?".into(),
            options: vec!["approve".into(), "deny".into()],
            question_type: None,
        });
        core.step(CoreCommand::Adapter(AdapterOutput::Event(raw)));
        let event_id = core.queue.ordered_snapshot()[0].event_id;

        let respond = || Message::with_request_id(
            "r-stale",
            Body::RespondRequest(RespondRequest {
                event_id,
                decision: Decision::Approve,
                selected_options: None,
                text_input: None,
            }),
        );

        // First response accepted
        core.step(CoreCommand::Client { client_id: 1, message: respond() });
        // Simulate delivery
        core.step(CoreCommand::AdapterResponse(AdapterOutcome::Delivered {
            event_id,
            decision: Decision::Approve,
        }));
        assert_eq!(
            core.queue.get(&event_id).unwrap().resolution,
            Some(agentdesk_model::Resolution::Approved)
        );

        // Stale second response: must return AlreadyResolved, not change state
        core.step(CoreCommand::Client { client_id: 1, message: respond() });
        assert_eq!(
            core.queue.get(&event_id).unwrap().resolution,
            Some(agentdesk_model::Resolution::Approved),
            "Stale response must not change already-resolved state"
        );
    }

    #[test]
    fn unknown_event_id_returns_no_such_event() {
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

        let unknown_id = TestUuid::new_v4();
        core.step(CoreCommand::Client {
            client_id: 1,
            message: Message::with_request_id(
                "r-unknown",
                Body::RespondRequest(RespondRequest {
                    event_id: unknown_id,
                    decision: Decision::Approve,
                    selected_options: None,
                    text_input: None,
                }),
            ),
        });

        // No queue changes, no panic
        assert_eq!(core.queue.len(), 0);
    }

    #[tokio::test]
    async fn no_adapter_responder_resolves_immediately() {
        // In simulator mode (no adapter responder), respond_request resolves to Approved immediately.
        let clock = Arc::new(VirtualClock::at_epoch());
        let mut core = CoreTask::new(
            PipelineMode::Agentdesk,
            clock.clone(),
            LogStoreConfig::default(),
            ThresholdTable::default(),
            None, // no adapter responder
        );
        core.step(CoreCommand::Connect {
            client_id: 1,
            sink: Box::new(VecSink::new()),
        });

        let mut raw = make_raw("approval_required", Some("task-sim"), vec![]);
        raw.request = Some(RequestInfo {
            prompt: "Allow?".into(),
            options: vec!["approve".into(), "deny".into()],
            question_type: None,
        });
        core.step(CoreCommand::Adapter(AdapterOutput::Event(raw)));
        let event_id = core.queue.ordered_snapshot()[0].event_id;

        core.step(CoreCommand::Client {
            client_id: 1,
            message: Message::with_request_id(
                "r-sim",
                Body::RespondRequest(RespondRequest {
                    event_id,
                    decision: Decision::Deny,
                    selected_options: None,
                    text_input: None,
                }),
            ),
        });

        // With no adapter responder, should resolve immediately to Denied
        assert_eq!(
            core.queue.get(&event_id).unwrap().resolution,
            Some(agentdesk_model::Resolution::Denied),
            "With no adapter responder, must resolve immediately"
        );
    }

    #[tokio::test]
    async fn adapter_outcome_for_unknown_event_is_ignored() {
        // If the agent exits between PendingDelivery and AdapterResponse,
        // the event_id may no longer be in the queue (e.g. task superseded).
        // The AdapterResponse must not panic.
        let clock = Arc::new(VirtualClock::at_epoch());
        let mut core = CoreTask::new(
            PipelineMode::Agentdesk,
            clock.clone(),
            LogStoreConfig::default(),
            ThresholdTable::default(),
            None,
        );

        let unknown_id = TestUuid::new_v4();
        // Must not panic
        core.step(CoreCommand::AdapterResponse(AdapterOutcome::Delivered {
            event_id: unknown_id,
            decision: Decision::Approve,
        }));
        core.step(CoreCommand::AdapterResponse(AdapterOutcome::DeliveryFailed {
            event_id: unknown_id,
            reason: "process gone".into(),
        }));
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
