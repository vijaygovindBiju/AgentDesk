//! AgentDesk shared data model: event schemas, queue metadata and the wire
//! protocol.
//!
//! This crate is pure data — no I/O, no async — so it can be mirrored into
//! the Flutter client. See docs/EVENT_MODEL.md, docs/DATA_MODEL.md and
//! docs/COMMUNICATION.md.

pub mod agent;
pub mod event;
pub mod message;
pub mod queue;

pub use agent::*;
pub use event::*;
pub use message::*;
pub use queue::*;
