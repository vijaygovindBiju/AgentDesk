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
            QuestionDetails::SingleChoice { options, .. } => {
                if Self::is_yes_no_options(options) {
                    QuestionType::YesNo
                } else {
                    QuestionType::SingleChoice
                }
            }
            QuestionDetails::MultipleChoice { .. } => QuestionType::MultipleChoice,
            QuestionDetails::FreeText { .. } => QuestionType::FreeText,
        }
    }

    fn is_yes_no_options(options: &[String]) -> bool {
        if options.len() != 2 {
            return false;
        }
        let normalized: Vec<String> = options
            .iter()
            .map(|option| option.trim().to_ascii_lowercase())
            .collect();
        matches!(
            (normalized[0].as_str(), normalized[1].as_str()),
            ("yes", "no")
                | ("no", "yes")
                | ("y", "n")
                | ("n", "y")
                | ("yes, continue", "no, cancel")
                | ("no, cancel", "yes, continue")
        )
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
    /// Waiting for workspace trust confirmation. `question` and `options` are the exact
    /// text Antigravity renders; `selected_index` is the live cursor row.
    WorkspaceTrust {
        directory: String,
        question: String,
        options: Vec<String>,
        selected_index: usize,
    },
    /// Idle prompt at the bottom input bar waiting for the user's next request.
    IdlePrompt,
    /// Antigravity finished its turn with a reply and focused the chat input: it is waiting
    /// for arbitrary text from the user (e.g. a question asked in plain chat rather than via
    /// an `ask_question` menu). `message` is the agent's exact reply text.
    AwaitingReply { message: String },
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
                | AntigravityState::AwaitingReply { .. }
        )
    }

    /// True when the chat input is focused, i.e. a new user message can be typed.
    pub fn accepts_chat_input(&self) -> bool {
        matches!(
            self,
            AntigravityState::IdlePrompt | AntigravityState::AwaitingReply { .. }
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
                AntigravityState::WorkspaceTrust {
                    directory: a,
                    question: qa,
                    options: oa,
                    ..
                },
                AntigravityState::WorkspaceTrust {
                    directory: b,
                    question: qb,
                    options: ob,
                    ..
                },
            ) => a == b && qa == qb && oa == ob,
            (
                AntigravityState::AwaitingReply { message: a },
                AntigravityState::AwaitingReply { message: b },
            ) => a == b,
            _ => false,
        }
    }
}

/// Question type for a workspace-trust menu: `YesNo` only when the rows are literally yes/no,
/// otherwise a single choice over Antigravity's exact option labels.
pub fn workspace_trust_question_type(options: &[String]) -> QuestionType {
    QuestionDetails::SingleChoice {
        options: options.to_vec(),
        selected_index: 0,
        write_in_index: None,
    }
    .question_type()
}

/// Structured request mirroring Antigravity's real trust dialog (exact prompt and options).
pub fn workspace_trust_request(question: &str, options: &[String]) -> RequestInfo {
    RequestInfo {
        prompt: question.to_string(),
        options: options.to_vec(),
        question_type: Some(workspace_trust_question_type(options)),
    }
}

pub fn workspace_trust_details_map(
    directory: &str,
    question: &str,
    options: &[String],
    selected_index: usize,
) -> BTreeMap<String, serde_json::Value> {
    let mut d = BTreeMap::new();
    d.insert("directory".to_string(), serde_json::Value::String(directory.to_string()));
    d.insert(
        "question_type".to_string(),
        serde_json::to_value(workspace_trust_question_type(options)).unwrap_or(serde_json::Value::Null),
    );
    d.insert("question".to_string(), serde_json::Value::String(question.to_string()));
    d.insert("options".to_string(), serde_json::json!(options));
    d.insert(
        "selected_options".to_string(),
        serde_json::json!(options.get(selected_index).into_iter().collect::<Vec<_>>()),
    );
    d.insert("allows_write_in".to_string(), serde_json::Value::Bool(false));
    d.insert("interaction".to_string(), serde_json::Value::String("workspace_trust".into()));
    d
}

/// Prompt shown for an Antigravity command-permission dialog (agy renders `Run this command?`
/// followed by the command).
pub fn command_approval_prompt(command: &str) -> String {
    format!("Run this command?\n{command}")
}

pub fn file_edit_approval_prompt(file_path: &str) -> String {
    format!("Allow edit to {file_path}?")
}

/// Structured request for a cursor-driven approval menu: the exact option rows agy renders
/// (any number of them), answered by selecting exactly one.
pub fn approval_menu_request(prompt: String, options: &[String]) -> RequestInfo {
    RequestInfo {
        prompt,
        options: options.to_vec(),
        question_type: Some(QuestionType::SingleChoice),
    }
}

pub fn approval_menu_details_map(
    interaction: &str,
    subject_key: &str,
    subject: &str,
    options: &[String],
    selected_index: usize,
) -> BTreeMap<String, serde_json::Value> {
    let mut d = BTreeMap::new();
    d.insert(subject_key.to_string(), serde_json::Value::String(subject.to_string()));
    d.insert(
        "question_type".to_string(),
        serde_json::to_value(QuestionType::SingleChoice).unwrap_or(serde_json::Value::Null),
    );
    d.insert("options".to_string(), serde_json::json!(options));
    d.insert("selected_option".to_string(), serde_json::json!(selected_index));
    d.insert(
        "selected_options".to_string(),
        serde_json::json!(options.get(selected_index).into_iter().collect::<Vec<_>>()),
    );
    d.insert("allows_write_in".to_string(), serde_json::Value::Bool(false));
    d.insert("interaction".to_string(), serde_json::Value::String(interaction.into()));
    d
}

/// Structured free-form request for an agent reply awaiting the user's text.
pub fn agent_reply_request(message: &str) -> RequestInfo {
    RequestInfo {
        prompt: message.to_string(),
        options: Vec::new(),
        question_type: Some(QuestionType::FreeText),
    }
}

pub fn agent_reply_details_map(message: &str) -> BTreeMap<String, serde_json::Value> {
    let mut d = BTreeMap::new();
    d.insert(
        "question_type".to_string(),
        serde_json::to_value(QuestionType::FreeText).unwrap_or(serde_json::Value::Null),
    );
    d.insert("question".to_string(), serde_json::Value::String(message.to_string()));
    d.insert("options".to_string(), serde_json::json!([] as [String; 0]));
    d.insert("allows_write_in".to_string(), serde_json::Value::Bool(false));
    d.insert("interaction".to_string(), serde_json::Value::String("agent_reply".into()));
    d
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

/// Format prompt string into standard log lines for timeline and terminal stream.
pub fn format_prompt_log_lines(prompt: &str) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("> Prompt: {prompt}"));
    for line in prompt.lines() {
        lines.push(format!("User: {line}"));
    }
    lines
}

/// State machine tracking Antigravity session transitions.
pub struct AntigravityStateMachine {
    agent_id: String,
    agent_seq: u64,
    current_state: AntigravityState,
    confirmed_state_count: usize,
    active_prompt: Option<String>,
    prompt_start_time: Option<std::time::Instant>,
}

impl AntigravityStateMachine {
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            agent_seq: 0,
            current_state: AntigravityState::Initializing,
            confirmed_state_count: 0,
            active_prompt: None,
            prompt_start_time: None,
        }
    }

    pub fn current_state(&self) -> &AntigravityState {
        &self.current_state
    }

    pub fn active_prompt(&self) -> Option<&str> {
        self.active_prompt.as_deref()
    }

    pub fn start_prompt(&mut self, prompt: &str) -> Option<RawAgentEvent> {
        self.active_prompt = Some(prompt.to_string());
        self.prompt_start_time = Some(std::time::Instant::now());
        self.current_state = AntigravityState::Working {
            details: format!("Executing prompt: {prompt}"),
        };
        let mut event = self.state_to_event(&self.current_state.clone());
        if let Some(ref mut ev) = event {
            ev.log_lines = format_prompt_log_lines(prompt);
            ev.log_lines.push(format!("Agent: Executing prompt: {prompt}"));
        }
        event
    }

    /// Update with a new screen snapshot and return a `RawAgentEvent` if an actionable
    /// state transition occurred.
    pub fn update(&mut self, snapshot: &ScreenSnapshot) -> Option<RawAgentEvent> {
        let is_recent_prompt = self
            .prompt_start_time
            .map(|t| t.elapsed() < std::time::Duration::from_millis(1500))
            .unwrap_or(false);

        let detected = detect_state(snapshot);

        // If no active prompt was tracked via start_prompt, try extracting user prompt from screen
        if self.active_prompt.is_none() {
            if let Some(p) = extract_user_prompt_from_snapshot(snapshot) {
                self.active_prompt = Some(p);
            }
        }

        if detected != self.current_state {
            if self.current_state.same_request(&detected) {
                self.current_state = detected;
                return None;
            }
            let prev_state = std::mem::replace(&mut self.current_state, detected.clone());
            self.confirmed_state_count = 1;
            let had_active_prompt = self.active_prompt.is_some();
            let was_working = matches!(prev_state, AntigravityState::Working { .. });
            if (was_working
                || matches!(
                    prev_state,
                    AntigravityState::CommandConfirmation { .. }
                        | AntigravityState::FileEditConfirmation { .. }
                        | AntigravityState::WorkspaceTrust { .. }
                )
                || had_active_prompt)
                && matches!(detected, AntigravityState::IdlePrompt)
            {
                if let Some(summary) = extract_work_summary(snapshot) {
                    if has_positive_completion_evidence(&summary, had_active_prompt || was_working) {
                        self.current_state = AntigravityState::Completed {
                            message: summary,
                        };
                        let ev = self.state_to_event(&self.current_state.clone());
                        self.active_prompt = None;
                        self.prompt_start_time = None;
                        return ev;
                    }
                }
                // If there is no completion evidence yet and we are within the initial launch grace period,
                // keep the working state and do not revert to IdlePrompt prematurely.
                if is_recent_prompt && had_active_prompt {
                    self.current_state = prev_state;
                    return None;
                }

                self.active_prompt = None;
                self.prompt_start_time = None;
                self.current_state = AntigravityState::IdlePrompt;
                return self.state_to_event(&self.current_state.clone());
            }
            if matches!(detected, AntigravityState::Completed { .. }) {
                let ev = self.state_to_event(&detected);
                self.active_prompt = None;
                self.prompt_start_time = None;
                return ev;
            }
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
            AntigravityState::CommandConfirmation {
                command,
                selected_option_index,
                options,
            } => {
                let op = classify_command_operation(command);
                let seq = self.next_seq();
                let mut log_lines = self.active_prompt.as_deref().map(format_prompt_log_lines).unwrap_or_default();
                log_lines.push(format!("Command: {command}"));
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "approval_required".to_string(),
                    operation: op,
                    message: format!("Command requires approval: {command}"),
                    details: approval_menu_details_map(
                        "command_approval",
                        "command",
                        command,
                        options,
                        *selected_option_index,
                    ),
                    log_lines,
                    request: Some(approval_menu_request(command_approval_prompt(command), options)),
                })
            }
            AntigravityState::FileEditConfirmation {
                file_path,
                selected_option_index,
                options,
            } => {
                let seq = self.next_seq();
                let mut log_lines = self.active_prompt.as_deref().map(format_prompt_log_lines).unwrap_or_default();
                log_lines.push(format!("File: {file_path}"));
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "approval_required".to_string(),
                    operation: Operation::Edit,
                    message: format!("File edit requires approval: {file_path}"),
                    details: approval_menu_details_map(
                        "file_edit_approval",
                        "file",
                        file_path,
                        options,
                        *selected_option_index,
                    ),
                    log_lines,
                    request: Some(approval_menu_request(file_edit_approval_prompt(file_path), options)),
                })
            }
            AntigravityState::WorkspaceTrust {
                directory,
                question,
                options,
                selected_index,
            } => {
                let seq = self.next_seq();
                let mut log_lines = self.active_prompt.as_deref().map(format_prompt_log_lines).unwrap_or_default();
                log_lines.push(format!("Trust requested for directory {directory}"));
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "approval_required".to_string(),
                    operation: Operation::Other,
                    message: format!("Workspace trust required for: {directory}"),
                    details: workspace_trust_details_map(directory, question, options, *selected_index),
                    log_lines,
                    request: Some(workspace_trust_request(question, options)),
                })
            }
            AntigravityState::UserQuestion {
                question,
                form,
                details,
            } => {
                let seq = self.next_seq();
                let mut log_lines = self.active_prompt.as_deref().map(format_prompt_log_lines).unwrap_or_default();
                log_lines.push(question.clone());
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "input_required".to_string(),
                    operation: Operation::Other,
                    message: question.clone(),
                    details: question_details_map(question, *form, details),
                    log_lines,
                    request: Some(RequestInfo {
                        prompt: question.clone(),
                        options: details.answer_options(),
                        question_type: Some(details.question_type()),
                    }),
                })
            }
            AntigravityState::Working { details } => {
                let seq = self.next_seq();
                let mut log_lines = self.active_prompt.as_deref().map(format_prompt_log_lines).unwrap_or_default();
                log_lines.push(format!("Agent: {details}"));
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "progress".to_string(),
                    operation: Operation::Other,
                    message: details.clone(),
                    details: BTreeMap::new(),
                    log_lines,
                    request: None,
                })
            }
            AntigravityState::Completed { message } => {
                let seq = self.next_seq();
                let mut details = BTreeMap::new();
                details.insert(
                    "status".to_string(),
                    serde_json::Value::String("completed".to_string()),
                );
                details.insert(
                    "description".to_string(),
                    serde_json::Value::String(message.clone()),
                );
                details.insert(
                    "summary".to_string(),
                    serde_json::Value::String(message.clone()),
                );
                if let Some(ref p) = self.active_prompt {
                    details.insert("prompt".to_string(), serde_json::Value::String(p.clone()));
                }
                if let Some(final_res) = extract_final_result(message) {
                    details.insert(
                        "final_result".to_string(),
                        serde_json::Value::String(final_res),
                    );
                }
                if let Some(file) = extract_file_path_from_summary(message) {
                    details.insert("file".to_string(), serde_json::Value::String(file));
                }
                if let Some(cmd) = extract_command_from_summary(message) {
                    details.insert("command".to_string(), serde_json::Value::String(cmd));
                }
                let tool_count = message
                    .lines()
                    .filter(|l| l.trim().starts_with("● "))
                    .count();
                if tool_count > 0 {
                    details.insert("tool_count".to_string(), serde_json::json!(tool_count));
                }
                let steps = parse_execution_steps(message);
                if !steps.is_empty() {
                    if let Ok(val) = serde_json::to_value(&steps) {
                        details.insert("execution_steps".to_string(), val);
                    }
                }
                let mut log_lines = self.active_prompt.as_deref().map(format_prompt_log_lines).unwrap_or_default();
                log_lines.extend(message.lines().map(|s| s.to_string()));
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "task_completed".to_string(),
                    operation: Operation::Other,
                    message: message.clone(),
                    details,
                    log_lines,
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
            AntigravityState::AwaitingReply { message } => {
                let seq = self.next_seq();
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "input_required".to_string(),
                    operation: Operation::Other,
                    message: message.clone(),
                    details: agent_reply_details_map(message),
                    log_lines: vec![message.clone()],
                    request: Some(agent_reply_request(message)),
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
        let task_id = Some(format!("{}-session", self.agent_id));
        match state {
            AntigravityState::CommandConfirmation {
                command,
                selected_option_index,
                options,
            } => {
                let op = classify_command_operation(command);
                let seq = self.next_seq();
                let mut log_lines = self.active_prompt.as_deref().map(format_prompt_log_lines).unwrap_or_default();
                log_lines.push(format!("Command: {command}"));
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "command_confirmation".to_string(),
                    operation: op,
                    message: format!("Command requires approval: {command}"),
                    details: approval_menu_details_map(
                        "command_approval",
                        "command",
                        command,
                        options,
                        *selected_option_index,
                    ),
                    log_lines,
                    request: Some(approval_menu_request(command_approval_prompt(command), options)),
                })
            }
            AntigravityState::FileEditConfirmation {
                file_path,
                selected_option_index,
                options,
            } => {
                let seq = self.next_seq();
                let mut log_lines = self.active_prompt.as_deref().map(format_prompt_log_lines).unwrap_or_default();
                log_lines.push(format!("File: {file_path}"));
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "file_edit_confirmation".to_string(),
                    operation: Operation::Edit,
                    message: format!("File edit requires approval: {file_path}"),
                    details: approval_menu_details_map(
                        "file_edit_approval",
                        "file",
                        file_path,
                        options,
                        *selected_option_index,
                    ),
                    log_lines,
                    request: Some(approval_menu_request(file_edit_approval_prompt(file_path), options)),
                })
            }
            AntigravityState::WorkspaceTrust {
                directory,
                question,
                options,
                selected_index,
            } => {
                let seq = self.next_seq();
                let mut log_lines = self.active_prompt.as_deref().map(format_prompt_log_lines).unwrap_or_default();
                log_lines.push(format!("Trust requested for directory {directory}"));
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "workspace_trust".to_string(),
                    operation: Operation::Other,
                    message: format!("Workspace trust required for: {directory}"),
                    details: workspace_trust_details_map(directory, question, options, *selected_index),
                    log_lines,
                    request: Some(workspace_trust_request(question, options)),
                })
            }
            AntigravityState::UserQuestion {
                question,
                form,
                details,
            } => {
                let seq = self.next_seq();
                let mut log_lines = self.active_prompt.as_deref().map(format_prompt_log_lines).unwrap_or_default();
                log_lines.push(question.clone());
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "question".to_string(),
                    operation: Operation::Other,
                    message: question.clone(),
                    details: question_details_map(question, *form, details),
                    log_lines,
                    request: Some(RequestInfo {
                        prompt: question.clone(),
                        options: details.answer_options(),
                        question_type: Some(details.question_type()),
                    }),
                })
            }
            AntigravityState::Working { details } => {
                let seq = self.next_seq();
                let mut log_lines = self.active_prompt.as_deref().map(format_prompt_log_lines).unwrap_or_default();
                log_lines.push(format!("Agent: {details}"));
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "progress".to_string(),
                    operation: Operation::Other,
                    message: details.clone(),
                    details: BTreeMap::new(),
                    log_lines,
                    request: None,
                })
            }
            AntigravityState::Completed { message } => {
                let seq = self.next_seq();
                let mut details = BTreeMap::new();
                details.insert(
                    "status".to_string(),
                    serde_json::Value::String("completed".to_string()),
                );
                details.insert(
                    "description".to_string(),
                    serde_json::Value::String(message.clone()),
                );
                details.insert(
                    "summary".to_string(),
                    serde_json::Value::String(message.clone()),
                );
                if let Some(ref p) = self.active_prompt {
                    details.insert("prompt".to_string(), serde_json::Value::String(p.clone()));
                }
                if let Some(final_res) = extract_final_result(message) {
                    details.insert(
                        "final_result".to_string(),
                        serde_json::Value::String(final_res),
                    );
                }
                if let Some(file) = extract_file_path_from_summary(message) {
                    details.insert("file".to_string(), serde_json::Value::String(file));
                }
                if let Some(cmd) = extract_command_from_summary(message) {
                    details.insert("command".to_string(), serde_json::Value::String(cmd));
                }
                let tool_count = message
                    .lines()
                    .filter(|l| l.trim().starts_with("● "))
                    .count();
                if tool_count > 0 {
                    details.insert("tool_count".to_string(), serde_json::json!(tool_count));
                }
                let steps = parse_execution_steps(message);
                if !steps.is_empty() {
                    if let Ok(val) = serde_json::to_value(&steps) {
                        details.insert("execution_steps".to_string(), val);
                    }
                }
                let mut log_lines = self.active_prompt.as_deref().map(format_prompt_log_lines).unwrap_or_default();
                log_lines.extend(message.lines().map(|s| s.to_string()));
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "task_completed".to_string(),
                    operation: Operation::Other,
                    message: message.clone(),
                    details,
                    log_lines,
                    request: None,
                })
            }
            AntigravityState::FatalError { error } => {
                let seq = self.next_seq();
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "error".to_string(),
                    operation: Operation::Other,
                    message: error.clone(),
                    details: BTreeMap::new(),
                    log_lines: vec![error.clone()],
                    request: None,
                })
            }
            AntigravityState::AwaitingReply { message } => {
                let seq = self.next_seq();
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
                    kind: "input_required".to_string(),
                    operation: Operation::Other,
                    message: message.clone(),
                    details: agent_reply_details_map(message),
                    log_lines: vec![message.clone()],
                    request: Some(agent_reply_request(message)),
                })
            }
            AntigravityState::IdlePrompt => {
                let seq = self.next_seq();
                Some(RawAgentEvent {
                    agent_id: self.agent_id.clone(),
                    agent_seq: seq,
                    task_id,
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

/// Returns true if the line matches a standard shell prompt (fish, bash, zsh, starship, bobthefish).
pub fn is_shell_prompt_line(line: &str) -> bool {
    let t = line.trim();
    if is_menu_row(t) {
        return false;
    }
    t.starts_with("╰─❯")
        || t.starts_with("╭─")
        || t.starts_with("❯")
        || t.ends_with("╰─❯")
        || t.ends_with("❯")
        || t == "$"
        || t == "%"
        || t == "#"
        || t.starts_with("$ ")
        || t.starts_with("% ")
        || t.starts_with("# ")
}

/// Detect the semantic state from a screen snapshot.
pub fn detect_state(snapshot: &ScreenSnapshot) -> AntigravityState {
    let non_empty_lines: Vec<&String> = snapshot.lines.iter().filter(|l| !l.is_empty()).collect();

    // 1. Workspace Trust prompt
    if let Some(trust) = extract_workspace_trust(snapshot) {
        return trust;
    }

    // 2. Interactive File Edit / Creation Confirmation Menu
    if let Some((file_path, options, selected)) = extract_file_edit_dialog(snapshot) {
        return AntigravityState::FileEditConfirmation {
            file_path,
            selected_option_index: selected,
            options,
        };
    }

    // 3. Interactive Command / Tool Confirmation Menu
    if let Some((command, options, selected)) = extract_command_dialog(snapshot) {
        return AntigravityState::CommandConfirmation {
            command,
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
    // Any braille-pattern glyph (U+2800..=U+28FF) is a spinner frame; it is stripped so that
    // successive animation frames map to the same Working state instead of new events.
    // The active spinner is displayed at the bottom of the terminal output (last 6 lines).
    let is_spinner = |c: char| ('\u{2800}'..='\u{28FF}').contains(&c);
    let working_candidates: Vec<&&String> = non_empty_lines.iter().rev().take(6).collect();
    for line in working_candidates {
        if line.chars().any(is_spinner)
            || line.contains("Thinking...")
            || line.contains("Working...")
            || line.contains("Searching...")
            || line.contains("Generating...")
        {
            return AntigravityState::Working {
                details: line
                    .trim()
                    .trim_start_matches(is_spinner)
                    .trim()
                    .to_string(),
            };
        }
    }

    // 7. Idle Prompt
    let is_chat_prompt_line = |line: &str| {
        let t = line.trim();
        if is_menu_row(t) {
            return false;
        }
        t.starts_with("> ")
            || t == ">"
            || t.starts_with("? ")
            || t == "?"
            || t.contains("Send message")
            || is_shell_prompt_line(t)
    };

    let last_rule_idx = snapshot.lines.iter().rposition(|l| is_rule_line(l));

    let has_active_menu_footer = snapshot.lines.iter().enumerate().any(|(idx, l)| {
        let t = l.trim();
        let is_footer = (t.contains("Navigate") && t.contains("esc to cancel"))
            || (t.contains("tab Amend") && t.contains("Navigate"));
        if !is_footer {
            return false;
        }
        if let Some(r) = last_rule_idx {
            idx >= r
        } else {
            non_empty_lines.iter().rev().take(6).any(|nl| nl.trim() == t)
        }
    });

    let has_active_permission_header = snapshot.lines.iter().enumerate().any(|(idx, l)| {
        let t = l.trim();
        let is_header = t.starts_with("Requesting permission for:")
            || t.starts_with("Run this command?")
            || t.starts_with("Allow creation of this file?")
            || t.starts_with("Allow editing this file?")
            || t.starts_with("Allow access to this file?")
            || t.starts_with("Allow calling this tool?")
            || t.starts_with("Allow access to this URL?")
            || t == "Create file"
            || t == "Edit file";
        if !is_header {
            return false;
        }
        if let Some(r) = last_rule_idx {
            idx >= r
        } else {
            non_empty_lines.iter().rev().take(6).any(|nl| nl.trim() == t)
        }
    });

    let is_idle = !has_active_menu_footer && !has_active_permission_header && (
        non_empty_lines.iter().rev().take(4).any(|line| is_chat_prompt_line(line))
        || (snapshot.cursor_row > 0
            && snapshot.cursor_visible
            && usize::from(snapshot.cursor_row) < snapshot.lines.len()
            && {
                let cur = snapshot.lines[usize::from(snapshot.cursor_row)].trim();
                !is_menu_row(cur) && (cur.starts_with("> ") || cur == ">" || cur.starts_with("╰─❯") || cur.starts_with("❯") || cur.is_empty())
            })
    );

    if is_idle {
        if let Some(message) = extract_agent_reply(snapshot) {
            if message.trim().ends_with('?') && !is_idle_welcome_text(&message) {
                return AntigravityState::AwaitingReply { message };
            }
        }
        return AntigravityState::IdlePrompt;
    }

    AntigravityState::Initializing
}

/// ANSI colour indices Antigravity uses for transcript roles (see `ScreenSnapshot::line_fg`).
const FG_USER_PROMPT: u8 = 4; // `ESC[34;1m> ...`

fn is_menu_row(line: &str) -> bool {
    let t = line.trim();
    if let Some(rest) = t
        .strip_prefix('>')
        .or_else(|| t.strip_prefix('❯'))
        .or_else(|| t.strip_prefix('›'))
    {
        let rest = rest.trim_start();
        rest.starts_with("Yes,")
            || rest.starts_with("No,")
            || rest.starts_with("Always")
            || rest.starts_with("Never")
            || rest.starts_with("Allow")
            || rest.starts_with("Deny")
            || rest.starts_with("Trust")
            || rest.starts_with("Don't")
            || rest.starts_with("Review in")
            || rest.starts_with("1.")
            || rest.starts_with("2.")
            || rest.starts_with("3.")
            || rest.starts_with("4.")
            || rest.starts_with("[1]")
            || rest.starts_with("[2]")
            || rest.starts_with("[x]")
            || rest.starts_with("[ ]")
            || rest.starts_with('●')
    } else {
        false
    }
}

/// Extract file path mentioned in a work summary or file creation screen.
pub fn extract_file_path_from_summary(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(start) = trimmed.find("● Write(") {
            let after = &trimmed[start + "● Write(".len()..];
            if let Some(end) = after.find(')') {
                let path = after[..end].trim();
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }
        if let Some(start) = trimmed.find("● Edit(") {
            let after = &trimmed[start + "● Edit(".len()..];
            if let Some(end) = after.find(')') {
                let path = after[..end].trim();
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }
        if let Some(start) = trimmed.find("● Create(") {
            let after = &trimmed[start + "● Create(".len()..];
            if let Some(end) = after.find(')') {
                let path = after[..end].trim();
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }
        if trimmed.starts_with("Allow edit to ") {
            let path = trimmed
                .trim_start_matches("Allow edit to ")
                .trim_end_matches('?')
                .trim();
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
        if trimmed.starts_with("Created file: ") || trimmed.starts_with("Created file ") {
            let path = trimmed
                .trim_start_matches("Created file: ")
                .trim_start_matches("Created file ")
                .trim();
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
        let first = trimmed.split_whitespace().next().unwrap_or("");
        if (first.starts_with('/') || first.contains('/'))
            && first.contains('.')
            && !first.starts_with('>')
            && !first.starts_with('●')
        {
            return Some(first.to_string());
        }
    }
    None
}

/// Extract command executed mentioned in a work summary.
pub fn extract_command_from_summary(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(start) = trimmed.find("● Bash(") {
            let after = &trimmed[start + "● Bash(".len()..];
            if let Some(end) = after.find(')') {
                let cmd = after[..end].trim();
                if !cmd.is_empty() {
                    return Some(cmd.to_string());
                }
            }
        }
    }
    None
}

pub fn is_rule_line(line: &str) -> bool {
    let t = line.trim();
    if t.chars().count() < 3 {
        return false;
    }
    t.chars().all(|c| {
        matches!(
            c,
            '─' | '━' | '═' | '┄' | '┅' | '┈' | '┉' | '—' | '-' | '_' | '–' | '―'
        )
    })
}

/// Returns `true` if the text matches Antigravity's idle welcome banner or prompt hint
/// (e.g. "Hello again! Ready whenever you are.", "What would you like to work on?", "> say hi").
pub fn is_idle_welcome_text(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_lowercase();
    lower.contains("ready whenever you are")
        || lower.contains("what would you like to work on")
        || lower.contains("hello again")
        || lower.contains("say hi")
        || lower.starts_with("> say hi")
        || lower == "say hi"
        || lower == ">"
        || lower == "?"
}

/// Returns `true` if the line contains terminal rendering artifacts that should be excluded
/// from semantic work summaries sent to the phone.
///
/// Covers:
/// - Horizontal rule lines and box drawings
/// - Idle welcome banners and prompt hints
/// - Kitty graphics protocol fragments (`Ga=`, base64 blobs ending in `==` with semicolons)
/// - Lines that are *entirely* braille/spinner characters (not meaningful text)
/// - Lines containing a leaked ESC (`\x1b`) byte or CSI bracket sequences
/// - Keyboard shortcuts, prompt chrome, and navigation bars
pub fn is_terminal_artifact(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    if is_rule_line(t) || is_idle_welcome_text(t) {
        return true;
    }
    // Kitty graphics protocol fragments (e.g. Ga=q,f=32,s=1,v=1,i=31;AAAAAAA==)
    if t.starts_with("Ga=") || t.contains(";AAAA") || (t.contains("==") && t.contains(';')) {
        return true;
    }
    // Terminal mode sequences or raw cursor control artifacts
    if t.starts_with("[?") || t.starts_with("\x1b[") || t.contains('\x1b') {
        return true;
    }
    // Lines starting with braille characters (spinners and status lines like "⠙ Working...")
    let first_char = t.chars().next();
    if let Some(c) = first_char {
        if matches!(
            c,
            '⣾' | '⣽' | '⣻' | '⢿' | '⡿' | '⣟' | '⣯' | '⣷' | '⠋' | '⠙' | '⠹' | '⠸'
                | '⠼' | '⠴' | '⠦' | '⠧' | '⠇' | '⠏'
        ) {
            return true;
        }
    }
    // Status bar / footer shortcuts
    if (t.contains("Navigate") && (t.contains("cancel") || t.contains("Confirm") || t.contains("esc")))
        || (t.contains("F1 Agent") && t.contains("F2 Project"))
        || (t.contains("tab Amend") && t.contains("Navigate"))
        || t.contains("ctrl+g edit/expand")
        || t.starts_with("Gemini")
        || t.starts_with("Send message")
    {
        return true;
    }
    // Prompt line artifacts
    if t == ">" || t == "?" || (t.starts_with("> ") && t.len() <= 12 && (t.contains("say hi") || t.contains("type"))) {
        return true;
    }
    false
}

/// A structured step in an agent's execution transcript.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct StepInfo {
    /// "thought", "tool_call", "message", or "result"
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Parse a multi-line semantic work summary into structured execution steps while
/// preserving strict chronological order.
pub fn parse_execution_steps(summary: &str) -> Vec<StepInfo> {
    let mut steps = Vec::new();
    let raw_lines: Vec<&str> = summary.lines().collect();
    let lines: Vec<&str> = raw_lines.iter().map(|l| l.trim()).collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        if line.is_empty() {
            i += 1;
            continue;
        }

        // 1. Thought block: starts with `▸ Thought` or `Thought for`
        if line.starts_with("▸ Thought") || line.starts_with("Thought for") {
            let header = line.strip_prefix('▸').unwrap_or(line).trim();
            let mut duration = None;
            let mut tokens = None;
            if let Some(after_for) = header.strip_prefix("Thought for ") {
                let parts: Vec<&str> = after_for.split(',').map(|s| s.trim()).collect();
                if let Some(d) = parts.first() {
                    duration = Some(d.to_string());
                }
                if parts.len() > 1 {
                    tokens = Some(parts[1].to_string());
                }
            }

            i += 1;
            let mut thought_lines = Vec::new();
            while i < lines.len() {
                let next = lines[i];
                let raw_next = raw_lines[i];
                if next.is_empty() {
                    let peek_non_empty = lines[i..].iter().find(|l| !l.is_empty());
                    if let Some(p) = peek_non_empty {
                        if p.starts_with('●')
                            || p.starts_with('▸')
                            || p.starts_with("Final result")
                            || p.starts_with("Result")
                            || p.starts_with('•')
                            || p.starts_with('#')
                            || p.starts_with("```")
                        {
                            break;
                        }
                    }
                    thought_lines.push("");
                    i += 1;
                    continue;
                }
                if next.starts_with('●')
                    || next.starts_with('▸')
                    || next.starts_with("Final result")
                    || next.starts_with("Result")
                    || next.starts_with('•')
                    || next.starts_with('#')
                    || next.starts_with("```")
                {
                    break;
                }
                // Antigravity thoughts are indented with 2+ spaces. A non-indented line indicates
                // the end of thought prose and the start of user-facing response prose.
                if !thought_lines.is_empty() && !raw_next.starts_with("  ") && !raw_next.starts_with('\t') {
                    break;
                }
                thought_lines.push(next);
                i += 1;
                // If thought line was single-line summary (indented) and next line is not indented, break
                if raw_next.starts_with("  ") && i < raw_lines.len() && !raw_lines[i].starts_with("  ") && !raw_lines[i].trim().is_empty() {
                    break;
                }
            }
            let text = thought_lines.join("\n").trim().to_string();
            steps.push(StepInfo {
                kind: "thought".to_string(),
                tool: None,
                command: None,
                duration,
                tokens,
                text: if text.is_empty() { None } else { Some(text) },
            });
            continue;
        }

        // 2. Tool invocation: starts with `● `
        if line.starts_with("● ") {
            let tool_name = line.strip_prefix("● ").unwrap().trim();
            let (tool, inline_cmd) = if let Some((t, rest)) = tool_name.split_once('(') {
                let cmd = rest.strip_suffix(')').unwrap_or(rest).trim();
                (t.trim(), Some(cmd.to_string()))
            } else {
                (tool_name, None)
            };

            i += 1;
            let mut cmd = inline_cmd;
            let mut output_lines = Vec::new();

            while i < lines.len() {
                let next = lines[i];
                if next.is_empty() {
                    i += 1;
                    continue;
                }
                if next.starts_with('●')
                    || next.starts_with('▸')
                    || next.starts_with("Final result")
                    || next.starts_with("Result")
                {
                    break;
                }
                if cmd.is_none() {
                    let clean_cmd = next
                        .strip_prefix('(')
                        .and_then(|s| s.strip_suffix(')'))
                        .unwrap_or(next)
                        .trim();
                    cmd = Some(clean_cmd.to_string());
                    i += 1;
                    continue;
                }
                if next == "(ctrl+o to expand)" || next.contains("(ctrl+o to expand)") {
                    i += 1;
                    continue;
                }
                if raw_lines[i].starts_with("  ") || raw_lines[i].starts_with('\t') {
                    output_lines.push(next);
                    i += 1;
                    continue;
                }
                let has_subsequent_steps = lines[i..].iter().any(|l| {
                    l.starts_with('●')
                        || l.starts_with('▸')
                        || l.starts_with("Final result")
                        || l.starts_with("Result")
                });
                if !has_subsequent_steps {
                    // Trailing non-indented text belongs to the final result / message, not tool output.
                    break;
                }
                output_lines.push(next);
                i += 1;
            }

            let output = if output_lines.is_empty() {
                None
            } else {
                Some(output_lines.join("\n").trim().to_string())
            };

            steps.push(StepInfo {
                kind: "tool_call".to_string(),
                tool: Some(tool.to_string()),
                command: cmd,
                duration: None,
                tokens: None,
                text: output,
            });
            continue;
        }

        // 3. Final result section
        if line.eq_ignore_ascii_case("Final result:")
            || line.eq_ignore_ascii_case("Result:")
            || line.starts_with("Final result")
        {
            i += 1;
            let mut res_lines = Vec::new();
            while i < lines.len() {
                let next = lines[i];
                if next.is_empty() {
                    if !res_lines.is_empty() && res_lines.last() != Some(&"") {
                        res_lines.push("");
                    }
                } else {
                    res_lines.push(next);
                }
                i += 1;
            }
            let res = res_lines.join("\n").trim().to_string();
            if !res.is_empty() {
                steps.push(StepInfo {
                    kind: "result".to_string(),
                    tool: None,
                    command: None,
                    duration: None,
                    tokens: None,
                    text: Some(res),
                });
            }
            continue;
        }

        // 4. General message / explanation block
        let mut msg_lines = vec![line];
        i += 1;
        while i < lines.len() {
            let next = lines[i];
            if next.starts_with('●')
                || next.starts_with('▸')
                || next.starts_with("Final result")
                || next.starts_with("Result")
            {
                break;
            }
            if next.is_empty() {
                if !msg_lines.is_empty() && msg_lines.last() != Some(&"") {
                    msg_lines.push("");
                }
            } else {
                msg_lines.push(next);
            }
            i += 1;
        }
        let msg = msg_lines.join("\n").trim().to_string();
        if !msg.is_empty() {
            let is_last = i >= lines.len();
            let kind = if is_last && steps.iter().any(|s| s.kind == "tool_call") {
                "result"
            } else {
                "message"
            };
            steps.push(StepInfo {
                kind: kind.to_string(),
                tool: None,
                command: None,
                duration: None,
                tokens: None,
                text: Some(msg),
            });
        }
    }

    steps
}

/// Extract the final non-chrome, non-empty text block from a work summary string.
///
/// Uses `parse_execution_steps` to accurately parse tool calls, thought blocks, and
/// the final user-facing result. If a "result" step is found, its complete text (including
/// all multi-paragraph markdown, bulleted lists, and code blocks) is returned.
/// Returns true if there is positive evidence of completed agent work.
///
/// Completion requires positive evidence:
/// - If an active prompt was tracked (`had_active_prompt == true`), the summary must contain
///   meaningful, non-welcome, non-artifact content.
/// - If no active prompt was tracked (e.g. user typed directly into PTY), positive evidence
///   requires tool invocations (`● Tool`), thought blocks (`▸ Thought`), or explicit result markers.
///
/// An idle welcome prompt or empty screen is NEVER positive evidence.
pub fn has_positive_completion_evidence(summary: &str, had_active_prompt: bool) -> bool {
    let trimmed = summary.trim();
    if trimmed.is_empty() || trimmed == "Task completed" || is_idle_welcome_text(trimmed) {
        return false;
    }
    if is_terminal_artifact(trimmed) {
        return false;
    }
    if had_active_prompt {
        return !is_idle_welcome_text(trimmed);
    }
    let steps = parse_execution_steps(trimmed);
    let has_tools_or_thoughts = steps.iter().any(|s| s.kind == "tool_call" || s.kind == "thought");
    let has_tool_markers = trimmed.lines().any(|l| {
        let t = l.trim();
        t.starts_with('●')
            || t.starts_with('▸')
            || t.starts_with("✓ ")
            || t.starts_with("• ")
            || t.starts_with("###")
            || t.starts_with("####")
    });
    let has_structured_content = trimmed.lines().any(|l| {
        let t = l.trim();
        t.starts_with("• ")
            || t.starts_with("###")
            || t.starts_with("####")
            || t.starts_with("```")
    });
    let has_result_or_message = steps.iter().any(|s| s.kind == "result" || s.kind == "message")
        && trimmed.lines().count() >= 2;
    has_tools_or_thoughts || has_tool_markers || has_structured_content || has_result_or_message
}

/// Extract the final non-chrome, non-empty text block from a work summary string.
///
/// Uses `parse_execution_steps` to accurately parse tool calls, thought blocks, and
/// the final user-facing result. If a "result" step is found, its complete text (including
/// all multi-paragraph markdown, bulleted lists, and code blocks) is returned.
pub fn extract_final_result(summary: &str) -> Option<String> {
    let steps = parse_execution_steps(summary);
    if let Some(res_step) = steps.iter().rev().find(|s| s.kind == "result") {
        if let Some(ref text) = res_step.text {
            let t = text.trim();
            if !t.is_empty() && !is_idle_welcome_text(t) && !is_terminal_artifact(t) {
                return Some(t.to_string());
            }
        }
    }
    // Fallback if no "result" step was classified (e.g. pure message response without tool calls)
    if let Some(last_step) = steps.last() {
        if last_step.kind == "message" {
            if let Some(ref text) = last_step.text {
                let t = text.trim();
                if !t.is_empty() && !is_idle_welcome_text(t) && !is_terminal_artifact(t) {
                    return Some(t.to_string());
                }
            }
        }
    }
    // Fallback 2: Reverse-line scanning for non-chrome human text
    let lines: Vec<&str> = summary.lines().collect();
    let mut collected = Vec::new();
    for &line in lines.iter().rev() {
        let t = line.trim();
        if t.is_empty() {
            if !collected.is_empty() && collected.last() != Some(&"") {
                collected.push("");
            }
            continue;
        }
        if t.starts_with('●')
            || t.starts_with('▸')
            || t.starts_with("> ")
            || t == ">"
            || is_rule_line(t)
            || is_terminal_artifact(t)
            || is_idle_welcome_text(t)
            || t.contains("(ctrl+o to expand)")
        {
            if !collected.is_empty() {
                break;
            }
            continue;
        }
        collected.push(t);
    }
    if !collected.is_empty() {
        collected.reverse();
        while collected.first() == Some(&"") {
            collected.remove(0);
        }
        while collected.last() == Some(&"") {
            collected.pop();
        }
        let res = collected.join("\n");
        let clean = if let Some(stripped) = res.strip_prefix("Final result:") {
            stripped.trim()
        } else if let Some(stripped) = res.strip_prefix("Result:") {
            stripped.trim()
        } else {
            res.trim()
        };
        if !clean.is_empty() && !is_idle_welcome_text(clean) && !is_terminal_artifact(clean) {
            return Some(clean.to_string());
        }
    }
    None
}

/// Extract Antigravity's final reply for the current turn when the chat input is focused.
///
/// Layout (styling from the VT screen, not guessed from text):
/// ```text
/// > user prompt ...            (blue, bold)
/// ▸ Thought for 5s             (grey)
///   thought summary ...        (grey)
///   Which language ...?        (default colour)  ← reply
/// ──────────────────────────
/// >                            ← input box
/// ```
/// Returns `None` when there is no user turn on screen or no default-colour reply after it.
fn extract_agent_reply(snapshot: &ScreenSnapshot) -> Option<String> {
    if snapshot.line_fg.len() != snapshot.lines.len() {
        return None;
    }
    let lines = &snapshot.lines;
    // The input box sits between the last two horizontal rules; the transcript ends at the
    // rule above the input box.
    let rules: Vec<usize> = (0..lines.len()).filter(|&i| is_rule_line(&lines[i])).collect();
    let transcript_end = *rules.iter().rev().nth(1).or(rules.last())?;
    // Last user prompt row above it.
    let user_row = (0..transcript_end)
        .rev()
        .find(|&i| snapshot.line_fg[i] == Some(FG_USER_PROMPT) && lines[i].starts_with("> "))?;

    let mut reply: Vec<&str> = Vec::new();
    for i in user_row + 1..transcript_end {
        let t = lines[i].trim();
        if t.is_empty() || is_terminal_artifact(t) || is_idle_welcome_text(t) {
            continue;
        }
        match snapshot.line_fg[i] {
            None => reply.push(t),
            // User prompt continuation: not part of the reply.
            Some(FG_USER_PROMPT) => {}
            // Thoughts / tool chrome after reply text start a new section; only the final
            // default-colour block is the agent's reply.
            Some(_) => reply.clear(),
        }
    }
    let message = reply.join("\n");
    (!message.is_empty() && !is_idle_welcome_text(&message)).then_some(message)
}

/// Extract clean prompt text from a single command or chat input line.
pub fn extract_prompt_from_line(line: &str) -> Option<String> {
    let t = line.trim();
    if is_terminal_artifact(t) || is_idle_welcome_text(t) || is_menu_row(t) {
        return None;
    }
    // Shell command with agy -i '<prompt>' or agy -i "<prompt>"
    if let Some(idx) = t.find("-i ") {
        let after = t[idx + 3..].trim();
        if (after.starts_with('\'') && after.ends_with('\'') && after.len() >= 2)
            || (after.starts_with('"') && after.ends_with('"') && after.len() >= 2)
        {
            let inner = &after[1..after.len() - 1];
            if !inner.trim().is_empty() {
                return Some(inner.trim().to_string());
            }
        } else if !after.is_empty() {
            return Some(after.to_string());
        }
    }
    // Interactive chat prompt starting with "> "
    if let Some(rest) = t.strip_prefix("> ") {
        let p = rest.trim();
        if !p.is_empty()
            && !is_idle_welcome_text(p)
            && !is_menu_row(p)
            && !is_terminal_artifact(p)
            && !p.contains("Navigate")
            && !p.contains("Yes,")
            && !p.contains("No,")
        {
            return Some(p.to_string());
        }
    }
    None
}

/// Extract user prompt row from screen snapshot if present.
pub fn extract_user_prompt_from_snapshot(snapshot: &ScreenSnapshot) -> Option<String> {
    let mut combined_lines = snapshot.scrollback_lines.clone();
    let scrollback_count = combined_lines.len();
    combined_lines.extend(snapshot.lines.iter().cloned());
    let lines = &combined_lines;

    let rules: Vec<usize> = (0..lines.len()).filter(|&i| is_rule_line(&lines[i])).collect();
    let prompt_box_rule = rules
        .iter()
        .rev()
        .find(|&&r| lines.len().saturating_sub(r) <= 6 && (r + 1..lines.len()).any(|i| {
            let t = lines[i].trim();
            t.starts_with("> ") || t == ">" || t.starts_with("? ") || t == "?" || t.contains("Send message")
        }));
    let transcript_end = prompt_box_rule.copied().unwrap_or(lines.len());

    let user_row = (0..transcript_end).rev().find(|&i| {
        let t = lines[i].trim();
        let is_fg_prompt = if i >= scrollback_count {
            snapshot.line_fg.get(i - scrollback_count).copied().flatten() == Some(FG_USER_PROMPT)
        } else {
            false
        };
        (is_fg_prompt && t.starts_with("> "))
            || (t.starts_with("> ")
                && !is_menu_row(t)
                && !is_terminal_artifact(t)
                && !is_idle_welcome_text(t)
                && !t.contains("Navigate")
                && !t.contains("Yes,")
                && !t.contains("No,"))
            || t.contains("agy -i")
            || t.contains("agy --dangerously-skip-permissions")
            || t.starts_with("╰─❯ agy")
            || t.starts_with("❯ agy")
            || (t.contains(" agy ")
                && (t.starts_with('$')
                    || t.starts_with('%')
                    || t.starts_with('#')
                    || t.starts_with('❯')
                    || t.starts_with("╰─❯")))
    })?;

    extract_prompt_from_line(&lines[user_row])
}

/// Extract summary of the work done prior to the idle prompt.
pub fn extract_work_summary(snapshot: &ScreenSnapshot) -> Option<String> {
    let mut combined_lines = snapshot.scrollback_lines.clone();
    let scrollback_count = combined_lines.len();
    combined_lines.extend(snapshot.lines.iter().cloned());
    let lines = &combined_lines;

    let rules: Vec<usize> = (0..lines.len()).filter(|&i| is_rule_line(&lines[i])).collect();
    // In Antigravity interactive Bubbletea TUI, the active prompt box is drawn at the bottom
    // framed by rules (e.g. within 6 lines of the bottom).
    // Markdown horizontal rules in the middle of transcripts must NOT be treated as the prompt box!
    let prompt_box_rule = rules
        .iter()
        .rev()
        .find(|&&r| lines.len().saturating_sub(r) <= 6 && (r + 1..lines.len()).any(|i| {
            let t = lines[i].trim();
            t.starts_with("> ") || t == ">" || t.starts_with("? ") || t == "?" || t.contains("Send message")
        }));
    let transcript_end = prompt_box_rule.copied().unwrap_or(lines.len());

    // Find the user prompt row above transcript_end, if visible.
    let user_row = (0..transcript_end).rev().find(|&i| {
        let t = lines[i].trim();
        let is_fg_prompt = if i >= scrollback_count {
            snapshot.line_fg.get(i - scrollback_count).copied().flatten() == Some(FG_USER_PROMPT)
        } else {
            false
        };
        (is_fg_prompt && t.starts_with("> "))
            || (t.starts_with("> ")
                && !is_menu_row(t)
                && !is_terminal_artifact(t)
                && !is_idle_welcome_text(t)
                && !t.contains("Navigate")
                && !t.contains("Yes,")
                && !t.contains("No,"))
            || t.contains("agy -i")
            || t.contains("agy --dangerously-skip-permissions")
            || t.starts_with("╰─❯ agy")
            || t.starts_with("❯ agy")
            || (t.contains(" agy ")
                && (t.starts_with('$')
                    || t.starts_with('%')
                    || t.starts_with('#')
                    || t.starts_with('❯')
                    || t.starts_with("╰─❯")))
    });

    // Check if there are visible tool calls or thought blocks
    let has_visible_execution = (0..transcript_end).any(|i| {
        let t = lines[i].trim();
        t.starts_with('●') || t.starts_with('▸') || t.starts_with("✓ ")
    });

    // If no user prompt row is found AND no tool calls / thoughts occurred, there is no work summary.
    if user_row.is_none() && !has_visible_execution {
        return None;
    }

    let start_idx = user_row.map(|r| r + 1).unwrap_or_else(|| {
        (0..transcript_end)
            .find(|&i| {
                let t = lines[i].trim();
                t.starts_with('●') || t.starts_with('▸') || t.starts_with("✓ ")
            })
            .unwrap_or_else(|| transcript_end.saturating_sub(30))
    });

    let mut summary_lines = Vec::new();
    for i in start_idx..transcript_end {
        let trimmed = lines[i].trim();
        if is_rule_line(trimmed) || is_terminal_artifact(trimmed) || is_idle_welcome_text(trimmed) {
            continue;
        }
        if trimmed.is_empty() {
            if !summary_lines.is_empty() && summary_lines.last() != Some(&"") {
                summary_lines.push("");
            }
            continue;
        }
        if trimmed.starts_with("> ")
            || trimmed == ">"
            || trimmed.starts_with("? ")
            || trimmed == "?"
            || is_shell_prompt_line(trimmed)
        {
            continue;
        }
        summary_lines.push(lines[i].trim_end());
    }

    while summary_lines.last().map(|s| s.trim().is_empty()).unwrap_or(false) {
        summary_lines.pop();
    }
    while summary_lines.first().map(|s| s.trim().is_empty()).unwrap_or(false) {
        summary_lines.remove(0);
    }

    if summary_lines.is_empty() {
        if let Some(reply) = extract_agent_reply(snapshot) {
            if !reply.trim().ends_with('?') && !is_idle_welcome_text(&reply) && !is_terminal_artifact(&reply) {
                return Some(reply);
            }
        }
        None
    } else {
        Some(summary_lines.join("\n"))
    }
}

/// Selection markers Antigravity/Bubbletea may render in front of the focused menu row.
const CURSOR_MARKERS: [char; 3] = ['>', '❯', '›'];

/// Strip a leading cursor marker; returns (had_cursor, label).
fn strip_cursor_marker(line: &str) -> (bool, &str) {
    let t = line.trim_start();
    for m in CURSOR_MARKERS {
        if let Some(rest) = t.strip_prefix(m) {
            return (true, rest.trim());
        }
    }
    (false, t.trim())
}

/// Parse Antigravity's workspace-trust dialog from the VT screen:
///
/// ```text
/// Accessing workspace:
/// /path/to/project
/// Do you trust the contents of this project?
/// Antigravity CLI requires permission to read, edit, and execute files here.
/// > Yes, I trust this folder
///   No, exit
///   ↑/↓ Navigate · enter Confirm
/// ```
///
/// Returns the exact question, option labels (cursor markers stripped), live cursor index and
/// directory. Plain chat text that merely mentions trust never matches: the option rows and
/// the trust question must both be present.
fn extract_workspace_trust(snapshot: &ScreenSnapshot) -> Option<AntigravityState> {
    let lines = &snapshot.lines;
    let q_idx = lines.iter().position(|l| {
        let t = l.trim();
        t.ends_with('?')
            && (t.contains("Do you trust the contents")
                || t.contains("Do you trust the authors of the files"))
    })?;
    let question = lines[q_idx].trim().to_string();

    let mut options = Vec::new();
    let mut selected_index = None;
    for line in &lines[q_idx + 1..] {
        let t = line.trim();
        if t.is_empty() {
            if options.is_empty() {
                continue;
            }
            break;
        }
        if t.contains("Navigate") && t.contains("enter") {
            break;
        }
        let (cursor, label) = strip_cursor_marker(line);
        let indented = line.starts_with("  ");
        let is_option = cursor
            || (indented && !options.is_empty())
            || label.starts_with("Yes,")
            || label.starts_with("No,");
        if !is_option {
            // Description text between the question and the menu.
            if options.is_empty() {
                continue;
            }
            break;
        }
        if cursor {
            selected_index = Some(options.len());
        }
        options.push(label.to_string());
    }
    if options.len() < 2 {
        return None;
    }

    let directory = lines
        .iter()
        .position(|l| l.trim() == "Accessing workspace:")
        .and_then(|i| lines[i + 1..].iter().map(|l| l.trim()).find(|l| !l.is_empty()))
        .map(str::to_string)
        .or_else(|| {
            question
                .split_once(" in ")
                .map(|(_, rest)| rest.trim_end_matches('?').trim().to_string())
        })
        .or_else(|| {
            lines
                .iter()
                .map(|l| l.trim())
                .find(|l| l.starts_with('/') || l.starts_with('~'))
                .map(str::to_string)
        })
        .unwrap_or_else(|| "current directory".to_string());

    Some(AntigravityState::WorkspaceTrust {
        directory,
        question,
        options,
        selected_index: selected_index.unwrap_or(0),
    })
}

fn extract_command_dialog(snapshot: &ScreenSnapshot) -> Option<(String, Vec<String>, usize)> {
    let mut options = Vec::new();
    let mut selected_index = 0;
    let mut explicit_command = String::new();
    let mut fallback_command = String::new();
    let mut has_selection = false;
    let mut expecting_command = false;

    for (r, line) in snapshot.lines.iter().enumerate() {
        let trimmed = line.trim();

        if expecting_command && !trimmed.is_empty() {
            explicit_command = trimmed.to_string();
            expecting_command = false;
        }

        if trimmed.starts_with("Requesting permission for:") {
            expecting_command = true;
        } else if trimmed.starts_with("Command:")
            || trimmed.starts_with("Run:")
            || trimmed.starts_with("$ ")
        {
            let extracted = trimmed
                .trim_start_matches("Command:")
                .trim_start_matches("Run:")
                .trim_start_matches("$ ")
                .trim()
                .to_string();
            if !extracted.is_empty() {
                explicit_command = extracted;
            }
        } else if let Some(start) = trimmed.find("● Bash(") {
            let after = &trimmed[start + "● Bash(".len()..];
            if let Some(end) = after.find(')') {
                let bash_cmd = after[..end].trim().to_string();
                if !bash_cmd.is_empty() {
                    fallback_command = bash_cmd;
                }
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

    let has_run_cmd = options.iter().any(|o| {
        o.starts_with("Yes,")
            || o.contains("run command")
            || o.contains("Run command")
            || o.contains("allow")
    });
    let has_deny = options.iter().any(|o| {
        o.starts_with("No,") || o.contains("deny") || o.contains("cancel")
    });

    if options.len() >= 2
        && has_run_cmd
        && has_deny
        && (has_selection || snapshot.in_alt_screen)
    {
        let command = if !explicit_command.is_empty() {
            explicit_command
        } else if !fallback_command.is_empty() {
            fallback_command
        } else {
            "run_command".to_string()
        };
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
    let mut is_file_prompt = false;

    for (r, line) in snapshot.lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("File:") || trimmed.starts_with("Editing:") {
            file_path = trimmed
                .trim_start_matches("File:")
                .trim_start_matches("Editing:")
                .trim()
                .to_string();
            is_file_prompt = true;
        } else if trimmed == "Create file" || trimmed == "Edit file" || trimmed == "Write to file" {
            is_file_prompt = true;
        } else if trimmed.starts_with("Allow creation of this file?")
            || trimmed.starts_with("Allow editing this file?")
            || trimmed.starts_with("Allow access to this file?")
        {
            is_file_prompt = true;
        } else if trimmed.starts_with("Allow edit to ") {
            file_path = trimmed
                .trim_start_matches("Allow edit to ")
                .trim_end_matches('?')
                .trim()
                .to_string();
            is_file_prompt = true;
        } else if is_file_prompt && file_path.is_empty() {
            // Check for diff header line or file path: e.g. "/home/pirate/grep.py  +200"
            let candidate = trimmed.split_whitespace().next().unwrap_or("").trim();
            if (candidate.starts_with('/') || (candidate.contains('/') && candidate.contains('.')))
                && !candidate.starts_with('>')
                && !candidate.starts_with('●')
            {
                file_path = candidate.to_string();
            }
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

    let has_accept = options.iter().any(|o| {
        o.starts_with("Yes,")
            || o.contains("accept")
            || o.contains("allow")
            || o.contains("overwrite")
    });
    let has_reject = options.iter().any(|o| {
        o.starts_with("No,") || o.contains("reject") || o.contains("deny") || o.contains("cancel")
    });
    let has_file_opt = options.iter().any(|o| {
        o.contains("creation") || o.contains("file") || o.contains("change") || o.contains("edit")
    });

    if options.len() >= 2
        && has_accept
        && has_reject
        && (is_file_prompt || has_file_opt)
        && (has_selection || snapshot.in_alt_screen)
    {
        if file_path.is_empty() {
            file_path = "file".to_string();
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
    fn classifies_explicit_yes_no_controls_without_relabeling_options() {
        let yes_no = QuestionDetails::SingleChoice {
            options: vec!["Yes".into(), "No".into()],
            selected_index: 0,
            write_in_index: None,
        };
        assert_eq!(yes_no.question_type(), QuestionType::YesNo);
        assert_eq!(yes_no.answer_options(), ["Yes", "No"]);

        let unrelated = QuestionDetails::SingleChoice {
            options: vec!["Keep".into(), "Delete".into()],
            selected_index: 0,
            write_in_index: None,
        };
        assert_eq!(unrelated.question_type(), QuestionType::SingleChoice);
    }

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
            line_fg: vec![],
            scrollback_lines: vec![],
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
            line_fg: vec![],
            scrollback_lines: vec![],
        };

        let event = sm.update(&snapshot).expect("Must emit event on transition");
        assert_eq!(event.kind, "command_confirmation");
        assert_eq!(event.operation, Operation::Test);
        assert!(event.request.is_some());
    }

    #[test]
    fn spinner_redraws_share_one_working_state() {
        let make_snapshot = |spinner: &str| ScreenSnapshot {
            cols: 80,
            rows: 24,
            cursor_row: 5,
            cursor_col: 0,
            in_alt_screen: true,
            cursor_visible: true,
            lines: vec![format!("{spinner} Generating...")],
            reversed_lines: vec![],
            line_fg: vec![],
            scrollback_lines: vec![],
        };

        assert_eq!(
            detect_state(&make_snapshot("⠋")),
            detect_state(&make_snapshot("⠙"))
        );
    }

    #[test]
    fn idle_prompt_detected_with_status_bar_and_rule_lines() {
        let snapshot = ScreenSnapshot {
            cols: 80,
            rows: 24,
            cursor_row: 2,
            cursor_col: 2,
            in_alt_screen: false,
            cursor_visible: true,
            lines: vec![
                "──────────────────────────────────────────────────".to_string(),
                ">".to_string(),
                "──────────────────────────────────────────────────".to_string(),
                "gemini-3.8-flash-high (8k tokens)".to_string(),
            ],
            reversed_lines: vec![],
            line_fg: vec![],
            scrollback_lines: vec![],
        };

        assert_eq!(detect_state(&snapshot), AntigravityState::IdlePrompt);
    }

    #[test]
    fn state_machine_emits_task_completed_when_transitioning_from_working_to_idle() {
        let mut sm = AntigravityStateMachine::new("test-agent");

        let working_snap = ScreenSnapshot {
            cols: 80,
            rows: 24,
            cursor_row: 0,
            cursor_col: 0,
            in_alt_screen: false,
            cursor_visible: true,
            lines: vec!["⠙ Working...".to_string()],
            reversed_lines: vec![],
            line_fg: vec![],
            scrollback_lines: vec![],
        };
        let ev1 = sm.update(&working_snap);
        assert!(ev1.is_some());
        assert_eq!(ev1.unwrap().kind, "progress");

        let idle_snap = ScreenSnapshot {
            cols: 80,
            rows: 24,
            cursor_row: 3,
            cursor_col: 2,
            in_alt_screen: false,
            cursor_visible: true,
            lines: vec![
                "● Bash(uname -a)".to_string(),
                "Linux Pirate 6.8.0-45-generic".to_string(),
                "──────────────────────────────────────────────────".to_string(),
                ">".to_string(),
            ],
            reversed_lines: vec![],
            line_fg: vec![],
            scrollback_lines: vec![],
        };
        let ev2 = sm.update(&idle_snap).expect("Must emit task_completed event");
        assert_eq!(ev2.kind, "task_completed");
        assert!(ev2.message.contains("Linux Pirate 6.8.0-45-generic"));
    }

    #[test]
    fn test_execution_transcript_fixture_order_preservation_and_artifact_removal() {
        let raw_lines = vec![
            "> user prompt".to_string(),
            // Terminal rendering / Kitty protocol / braille artifacts that MUST be removed:
            "Ga=q,f=32,s=1,v=1,i=31;AAAAAAA==".to_string(),
            "⣾⣽⣻⢿".to_string(),
            "\x1b[?25h".to_string(),
            // 1. Thought
            "▸ Thought for 11s, 235 tokens".to_string(),
            "The objective was to write \"hello world\" to the designated file.".to_string(),
            "".to_string(),
            // 2. Bash (rm)
            "● Bash".to_string(),
            "(rm -f /home/pirate/test_summary.txt)".to_string(),
            "".to_string(),
            // 3. Bash (git status)
            "● Bash".to_string(),
            "git status".to_string(),
            "".to_string(),
            // 4. Bash (git diff)
            "● Bash".to_string(),
            "git diff --stat".to_string(),
            "".to_string(),
            // 5. Thought
            "▸ Thought for 9s".to_string(),
            "All core components now function as expected...".to_string(),
            "".to_string(),
            // 6. Edit
            "● Edit".to_string(),
            "~/test_summary.txt".to_string(),
            "".to_string(),
            // 7. Final result
            "Final result:".to_string(),
            "Written hello world to test_summary.txt.".to_string(),
            "──────────────────────────────────────────────────".to_string(),
            ">".to_string(),
            "──────────────────────────────────────────────────".to_string(),
        ];

        let snapshot = ScreenSnapshot {
            cols: 80,
            rows: raw_lines.len() as u16,
            cursor_row: (raw_lines.len() - 2) as u16,
            cursor_col: 2,
            in_alt_screen: false,
            cursor_visible: true,
            lines: raw_lines,
            reversed_lines: vec![],
            line_fg: vec![],
            scrollback_lines: vec![],
        };

        // 1. Extract summary
        let summary = extract_work_summary(&snapshot).expect("Summary must be extracted");

        // 2. Verify terminal artifacts are completely absent
        assert!(!summary.contains("Ga="), "Must strip Kitty graphics protocol");
        assert!(!summary.contains("AAAAAAA=="), "Must strip base64 Kitty data");
        assert!(!summary.contains("⣾⣽⣻⢿"), "Must strip braille spinner artifact");
        assert!(!summary.contains("\x1b"), "Must strip ESC byte");

        // 3. Verify all legitimate agent content is retained
        assert!(summary.contains("Thought for 11s, 235 tokens"));
        assert!(summary.contains("The objective was to write \"hello world\""));
        assert!(summary.contains("rm -f /home/pirate/test_summary.txt"));
        assert!(summary.contains("git status"));
        assert!(summary.contains("git diff --stat"));
        assert!(summary.contains("Thought for 9s"));
        assert!(summary.contains("~/test_summary.txt"));
        assert!(summary.contains("Written hello world to test_summary.txt."));

        // 4. Verify chronological ordering in summary
        let pos_t1 = summary.find("Thought for 11s").unwrap();
        let pos_b1 = summary.find("rm -f").unwrap();
        let pos_b2 = summary.find("git status").unwrap();
        let pos_b3 = summary.find("git diff --stat").unwrap();
        let pos_t2 = summary.find("Thought for 9s").unwrap();
        let pos_e1 = summary.find("~/test_summary.txt").unwrap();
        let pos_res = summary.find("Written hello world").unwrap();

        assert!(pos_t1 < pos_b1, "Thought 1 must precede Bash 1");
        assert!(pos_b1 < pos_b2, "Bash 1 must precede Bash 2");
        assert!(pos_b2 < pos_b3, "Bash 2 must precede Bash 3");
        assert!(pos_b3 < pos_t2, "Bash 3 must precede Thought 2");
        assert!(pos_t2 < pos_e1, "Thought 2 must precede Edit");
        assert!(pos_e1 < pos_res, "Edit must precede Final result");

        // 5. Test parse_execution_steps
        let steps = parse_execution_steps(&summary);
        assert_eq!(steps.len(), 7, "Must extract exactly 7 execution steps: {:?}", steps);

        // Step 0: Thought 1
        assert_eq!(steps[0].kind, "thought");
        assert_eq!(steps[0].duration.as_deref(), Some("11s"));
        assert_eq!(steps[0].tokens.as_deref(), Some("235 tokens"));
        assert!(steps[0].text.as_ref().unwrap().contains("The objective was to write"));

        // Step 1: Bash (rm)
        assert_eq!(steps[1].kind, "tool_call");
        assert_eq!(steps[1].tool.as_deref(), Some("Bash"));
        assert_eq!(steps[1].command.as_deref(), Some("rm -f /home/pirate/test_summary.txt"));

        // Step 2: Bash (git status)
        assert_eq!(steps[2].kind, "tool_call");
        assert_eq!(steps[2].tool.as_deref(), Some("Bash"));
        assert_eq!(steps[2].command.as_deref(), Some("git status"));

        // Step 3: Bash (git diff)
        assert_eq!(steps[3].kind, "tool_call");
        assert_eq!(steps[3].tool.as_deref(), Some("Bash"));
        assert_eq!(steps[3].command.as_deref(), Some("git diff --stat"));

        // Step 4: Thought 2
        assert_eq!(steps[4].kind, "thought");
        assert_eq!(steps[4].duration.as_deref(), Some("9s"));
        assert!(steps[4].text.as_ref().unwrap().contains("All core components now function"));

        // Step 5: Edit
        assert_eq!(steps[5].kind, "tool_call");
        assert_eq!(steps[5].tool.as_deref(), Some("Edit"));
        assert_eq!(steps[5].command.as_deref(), Some("~/test_summary.txt"));

        // Step 6: Final result
        assert_eq!(steps[6].kind, "result");
        assert_eq!(steps[6].text.as_deref(), Some("Written hello world to test_summary.txt."));

        // 6. Test extract_final_result
        let final_result = extract_final_result(&summary).expect("Final result must be extracted");
        assert_eq!(final_result, "Written hello world to test_summary.txt.");

        // 7. Verify state machine update generates rich event details
        let mut sm = AntigravityStateMachine::new("test-agent");
        sm.start_prompt("write hello world");
        let ev = sm.update(&snapshot).expect("Must emit task_completed event");
        assert_eq!(ev.kind, "task_completed");
        assert_eq!(
            ev.details.get("final_result").and_then(|v| v.as_str()),
            Some("Written hello world to test_summary.txt.")
        );
        assert_eq!(
            ev.details.get("tool_count").and_then(|v| v.as_u64()),
            Some(4) // 3 Bash + 1 Edit
        );
        let exec_steps = ev.details.get("execution_steps").expect("execution_steps must be in details");
        assert!(exec_steps.is_array());
        assert_eq!(exec_steps.as_array().unwrap().len(), 7);
    }

    #[test]
    fn test_extract_final_result_multi_paragraph() {
        let summary = r#"
● Bash(cargo build --release)
● Bash(/tmp/test_folder/grep --help)
The directory and Rust grep program have been created:

• Directory: test_folder
• Executable binary: grep
• Source code: grep.rs

### Features Supported

• Regex matching: Powered by the Rust regex engine.
• Flags: -i, -n, -v, -c, -r
"#;
        let res = extract_final_result(summary).expect("Must extract multi-paragraph result");
        assert!(res.starts_with("The directory and Rust grep program have been created:"));
        assert!(res.contains("• Directory: test_folder"));
        assert!(res.contains("### Features Supported"));
        assert!(res.contains("• Flags: -i, -n, -v, -c, -r"));
        assert!(!res.contains("Bash"));
    }

    #[test]
    fn test_extract_final_result_full_antigravity_session() {
        let summary = r#"
> rust

▸ Thought for 4s, 521 tokens
  The user has approved the Rust implementation, so the task requirements are clear.
● Bash(which rustc cargo)
● Bash(ls -ld /tmp/test_folder 2>&1 || true)
● Bash(cargo build --release --manifest-path /tmp/test_folder/Cargo.toml)
● Bash(/tmp/test_folder/grep "edition" /tmp/test_folder/Cargo.toml) (ctrl+o to expand)

The directory and Rust grep program have been created:

• Directory: test_folder
• Executable binary: grep
• Source code: grep.rs (also located at main.rs)
• Cargo manifest: Cargo.toml

### Features Supported

• Regex matching: Powered by the Rust regex engine.
• Flags:
    • -e, --regexp=PATTERN: specify pattern
    • -i, --ignore-case: case-insensitive search
    • -n, --line-number: print 1-based line numbers
    • -v, --invert-match: select non-matching lines
    • -c, --count: print count of matching lines
    • -r, -R, --recursive: search directories recursively
    • -l, --files-with-matches: print filenames containing matches
    • -h, --no-filename / -H, --with-filename
• I/O: Supports standard input (pipe or -) as well as multiple file arguments.
• Exit codes: 0 on match, 1 on no match, and 2 on syntax or file errors.

### Example Usage

  # Pattern search in a file with line numbers
  /tmp/test_folder/grep -n "edition" /tmp/test_folder/Cargo.toml

  # Reading from stdin with case insensitivity
  printf "apple\nBanana\ncherry\n" | /tmp/test_folder/grep -i "banana"

  # Recursive search with count
  /tmp/test_folder/grep -r -c "Config" /tmp/test_folder/src
"#;
        let res = extract_final_result(summary).expect("Must extract full result");
        assert!(res.contains("The directory and Rust grep program have been created:"));
        assert!(res.contains("• Directory: test_folder"));
        assert!(res.contains("### Features Supported"));
        assert!(res.contains("### Example Usage"));
        assert!(res.contains("/tmp/test_folder/grep -n \"edition\" /tmp/test_folder/Cargo.toml"));
        assert!(!res.contains("● Bash"));
        assert!(!res.contains("Thought for 4s"));
    }

    #[test]
    fn test_fish_shell_agy_execution_emits_task_completed_for_structure_query() {
        let mut sm = AntigravityStateMachine::new("test-agent");

        // Step 1: Agent is initially working in fish terminal
        let working_lines = vec![
            "╭─ 🟥 pirate @ 📍 Reactor in /m/p/S/c/AgentDesk on master ●7".to_string(),
            "╰─❯ agy -i \"what is the structure of forgex\"".to_string(),
            "Thinking... ⣾".to_string(),
        ];
        let snap_working = ScreenSnapshot {
            cols: 80,
            rows: working_lines.len() as u16,
            cursor_row: (working_lines.len() - 1) as u16,
            cursor_col: 10,
            in_alt_screen: false,
            cursor_visible: true,
            lines: working_lines,
            reversed_lines: vec![],
            line_fg: vec![],
            scrollback_lines: vec![],
        };
        let _ev1 = sm.update(&snap_working);
        assert!(matches!(sm.current_state, AntigravityState::Working { .. }));

        // Step 2: Agent completes and fish shell prompt returns
        let completed_lines = vec![
            "╭─ 🟥 pirate @ 📍 Reactor in /m/p/S/c/AgentDesk on master ●7".to_string(),
            "╰─❯ agy -i \"what is the structure of forgex\"".to_string(),
            "#### Key Components:".to_string(),
            "".to_string(),
            "• Core Domain & Scoring (core & scoring): Implements the math, rule classification, and recovery engine.".to_string(),
            "• Embedded Storage (storage): Zero-config SQLite database managed via rusqlite.".to_string(),
            "• Interactive TUI & Watcher CLI (cli): Built-in interactive Ratatui terminal dashboard.".to_string(),
            "• REST Sync Server (server): Axum-powered local daemon enabling secure pairing and multi-device state synchronization.".to_string(),
            "• Native FFI & Flutter App (ffi & mobile): A companion mobile and desktop UI supporting remote REST and native embedded offline operation via libforgex_ffi.".to_string(),
            "──────────────────────────────────────────────────".to_string(),
            "### Quick Exploration".to_string(),
            "".to_string(),
            "• Check the full documentation in README.md.".to_string(),
            "• Launch the CLI help via cargo run -p forgex -- --help.".to_string(),
            "╭─ 🟥 pirate @ 📍 Reactor in /m/p/S/c/AgentDesk on master ●7".to_string(),
            "╰─❯ ".to_string(),
        ];
        let snap_completed = ScreenSnapshot {
            cols: 80,
            rows: completed_lines.len() as u16,
            cursor_row: (completed_lines.len() - 1) as u16,
            cursor_col: 4,
            in_alt_screen: false,
            cursor_visible: true,
            lines: completed_lines,
            reversed_lines: vec![],
            line_fg: vec![],
            scrollback_lines: vec![],
        };

        // Screen state is detected as IdlePrompt because fish prompt is back
        let detected = detect_state(&snap_completed);
        assert_eq!(detected, AntigravityState::IdlePrompt);

        // State machine transitions from Working -> Completed and emits task_completed event
        let ev2 = sm.update(&snap_completed).expect("Must emit task_completed event");
        assert_eq!(ev2.kind, "task_completed");
        assert!(ev2.message.contains("Key Components:"));
        assert!(ev2.message.contains("Core Domain & Scoring"));
        assert!(ev2.message.contains("Embedded Storage"));
        assert!(ev2.message.contains("Quick Exploration"));
        // Shell prompt headers must be stripped from summary
        assert!(!ev2.message.contains("╰─❯ "));
        assert!(!ev2.message.contains("Reactor in /m/p/S/c/AgentDesk"));
    }
}

