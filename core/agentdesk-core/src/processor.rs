//! Event Processor: assigns identity, global sequence, timestamp, and log range;
//! runs classification; pins logs for attention events; and stores events.
//! See docs/ARCHITECTURE.md "Event Processor" and docs/EVENT_MODEL.md.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use agentdesk_model::{
    AgentId, AgentInfo, Category, Event, EventId, LogRange, QueueEntry, RawAgentEvent,
    SCHEMA_VERSION,
};

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use crate::classifier::{Matched, classify};
use crate::event_store::EventStore;
use crate::log_store::LogStore;
use crate::metrics::Metrics;
use crate::queue::PriorityQueue;
use crate::scoring;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessError {
    Malformed(String),
}

pub struct EventProcessor {
    next_seq: u64,
    agents: HashMap<AgentId, AgentInfo>,
    rng: Option<StdRng>,
}

impl Default for EventProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl EventProcessor {
    pub fn new() -> Self {
        EventProcessor {
            next_seq: 0,
            agents: HashMap::new(),
            rng: None,
        }
    }

    pub fn with_seed(seed: u64) -> Self {
        EventProcessor {
            next_seq: 0,
            agents: HashMap::new(),
            rng: Some(StdRng::seed_from_u64(seed)),
        }
    }

    fn next_event_id(&mut self) -> EventId {
        if let Some(ref mut rng) = self.rng {
            let bytes: [u8; 16] = rng.random();
            uuid::Builder::from_random_bytes(bytes).into_uuid()
        } else {
            Uuid::new_v4()
        }
    }

    pub fn current_seq(&self) -> u64 {
        self.next_seq
    }

    pub fn register_agent(&mut self, info: AgentInfo) {
        self.agents.insert(info.agent_id.clone(), info);
    }

    pub fn register_agents(&mut self, agents: &[AgentInfo]) {
        for a in agents {
            self.register_agent(a.clone());
        }
    }

    /// Process a raw log line (pure noise not emitted as an event).
    pub fn process_line(
        &mut self,
        agent_id: &str,
        text: String,
        now: DateTime<Utc>,
        log_store: &mut LogStore,
        metrics: &mut Metrics,
    ) {
        log_store.append(agent_id, text, now);
        metrics.raw_lines += 1;
    }

    /// Process a RawAgentEvent into a stored, queued, and classified Event.
    pub fn process_raw_event(
        &mut self,
        raw: RawAgentEvent,
        now: DateTime<Utc>,
        event_store: &mut EventStore,
        queue: &mut PriorityQueue,
        log_store: &mut LogStore,
        metrics: &mut Metrics,
    ) -> Result<(Event, QueueEntry), ProcessError> {
        metrics.raw_events += 1;

        // Validation: reject malformed events
        if raw.agent_id.trim().is_empty() {
            metrics.dropped_events += 1;
            return Err(ProcessError::Malformed("agent_id cannot be empty".into()));
        }
        if raw.kind.trim().is_empty() {
            metrics.dropped_events += 1;
            return Err(ProcessError::Malformed("kind cannot be empty".into()));
        }
        if raw.agent_seq == 0 {
            metrics.dropped_events += 1;
            return Err(ProcessError::Malformed("agent_seq must be >= 1".into()));
        }

        // 1. Append any log lines accompanying this event to the agent's ring buffer
        let start_offset = log_store.next_offset(&raw.agent_id);
        if !raw.log_lines.is_empty() {
            log_store.append_lines(&raw.agent_id, &raw.log_lines, now);
            metrics.raw_lines += raw.log_lines.len() as u64;
        }
        let end_offset = log_store.next_offset(&raw.agent_id);

        let mut log_range = LogRange {
            start: start_offset,
            end: end_offset,
            pinned: false,
        };

        // 2. Classify kind -> category, severity, summary
        let classification = classify(&raw.kind);
        if classification.matched == Matched::Fallback {
            metrics.unclassified_events += 1;
        }

        // 3. Assign identity and strictly increasing global seq
        let event_id: EventId = self.next_event_id();
        self.next_seq += 1;
        let seq = self.next_seq;

        // 4. Pin logs for Request and Error categories
        let is_attention_category = classification.category == Category::Request
            || classification.category == Category::Error;
        if is_attention_category {
            log_store.pin(event_id, &raw.agent_id, log_range);
            log_range.pinned = true;
        } else {
            log_store.register_event(event_id, raw.agent_id.clone());
        }

        // 5. Lookup agent metadata or use sensible defaults
        let (agent_name, project) = match self.agents.get(&raw.agent_id) {
            Some(info) => (info.name.clone(), info.project.clone()),
            None => (raw.agent_id.clone(), "Default".to_string()),
        };

        // 6. Build immutable Event
        let event = Event {
            schema_version: SCHEMA_VERSION,
            event_id,
            seq,
            agent_seq: raw.agent_seq,
            agent_id: raw.agent_id,
            agent_name,
            project,
            task_id: raw.task_id,
            ts: now,
            category: classification.category,
            severity: classification.severity,
            kind: raw.kind,
            operation: raw.operation,
            summary: classification.summary.to_string(),
            message: raw.message,
            details: raw.details,
            log_range,
            request: raw.request,
        };

        // 7. Store in EventStore
        event_store.insert(event.clone());

        // 8. Create QueueEntry, score, and insert into PriorityQueue
        let initial_entry = QueueEntry::new(event_id, event.category, seq, 0);
        let initial_score = scoring::score(&initial_entry, &event, now);
        let entry = QueueEntry::new(event_id, event.category, seq, initial_score);
        queue.insert(entry.clone());

        // 9. Metrics
        metrics.processed_events += 1;
        metrics.surfaced_summaries += 1;

        Ok((event, entry))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentdesk_model::{AdapterKind, Details, Operation, RequestInfo};
    use chrono::Utc;

    fn sample_raw(agent_id: &str, kind: &str, agent_seq: u64) -> RawAgentEvent {
        RawAgentEvent {
            agent_id: agent_id.into(),
            agent_seq,
            task_id: Some("task-1".into()),
            kind: kind.into(),
            operation: Operation::Build,
            message: "message".into(),
            details: Details::new(),
            log_lines: vec!["line 1".into(), "line 2".into()],
            request: None,
        }
    }

    #[test]
    fn processor_strictly_increasing_seq_and_preserves_agent_seq() {
        let mut processor = EventProcessor::new();
        let mut store = EventStore::new();
        let mut queue = PriorityQueue::new();
        let mut logs = LogStore::new(Default::default());
        let mut metrics = Metrics::new();
        let now = Utc::now();

        let raw1 = sample_raw("agent-a", "progress", 10);
        let raw2 = sample_raw("agent-b", "progress", 100);
        let raw3 = sample_raw("agent-a", "build_completed", 11);

        let (ev1, _) = processor
            .process_raw_event(raw1, now, &mut store, &mut queue, &mut logs, &mut metrics)
            .unwrap();
        let (ev2, _) = processor
            .process_raw_event(raw2, now, &mut store, &mut queue, &mut logs, &mut metrics)
            .unwrap();
        let (ev3, _) = processor
            .process_raw_event(raw3, now, &mut store, &mut queue, &mut logs, &mut metrics)
            .unwrap();

        // Strictly increasing global seq
        assert_eq!(ev1.seq, 1);
        assert_eq!(ev2.seq, 2);
        assert_eq!(ev3.seq, 3);

        // Preserved agent seq
        assert_eq!(ev1.agent_seq, 10);
        assert_eq!(ev2.agent_seq, 100);
        assert_eq!(ev3.agent_seq, 11);
    }

    #[test]
    fn log_range_matches_ring_offsets_and_pins_only_request_and_error() {
        let mut processor = EventProcessor::new();
        let mut store = EventStore::new();
        let mut queue = PriorityQueue::new();
        let mut logs = LogStore::new(Default::default());
        let mut metrics = Metrics::new();
        let now = Utc::now();

        // 1. Working event: 2 log lines appended (offsets 0..2)
        let raw_working = sample_raw("agent-a", "progress", 1);
        let (ev_w, _) = processor
            .process_raw_event(
                raw_working,
                now,
                &mut store,
                &mut queue,
                &mut logs,
                &mut metrics,
            )
            .unwrap();

        assert_eq!(ev_w.log_range.start, 0);
        assert_eq!(ev_w.log_range.end, 2);
        assert!(!ev_w.log_range.pinned);
        assert!(!logs.is_pinned(&ev_w.event_id));

        // 2. Error event: 3 log lines appended (offsets 2..5)
        let mut raw_error = sample_raw("agent-a", "build_failed", 2);
        raw_error.log_lines = vec!["err 1".into(), "err 2".into(), "err 3".into()];
        let (ev_e, _) = processor
            .process_raw_event(
                raw_error,
                now,
                &mut store,
                &mut queue,
                &mut logs,
                &mut metrics,
            )
            .unwrap();

        assert_eq!(ev_e.log_range.start, 2);
        assert_eq!(ev_e.log_range.end, 5);
        assert!(ev_e.log_range.pinned);
        assert!(logs.is_pinned(&ev_e.event_id));

        // 3. Request event: 1 log line appended (offsets 5..6)
        let mut raw_req = sample_raw("agent-a", "approval_required", 3);
        raw_req.log_lines = vec!["prompt line".into()];
        raw_req.request = Some(RequestInfo {
            prompt: "Allow?".into(),
            options: vec!["approve".into(), "deny".into()],
        });
        let (ev_r, _) = processor
            .process_raw_event(
                raw_req,
                now,
                &mut store,
                &mut queue,
                &mut logs,
                &mut metrics,
            )
            .unwrap();

        assert_eq!(ev_r.log_range.start, 5);
        assert_eq!(ev_r.log_range.end, 6);
        assert!(ev_r.log_range.pinned);
        assert!(logs.is_pinned(&ev_r.event_id));

        // 4. Completed event: no log lines (start == end == 6)
        let mut raw_comp = sample_raw("agent-a", "task_completed", 4);
        raw_comp.log_lines = vec![];
        let (ev_c, _) = processor
            .process_raw_event(
                raw_comp,
                now,
                &mut store,
                &mut queue,
                &mut logs,
                &mut metrics,
            )
            .unwrap();

        assert_eq!(ev_c.log_range.start, 6);
        assert_eq!(ev_c.log_range.end, 6);
        assert!(!ev_c.log_range.pinned);
        assert!(!logs.is_pinned(&ev_c.event_id));
    }

    #[test]
    fn malformed_raw_event_dropped_and_counted() {
        let mut processor = EventProcessor::new();
        let mut store = EventStore::new();
        let mut queue = PriorityQueue::new();
        let mut logs = LogStore::new(Default::default());
        let mut metrics = Metrics::new();
        let now = Utc::now();

        // Empty agent_id
        let mut raw = sample_raw("", "progress", 1);
        let res =
            processor.process_raw_event(raw, now, &mut store, &mut queue, &mut logs, &mut metrics);
        assert!(matches!(res, Err(ProcessError::Malformed(_))));
        assert_eq!(metrics.dropped_events, 1);

        // Empty kind
        raw = sample_raw("agent", "", 1);
        let res =
            processor.process_raw_event(raw, now, &mut store, &mut queue, &mut logs, &mut metrics);
        assert!(matches!(res, Err(ProcessError::Malformed(_))));
        assert_eq!(metrics.dropped_events, 2);

        // Zero agent_seq
        raw = sample_raw("agent", "progress", 0);
        let res =
            processor.process_raw_event(raw, now, &mut store, &mut queue, &mut logs, &mut metrics);
        assert!(matches!(res, Err(ProcessError::Malformed(_))));
        assert_eq!(metrics.dropped_events, 3);

        // Nothing was inserted into store or queue
        assert_eq!(store.len(), 0);
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn processor_uses_registered_agent_info() {
        let mut processor = EventProcessor::new();
        processor.register_agent(AgentInfo {
            agent_id: "backend".into(),
            name: "Backend Service Agent".into(),
            project: "Core Service".into(),
            adapter_kind: AdapterKind::Simulator,
        });

        let mut store = EventStore::new();
        let mut queue = PriorityQueue::new();
        let mut logs = LogStore::new(Default::default());
        let mut metrics = Metrics::new();

        let raw = sample_raw("backend", "started", 1);
        let (ev, _) = processor
            .process_raw_event(
                raw,
                Utc::now(),
                &mut store,
                &mut queue,
                &mut logs,
                &mut metrics,
            )
            .unwrap();

        assert_eq!(ev.agent_name, "Backend Service Agent");
        assert_eq!(ev.project, "Core Service");
    }
}
