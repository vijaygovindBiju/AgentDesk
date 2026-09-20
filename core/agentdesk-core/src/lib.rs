//! AgentDesk core: adapter seam, classification, and (in later phases) the
//! event pipeline, priority queue, log store, task tracker and metrics.
//! See docs/ARCHITECTURE.md.

pub mod acp;
pub mod adapter;
pub mod classifier;
pub mod clock;
pub mod core_task;
pub mod event_store;
pub mod log_store;
pub mod metrics;
pub mod pipeline;
pub mod processor;
pub mod queue;
pub mod scoring;
pub mod sink;
pub mod tracker;

pub use acp::{
    AcpAdapter, AcpConfig, AcpPermissionOption, AcpState, AcpToolCall, AcpTransport,
    MockAcpTransport, ProcessTransport, TransportMessage,
};
pub use adapter::{Adapter, AdapterOutput, RespondError};
pub use classifier::{classify, Classification, Matched, Rule, RULES};
pub use clock::{Clock, SystemClock, VirtualClock};
pub use core_task::{AdapterCommand, ClientId, CoreCommand, CoreHandle, CoreTask};
pub use event_store::EventStore;
pub use log_store::{
    LogPage, LogStore, LogStoreConfig, PinnedLogWindow, DEFAULT_PAGE_CAP, DEFAULT_PIN_AFTER,
    DEFAULT_PIN_BEFORE, DEFAULT_RING_CAPACITY,
};
pub use metrics::Metrics;
pub use pipeline::Pipeline;
pub use processor::{EventProcessor, ProcessError};
pub use queue::PriorityQueue;
pub use scoring::{
    base_score, escalation_bonus, recency_bonus, resolved_penalty, score, seen_penalty,
};
pub use sink::{ChannelSink, CountingSink, SinkError, TransportSink, VecSink};
pub use tracker::{EscalationAction, OpenTask, TaskTracker, ThresholdTable};
