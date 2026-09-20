//! Priority Queue and event lifecycle state machine.
//! See docs/ARCHITECTURE.md, docs/SYSTEM_DESIGN.md "Event lifecycle",
//! and docs/DATA_MODEL.md.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use agentdesk_model::{
    CommandError, Decision, EntryState, EventId, QueueEntry, Resolution, ScoreUpdate, StateUpdate,
};

use crate::event_store::EventStore;
use crate::scoring;

#[derive(Debug, Default)]
pub struct PriorityQueue {
    entries: HashMap<EventId, QueueEntry>,
}

impl PriorityQueue {
    pub fn new() -> Self {
        PriorityQueue {
            entries: HashMap::new(),
        }
    }

    /// Insert or replace a queue entry.
    pub fn insert(&mut self, entry: QueueEntry) {
        self.entries.insert(entry.event_id, entry);
    }

    pub fn get(&self, event_id: &EventId) -> Option<&QueueEntry> {
        self.entries.get(event_id)
    }

    pub fn get_mut(&mut self, event_id: &EventId) -> Option<&mut QueueEntry> {
        self.entries.get_mut(event_id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns all live entries ordered by `(tier asc, score desc, seq desc)`.
    /// Dismissed and superseded entries are excluded.
    pub fn ordered_snapshot(&self) -> Vec<QueueEntry> {
        let mut live: Vec<QueueEntry> = self
            .entries
            .values()
            .filter(|e| e.is_live())
            .cloned()
            .collect();
        live.sort_by_key(|e| e.order_key());
        live
    }

    /// Re-score live entries and return `ScoreUpdate`s for entries whose score changed.
    pub fn rescore(&mut self, events: &EventStore, now: DateTime<Utc>) -> Vec<ScoreUpdate> {
        let mut updates = Vec::new();
        for entry in self.entries.values_mut() {
            if !entry.is_live() {
                continue;
            }
            if let Some(event) = events.get(&entry.event_id) {
                let new_score = scoring::score(entry, event, now);
                if new_score != entry.score {
                    entry.score = new_score;
                    updates.push(ScoreUpdate {
                        event_id: entry.event_id,
                        score: new_score,
                        escalation_level: entry.escalation_level,
                    });
                }
            }
        }
        updates.sort_by_key(|u| u.event_id);
        updates
    }

    /// Update score for a single entry after a state/resolution/escalation change.
    pub fn update_entry_score(
        &mut self,
        event_id: &EventId,
        events: &EventStore,
        now: DateTime<Utc>,
    ) -> Option<ScoreUpdate> {
        let entry = self.entries.get_mut(event_id)?;
        let event = events.get(event_id)?;
        let new_score = scoring::score(entry, event, now);
        if new_score != entry.score {
            entry.score = new_score;
            Some(ScoreUpdate {
                event_id: *event_id,
                score: new_score,
                escalation_level: entry.escalation_level,
            })
        } else {
            None
        }
    }

    /// Acknowledge an event: `new -> seen`.
    /// Idempotent: acking a seen or dismissed event returns `Ok(None)`.
    pub fn ack(&mut self, event_id: &EventId) -> Result<Option<StateUpdate>, CommandError> {
        let entry = self
            .entries
            .get_mut(event_id)
            .ok_or(CommandError::NoSuchEvent)?;

        if entry.state == EntryState::New {
            entry.state = EntryState::Seen;
            Ok(Some(StateUpdate {
                event_id: *event_id,
                state: entry.state,
                resolution: entry.resolution,
                superseded: entry.superseded,
            }))
        } else {
            Ok(None)
        }
    }

    /// Mark an event as seen (e.g. when opening details).
    pub fn mark_seen(&mut self, event_id: &EventId) -> Result<Option<StateUpdate>, CommandError> {
        self.ack(event_id)
    }

    /// Dismiss an event: `new | seen -> dismissed`.
    /// Idempotent: dismissing an already-dismissed event returns `Ok(None)`.
    /// Note: `resolution` remains independent of `state`.
    pub fn dismiss(&mut self, event_id: &EventId) -> Result<Option<StateUpdate>, CommandError> {
        let entry = self
            .entries
            .get_mut(event_id)
            .ok_or(CommandError::NoSuchEvent)?;

        if entry.state != EntryState::Dismissed {
            entry.state = EntryState::Dismissed;
            Ok(Some(StateUpdate {
                event_id: *event_id,
                state: entry.state,
                resolution: entry.resolution,
                superseded: entry.superseded,
            }))
        } else {
            Ok(None)
        }
    }

    /// Deliver human decision to a request event: `unresolved -> approved | denied`.
    /// Returns error if not a request or if already resolved.
    pub fn respond_request(
        &mut self,
        event_id: &EventId,
        decision: Decision,
    ) -> Result<StateUpdate, CommandError> {
        let entry = self
            .entries
            .get_mut(event_id)
            .ok_or(CommandError::NoSuchEvent)?;

        match entry.resolution {
            None => Err(CommandError::NotARequest),
            Some(Resolution::Approved) | Some(Resolution::Denied) => {
                Err(CommandError::AlreadyResolved)
            }
            Some(Resolution::Unresolved) => {
                let res = match decision {
                    Decision::Approve => Resolution::Approved,
                    Decision::Deny => Resolution::Denied,
                };
                entry.resolution = Some(res);
                Ok(StateUpdate {
                    event_id: *event_id,
                    state: entry.state,
                    resolution: entry.resolution,
                    superseded: entry.superseded,
                })
            }
        }
    }

    /// Escalate a working event: raises `escalation_level`.
    pub fn escalate(
        &mut self,
        event_id: &EventId,
        level: u8,
    ) -> Result<Option<ScoreUpdate>, CommandError> {
        let entry = self
            .entries
            .get_mut(event_id)
            .ok_or(CommandError::NoSuchEvent)?;

        if entry.escalation_level != level {
            entry.escalation_level = level;
            Ok(Some(ScoreUpdate {
                event_id: *event_id,
                score: entry.score,
                escalation_level: entry.escalation_level,
            }))
        } else {
            Ok(None)
        }
    }

    /// Mark an entry as superseded.
    pub fn supersede(&mut self, event_id: &EventId) -> Result<Option<StateUpdate>, CommandError> {
        let entry = self
            .entries
            .get_mut(event_id)
            .ok_or(CommandError::NoSuchEvent)?;

        if !entry.superseded {
            entry.superseded = true;
            Ok(Some(StateUpdate {
                event_id: *event_id,
                state: entry.state,
                resolution: entry.resolution,
                superseded: true,
            }))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentdesk_model::{
        Category, Details, Event, LogRange, Operation, SCHEMA_VERSION, Severity,
    };
    use chrono::Duration;
    use uuid::Uuid;

    fn make_test_event(id: EventId, category: Category, seq: u64, ts: DateTime<Utc>) -> Event {
        Event {
            schema_version: SCHEMA_VERSION,
            event_id: id,
            seq,
            agent_seq: seq,
            agent_id: "agent".into(),
            agent_name: "Agent".into(),
            project: "Project".into(),
            task_id: Some("task-1".into()),
            ts,
            category,
            severity: Severity::Important,
            kind: "kind".into(),
            operation: Operation::Build,
            summary: "summary".into(),
            message: "msg".into(),
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
    fn ordering_lower_tier_first_regardless_of_score_or_state() {
        let mut queue = PriorityQueue::new();
        // Working (tier 3) with max score 100
        let id_w = Uuid::new_v4();
        let mut entry_w = QueueEntry::new(id_w, Category::Working, 1, 100);
        entry_w.escalation_level = 2;

        // Completed (tier 2) with score 0
        let id_c = Uuid::new_v4();
        let entry_c = QueueEntry::new(id_c, Category::Completed, 2, 0);

        // Error (tier 1) with score 10
        let id_e = Uuid::new_v4();
        let entry_e = QueueEntry::new(id_e, Category::Error, 3, 10);

        // Request (tier 0) with score 5
        let id_r = Uuid::new_v4();
        let entry_r = QueueEntry::new(id_r, Category::Request, 4, 5);

        queue.insert(entry_w);
        queue.insert(entry_c);
        queue.insert(entry_e);
        queue.insert(entry_r);

        let snap = queue.ordered_snapshot();
        assert_eq!(snap.len(), 4);
        assert_eq!(snap[0].event_id, id_r, "Request tier 0 first");
        assert_eq!(snap[1].event_id, id_e, "Error tier 1 second");
        assert_eq!(snap[2].event_id, id_c, "Completed tier 2 third");
        assert_eq!(snap[3].event_id, id_w, "Working tier 3 fourth");
    }

    #[test]
    fn property_test_lower_tier_always_first() {
        let scores = [0, 1, 10, 50, 90, 100];
        let escalations = [0, 1, 2];
        let states = [EntryState::New, EntryState::Seen, EntryState::Dismissed];
        let seqs = [1, 50, 1000];

        for &c1 in &Category::ALL {
            for &c2 in &Category::ALL {
                if c1.tier() >= c2.tier() {
                    continue;
                }
                for &s1 in &scores {
                    for &s2 in &scores {
                        for &esc1 in &escalations {
                            for &esc2 in &escalations {
                                for &st1 in &states {
                                    for &st2 in &states {
                                        for &sq1 in &seqs {
                                            for &sq2 in &seqs {
                                                let mut e1 =
                                                    QueueEntry::new(Uuid::new_v4(), c1, sq1, s1);
                                                e1.escalation_level = esc1;
                                                e1.state = st1;

                                                let mut e2 =
                                                    QueueEntry::new(Uuid::new_v4(), c2, sq2, s2);
                                                e2.escalation_level = esc2;
                                                e2.state = st2;

                                                assert!(
                                                    e1.order_key() < e2.order_key(),
                                                    "tier {} must precede tier {} regardless of score/escalation/state/seq",
                                                    c1.tier(),
                                                    c2.tier()
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tie_breaking_by_score_then_newer_seq() {
        let mut queue = PriorityQueue::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        let e1 = QueueEntry::new(id1, Category::Working, 10, 50);
        let e2 = QueueEntry::new(id2, Category::Working, 20, 80); // higher score
        let e3 = QueueEntry::new(id3, Category::Working, 30, 50); // same score as e1, newer seq

        queue.insert(e1);
        queue.insert(e2);
        queue.insert(e3);

        let snap = queue.ordered_snapshot();
        assert_eq!(snap[0].event_id, id2); // score 80
        assert_eq!(snap[1].event_id, id3); // score 50, seq 30
        assert_eq!(snap[2].event_id, id1); // score 50, seq 10
    }

    #[test]
    fn state_machine_transitions() {
        let mut queue = PriorityQueue::new();
        let id = Uuid::new_v4();
        let entry = QueueEntry::new(id, Category::Request, 1, 70);
        queue.insert(entry);

        // Ack: new -> seen
        let update = queue.ack(&id).unwrap().unwrap();
        assert_eq!(update.state, EntryState::Seen);
        assert_eq!(queue.get(&id).unwrap().state, EntryState::Seen);

        // Idempotent ack: seen -> seen (Ok(None))
        assert!(queue.ack(&id).unwrap().is_none());

        // Respond request: unresolved -> approved
        let resp_update = queue.respond_request(&id, Decision::Approve).unwrap();
        assert_eq!(resp_update.resolution, Some(Resolution::Approved));
        // State remains Seen (orthogonal)
        assert_eq!(resp_update.state, EntryState::Seen);

        // Repeated respond -> already_resolved
        let err = queue.respond_request(&id, Decision::Deny).unwrap_err();
        assert_eq!(err, CommandError::AlreadyResolved);

        // Dismiss: seen -> dismissed
        let dis_update = queue.dismiss(&id).unwrap().unwrap();
        assert_eq!(dis_update.state, EntryState::Dismissed);
        // Resolution is preserved even when dismissed!
        assert_eq!(dis_update.resolution, Some(Resolution::Approved));

        // Idempotent dismiss -> Ok(None)
        assert!(queue.dismiss(&id).unwrap().is_none());
    }

    #[test]
    fn dismiss_from_new_allowed_and_resolution_independent() {
        let mut queue = PriorityQueue::new();
        let id = Uuid::new_v4();
        let entry = QueueEntry::new(id, Category::Request, 1, 70);
        queue.insert(entry);

        // Dismiss directly from New
        let dis_update = queue.dismiss(&id).unwrap().unwrap();
        assert_eq!(dis_update.state, EntryState::Dismissed);
        assert_eq!(dis_update.resolution, Some(Resolution::Unresolved));
    }

    #[test]
    fn respond_on_non_request_returns_not_a_request() {
        let mut queue = PriorityQueue::new();
        let id = Uuid::new_v4();
        let entry = QueueEntry::new(id, Category::Error, 1, 70);
        queue.insert(entry);

        let err = queue.respond_request(&id, Decision::Approve).unwrap_err();
        assert_eq!(err, CommandError::NotARequest);
    }

    #[test]
    fn command_on_missing_id_returns_no_such_event() {
        let mut queue = PriorityQueue::new();
        let missing = Uuid::new_v4();

        assert_eq!(queue.ack(&missing).unwrap_err(), CommandError::NoSuchEvent);
        assert_eq!(
            queue.dismiss(&missing).unwrap_err(),
            CommandError::NoSuchEvent
        );
        assert_eq!(
            queue
                .respond_request(&missing, Decision::Approve)
                .unwrap_err(),
            CommandError::NoSuchEvent
        );
        assert_eq!(
            queue.escalate(&missing, 1).unwrap_err(),
            CommandError::NoSuchEvent
        );
        assert_eq!(
            queue.supersede(&missing).unwrap_err(),
            CommandError::NoSuchEvent
        );
    }

    #[test]
    fn dismissed_and_superseded_excluded_from_snapshot_but_in_event_store() {
        let mut queue = PriorityQueue::new();
        let mut store = EventStore::new();

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        let now = Utc::now();
        let ev1 = make_test_event(id1, Category::Working, 1, now);
        let ev2 = make_test_event(id2, Category::Working, 2, now);
        let ev3 = make_test_event(id3, Category::Working, 3, now);

        store.insert(ev1.clone());
        store.insert(ev2.clone());
        store.insert(ev3.clone());

        queue.insert(QueueEntry::new(id1, Category::Working, 1, 10));
        queue.insert(QueueEntry::new(id2, Category::Working, 2, 10));
        queue.insert(QueueEntry::new(id3, Category::Working, 3, 10));

        // Dismiss id1, supersede id2
        queue.dismiss(&id1).unwrap();
        queue.supersede(&id2).unwrap();

        let snapshot = queue.ordered_snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].event_id, id3);

        // All 3 events remain retrievable from EventStore!
        assert_eq!(store.get(&id1), Some(&ev1));
        assert_eq!(store.get(&id2), Some(&ev2));
        assert_eq!(store.get(&id3), Some(&ev3));
    }

    #[test]
    fn rescore_emits_only_changed_entries() {
        let mut queue = PriorityQueue::new();
        let mut store = EventStore::new();
        let start: DateTime<Utc> = "2026-09-17T12:00:00Z".parse().unwrap();

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        // Error event (will decay with recency)
        let ev1 = make_test_event(id1, Category::Error, 1, start);
        // Working event (no recency decay)
        let ev2 = make_test_event(id2, Category::Working, 2, start);

        store.insert(ev1.clone());
        store.insert(ev2.clone());

        // Initial scores at t=0
        let s1 = scoring::score(&QueueEntry::new(id1, Category::Error, 1, 0), &ev1, start);
        let s2 = scoring::score(&QueueEntry::new(id2, Category::Working, 2, 0), &ev2, start);

        queue.insert(QueueEntry::new(id1, Category::Error, 1, s1));
        queue.insert(QueueEntry::new(id2, Category::Working, 2, s2));

        // Advance by 15 minutes: ev1 decays, ev2 stays same
        let t_15 = start + Duration::minutes(15);
        let updates = queue.rescore(&store, t_15);

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].event_id, id1);
        assert!(updates[0].score < s1);
    }
}
