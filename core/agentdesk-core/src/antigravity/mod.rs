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

pub use adapter::{
    AntigravityConfig, AntigravityLifecycle, AntigravityPtyAdapter, PendingRequest, RequestOutcome,
};
pub use input_encoder::{
    QuestionInput, encode_decision, encode_question_response, encode_response_for_state,
    encode_text_submission, keys, validate_text_input,
};
pub use pty::{MockPtyTransport, PtyChunk, PtyRecording, PtySession, PtyTransport};
pub use screen::{Cell, CellAttributes, Screen, ScreenSnapshot};
pub use state_machine::{
    AntigravityState, AntigravityStateMachine, QuestionDetails, QuestionDialog, QuestionForm,
    StepInfo, WRITE_IN_LABEL, classify_command_operation, detect_state,
    extract_command_from_summary, extract_file_path_from_summary, extract_final_result,
    extract_question_dialog, extract_work_summary, is_terminal_artifact, parse_execution_steps,
};
