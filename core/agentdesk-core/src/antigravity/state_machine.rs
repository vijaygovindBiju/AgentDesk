//! State detector for Antigravity terminal sessions.
//!
//! Analyzes VT100 `ScreenSnapshot` structures to determine the high-level semantic state
//! of `agy`, detecting interactive confirmation menus, questions, progress, and prompts,
//! and translating them into canonical `agentdesk_model::RawAgentEvent` without relying on naive
//! unstructured text scraping.

use std::collections::BTreeMap;

use agentdesk_model::{Operation, QuestionType, RawAgentEvent, RequestInfo, TaskId};

use crate::antigravity::screen::ScreenSnapshot;

/// Label Antigravity renders for the free-text write-in row of an `ask_question` menu.
pub const WRITE_IN_LABEL: &str = "Write-in...";

/// Prefix Antigravity renders on the chosen row once a question has been answered.
pub const ANSWERED_MARK: &str = "✓";

/// Structural details of an interactive question prompt.
///
/// `options` always lists the raw menu rows in screen order (including the `Write-in...`
/// row when present) because keystroke navigation is index based. Use
/// [`QuestionDetails::answer_options`] for the answer set exposed to AgentDesk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestionDetails {
    /// Single selection from a numbered menu (`> 1. Option`).
    SingleChoice {
        options: Vec<String>,
        selected_index: usize,
        write_in_index: Option<usize>,
    },
    /// Multiple selection with togglable checkboxes (`> 1. [ ] Option` / `2. [x] Option`).
    MultipleChoice {
        options: Vec<String>,
        checked_indices: Vec<usize>,
        cursor_index: usize,
        write_in_index: Option<usize>,
    },
    /// Write-in text entry control is active (`Your answer:` ... `enter Submit · esc Back`).
    FreeText { current_text: String },
}

impl QuestionDetails {
    pub fn question_type(&self) -> QuestionType {
        match self {
            QuestionDetails::SingleChoice { .. } => QuestionType::SingleChoice,
            QuestionDetails::MultipleChoice { .. } => QuestionType::MultipleChoice,
            QuestionDetails::FreeText { .. } => QuestionType::FreeText,
        }
    }

    /// Raw menu rows in screen order (navigation index space).
    pub fn menu_rows(&self) -> &[String] {
        match self {
            QuestionDetails::SingleChoice { options, .. }
            | QuestionDetails::MultipleChoice { options, .. } => options,
            QuestionDetails::FreeText { .. } => &[],
        }
    }

    pub fn write_in_index(&self) -> Option<usize> {
        match self {
            QuestionDetails::SingleChoice { write_in_index, .. }
            | QuestionDetails::MultipleChoice { write_in_index, .. } => *write_in_index,
            QuestionDetails::FreeText { .. } => None,
        }
    }

    /// Answer options exposed to AgentDesk (excludes the TUI's `Write-in...` affordance).
    pub fn answer_options(&self) -> Vec<String> {
        let write_in = self.write_in_index();
        self.menu_rows()
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != write_in)
            .map(|(_, o)| o.clone())
            .collect()
    }

    /// Whether the TUI offers a free-text write-in path for this question.
    pub fn allows_write_in(&self) -> bool {
        self.write_in_index().is_some()
    }

    /// Currently selected/checked answer options (excluding the write-in row).
    pub fn selected_options(&self) -> Vec<String> {
        match self {
            QuestionDetails::SingleChoice {
                options,
                selected_index,
                write_in_index,
            } => options
                .get(*selected_index)
                .filter(|_| Some(*selected_index) != *write_in_index)
                .cloned()
                .into_iter()
                .collect(),
            QuestionDetails::MultipleChoice {
                options,
                checked_indices,
                ..
            } => checked_indices
                .iter()
                .filter_map(|i| options.get(*i).cloned())
                .collect(),
            QuestionDetails::FreeText { .. } => Vec::new(),
        }
    }

    /// True when both describe the same question control set, ignoring live cursor/toggle/text
    /// state that a human at the laptop may legitimately have changed.
    pub fn same_controls(&self, other: &QuestionDetails) -> bool {
        match (self, other) {
            (
                QuestionDetails::SingleChoice {
                    options: a,
                    write_in_index: wa,
                    ..
                },
                QuestionDetails::SingleChoice {
                    options: b,
                    write_in_index: wb,
                    ..
                },
            ) => a == b && wa == wb,
            (
                QuestionDetails::MultipleChoice {
                    options: a,
                    write_in_index: wa,
                    ..
                },
                QuestionDetails::MultipleChoice {
                    options: b,
                    write_in_index: wb,
                    ..
                },
            ) => a == b && wa == wb,
            (QuestionDetails::FreeText { .. }, QuestionDetails::FreeText { .. }) => true,
            _ => false,
        }
    }
}

/// Position of a question within a multi-question `ask_question` form (`Question 1/2:`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuestionForm {
    pub index: u32,
    pub total: u32,
}

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
    /// Waiting for user to answer an interactive question (`ask_question`).
    UserQuestion {
        question: String,
        form: Option<QuestionForm>,
        details: QuestionDetails,
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

impl AntigravityState {
    /// True for states that block Antigravity on a human decision.
    pub fn is_blocking(&self) -> bool {
        matches!(
            self,
            AntigravityState::CommandConfirmation { .. }
                | AntigravityState::FileEditConfirmation { .. }
                | AntigravityState::UserQuestion { .. }
                | AntigravityState::WorkspaceTrust { .. }
        )
    }

    /// True when `other` is the same human request as `self`, ignoring live cursor /
    /// toggle / typed-text state. Used to bind a response to the request it answers.
    pub fn same_request(&self, other: &AntigravityState) -> bool {
        match (self, other) {
            (
                AntigravityState::CommandConfirmation {
                    command: a,
                    options: oa,
                    ..
                },
                AntigravityState::CommandConfirmation {
                    command: b,
                    options: ob,
                    ..
                },
            ) => a == b && oa == ob,
            (
                AntigravityState::FileEditConfirmation {
                    file_path: a,
                    options: oa,
                    ..
                },
                AntigravityState::FileEditConfirmation {
                    file_path: b,
                    options: ob,
                    ..
                },
            ) => a == b && oa == ob,
            (
                AntigravityState::UserQuestion {
                    question: qa,
                    form: fa,
                    details: da,
                },
                AntigravityState::UserQuestion {
                    question: qb,
                    form: fb,
                    details: db,
                },
            ) => qa == qb && fa == fb && da.same_controls(db),
            (
                AntigravityState::WorkspaceTrust { directory: a },
                AntigravityState::WorkspaceTrust { directory: b },
            ) => a == b,
            _ => false,
        }
    }
}

/// Build the normalized `details` map for a detected question. Only structured, sanitized
/// values are included; no terminal bytes or layout data.
pub fn question_details_map(
    question: &str,
    form: Option<QuestionForm>,
    details: &QuestionDetails,
) -> BTreeMap<String, serde_json::Value> {
    let mut d = BTreeMap::new();
    d.insert(
        "question_type".to_string(),
        serde_json::to_value(details.question_type()).unwrap_or(serde_json::Value::Null),
    );
    d.insert(
        "question".to_string(),
        serde_json::Value::String(question.to_string()),
    );
    d.insert(
        "options".to_string(),
        serde_json::json!(details.answer_options()),
    );
    d.insert(
        "selected_options".to_string(),
        serde_json::json!(details.selected_options()),
    );
    d.insert(
        "allows_write_in".to_string(),
        serde_json::Value::Bool(details.allows_write_in()),
    );
    if let Some(f) = form {
        d.insert("question_index".to_string(), serde_json::json!(f.index));
        d.insert("question_total".to_string(), serde_json::json!(f.total));
    }
    if let QuestionDetails::FreeText { current_text } = details {
        d.insert(
            "current_text".to_string(),
            serde_json::Value::String(current_text.clone()),
        );
    }
    d
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

    pub fn next_seq(&mut self) -> u64 {
        self.agent_seq += 1;
        self.agent_seq
    }

    /// Convert detected state into canonical normalized AgentDesk `RawAgentEvent`.
    pub fn to_normalized_event(
        &mut self,
        state: &AntigravityState,
        task_id: Option<TaskId>,
    ) -> Option<RawAgentEvent> {
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
                    task_id,
                    kind: "approval_required".to_string(),
                    operation: op,
                    message: format!("Command requires approval: {command}"),
                    details,
                    log_lines: vec![format!("Command: {command}")],
                    request: Some(RequestInfo {
                        prompt: command.clone(),
                        options: vec!["approve".to_string(), "deny".to_string()],
                        question_type: None,
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
                    task_id,
                    kind: "approval_required".to_string(),
                    operation: Operation::Edit,
                    message: format!("File edit requires approval: {file_path}"),
                    details,
                    log_lines: vec![format!("File: {file_path}")],
                    request: Some(RequestInfo {
                        prompt: format!("Allow edit to {file_path}?"),
                        options: vec!["approve".to_string(), "deny".to_string()],
                        question_type: None,
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
                    task_id,
                    kind: "approval_required".to_string(),
                    operation: Operation::Other,
                    message: format!("Workspace trust required for: {directory}"),
                    details,
                    log_lines: vec![format!("Trust requested for directory {directory}")],
                    request: Some(RequestInfo {
                        prompt: format!("Trust directory {directory}?"),
                        options: vec!["approve".to_string(), "deny".to_string()],
                        question_type: None,
                    }),
                })
            }
            AntigravityState::UserQuestion {
                question,
                form,
                details,
            } => {
                let seq = self.next_seq();
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "input_required".to_string(),
                    operation: Operation::Other,
                    message: question.clone(),
                    details: question_details_map(question, *form, details),
                    log_lines: vec![question.clone()],
                    request: Some(RequestInfo {
                        prompt: question.clone(),
                        options: details.answer_options(),
                        question_type: Some(details.question_type()),
                    }),
                })
            }
            AntigravityState::Working { details } => {
                let seq = self.next_seq();
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
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
                    task_id,
                    kind: "task_completed".to_string(),
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
                    task_id,
                    kind: "command_failed".to_string(),
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
                    task_id,
                    kind: "progress".to_string(),
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

    /// Event translation matching the original laboratory fixtures.
    pub fn state_to_event(&mut self, state: &AntigravityState) -> Option<RawAgentEvent> {
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
                        question_type: None,
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
                        question_type: None,
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
                        question_type: None,
                    }),
                })
            }
            AntigravityState::UserQuestion {
                question,
                form,
                details,
            } => {
                let seq = self.next_seq();
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id: None,
                    kind: "question".to_string(),
                    operation: Operation::Other,
                    message: question.clone(),
                    details: question_details_map(question, *form, details),
                    log_lines: vec![question.clone()],
                    request: Some(RequestInfo {
                        prompt: question.clone(),
                        options: details.answer_options(),
                        question_type: Some(details.question_type()),
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
    if let Some(dialog) = extract_question_dialog(snapshot) {
        return AntigravityState::UserQuestion {
            question: dialog.question,
            form: dialog.form,
            details: dialog.details,
        };
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
            || last_line.as_str() == ">"
            || last_line.starts_with("? ")
            || last_line.as_str() == "?"
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
    let mut expecting_command = false;

    for (r, line) in snapshot.lines.iter().enumerate() {
        let trimmed = line.trim();

        if expecting_command && !trimmed.is_empty() {
            if command.is_empty() {
                command = trimmed.to_string();
            }
            expecting_command = false;
        }

        if trimmed.starts_with("Requesting permission for:") {
            expecting_command = true;
        } else if trimmed.starts_with("Command:")
            || trimmed.starts_with("Run:")
            || trimmed.starts_with("$ ")
        {
            if command.is_empty() {
                command = trimmed
                    .trim_start_matches("Command:")
                    .trim_start_matches("Run:")
                    .trim_start_matches("$ ")
                    .trim()
                    .to_string();
            }
        } else if let Some(start) = trimmed.find("● Bash(") {
            let after = &trimmed[start + "● Bash(".len()..];
            if let Some(end) = after.find(')')
                && command.is_empty()
            {
                command = after[..end].trim().to_string();
            }
        }

        // Check if line is an interactive option (strip cursor/bullet, digits, dots, spaces)
        let opt_candidate = trimmed.trim_start_matches(|c: char| {
            c == '>' || c == '●' || c == ' ' || c.is_ascii_digit() || c == '.'
        });
        let opt_candidate = opt_candidate.trim();

        if opt_candidate.starts_with("Yes,") || opt_candidate.starts_with("No,") {
            let opt_text = opt_candidate.to_string();
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
        } else {
            let opt_candidate = trimmed.trim_start_matches(|c: char| {
                c == '>' || c == '●' || c == ' ' || c.is_ascii_digit() || c == '.'
            });
            let opt_candidate = opt_candidate.trim();

            if opt_candidate.starts_with("Yes,")
                || opt_candidate.starts_with("No,")
                || opt_candidate.starts_with("Review in")
            {
                let opt_text = opt_candidate.to_string();
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

/// Fully parsed interactive question dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionDialog {
    pub question: String,
    pub form: Option<QuestionForm>,
    pub details: QuestionDetails,
}

/// A parsed numbered menu row: `> 3. [x] Logging`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MenuRow {
    cursor: bool,
    number: usize,
    checkbox: Option<bool>,
    label: String,
}

/// Parse a single numbered menu row. Only the exact Bubbletea list layout is accepted:
/// optional `>` cursor in the first column, one or more spaces, `<n>.`, a space, label.
fn parse_menu_row(line: &str) -> Option<MenuRow> {
    let (cursor, rest) = match line.strip_prefix('>') {
        Some(rest) => (true, rest),
        None => (false, line),
    };
    if !rest.starts_with(' ') {
        return None;
    }
    let rest = rest.trim_start_matches(' ');
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    let number: usize = rest[..digits_end].parse().ok()?;
    let after_num = &rest[digits_end..];
    let label = after_num.strip_prefix(". ")?;
    let label = label.trim_end();
    if label.is_empty() {
        return None;
    }
    let (checkbox, label) = if let Some(l) = label.strip_prefix("[ ] ") {
        (Some(false), l)
    } else if let Some(l) = label
        .strip_prefix("[x] ")
        .or_else(|| label.strip_prefix("[X] "))
    {
        (Some(true), l)
    } else {
        (None, label)
    };
    let label = label.trim();
    if label.is_empty() {
        return None;
    }
    Some(MenuRow {
        cursor,
        number,
        checkbox,
        label: label.to_string(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuLegend {
    /// `↑/↓ Navigate · enter Select · esc Skip`
    Single,
    /// `↑/↓ Navigate · space Toggle · enter Submit · esc Skip`
    Multi,
}

fn parse_menu_legend(line: &str) -> Option<MenuLegend> {
    let t = line.trim();
    if !(t.contains("Navigate") && t.contains("esc Skip")) {
        return None;
    }
    if t.contains("space Toggle") && t.contains("enter Submit") {
        Some(MenuLegend::Multi)
    } else if t.contains("enter Select") {
        Some(MenuLegend::Single)
    } else {
        None
    }
}

fn is_write_in_legend(line: &str) -> bool {
    let t = line.trim();
    t.contains("enter Submit") && t.contains("esc Back") && !t.contains("Navigate")
}

/// Parse the `Question <i>/<n>: <text>` form header.
fn parse_question_header(line: &str) -> Option<(QuestionForm, String)> {
    let rest = line.trim().strip_prefix("Question ")?;
    let (counter, text) = rest.split_once(':')?;
    let (i, n) = counter.trim().split_once('/')?;
    let index: u32 = i.trim().parse().ok()?;
    let total: u32 = n.trim().parse().ok()?;
    if index == 0 || total == 0 || index > total {
        return None;
    }
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some((QuestionForm { index, total }, text.to_string()))
}

/// Parsed option block anchored at a legend row: rows in screen order plus the row index
/// (in `lines`) of the first option row.
struct MenuBlock {
    rows: Vec<MenuRow>,
    first_row_line: usize,
}

/// Walk upward from `legend_line` collecting the contiguous numbered rows that end at the
/// legend. Numbering must run `n, n-1, ..., 1` (this rejects stale redraw residue above the
/// live block) and exactly one row must carry the cursor.
fn collect_menu_block(lines: &[String], legend_line: usize) -> Option<MenuBlock> {
    let mut rows_rev = Vec::new();
    let mut r = legend_line;
    let mut expected: Option<usize> = None;
    let mut leading_blanks = 0;
    while r > 0 {
        r -= 1;
        let row = match parse_menu_row(&lines[r]) {
            Some(row) => row,
            // Up to two blank spacer lines may sit between the block and its legend.
            None if rows_rev.is_empty() && lines[r].trim().is_empty() && leading_blanks < 2 => {
                leading_blanks += 1;
                continue;
            }
            None => break,
        };
        match expected {
            None => expected = Some(row.number),
            Some(e) if row.number == e => {}
            Some(_) => break,
        }
        rows_rev.push(row);
        expected = Some(expected.unwrap() - 1);
        if expected == Some(0) {
            break;
        }
    }
    if expected != Some(0) {
        return None;
    }
    let first_row_line = r;
    let rows: Vec<MenuRow> = rows_rev.into_iter().rev().collect();
    if rows.len() < 2 || rows.iter().filter(|x| x.cursor).count() != 1 {
        return None;
    }
    // A `✓` mark is the post-submit confirmation frame: the answer has been taken, the menu is
    // no longer live.
    if rows.iter().any(|x| x.label.starts_with(ANSWERED_MARK)) {
        return None;
    }
    Some(MenuBlock {
        rows,
        first_row_line,
    })
}

/// Locate the `Question i/n:` header above the option block. Stale option rows left behind by
/// in-place redraws are skipped; up to two blank lines and up to two wrapped continuation lines
/// of the question text are tolerated.
fn find_form_header(lines: &[String], first_row_line: usize) -> Option<(QuestionForm, String)> {
    let mut continuation: Vec<String> = Vec::new();
    let mut blanks = 0;
    let mut r = first_row_line;
    while r > 0 {
        r -= 1;
        let t = lines[r].trim();
        if t.is_empty() {
            blanks += 1;
            if blanks > 2 {
                return None;
            }
            continue;
        }
        if let Some((form, text)) = parse_question_header(t) {
            let mut question = text;
            for c in continuation.iter().rev() {
                question.push(' ');
                question.push_str(c);
            }
            return Some((form, question));
        }
        if parse_menu_row(&lines[r]).is_some() {
            continue;
        }
        if continuation.len() >= 2 {
            return None;
        }
        continuation.push(t.to_string());
    }
    None
}

/// Spinner glyphs and progress labels Antigravity renders while the model is streaming.
const SPINNER_CHARS: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const BRAILLE_SPINNER_RANGE: std::ops::RangeInclusive<char> = '\u{2800}'..='\u{28FF}';

fn is_progress_line(line: &str) -> bool {
    line.chars()
        .next()
        .is_some_and(|c| SPINNER_CHARS.contains(&c) || BRAILLE_SPINNER_RANGE.contains(&c))
        || line.contains("Generating...")
        || line.contains("Working...")
}

/// A live dialog is the bottom-most region of the TUI: the first non-blank line below its
/// footer must be Antigravity's own `esc to cancel` status bar, and no progress/spinner line
/// may follow. Text merely *printed* by the agent or a tool (transcript area) is always
/// followed by the live input box / spinner / status bar and therefore fails this check.
fn footer_is_bottom_live_region(lines: &[String], footer_line: usize) -> bool {
    let mut saw_status_bar = false;
    for line in &lines[footer_line + 1..] {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if is_progress_line(t) {
            return false;
        }
        if !saw_status_bar {
            if !t.starts_with("esc to cancel") {
                return false;
            }
            saw_status_bar = true;
        }
    }
    saw_status_bar
}

fn build_menu_details(legend: MenuLegend, rows: &[MenuRow]) -> Option<QuestionDetails> {
    let last = rows.len() - 1;
    let write_in_index =
        (rows[last].label == WRITE_IN_LABEL && rows[last].checkbox.is_none()).then_some(last);
    let options: Vec<String> = rows.iter().map(|r| r.label.clone()).collect();
    let cursor = rows.iter().position(|r| r.cursor)?;
    match legend {
        MenuLegend::Single => {
            if rows.iter().any(|r| r.checkbox.is_some()) {
                return None;
            }
            Some(QuestionDetails::SingleChoice {
                options,
                selected_index: cursor,
                write_in_index,
            })
        }
        MenuLegend::Multi => {
            let mut checked = Vec::new();
            for (i, r) in rows.iter().enumerate() {
                match r.checkbox {
                    Some(true) => checked.push(i),
                    Some(false) => {}
                    None if Some(i) == write_in_index => {}
                    None => return None,
                }
            }
            Some(QuestionDetails::MultipleChoice {
                options,
                checked_indices: checked,
                cursor_index: cursor,
                write_in_index,
            })
        }
    }
}

/// Parse the Antigravity `ask_question` form (numbered menu + key legend + form header).
fn extract_agy_question_form(snapshot: &ScreenSnapshot) -> Option<QuestionDialog> {
    let lines = &snapshot.lines;
    // Bottom-up: the lowest footer on screen is the live control.
    let mut footer = None;
    for r in (0..lines.len()).rev() {
        if is_write_in_legend(&lines[r]) {
            footer = Some((r, None));
            break;
        }
        if let Some(legend) = parse_menu_legend(&lines[r]) {
            footer = Some((r, Some(legend)));
            break;
        }
    }
    let (footer_line, legend) = footer?;
    if !footer_is_bottom_live_region(lines, footer_line) {
        return None;
    }

    // Write-in text entry: `Your answer:` label above the footer, and the option block (cursor
    // on `Write-in...`) with its form header directly above that.
    if legend.is_none() {
        // Walk up from the footer: blank spacers and at most two lines of typed text, then
        // the `Your answer:` label must appear within six rows.
        let mut answer_label = None;
        let mut text_lines: Vec<&str> = Vec::new();
        for r in (footer_line.saturating_sub(6)..footer_line).rev() {
            let t = lines[r].trim();
            if t == "Your answer:" {
                answer_label = Some(r);
                break;
            }
            if t.is_empty() {
                continue;
            }
            if text_lines.len() >= 2 || parse_menu_legend(t).is_some() {
                return None;
            }
            text_lines.push(t);
        }
        let answer_label = answer_label?;
        text_lines.reverse();
        let current_text = text_lines.join(" ");
        // Directly above the label (blank spacers allowed) sits the option block with the
        // cursor parked on its `Write-in...` row; the block's own legend is no longer shown.
        let block = collect_menu_block(lines, answer_label)?;
        let cursor_row = block.rows.iter().find(|r| r.cursor)?;
        if cursor_row.label != WRITE_IN_LABEL || cursor_row.checkbox.is_some() {
            return None;
        }
        let (form, question) = find_form_header(lines, block.first_row_line)?;
        return Some(QuestionDialog {
            question,
            form: Some(form),
            details: QuestionDetails::FreeText { current_text },
        });
    }

    let block = collect_menu_block(lines, footer_line)?;
    let (form, question) = find_form_header(lines, block.first_row_line)?;
    let details = build_menu_details(legend?, &block.rows)?;
    Some(QuestionDialog {
        question,
        form: Some(form),
        details,
    })
}

/// Detect an interactive question dialog. Evidence of an actual TUI control is required:
/// the Antigravity form header + numbered menu (exactly one cursor row, numbering `1..n`) +
/// key legend (or its write-in text entry), positioned as the bottom live region above the
/// `esc to cancel` status bar. Question-like prose alone is never sufficient.
pub fn extract_question_dialog(snapshot: &ScreenSnapshot) -> Option<QuestionDialog> {
    extract_agy_question_form(snapshot)
}

pub fn classify_command_operation(cmd: &str) -> Operation {
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
        || c.starts_with("cargo add")
        || c.starts_with("pip install")
        || c.starts_with("apt")
    {
        Operation::Install
    } else if c.starts_with("git")
        || c.starts_with("ls")
        || c.starts_with("cat")
        || c.starts_with("grep")
        || c.starts_with("find")
    {
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
            cursor_row: 5,
            cursor_col: 0,
            in_alt_screen: true,
            cursor_visible: true,
            lines: vec![
                "Command: cargo test --workspace".to_string(),
                "Yes, run command".to_string(),
                "Yes, always allow".to_string(),
                "No, deny".to_string(),
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
                assert_eq!(options.len(), 3);
            }
            other => panic!("Expected CommandConfirmation, got {:?}", other),
        }
    }

    #[test]
    fn test_state_machine_emits_request_event() {
        let mut sm = AntigravityStateMachine::new("test-agent");
        let snapshot = ScreenSnapshot {
            cols: 80,
            rows: 24,
            cursor_row: 5,
            cursor_col: 0,
            in_alt_screen: true,
            cursor_visible: true,
            lines: vec![
                "Command: cargo test --workspace".to_string(),
                "Yes, run command".to_string(),
                "No, deny".to_string(),
            ],
            reversed_lines: vec![(1, "Yes, run command".to_string())],
        };

        let event = sm.update(&snapshot).expect("Must emit event on transition");
        assert_eq!(event.kind, "command_confirmation");
        assert_eq!(event.operation, Operation::Test);
        assert!(event.request.is_some());
    }
}
