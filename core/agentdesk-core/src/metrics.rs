//! Flat metrics counters for measurement and diagnostics.
//! See docs/ARCHITECTURE.md "Metrics" and docs/COMMUNICATION.md.

use agentdesk_model::MetricsSnapshot;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metrics {
    pub raw_lines: u64,
    pub raw_events: u64,
    pub processed_events: u64,
    pub dropped_events: u64,
    pub unclassified_events: u64,
    pub transmitted_events: u64,
    pub transmitted_bytes: u64,
    pub surfaced_summaries: u64,
    pub escalations: u64,
    pub log_page_requests: u64,
    pub detail_refetches: u64,
    pub acks: u64,
    pub dismissals: u64,
    pub responses: u64,
    pub slow_client_disconnects: u64,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        let mut map = MetricsSnapshot::new();
        map.insert("raw_lines".into(), self.raw_lines);
        map.insert("raw_events".into(), self.raw_events);
        map.insert("processed_events".into(), self.processed_events);
        map.insert("dropped_events".into(), self.dropped_events);
        map.insert("unclassified_events".into(), self.unclassified_events);
        map.insert("transmitted_events".into(), self.transmitted_events);
        map.insert("transmitted_bytes".into(), self.transmitted_bytes);
        map.insert("surfaced_summaries".into(), self.surfaced_summaries);
        map.insert("escalations".into(), self.escalations);
        map.insert("log_page_requests".into(), self.log_page_requests);
        map.insert("detail_refetches".into(), self.detail_refetches);
        map.insert("acks".into(), self.acks);
        map.insert("dismissals".into(), self.dismissals);
        map.insert("responses".into(), self.responses);
        map.insert(
            "slow_client_disconnects".into(),
            self.slow_client_disconnects,
        );
        map
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot()).expect("MetricsSnapshot is always valid JSON")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_snapshot_contains_all_counters() {
        let mut m = Metrics::new();
        m.raw_lines = 10;
        m.raw_events = 5;
        m.processed_events = 4;
        m.dropped_events = 1;

        let snap = m.snapshot();
        assert_eq!(snap.get("raw_lines"), Some(&10));
        assert_eq!(snap.get("raw_events"), Some(&5));
        assert_eq!(snap.get("processed_events"), Some(&4));
        assert_eq!(snap.get("dropped_events"), Some(&1));
        assert_eq!(snap.get("transmitted_events"), Some(&0));

        let val = m.to_json();
        assert_eq!(val["raw_lines"], 10);
    }
}
