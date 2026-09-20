//! Offline deterministic replayer for recorded Antigravity PTY sessions.
//!
//! Loads `PtyRecording` JSON fixtures and steps through chunks, driving `Screen`
//! and `AntigravityStateMachine` without running external processes.

use std::path::Path;

use agentdesk_model::RawAgentEvent;

use crate::pty::PtyRecording;
use crate::screen::Screen;
use crate::state_machine::{AntigravityState, AntigravityStateMachine};

/// Result of replaying a PTY recording through the parser and state machine.
#[derive(Debug, Clone)]
pub struct ReplayResult {
    pub states: Vec<AntigravityState>,
    pub events: Vec<RawAgentEvent>,
    pub final_screen_lines: Vec<String>,
}

/// Offline replayer for `PtyRecording` sessions.
pub struct PtyReplayer {
    recording: PtyRecording,
    screen: Screen,
    state_machine: AntigravityStateMachine,
}

impl PtyReplayer {
    pub fn new(recording: PtyRecording) -> Self {
        let cols = recording.initial_cols;
        let rows = recording.initial_rows;
        Self {
            recording,
            screen: Screen::new(cols, rows),
            state_machine: AntigravityStateMachine::new("replay-agent"),
        }
    }

    pub fn from_file(path: &Path) -> std::io::Result<Self> {
        let recording = PtyRecording::load_from_file(path)?;
        Ok(Self::new(recording))
    }

    /// Replay all recorded chunks and collect observed states and emitted events.
    pub fn replay(&mut self) -> ReplayResult {
        let mut states = Vec::new();
        let mut events = Vec::new();

        for chunk in &self.recording.chunks {
            self.screen.process_bytes(&chunk.bytes);
            let snapshot = self.screen.snapshot();
            if let Some(event) = self.state_machine.update(&snapshot) {
                events.push(event);
            }
            let current = self.state_machine.current_state().clone();
            if states.last() != Some(&current) {
                states.push(current);
            }
        }

        ReplayResult {
            states,
            events,
            final_screen_lines: self.screen.all_lines(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::PtyChunk;

    #[test]
    fn test_replay_deterministic_chunks() {
        let mut recording = PtyRecording::new("test-session", "agy", vec![], "/workspace", 80, 24);
        recording.chunks.push(PtyChunk {
            timestamp_ms: 0,
            bytes: b"Thinking...\r\n".to_vec(),
        });
        recording.chunks.push(PtyChunk {
            timestamp_ms: 100,
            bytes:
                b"\x1b[?1049hCommand: git status\r\n\x1b[7mYes, run command\x1b[0m\r\nNo, deny\r\n"
                    .to_vec(),
        });

        let mut replayer = PtyReplayer::new(recording);
        let result = replayer.replay();

        assert!(!result.states.is_empty());
        let has_command_conf = result.states.iter().any(|s| {
            matches!(
                s,
                AntigravityState::CommandConfirmation {
                    command,
                    ..
                } if command == "git status"
            )
        });
        assert!(has_command_conf, "Must detect command confirmation");
        assert!(
            result
                .events
                .iter()
                .any(|e| e.kind == "command_confirmation"),
            "Must emit command confirmation event"
        );
    }
}
