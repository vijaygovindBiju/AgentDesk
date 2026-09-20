//! Antigravity Pseudo-Terminal (PTY) Adapter module.
//!
//! Provides the complete laptop-side boundary between Antigravity's Charm Bubbletea
//! terminal UI and AgentDesk's agent-agnostic core pipeline.
//!
//! Implements VT100 terminal emulation, visual state detection, deterministic
//! keystroke reverse control, and the `Adapter` trait.

pub mod adapter;
pub mod input_encoder;
pub mod pty;
pub mod screen;
pub mod state_machine;

pub use adapter::{AntigravityConfig, AntigravityLifecycle, AntigravityPtyAdapter};
pub use input_encoder::{encode_decision, encode_text_submission, keys};
pub use pty::{MockPtyTransport, PtyChunk, PtyRecording, PtySession, PtyTransport};
pub use screen::{Cell, CellAttributes, Screen, ScreenSnapshot};
pub use state_machine::{
    AntigravityState, AntigravityStateMachine, classify_command_operation, detect_state,
};
