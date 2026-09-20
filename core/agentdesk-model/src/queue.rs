//! Mutable per-event queue metadata. Events themselves are immutable; this
//! is where state, score and escalation live. See docs/DATA_MODEL.md and
//! docs/SYSTEM_DESIGN.md "Event lifecycle".

use serde::{Deserialize, Serialize};

use crate::event::{Category, EventId};

/// User-attention state. Independent of `Resolution`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    New,
    Seen,
    Dismissed,
}

/// Task-outcome state for `Category::Request` entries only. A dismissed
/// request that is still `Unresolved` is still blocking the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Unresolved,
    Approved,
    Denied,
}

/// Maximum escalation level; the watchdog never raises beyond this.
pub const MAX_ESCALATION_LEVEL: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueEntry {
    pub event_id: EventId,
    /// Derived from the event's category; fixed for the entry's lifetime.
    pub tier: u8,
    /// Recomputed on tick; orders within a tier. 0..=100.
    pub score: u16,
    pub state: EntryState,
    /// `Some` only for request entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<Resolution>,
    /// Working entries of a task that has since closed.
    #[serde(default)]
    pub superseded: bool,
    /// Working entries only; 0..=MAX_ESCALATION_LEVEL.
    #[serde(default)]
    pub escalation_level: u8,
    /// Copied from the event for tie-breaking.
    pub seq: u64,
}

impl QueueEntry {
    /// A fresh entry for a newly processed event.
    pub fn new(event_id: EventId, category: Category, seq: u64, score: u16) -> Self {
        QueueEntry {
            event_id,
            tier: category.tier(),
            score,
            state: EntryState::New,
            resolution: (category == Category::Request).then_some(Resolution::Unresolved),
            superseded: false,
            escalation_level: 0,
            seq,
        }
    }

    /// Included in snapshots and re-scoring.
    pub fn is_live(&self) -> bool {
        self.state != EntryState::Dismissed && !self.superseded
    }

    /// Sort key: lower tier first, higher score first, newer first.
    pub fn order_key(&self) -> (u8, std::cmp::Reverse<u16>, std::cmp::Reverse<u64>) {
        (
            self.tier,
            std::cmp::Reverse(self.score),
            std::cmp::Reverse(self.seq),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn new_request_entry_is_unresolved_and_others_have_no_resolution() {
        let r = QueueEntry::new(Uuid::nil(), Category::Request, 1, 70);
        assert_eq!(r.resolution, Some(Resolution::Unresolved));
        assert_eq!(r.tier, 0);
        let w = QueueEntry::new(Uuid::nil(), Category::Working, 2, 10);
        assert_eq!(w.resolution, None);
        assert_eq!(w.tier, 3);
        assert_eq!(w.state, EntryState::New);
    }

    #[test]
    fn round_trip_and_optional_fields() {
        let e = QueueEntry::new(Uuid::nil(), Category::Error, 5, 80);
        let s = serde_json::to_string(&e).unwrap();
        assert!(!s.contains("resolution"));
        let back: QueueEntry = serde_json::from_str(&s).unwrap();
        assert_eq!(e, back);

        let minimal = json!({
            "event_id": Uuid::nil(), "tier": 3, "score": 10, "state": "new", "seq": 1
        });
        let back: QueueEntry = serde_json::from_value(minimal).unwrap();
        assert!(!back.superseded && back.escalation_level == 0 && back.resolution.is_none());
    }

    #[test]
    fn wire_names() {
        assert_eq!(
            serde_json::to_value(EntryState::Dismissed).unwrap(),
            json!("dismissed")
        );
        assert_eq!(
            serde_json::to_value(Resolution::Approved).unwrap(),
            json!("approved")
        );
    }

    #[test]
    fn liveness_and_ordering() {
        let mut a = QueueEntry::new(Uuid::nil(), Category::Working, 1, 90);
        let b = QueueEntry::new(Uuid::nil(), Category::Completed, 2, 5);
        let c = QueueEntry::new(Uuid::nil(), Category::Working, 3, 90);
        // Lower tier wins regardless of score.
        assert!(b.order_key() < a.order_key());
        // Same tier and score: newer seq first.
        assert!(c.order_key() < a.order_key());

        assert!(a.is_live());
        a.state = EntryState::Dismissed;
        assert!(!a.is_live());
        let mut d = QueueEntry::new(Uuid::nil(), Category::Working, 4, 1);
        d.superseded = true;
        assert!(!d.is_live());
    }
}
