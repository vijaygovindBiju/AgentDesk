//! Event types: what adapters produce (`RawAgentEvent`) and what the
//! processor stores and pushes (`Event`). See docs/EVENT_MODEL.md.

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};
use uuid::Uuid;

/// Version of the wire schema. Bumped on incompatible changes; the phone
/// refuses daemons whose version it does not understand.
pub const SCHEMA_VERSION: u32 = 1;

pub type AgentId = String;
pub type TaskId = String;
pub type EventId = Uuid;

/// Flat map of short Level-2 fields (strings/numbers). Large content
/// belongs in logs, not here. `BTreeMap` keeps serialisation deterministic.
pub type Details = BTreeMap<String, serde_json::Value>;

/// The four attention categories. Tier order is fixed by this enum and
/// nothing may move an entry across tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Request,
    Error,
    Completed,
    Working,
}

impl Category {
    /// Queue tier: lower sorts first.
    pub fn tier(self) -> u8 {
        match self {
            Category::Request => 0,
            Category::Error => 1,
            Category::Completed => 2,
            Category::Working => 3,
        }
    }

    pub const ALL: [Category; 4] = [
        Category::Request,
        Category::Error,
        Category::Completed,
        Category::Working,
    ];
}

/// Severity 0..=3, serialised as a plain integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    Routine = 0,
    Notable = 1,
    Important = 2,
    Critical = 3,
}

impl Severity {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for Severity {
    type Error = u8;
    fn try_from(v: u8) -> Result<Self, u8> {
        match v {
            0 => Ok(Severity::Routine),
            1 => Ok(Severity::Notable),
            2 => Ok(Severity::Important),
            3 => Ok(Severity::Critical),
            other => Err(other),
        }
    }
}

impl Serialize for Severity {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(self.as_u8())
    }
}

impl<'de> Deserialize<'de> for Severity {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = u8::deserialize(d)?;
        Severity::try_from(v).map_err(|v| de::Error::custom(format!("severity out of range: {v}")))
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_u8())
    }
}

/// Kind of operation a task performs. Drives the escalation threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Build,
    Test,
    Install,
    Analyze,
    Edit,
    Other,
}

impl Operation {
    pub const ALL: [Operation; 6] = [
        Operation::Build,
        Operation::Test,
        Operation::Install,
        Operation::Analyze,
        Operation::Edit,
        Operation::Other,
    ];
}

/// Reference into the per-agent log store. Offsets are per-agent line
/// counters that never reset. `pinned` means the window was copied and
/// survives ring eviction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogRange {
    pub start: u64,
    pub end: u64,
    pub pinned: bool,
}

/// Type of interactive question requested by an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionType {
    SingleChoice,
    MultipleChoice,
    FreeText,
}

/// Present only on `Category::Request` events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestInfo {
    pub prompt: String,
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_type: Option<QuestionType>,
}

/// What an adapter emits. Nothing downstream sees agent-specific data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawAgentEvent {
    pub agent_id: AgentId,
    pub agent_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    /// Stable per adapter; the classifier keys on it.
    pub kind: String,
    pub operation: Operation,
    pub message: String,
    #[serde(default)]
    pub details: Details,
    #[serde(default)]
    pub log_lines: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<RequestInfo>,
}

/// Immutable processed event. Mutable per-event data (state, score,
/// escalation) lives in `QueueEntry`, never here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub schema_version: u32,
    pub event_id: EventId,
    /// Global, strictly monotonic per daemon run.
    pub seq: u64,
    /// Strictly monotonic per agent.
    pub agent_seq: u64,
    pub agent_id: AgentId,
    pub agent_name: String,
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    pub ts: DateTime<Utc>,
    pub category: Category,
    pub severity: Severity,
    pub kind: String,
    pub operation: Operation,
    /// Level 1: one line.
    pub summary: String,
    /// Level 1: one line.
    pub message: String,
    /// Level 2.
    #[serde(default)]
    pub details: Details,
    /// Level 3 reference.
    pub log_range: LogRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<RequestInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_event() -> Event {
        Event {
            schema_version: SCHEMA_VERSION,
            event_id: Uuid::nil(),
            seq: 1042,
            agent_seq: 87,
            agent_id: "sim-backend".into(),
            agent_name: "Backend Agent".into(),
            project: "Hybrid".into(),
            task_id: Some("task-auth-01".into()),
            ts: "2026-09-17T10:32:05.123Z".parse().unwrap(),
            category: Category::Error,
            severity: Severity::Critical,
            kind: "build_failed".into(),
            operation: Operation::Build,
            summary: "Build Failed".into(),
            message: "auth_service.dart:42 Undefined variable: token".into(),
            details: Details::from([
                ("file".to_string(), json!("auth_service.dart")),
                ("line".to_string(), json!(42)),
            ]),
            log_range: LogRange {
                start: 9310,
                end: 9412,
                pinned: true,
            },
            request: None,
        }
    }

    #[test]
    fn event_round_trips() {
        let e = sample_event();
        let s = serde_json::to_string(&e).unwrap();
        let back: Event = serde_json::from_str(&s).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn raw_event_round_trips_with_defaults() {
        let raw = RawAgentEvent {
            agent_id: "sim-backend".into(),
            agent_seq: 1,
            task_id: None,
            kind: "progress".into(),
            operation: Operation::Other,
            message: "Compiling 1/500".into(),
            details: Details::new(),
            log_lines: vec![],
            request: None,
        };
        let s = serde_json::to_string(&raw).unwrap();
        assert!(!s.contains("task_id"), "None task_id must be omitted: {s}");
        let back: RawAgentEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(raw, back);

        // Optional collections may be absent on the wire.
        let minimal = json!({
            "agent_id": "a", "agent_seq": 1, "kind": "progress",
            "operation": "build", "message": "m"
        });
        let back: RawAgentEvent = serde_json::from_value(minimal).unwrap();
        assert!(back.details.is_empty() && back.log_lines.is_empty());
    }

    #[test]
    fn ts_is_rfc3339_utc() {
        let v = serde_json::to_value(sample_event()).unwrap();
        assert_eq!(v["ts"], json!("2026-09-17T10:32:05.123Z"));
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let mut v = serde_json::to_value(sample_event()).unwrap();
        v["future_field"] = json!("ignored");
        let back: Event = serde_json::from_value(v).unwrap();
        assert_eq!(back, sample_event());
    }

    #[test]
    fn missing_required_field_is_rejected() {
        let mut v = serde_json::to_value(sample_event()).unwrap();
        v.as_object_mut().unwrap().remove("category");
        assert!(serde_json::from_value::<Event>(v).is_err());
    }

    #[test]
    fn category_wire_names_and_tiers() {
        let names: Vec<String> = Category::ALL
            .iter()
            .map(|c| {
                serde_json::to_value(c)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(names, ["request", "error", "completed", "working"]);
        let tiers: Vec<u8> = Category::ALL.iter().map(|c| c.tier()).collect();
        assert_eq!(tiers, [0, 1, 2, 3]);
    }

    #[test]
    fn operation_wire_names() {
        let names: Vec<String> = Operation::ALL
            .iter()
            .map(|o| {
                serde_json::to_value(o)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            names,
            ["build", "test", "install", "analyze", "edit", "other"]
        );
    }

    #[test]
    fn severity_is_integer_and_range_checked() {
        assert_eq!(serde_json::to_value(Severity::Critical).unwrap(), json!(3));
        assert_eq!(
            serde_json::from_value::<Severity>(json!(0)).unwrap(),
            Severity::Routine
        );
        assert!(serde_json::from_value::<Severity>(json!(4)).is_err());
        assert!(serde_json::from_value::<Severity>(json!("3")).is_err());
    }
}
