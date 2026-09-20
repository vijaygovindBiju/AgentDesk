//! Antigravity PTY Lab: Controlled laboratory for evaluating Antigravity terminal integration.
//!
//! Provides POSIX pseudo-terminal lifecycle management, VT100/ANSI screen emulation,
//! visual state detection, reverse keystroke encoding, offline replay, and adversarial testing.

pub mod adversarial;
pub mod input_encoder;
pub mod pty;
pub mod replayer;
pub mod screen;
pub mod state_machine;

pub use input_encoder::{encode_decision, encode_text_submission, keys};
pub use pty::{PtyChunk, PtyRecording, PtySession};
pub use replayer::{PtyReplayer, ReplayResult};
pub use screen::{Cell, CellAttributes, Screen, ScreenSnapshot};
pub use state_machine::{AntigravityState, AntigravityStateMachine, detect_state};
