//! Event Store: in-memory store for immutable processed events.
//! Events are immutable once inserted. Mutable per-event metadata (state,
//! score, escalation) lives in the priority queue, never here.
//! See docs/ARCHITECTURE.md and docs/DATA_MODEL.md.

use std::collections::HashMap;

use agentdesk_model::{Event, EventId};

#[derive(Debug, Default)]
pub struct EventStore {
    events: HashMap<EventId, Event>,
}

impl EventStore {
    pub fn new() -> Self {
        EventStore {
            events: HashMap::new(),
        }
    }

    /// Insert an event into the store.
    pub fn insert(&mut self, event: Event) {
        self.events.insert(event.event_id, event);
    }

    /// Retrieve an event by id.
    pub fn get(&self, event_id: &EventId) -> Option<&Event> {
        self.events.get(event_id)
    }

    /// Returns true if an event with this id is stored.
    pub fn contains(&self, event_id: &EventId) -> bool {
        self.events.contains_key(event_id)
    }

    /// Number of events in the store.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        self.events.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentdesk_model::{Category, Details, LogRange, Operation, SCHEMA_VERSION, Severity};
    use chrono::Utc;
    use uuid::Uuid;

    fn sample_event(id: EventId) -> Event {
        Event {
            schema_version: SCHEMA_VERSION,
            event_id: id,
            seq: 1,
            agent_seq: 1,
            agent_id: "agent".into(),
            agent_name: "Agent".into(),
            project: "Project".into(),
            task_id: None,
            ts: Utc::now(),
            category: Category::Working,
            severity: Severity::Routine,
            kind: "progress".into(),
            operation: Operation::Build,
            summary: "Working".into(),
            message: "Building".into(),
            details: Details::new(),
            log_range: LogRange {
                start: 0,
                end: 0,
                pinned: false,
            },
            request: None,
        }
    }

    #[test]
    fn insert_and_get() {
        let mut store = EventStore::new();
        assert!(store.is_empty());

        let id = Uuid::new_v4();
        let ev = sample_event(id);
        store.insert(ev.clone());

        assert_eq!(store.len(), 1);
        assert!(store.contains(&id));
        assert_eq!(store.get(&id), Some(&ev));

        let other = Uuid::new_v4();
        assert_eq!(store.get(&other), None);
    }
}
