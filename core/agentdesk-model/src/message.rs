//! Wire protocol: the envelope and every message type in
//! docs/COMMUNICATION.md. One JSON text frame carries one `Message`.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::event::{AgentId, Event, EventId, RawAgentEvent};
use crate::queue::{EntryState, QueueEntry, Resolution};

/// WebSocket close codes used by the daemon.
pub mod close_code {
    /// Missing/invalid token, or first frame was not `hello`.
    pub const UNAUTHORIZED: u16 = 4001;
    /// Client schema version not supported.
    pub const SCHEMA_MISMATCH: u16 = 4002;
    /// Client could not keep up; outbound channel overflowed.
    pub const SLOW_CLIENT: u16 = 4003;
}

/// Which stages of the laptop pipeline are active. Selected at startup and
/// reported in `welcome`; used by the bench for like-for-like comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineMode {
    RawLines,
    RawEvents,
    Agentdesk,
}

/// How the connection is secured. `InsecureDev` is loopback-only and the
/// phone shows a warning banner when it sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportMode {
    Tls,
    InsecureDev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approve,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandError {
    NoSuchEvent,
    NotARequest,
    AlreadyResolved,
    Invalid,
}

/// One raw log line as stored and paged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    pub offset: u64,
    pub ts: DateTime<Utc>,
    pub text: String,
}

/// Flat counter snapshot; keys are the metric names in ARCHITECTURE.md.
pub type MetricsSnapshot = BTreeMap<String, u64>;

/// Sentinel for `GetEventLogs.offset` meaning "the tail".
pub const LOG_OFFSET_TAIL: i64 = -1;

/// The envelope. `request_id` is present on phone requests and echoed on
/// exactly one reply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(flatten)]
    pub body: Body,
}

impl Message {
    pub fn push(body: Body) -> Self {
        Message {
            request_id: None,
            body,
        }
    }
    pub fn with_request_id(request_id: impl Into<String>, body: Body) -> Self {
        Message {
            request_id: Some(request_id.into()),
            body,
        }
    }
}

/// All message types, tagged by `type` with the content under `payload`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum Body {
    // ---- handshake ----
    Hello(Hello),
    Welcome(Welcome),
    Snapshot(Snapshot),

    // ---- laptop → phone push ----
    Event(EventPush),
    ScoreUpdate(ScoreUpdate),
    StateUpdate(StateUpdate),
    RawEvent(RawAgentEvent),
    RawLine(RawLine),

    // ---- phone → laptop requests ----
    GetEventDetails(EventRef),
    GetEventLogs(GetEventLogs),
    Ack(EventRef),
    Dismiss(EventRef),
    RespondRequest(RespondRequest),
    GetMetrics(Empty),

    // ---- laptop → phone replies ----
    EventDetails(EventPush),
    EventLogs(EventLogs),
    CommandResult(CommandResult),
    Metrics(MetricsSnapshot),
    Error(ErrorReply),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub token: String,
    pub device_id: String,
    pub client_version: String,
    pub schema_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    pub daemon_version: String,
    pub schema_version: u32,
    pub pipeline_mode: PipelineMode,
    pub transport: TransportMode,
    pub server_time: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub entries: Vec<QueueEntry>,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventPush {
    pub event: Event,
    pub entry: QueueEntry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoreUpdate {
    pub event_id: EventId,
    pub score: u16,
    pub escalation_level: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateUpdate {
    pub event_id: EventId,
    pub state: EntryState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<Resolution>,
    #[serde(default)]
    pub superseded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawLine {
    pub agent_id: AgentId,
    #[serde(flatten)]
    pub line: LogLine,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRef {
    pub event_id: EventId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetEventLogs {
    pub event_id: EventId,
    /// Per-agent line offset, or `LOG_OFFSET_TAIL`.
    pub offset: i64,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventLogs {
    pub event_id: EventId,
    /// Real start offset of `lines`.
    pub offset: u64,
    /// Lines currently retrievable for this event.
    pub total: u64,
    pub evicted: bool,
    pub lines: Vec<LogLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RespondRequest {
    pub event_id: EventId,
    pub decision: Decision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<CommandError>,
}

impl CommandResult {
    pub fn ok() -> Self {
        CommandResult {
            ok: true,
            error: None,
        }
    }
    pub fn err(error: CommandError) -> Self {
        CommandResult {
            ok: false,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorReply {
    pub code: String,
    pub message: String,
}

/// Payload for messages that carry nothing. Serialises as `{}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Empty {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::*;
    use serde_json::json;
    use uuid::Uuid;

    fn ts() -> DateTime<Utc> {
        "2026-09-17T10:32:05Z".parse().unwrap()
    }

    fn event() -> Event {
        Event {
            schema_version: SCHEMA_VERSION,
            event_id: Uuid::nil(),
            seq: 1,
            agent_seq: 1,
            agent_id: "a".into(),
            agent_name: "A".into(),
            project: "P".into(),
            task_id: Some("t".into()),
            ts: ts(),
            category: Category::Request,
            severity: Severity::Critical,
            kind: "approval_required".into(),
            operation: Operation::Other,
            summary: "Approval required".into(),
            message: "Allow migration?".into(),
            details: Details::new(),
            log_range: LogRange {
                start: 0,
                end: 3,
                pinned: true,
            },
            request: Some(RequestInfo {
                prompt: "Allow migration?".into(),
                options: vec!["approve".into(), "deny".into()],
            }),
        }
    }

    fn entry() -> QueueEntry {
        QueueEntry::new(Uuid::nil(), Category::Request, 1, 90)
    }

    fn line(offset: u64) -> LogLine {
        LogLine {
            offset,
            ts: ts(),
            text: format!("line {offset}"),
        }
    }

    /// One sample of every message type in the catalogue.
    fn all_messages() -> Vec<Message> {
        let id = Uuid::nil();
        vec![
            Message::push(Body::Hello(Hello {
                token: "t".into(),
                device_id: "d".into(),
                client_version: "0.1.0".into(),
                schema_version: SCHEMA_VERSION,
            })),
            Message::push(Body::Welcome(Welcome {
                daemon_version: "0.1.0".into(),
                schema_version: SCHEMA_VERSION,
                pipeline_mode: PipelineMode::Agentdesk,
                transport: TransportMode::Tls,
                server_time: ts(),
            })),
            Message::push(Body::Snapshot(Snapshot {
                entries: vec![entry()],
                events: vec![event()],
            })),
            Message::push(Body::Event(EventPush {
                event: event(),
                entry: entry(),
            })),
            Message::push(Body::ScoreUpdate(ScoreUpdate {
                event_id: id,
                score: 55,
                escalation_level: 1,
            })),
            Message::push(Body::StateUpdate(StateUpdate {
                event_id: id,
                state: EntryState::Seen,
                resolution: Some(Resolution::Unresolved),
                superseded: false,
            })),
            Message::push(Body::RawEvent(RawAgentEvent {
                agent_id: "a".into(),
                agent_seq: 1,
                task_id: None,
                kind: "progress".into(),
                operation: Operation::Build,
                message: "m".into(),
                details: Details::new(),
                log_lines: vec![],
                request: None,
            })),
            Message::push(Body::RawLine(RawLine {
                agent_id: "a".into(),
                line: line(7),
            })),
            Message::with_request_id("r-1", Body::GetEventDetails(EventRef { event_id: id })),
            Message::with_request_id(
                "r-2",
                Body::GetEventLogs(GetEventLogs {
                    event_id: id,
                    offset: LOG_OFFSET_TAIL,
                    limit: 200,
                }),
            ),
            Message::with_request_id("r-3", Body::Ack(EventRef { event_id: id })),
            Message::with_request_id("r-4", Body::Dismiss(EventRef { event_id: id })),
            Message::with_request_id(
                "r-5",
                Body::RespondRequest(RespondRequest {
                    event_id: id,
                    decision: Decision::Approve,
                }),
            ),
            Message::with_request_id("r-6", Body::GetMetrics(Empty {})),
            Message::with_request_id(
                "r-1",
                Body::EventDetails(EventPush {
                    event: event(),
                    entry: entry(),
                }),
            ),
            Message::with_request_id(
                "r-2",
                Body::EventLogs(EventLogs {
                    event_id: id,
                    offset: 5,
                    total: 8,
                    evicted: false,
                    lines: vec![line(5), line(6)],
                }),
            ),
            Message::with_request_id(
                "r-5",
                Body::CommandResult(CommandResult::err(CommandError::AlreadyResolved)),
            ),
            Message::with_request_id(
                "r-6",
                Body::Metrics(MetricsSnapshot::from([("raw_events".to_string(), 42u64)])),
            ),
            Message::with_request_id(
                "r-9",
                Body::Error(ErrorReply {
                    code: "bad_request".into(),
                    message: "cannot parse".into(),
                }),
            ),
        ]
    }

    #[test]
    fn every_message_type_round_trips() {
        let msgs = all_messages();
        assert_eq!(
            msgs.len(),
            19,
            "catalogue size changed; update COMMUNICATION.md"
        );
        for m in msgs {
            let s = serde_json::to_string(&m).unwrap();
            let back: Message = serde_json::from_str(&s).unwrap_or_else(|e| panic!("{e}: {s}"));
            assert_eq!(m, back, "{s}");
        }
    }

    #[test]
    fn envelope_shape_matches_protocol_doc() {
        let m = Message::with_request_id(
            "r-2",
            Body::GetEventLogs(GetEventLogs {
                event_id: Uuid::nil(),
                offset: -1,
                limit: 10,
            }),
        );
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["type"], json!("get_event_logs"));
        assert_eq!(v["request_id"], json!("r-2"));
        assert_eq!(v["payload"]["offset"], json!(-1));
        assert_eq!(v.as_object().unwrap().len(), 3);

        // Push messages omit request_id entirely.
        let p = serde_json::to_value(Message::push(Body::GetMetrics(Empty {}))).unwrap();
        assert!(p.get("request_id").is_none());
        assert_eq!(p["payload"], json!({}));
    }

    #[test]
    fn type_names_are_snake_case_catalogue_names() {
        let names: Vec<String> = all_messages()
            .iter()
            .map(|m| {
                serde_json::to_value(m).unwrap()["type"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            names,
            [
                "hello",
                "welcome",
                "snapshot",
                "event",
                "score_update",
                "state_update",
                "raw_event",
                "raw_line",
                "get_event_details",
                "get_event_logs",
                "ack",
                "dismiss",
                "respond_request",
                "get_metrics",
                "event_details",
                "event_logs",
                "command_result",
                "metrics",
                "error",
            ]
        );
    }

    #[test]
    fn unknown_type_is_rejected_and_unknown_fields_ignored() {
        let bad = json!({ "type": "teleport", "payload": {} });
        assert!(serde_json::from_value::<Message>(bad).is_err());

        let ok = json!({ "type": "ack", "request_id": "r", "payload": { "event_id": Uuid::nil(), "extra": 1 }, "trace": "x" });
        let m: Message = serde_json::from_value(ok).unwrap();
        assert_eq!(
            m.body,
            Body::Ack(EventRef {
                event_id: Uuid::nil()
            })
        );
    }

    #[test]
    fn raw_line_flattens_log_line() {
        let v = serde_json::to_value(RawLine {
            agent_id: "a".into(),
            line: line(3),
        })
        .unwrap();
        assert_eq!(v["offset"], json!(3));
        assert_eq!(v["agent_id"], json!("a"));
    }

    #[test]
    fn close_codes() {
        assert_eq!(close_code::UNAUTHORIZED, 4001);
        assert_eq!(close_code::SCHEMA_MISMATCH, 4002);
        assert_eq!(close_code::SLOW_CLIENT, 4003);
    }
}
