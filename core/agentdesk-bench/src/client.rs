//! Fake client with scripted tap policy for measurement bench (P5.2).
//! See docs/SYSTEM_DESIGN.md "Pipeline modes" and docs/TODO.md P5.2.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use agentdesk_core::{ClientId, CoreCommand, CoreHandle, SinkError, TransportSink};
use agentdesk_model::{
    Body, Category, Decision, Event, EventId, EventPush, EventRef, GetEventLogs, Message,
    PipelineMode, QueueEntry, RespondRequest, TaskId, LOG_OFFSET_TAIL,
};

/// Scripted tap policy controlling fake client interactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TapPolicy {
    /// Open every Request and Error, request one tail log page per Error,
    /// approve every Request after N virtual seconds.
    #[default]
    Default,
    /// Don't perform any tap actions.
    None,
}

/// Client-side counters recorded during a benchmark run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientCounters {
    /// Number of summaries rendered in the UI list.
    pub summaries_rendered: u64,
    /// Number of user taps simulated.
    pub taps: u64,
    /// Number of log page requests issued.
    pub log_pages_requested: u64,
    /// Count of duplicate event deliveries observed.
    pub duplicates: u64,
    /// Number of score update messages received.
    pub score_updates: u64,
    /// Number of state update messages received.
    pub state_updates: u64,
}

/// A pending scripted action scheduled at a future virtual timestamp.
enum PendingAction {
    ApproveRequest {
        event_id: EventId,
        execute_at: DateTime<Utc>,
    },
}

/// Sink that forwards core messages into the fake client's inbound queue.
pub struct FakeClientSink {
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
    bytes_written: Arc<AtomicUsize>,
}

impl FakeClientSink {
    pub fn new(
        tx: tokio::sync::mpsc::UnboundedSender<Message>,
        bytes_written: Arc<AtomicUsize>,
    ) -> Self {
        FakeClientSink { tx, bytes_written }
    }
}

impl TransportSink for FakeClientSink {
    fn send(&mut self, msg: &Message) -> Result<usize, SinkError> {
        let json = serde_json::to_string(msg).map_err(|e| SinkError::Io(e.to_string()))?;
        let len = json.len();
        self.bytes_written.fetch_add(len, Ordering::SeqCst);
        let _ = self.tx.send(msg.clone());
        Ok(len)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Fake client acting as a connected mobile device.
pub struct FakeClient {
    client_id: ClientId,
    mode: PipelineMode,
    policy: TapPolicy,
    rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    bytes_written: Arc<AtomicUsize>,
    counters: ClientCounters,
    pending_actions: Vec<PendingAction>,

    // Local client state
    known_events: HashSet<EventId>,
    events_by_id: HashMap<EventId, Event>,
    entries_by_id: HashMap<EventId, QueueEntry>,
    observed_escalations: HashMap<TaskId, u8>,
}

impl FakeClient {
    pub fn new(client_id: ClientId, mode: PipelineMode, policy: TapPolicy) -> (Self, FakeClientSink) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let bytes_written = Arc::new(AtomicUsize::new(0));
        let sink = FakeClientSink::new(tx, bytes_written.clone());
        let client = FakeClient {
            client_id,
            mode,
            policy,
            rx,
            bytes_written,
            counters: ClientCounters::default(),
            pending_actions: Vec::new(),
            known_events: HashSet::new(),
            events_by_id: HashMap::new(),
            entries_by_id: HashMap::new(),
            observed_escalations: HashMap::new(),
        };
        (client, sink)
    }

    pub fn counters(&self) -> &ClientCounters {
        &self.counters
    }

    pub fn total_bytes(&self) -> usize {
        self.bytes_written.load(Ordering::SeqCst)
    }

    pub fn observed_escalations(&self) -> &HashMap<TaskId, u8> {
        &self.observed_escalations
    }

    /// Process all currently available incoming messages from the sink.
    pub fn poll_incoming(&mut self, now: DateTime<Utc>, core: &CoreHandle) {
        while let Ok(msg) = self.rx.try_recv() {
            self.handle_message(msg, now, core);
        }
    }

    /// Execute any scheduled actions whose execution time is <= now.
    pub fn execute_pending_actions(&mut self, now: DateTime<Utc>, core: &CoreHandle) {
        let mut remaining = Vec::new();
        let actions = std::mem::take(&mut self.pending_actions);

        for action in actions {
            match action {
                PendingAction::ApproveRequest {
                    event_id,
                    execute_at,
                } => {
                    if now >= execute_at {
                        // User taps to open the request, then taps approve
                        self.counters.taps += 2;
                        let _ = core.try_send(CoreCommand::Client {
                            client_id: self.client_id,
                            message: Message::with_request_id(
                                "client-resp",
                                Body::RespondRequest(RespondRequest {
                                    event_id,
                                    decision: Decision::Approve,
                                }),
                            ),
                        });
                    } else {
                        remaining.push(PendingAction::ApproveRequest {
                            event_id,
                            execute_at,
                        });
                    }
                }
            }
        }

        self.pending_actions = remaining;
    }

    fn handle_message(&mut self, msg: Message, now: DateTime<Utc>, core: &CoreHandle) {
        match msg.body {
            Body::RawLine(_) => {
                self.counters.summaries_rendered += 1;
            }
            Body::RawEvent(_) => {
                self.counters.summaries_rendered += 1;
            }
            Body::Event(EventPush { event, entry }) => {
                if !self.known_events.insert(event.event_id) {
                    self.counters.duplicates += 1;
                }

                let is_request = event.category == Category::Request;
                let is_error = event.category == Category::Error;
                let event_id = entry.event_id;

                self.events_by_id.insert(event.event_id, event);
                self.entries_by_id.insert(event_id, entry);

                if self.policy == TapPolicy::Default {
                    if is_request {
                        // Schedule approval after 5 virtual seconds
                        self.pending_actions.push(PendingAction::ApproveRequest {
                            event_id,
                            execute_at: now + Duration::seconds(5),
                        });
                    } else if is_error {
                        // Open error details
                        self.counters.taps += 1;
                        let _ = core.try_send(CoreCommand::Client {
                            client_id: self.client_id,
                            message: Message::with_request_id(
                                "client-details",
                                Body::GetEventDetails(EventRef { event_id }),
                            ),
                        });

                        // Tap to view tail logs
                        self.counters.taps += 1;
                        self.counters.log_pages_requested += 1;
                        let _ = core.try_send(CoreCommand::Client {
                            client_id: self.client_id,
                            message: Message::with_request_id(
                                "client-logs",
                                Body::GetEventLogs(GetEventLogs {
                                    event_id,
                                    offset: LOG_OFFSET_TAIL,
                                    limit: 100,
                                }),
                            ),
                        });
                    }
                }
            }
            Body::ScoreUpdate(score_upd) => {
                self.counters.score_updates += 1;
                if let Some(entry) = self.entries_by_id.get_mut(&score_upd.event_id) {
                    entry.score = score_upd.score;
                    entry.escalation_level = score_upd.escalation_level;
                }
                if score_upd.escalation_level > 0
                    && let Some(tid) = self
                        .events_by_id
                        .get(&score_upd.event_id)
                        .and_then(|ev| ev.task_id.as_ref())
                {
                    let entry = self.observed_escalations.entry(tid.clone()).or_insert(0);
                    *entry = (*entry).max(score_upd.escalation_level);
                }
            }
            Body::StateUpdate(state_upd) => {
                self.counters.state_updates += 1;
                if let Some(entry) = self.entries_by_id.get_mut(&state_upd.event_id) {
                    entry.state = state_upd.state;
                    entry.resolution = state_upd.resolution;
                    entry.superseded = state_upd.superseded;
                }
            }
            Body::Snapshot(snapshot) => {
                for entry in snapshot.entries {
                    self.entries_by_id.insert(entry.event_id, entry);
                }
                for event in snapshot.events {
                    self.known_events.insert(event.event_id);
                    self.events_by_id.insert(event.event_id, event);
                }
            }
            Body::CommandResult(_) | Body::EventDetails(_) | Body::EventLogs(_) | Body::Metrics(_) => {
                // Command replies handled without changing queue state
            }
            _ => {}
        }
    }

    /// Finish client processing and compute final surfaced events.
    pub fn finalize(&mut self) {
        if self.mode == PipelineMode::Agentdesk {
            // Surfaced events are all attention items remaining active/non-superseded in the client view
            let surfaced_count = self
                .entries_by_id
                .values()
                .filter(|e| e.is_live())
                .count() as u64;
            self.counters.summaries_rendered = surfaced_count;
        }
    }

    /// Returns the surfaced events in the client view.
    pub fn surfaced_events(&self) -> Vec<&Event> {
        match self.mode {
            PipelineMode::Agentdesk => self
                .entries_by_id
                .values()
                .filter(|e| e.is_live())
                .filter_map(|e| self.events_by_id.get(&e.event_id))
                .collect(),
            _ => Vec::new(),
        }
    }
}
