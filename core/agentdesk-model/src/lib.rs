//! AgentDesk shared data model: event schemas and the wire protocol.
//!
//! This crate is pure data — no I/O, no async — so it can be mirrored into
//! the Flutter client. See docs/EVENT_MODEL.md and docs/COMMUNICATION.md.

pub mod event;

pub use event::*;
