//! Priority queue scoring for events. See docs/EVENT_MODEL.md "Scoring".
//!
//! Score orders entries within a tier only:
//! score = base(severity)
//!       + recency_bonus(age)      (request, error, completed only)
//!       + escalation_bonus(level) (working only)
//!       - seen_penalty
//!       - resolved_penalty
//! clamped to 0..=100.

use chrono::{DateTime, Utc};

use agentdesk_model::{Category, EntryState, Event, QueueEntry, Resolution, Severity};

pub const RECENCY_MAX_BONUS: u16 = 20;
pub const RECENCY_WINDOW_SECS: i64 = 30 * 60; // 30 minutes

pub const ESCALATION_LEVEL_1_BONUS: u16 = 30;
pub const ESCALATION_LEVEL_2_BONUS: u16 = 60;

pub const SEEN_PENALTY: u16 = 15;
pub const RESOLVED_PENALTY: u16 = 40;

/// Base score by severity: 0:10, 1:25, 2:45, 3:70.
pub fn base_score(severity: Severity) -> u16 {
    match severity {
        Severity::Routine => 10,
        Severity::Notable => 25,
        Severity::Important => 45,
        Severity::Critical => 70,
    }
}

/// Recency bonus: +20 at 0 min, decaying linearly to 0 at 30 min.
/// Applies only to `Request`, `Error`, and `Completed` (never `Working`).
pub fn recency_bonus(category: Category, age_seconds: i64) -> u16 {
    if category == Category::Working {
        return 0;
    }
    if age_seconds <= 0 {
        return RECENCY_MAX_BONUS;
    }
    if age_seconds >= RECENCY_WINDOW_SECS {
        return 0;
    }

    let remaining = RECENCY_WINDOW_SECS - age_seconds;
    ((RECENCY_MAX_BONUS as f64 * remaining as f64) / RECENCY_WINDOW_SECS as f64).round() as u16
}

/// Escalation bonus: 0:0, 1:+30, 2:+60.
/// Applies only to `Category::Working`.
pub fn escalation_bonus(category: Category, level: u8) -> u16 {
    if category != Category::Working {
        return 0;
    }
    match level {
        0 => 0,
        1 => ESCALATION_LEVEL_1_BONUS,
        _ => ESCALATION_LEVEL_2_BONUS,
    }
}

/// User-attention penalty: -15 if `state == Seen`.
pub fn seen_penalty(state: EntryState) -> u16 {
    match state {
        EntryState::Seen => SEEN_PENALTY,
        _ => 0,
    }
}

/// Task-outcome penalty: -40 if `resolution in {Approved, Denied}`.
pub fn resolved_penalty(resolution: Option<Resolution>) -> u16 {
    match resolution {
        Some(Resolution::Approved) | Some(Resolution::Denied) => RESOLVED_PENALTY,
        _ => 0,
    }
}

/// Calculate the score for an entry at the given `now` timestamp.
/// Clamped to 0..=100.
pub fn score(entry: &QueueEntry, event: &Event, now: DateTime<Utc>) -> u16 {
    let age_seconds = (now - event.ts).num_seconds().max(0);

    let base = base_score(event.severity);
    let recency = recency_bonus(event.category, age_seconds);
    let escalation = escalation_bonus(event.category, entry.escalation_level);
    let seen = seen_penalty(entry.state);
    let resolved = resolved_penalty(entry.resolution);

    let total = (base as i32) + (recency as i32) + (escalation as i32) - (seen as i32) - (resolved as i32);
    total.clamp(0, 100) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentdesk_model::{Details, LogRange, Operation, SCHEMA_VERSION};
    use chrono::Duration;
    use uuid::Uuid;

    fn make_test_event(category: Category, severity: Severity, ts: DateTime<Utc>) -> Event {
        Event {
            schema_version: SCHEMA_VERSION,
            event_id: Uuid::new_v4(),
            seq: 1,
            agent_seq: 1,
            agent_id: "a".into(),
            agent_name: "Agent".into(),
            project: "Project".into(),
            task_id: Some("t1".into()),
            ts,
            category,
            severity,
            kind: "test_kind".into(),
            operation: Operation::Build,
            summary: "summary".into(),
            message: "message".into(),
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
    fn recency_decays_to_zero_at_30_min_and_not_below() {
        let start: DateTime<Utc> = "2026-09-17T12:00:00Z".parse().unwrap();
        let event = make_test_event(Category::Error, Severity::Critical, start);
        let entry = QueueEntry::new(event.event_id, event.category, event.seq, 70);

        // At 0 min: base 70 + recency 20 = 90
        assert_eq!(score(&entry, &event, start), 90);

        // At 15 min: base 70 + recency 10 = 80
        assert_eq!(score(&entry, &event, start + Duration::minutes(15)), 80);

        // At 30 min: base 70 + recency 0 = 70
        assert_eq!(score(&entry, &event, start + Duration::minutes(30)), 70);

        // At 60 min: still base 70 (does not go below 0 recency bonus)
        assert_eq!(score(&entry, &event, start + Duration::minutes(60)), 70);
    }

    #[test]
    fn seen_and_resolved_penalties_applied_and_clamp_0_to_100() {
        let start: DateTime<Utc> = "2026-09-17T12:00:00Z".parse().unwrap();
        let event = make_test_event(Category::Request, Severity::Routine, start); // base 10
        let mut entry = QueueEntry::new(event.event_id, event.category, event.seq, 10);

        // At 35 min (no recency): base 10
        let t_old = start + Duration::minutes(35);
        assert_eq!(score(&entry, &event, t_old), 10);

        // Seen: -15 penalty => 10 - 15 = -5 => clamped to 0
        entry.state = EntryState::Seen;
        assert_eq!(score(&entry, &event, t_old), 0);

        // Resolved: -40 penalty => clamped to 0
        entry.resolution = Some(Resolution::Approved);
        assert_eq!(score(&entry, &event, t_old), 0);

        // Now test maximum clamping:
        // Critical (70) + recency (20) = 90. If we had extra bonus, clamped to 100.
        let crit_event = make_test_event(Category::Error, Severity::Critical, start);
        let crit_entry = QueueEntry::new(crit_event.event_id, crit_event.category, crit_event.seq, 70);
        assert_eq!(score(&crit_entry, &crit_event, start), 90);
    }

    #[test]
    fn working_entries_have_no_recency_bonus() {
        let start: DateTime<Utc> = "2026-09-17T12:00:00Z".parse().unwrap();
        let event = make_test_event(Category::Working, Severity::Routine, start);
        let entry = QueueEntry::new(event.event_id, event.category, event.seq, 10);

        // At 0 min: base 10, no recency
        assert_eq!(score(&entry, &event, start), 10);
        // At 20 min: still base 10
        assert_eq!(score(&entry, &event, start + Duration::minutes(20)), 10);
    }

    #[test]
    fn within_working_escalation_levels_outscore_each_other() {
        let start: DateTime<Utc> = "2026-09-17T12:00:00Z".parse().unwrap();
        let event = make_test_event(Category::Working, Severity::Routine, start);

        let mut entry0 = QueueEntry::new(event.event_id, event.category, event.seq, 10);
        entry0.escalation_level = 0;

        let mut entry1 = entry0.clone();
        entry1.escalation_level = 1;

        let mut entry2 = entry0.clone();
        entry2.escalation_level = 2;

        let now = start + Duration::minutes(10);
        let s0 = score(&entry0, &event, now);
        let s1 = score(&entry1, &event, now);
        let s2 = score(&entry2, &event, now);

        assert_eq!(s0, 10);
        assert_eq!(s1, 40); // +30
        assert_eq!(s2, 70); // +60
        assert!(s1 > s0);
        assert!(s2 > s1);
    }

    #[test]
    fn property_test_working_escalation_levels_outscore_equal_age() {
        let start: DateTime<Utc> = "2026-09-17T12:00:00Z".parse().unwrap();
        let ages_secs = [0, 5, 60, 300, 1800, 3600];
        let severities = [Severity::Routine, Severity::Notable];
        let states = [EntryState::New, EntryState::Seen];

        for &sev in &severities {
            let event = make_test_event(Category::Working, sev, start);
            for &age in &ages_secs {
                let now = start + Duration::seconds(age);
                for &st in &states {
                    let mut e0 = QueueEntry::new(event.event_id, event.category, event.seq, 0);
                    e0.escalation_level = 0;
                    e0.state = st;

                    let mut e1 = e0.clone();
                    e1.escalation_level = 1;

                    let mut e2 = e0.clone();
                    e2.escalation_level = 2;

                    let s0 = score(&e0, &event, now);
                    let s1 = score(&e1, &event, now);
                    let s2 = score(&e2, &event, now);

                    assert!(
                        s1 > s0,
                        "escalation level 1 ({s1}) must outscore level 0 ({s0}) at age {age}s, state {st:?}"
                    );
                    assert!(
                        s2 > s1,
                        "escalation level 2 ({s2}) must outscore level 1 ({s1}) at age {age}s, state {st:?}"
                    );
                }
            }
        }
    }
}
