//! State detector for Antigravity terminal sessions.
//!
//! Analyzes VT100 `ScreenSnapshot` structures to determine the high-level semantic state
//! of `agy`, detecting interactive confirmation menus, questions, progress, and prompts,
//! and translating them into `agentdesk_model::RawAgentEvent` without relying on naive
//! unstructured text scraping.

use std::collections::BTreeMap;

use agentdesk_model::{Operation, RawAgentEvent, RequestInfo};

use crate::screen::ScreenSnapshot;

/// Semantic state of the Antigravity session as observed through the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AntigravityState {
    /// Initial startup / splash screen.
    Initializing,
    /// Actively thinking, calling tools, or running background work.
    Working { details: String },
    /// Waiting for user approval on a shell command execution.
    CommandConfirmation {
        command: String,
        selected_option_index: usize,
        options: Vec<String>,
    },
    /// Waiting for user approval on a file creation / edit / diff.
    FileEditConfirmation {
        file_path: String,
        selected_option_index: usize,
        options: Vec<String>,
    },
    /// Waiting for user to answer a multiple-choice or text question (`ask_question`).
    UserQuestion {
        question: String,
        options: Vec<String>,
    },
    /// Waiting for workspace trust confirmation.
    WorkspaceTrust { directory: String },
    /// Idle prompt at the bottom input bar waiting for the user's next request.
    IdlePrompt,
    /// Task or goal successfully completed.
    Completed { message: String },
    /// Fatal error encountered.
    FatalError { error: String },
}

/// State machine tracking Antigravity session transitions.
pub struct AntigravityStateMachine {
    agent_id: String,
    agent_seq: u64,
    current_state: AntigravityState,
    confirmed_state_count: usize,
}

impl AntigravityStateMachine {
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            agent_seq: 0,
            current_state: AntigravityState::Initializing,
            confirmed_state_count: 0,
        }
    }

    pub fn current_state(&self) -> &AntigravityState {
        &self.current_state
    }

    /// Update with a new screen snapshot and return a `RawAgentEvent` if an actionable
    /// state transition occurred.
    pub fn update(&mut self, snapshot: &ScreenSnapshot) -> Option<RawAgentEvent> {
        let detected = detect_state(snapshot);
        if detected != self.current_state {
            self.current_state = detected.clone();
            self.confirmed_state_count = 1;
            self.state_to_event(&detected)
        } else {
            self.confirmed_state_count += 1;
            None
        }
    }

    fn next_seq(&mut self) -> u64 {
        self.agent_seq += 1;
        self.agent_seq
    }

    fn state_to_event(&mut self, state: &AntigravityState) -> Option<RawAgentEvent> {
        match state {
            AntigravityState::CommandConfirmation { command, .. } => {
                let op = classify_command_operation(command);
                let seq = self.next_seq();
                let mut details = BTreeMap::new();
                details.insert(
                    "command".to_string(),
                    serde_json::Value::String(command.clone()),
                );

                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id: None,
                    kind: "command_confirmation".to_string(),
                    operation: op,
                    message: format!("Command requires approval: {command}"),
                    details,
                    log_lines: vec![format!("Command: {command}")],
                    request: Some(RequestInfo {
                        prompt: command.clone(),
                        options: vec!["approve".to_string(), "deny".to_string()],
                    }),
                })
            }
            AntigravityState::FileEditConfirmation { file_path, .. } => {
                let seq = self.next_seq();
                let mut details = BTreeMap::new();
                details.insert(
                    "file".to_string(),
                    serde_json::Value::String(file_path.clone()),
                );

                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id: None,
                    kind: "file_edit_confirmation".to_string(),
                    operation: Operation::Edit,
                    message: format!("File edit requires approval: {file_path}"),
                    details,
                    log_lines: vec![format!("File: {file_path}")],
                    request: Some(RequestInfo {
                        prompt: format!("Allow edit to {file_path}?"),
                        options: vec!["approve".to_string(), "deny".to_string()],
                    }),
                })
            }
            AntigravityState::WorkspaceTrust { directory } => {
                let seq = self.next_seq();
                let mut details = BTreeMap::new();
                details.insert(
                    "directory".to_string(),
                    serde_json::Value::String(directory.clone()),
                );

                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id: None,
                    kind: "workspace_trust".to_string(),
                    operation: Operation::Other,
                    message: format!("Workspace trust required for: {directory}"),
                    details,
                    log_lines: vec![format!("Trust requested for directory {directory}")],
                    request: Some(RequestInfo {
                        prompt: format!("Trust directory {directory}?"),
                        options: vec!["approve".to_string(), "deny".to_string()],
                    }),
                })
            }
            AntigravityState::UserQuestion { question, options } => {
                let seq = self.next_seq();
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id: None,
                    kind: "question".to_string(),
                    operation: Operation::Other,
                    message: question.clone(),
                    details: BTreeMap::new(),
                    log_lines: vec![question.clone()],
                    request: Some(RequestInfo {
                        prompt: question.clone(),
                        options: options.clone(),
                    }),
                })
            }
            AntigravityState::Working { details } => {
                let seq = self.next_seq();
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id: None,
                    kind: "progress".to_string(),
                    operation: Operation::Other,
                    message: details.clone(),
                    details: BTreeMap::new(),
                    log_lines: vec![],
                    request: None,
                })
            }
            AntigravityState::Completed { message } => {
                let seq = self.next_seq();
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id: None,
                    kind: "completed".to_string(),
                    operation: Operation::Other,
                    message: message.clone(),
                    details: BTreeMap::new(),
                    log_lines: vec![message.clone()],
                    request: None,
                })
            }
            AntigravityState::FatalError { error } => {
                let seq = self.next_seq();
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id: None,
                    kind: "error".to_string(),
                    operation: Operation::Other,
                    message: error.clone(),
                    details: BTreeMap::new(),
                    log_lines: vec![error.clone()],
                    request: None,
                })
            }
            AntigravityState::IdlePrompt => {
                let seq = self.next_seq();
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id: None,
                    kind: "idle".to_string(),
                    operation: Operation::Other,
                    message: "Agent ready for input".to_string(),
                    details: BTreeMap::new(),
                    log_lines: vec![],
                    request: None,
                })
            }
            AntigravityState::Initializing => None,
        }
    }
}

/// Detect the semantic state from a screen snapshot.
pub fn detect_state(snapshot: &ScreenSnapshot) -> AntigravityState {
    let non_empty_lines: Vec<&String> = snapshot.lines.iter().filter(|l| !l.is_empty()).collect();

    // 1. Workspace Trust prompt
    for line in &non_empty_lines {
        if line.contains("Do you trust the authors of the files")
            || line.contains("trust this folder")
        {
            let dir = non_empty_lines
                .iter()
                .find(|l| l.contains('/') || l.contains('\\'))
                .cloned()
                .cloned()
                .unwrap_or_else(|| "current directory".to_string());
            return AntigravityState::WorkspaceTrust { directory: dir };
        }
    }

    // 2. Interactive Command Confirmation Menu
    if let Some((command, options, selected)) = extract_command_dialog(snapshot) {
        return AntigravityState::CommandConfirmation {
            command,
            selected_option_index: selected,
            options,
        };
    }

    // 3. Interactive File Edit Confirmation Menu
    if let Some((file_path, options, selected)) = extract_file_edit_dialog(snapshot) {
        return AntigravityState::FileEditConfirmation {
            file_path,
            selected_option_index: selected,
            options,
        };
    }

    // 4. Interactive User Question (ask_question)
    let has_question_options = non_empty_lines
        .iter()
        .any(|l| l.contains("( )") || l.contains("[ ]") || l.contains("(•)") || l.contains("[x]"));
    if has_question_options {
        let (question, options) = extract_question_dialog(snapshot);
        return AntigravityState::UserQuestion { question, options };
    }

    // 5. Fatal Error check
    for line in &non_empty_lines {
        if line.contains("FATAL:")
            || line.contains("Panic:")
            || line.contains("Error: failed to connect to")
        {
            return AntigravityState::FatalError {
                error: line.to_string(),
            };
        }
    }

    // 6. Working / Progress / Thinking
    let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    for line in &non_empty_lines {
        if spinner_chars.iter().any(|&c| line.contains(c))
            || line.contains("Thinking...")
            || line.contains("Working...")
            || line.contains("Searching...")
            || line.contains("Generating...")
        {
            return AntigravityState::Working {
                details: line.trim().to_string(),
            };
        }
    }

    // 7. Idle Prompt
    if let Some(last_line) = non_empty_lines.last() {
        let is_idle = last_line.starts_with("> ")
            || last_line.starts_with("? ")
            || last_line.contains("Send message")
            || (snapshot.cursor_row > 0 && snapshot.cursor_visible && last_line.trim().is_empty());
        if is_idle {
            return AntigravityState::IdlePrompt;
        }
    }

    AntigravityState::Initializing
}

fn extract_command_dialog(snapshot: &ScreenSnapshot) -> Option<(String, Vec<String>, usize)> {
    let mut options = Vec::new();
    let mut selected_index = 0;
    let mut command = String::new();
    let mut has_selection = false;

    for (r, line) in snapshot.lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("Command:")
            || trimmed.starts_with("Run:")
            || trimmed.starts_with("$ ")
        {
            command = trimmed
                .trim_start_matches("Command:")
                .trim_start_matches("Run:")
                .trim_start_matches("$ ")
                .trim()
                .to_string();
        } else if trimmed.starts_with("Yes,") || trimmed.starts_with("No,") {
            let opt_text = trimmed.to_string();
            let is_rev = snapshot
                .reversed_lines
                .iter()
                .any(|(row, _)| *row == r as u16);
            let has_cursor = trimmed.starts_with('>') || trimmed.starts_with('●');
            if is_rev || has_cursor {
                selected_index = options.len();
                has_selection = true;
            }
            options.push(opt_text);
        }
    }

    let has_run_cmd = options
        .iter()
        .any(|o| o.contains("run command") || o.contains("Run command"));
    let has_deny = options
        .iter()
        .any(|o| o.contains("deny") || o.contains("cancel"));

    if options.len() >= 2 && has_run_cmd && has_deny && (has_selection || snapshot.in_alt_screen) {
        if command.is_empty() {
            command = "run_command".to_string();
        }
        Some((command, options, selected_index))
    } else {
        None
    }
}

fn extract_file_edit_dialog(snapshot: &ScreenSnapshot) -> Option<(String, Vec<String>, usize)> {
    let mut options = Vec::new();
    let mut selected_index = 0;
    let mut file_path = String::new();
    let mut has_selection = false;

    for (r, line) in snapshot.lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("File:") || trimmed.starts_with("Editing:") {
            file_path = trimmed
                .trim_start_matches("File:")
                .trim_start_matches("Editing:")
                .trim()
                .to_string();
        } else if trimmed.starts_with("Yes,")
            || trimmed.starts_with("No,")
            || trimmed.starts_with("Review in")
        {
            let is_rev = snapshot
                .reversed_lines
                .iter()
                .any(|(row, _)| *row == r as u16);
            let has_cursor = trimmed.starts_with('>') || trimmed.starts_with('●');
            if is_rev || has_cursor {
                selected_index = options.len();
                has_selection = true;
            }
            options.push(trimmed.to_string());
        }
    }

    let has_accept = options
        .iter()
        .any(|o| o.contains("accept") || o.contains("overwrite"));
    let has_reject = options
        .iter()
        .any(|o| o.contains("reject") || o.contains("cancel"));

    if options.len() >= 2 && has_accept && has_reject && (has_selection || snapshot.in_alt_screen) {
        if file_path.is_empty() {
            file_path = "unknown_file".to_string();
        }
        Some((file_path, options, selected_index))
    } else {
        None
    }
}

fn extract_question_dialog(snapshot: &ScreenSnapshot) -> (String, Vec<String>) {
    let mut question = String::new();
    let mut options = Vec::new();

    for line in &snapshot.lines {
        let trimmed = line.trim();
        if trimmed.contains("( )") || trimmed.contains("[ ]") {
            options.push(trimmed.to_string());
        } else if question.is_empty() && !trimmed.is_empty() && !trimmed.starts_with('─') {
            question = trimmed.to_string();
        }
    }

    (question, options)
}

fn classify_command_operation(cmd: &str) -> Operation {
    let c = cmd.trim();
    if c.starts_with("cargo test") || c.starts_with("pytest") || c.starts_with("npm test") {
        Operation::Test
    } else if c.starts_with("cargo build")
        || c.starts_with("make")
        || c.starts_with("gcc")
        || c.starts_with("go build")
    {
        Operation::Build
    } else if c.starts_with("npm install")
        || c.starts_with("cargo install")
        || c.starts_with("pip install")
    {
        Operation::Install
    } else if c.starts_with("git status") || c.starts_with("ls") || c.starts_with("cat") {
        Operation::Analyze
    } else {
        Operation::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_command_confirmation() {
        let snapshot = ScreenSnapshot {
            cols: 80,
            rows: 24,
            cursor_row: 10,
            cursor_col: 0,
            in_alt_screen: true,
            cursor_visible: true,
            lines: vec![
                "Command: cargo test --workspace".to_string(),
                "Yes, run command".to_string(),
                "Yes, and always allow in this conversation".to_string(),
                "No, deny".to_string(),
                "No, and tell agent...".to_string(),
            ],
            reversed_lines: vec![(1, "Yes, run command".to_string())],
        };

        let state = detect_state(&snapshot);
        match state {
            AntigravityState::CommandConfirmation {
                command,
                selected_option_index,
                options,
            } => {
                assert_eq!(command, "cargo test --workspace");
                assert_eq!(selected_option_index, 0);
                assert_eq!(options.len(), 4);
            }
            other => panic!("Unexpected state: {:?}", other),
        }
    }

    #[test]
    fn test_state_machine_emits_request_event() {
        let mut sm = AntigravityStateMachine::new("test-agy");
        let snapshot = ScreenSnapshot {
            cols: 80,
            rows: 24,
            cursor_row: 10,
            cursor_col: 0,
            in_alt_screen: true,
            cursor_visible: true,
            lines: vec![
                "Command: npm test".to_string(),
                "Yes, run command".to_string(),
                "No, deny".to_string(),
            ],
            reversed_lines: vec![(1, "Yes, run command".to_string())],
        };

        let evt = sm.update(&snapshot).expect("Expected RawAgentEvent");
        assert_eq!(evt.kind, "command_confirmation");
        assert_eq!(evt.operation, Operation::Test);
        assert!(evt.request.is_some());
        let req = evt.request.unwrap();
        assert_eq!(req.prompt, "npm test");
        assert_eq!(req.options, vec!["approve", "deny"]);
    }
}
