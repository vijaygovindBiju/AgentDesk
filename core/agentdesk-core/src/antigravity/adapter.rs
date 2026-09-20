//! Real-Agent Pseudo-Terminal (PTY) Adapter for Antigravity (`agy`).
//!
//! Bridges interactive Bubbletea terminal sessions into the normalized AgentDesk
//! adapter architecture. The phone client NEVER sees raw PTY byte streams or escape sequences;
//! all terminal activity is parsed into a VT100 2D screen grid on the host machine,
//! semantically classified into canonical AgentDesk events, and decisions from the
//! phone are encoded into deterministic terminal keystrokes.
//!
//! Every blocking request the adapter emits is tracked as a [`PendingRequest`] carrying a
//! monotonically increasing generation. A response is only delivered to the PTY when the
//! request is still pending, the PTY is alive, and the live screen still shows the same
//! request; otherwise it is rejected without sending any keystrokes.

use std::io;
use std::path::PathBuf;

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

use agentdesk_model::{
    AdapterKind, AgentId, AgentInfo, Decision, Details, Operation, RawAgentEvent, RequestInfo,
    RequestResponse, TaskId,
};

use crate::adapter::{Adapter, AdapterOutput, RespondError};
use crate::antigravity::input_encoder::{
    QuestionInput, encode_decision, encode_question_response, validate_text_input,
};
use crate::antigravity::pty::{PtySession, PtyTransport};
use crate::antigravity::screen::Screen;
use crate::antigravity::state_machine::{
    AntigravityState, QuestionDetails, classify_command_operation, detect_state,
    question_details_map,
};

/// Lifecycle state machine for the Antigravity PTY Adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AntigravityLifecycle {
    /// Adapter created, PTY not yet polled/spawned.
    Uninitialized,
    /// PTY spawned, child process starting, waiting for readiness.
    Starting,
    /// Agent is idle/ready for user turn.
    Running,
    /// Agent is actively thinking, calling tools, running commands.
    Working { details: String },
    /// Agent is halted on an interactive confirmation menu or question.
    BlockedOnPermission { state: AntigravityState },
    /// Agent has completed its goal / turn.
    Completed { message: String },
    /// Process exited or fatal error.
    Exited { exit_code: Option<i32> },
}

/// A blocking request that has been emitted to AgentDesk and not yet resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRequest {
    /// Monotonic identity of this request within the adapter session.
    pub generation: u64,
    /// `agent_seq` of the `RawAgentEvent` that announced this request.
    pub agent_seq: u64,
    /// Screen state the request was extracted from (refreshed with live cursor state).
    pub state: AntigravityState,
}

/// How the most recent request left the pending slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestOutcome {
    /// A response was delivered to the PTY.
    Consumed,
    /// The TUI moved on (question disappeared / replaced) before any response arrived.
    Expired,
}

/// A request whose response was just delivered. Antigravity processes queued keystrokes one
/// render at a time, so the same dialog (with a moved cursor) may be observed briefly after
/// delivery; it must not be re-emitted as a new request until `grace_until` has passed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ConsumedRequest {
    state: AntigravityState,
    grace_until: DateTime<Utc>,
}

/// How long a just-answered dialog may linger on screen before it is surfaced again.
const CONSUMED_GRACE_SECS: i64 = 5;

/// Phase-two state for a write-in answer: keystrokes to open the text entry have been sent and
/// `text` must be typed once the entry control is actually observed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingWriteIn {
    question: String,
    text: String,
    deadline: DateTime<Utc>,
}

/// Configuration options for launching and running Antigravity via PTY.
#[derive(Debug, Clone)]
pub struct AntigravityConfig {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub agent_id: AgentId,
    pub agent_name: String,
    pub project: String,
    pub initial_prompt: Option<String>,
    pub cols: u16,
    pub rows: u16,
}

impl Default for AntigravityConfig {
    fn default() -> Self {
        Self {
            command: "agy".into(),
            args: Vec::new(),
            cwd: None,
            agent_id: "antigravity".into(),
            agent_name: "Antigravity".into(),
            project: "AgentDesk".into(),
            initial_prompt: None,
            cols: 120,
            rows: 40,
        }
    }
}

/// Real-agent adapter driving Antigravity inside a POSIX pseudo-terminal.
pub struct AntigravityPtyAdapter<T: PtyTransport> {
    config: AntigravityConfig,
    agents: Vec<AgentInfo>,
    lifecycle: AntigravityLifecycle,
    agent_seq: u64,
    task_id: TaskId,
    transport: T,
    screen: Screen,
    current_state: AntigravityState,
    buffered_outputs: Vec<AdapterOutput>,
    request_generation: u64,
    pending: Option<PendingRequest>,
    last_outcome: Option<RequestOutcome>,
    last_consumed: Option<ConsumedRequest>,
    pending_write_in: Option<PendingWriteIn>,
}

impl AntigravityPtyAdapter<PtySession> {
    /// Spawn a real Antigravity process inside a newly allocated POSIX PTY.
    pub fn spawn(config: AntigravityConfig) -> io::Result<Self> {
        let cols = config.cols;
        let rows = config.rows;
        let mut args = config.args.clone();

        // If an initial prompt was specified and not already in args, pass -i <prompt>
        if let Some(ref prompt) = config.initial_prompt
            && !args
                .iter()
                .any(|a| a == "-i" || a == "--prompt-interactive" || a == "-p" || a == "--print")
        {
            args.push("-i".into());
            args.push(prompt.clone());
        }

        let session = PtySession::spawn(&config.command, &args, config.cwd.as_ref(), cols, rows)?;
        Ok(Self::new_with_transport(config, session))
    }
}

impl<T: PtyTransport> AntigravityPtyAdapter<T> {
    /// Create a new adapter using an existing or mock PTY transport.
    pub fn new_with_transport(config: AntigravityConfig, transport: T) -> Self {
        let agent = AgentInfo {
            agent_id: config.agent_id.clone(),
            name: config.agent_name.clone(),
            project: config.project.clone(),
            adapter_kind: AdapterKind::AntigravityPty,
        };
        let task_id = format!("{}-session", config.agent_id);
        let screen = Screen::new(config.cols, config.rows);

        Self {
            config,
            agents: vec![agent],
            lifecycle: AntigravityLifecycle::Uninitialized,
            agent_seq: 0,
            task_id,
            transport,
            screen,
            current_state: AntigravityState::Initializing,
            buffered_outputs: Vec::new(),
            request_generation: 0,
            pending: None,
            last_outcome: None,
            last_consumed: None,
            pending_write_in: None,
        }
    }

    pub fn lifecycle(&self) -> &AntigravityLifecycle {
        &self.lifecycle
    }

    pub fn current_task_id(&self) -> &TaskId {
        &self.task_id
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// The request currently awaiting a human response, if any.
    pub fn pending_request(&self) -> Option<&PendingRequest> {
        self.pending.as_ref()
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        self.screen.resize(cols, rows);
        self.transport.resize(cols, rows)
    }

    fn next_agent_seq(&mut self) -> u64 {
        self.agent_seq += 1;
        self.agent_seq
    }

    fn event(
        &mut self,
        kind: &str,
        operation: Operation,
        message: String,
        details: Details,
        log_lines: Vec<String>,
        request: Option<RequestInfo>,
    ) -> AdapterOutput {
        let seq = self.next_agent_seq();
        AdapterOutput::Event(RawAgentEvent {
            agent_id: self.config.agent_id.clone(),
            agent_seq: seq,
            task_id: Some(self.task_id.clone()),
            kind: kind.into(),
            operation,
            message,
            details,
            log_lines,
            request,
        })
    }

    /// Drop the pending request (if any) because the TUI no longer shows it.
    fn expire_pending(&mut self) {
        if self.pending.take().is_some() {
            self.last_outcome = Some(RequestOutcome::Expired);
        }
    }

    /// Build the normalized request event for a blocking state.
    fn request_event(&mut self, state: &AntigravityState, generation: u64) -> AdapterOutput {
        match state {
            AntigravityState::CommandConfirmation {
                command,
                selected_option_index,
                ..
            } => {
                let op = classify_command_operation(command);
                let mut details = Details::new();
                details.insert("command".into(), Value::String(command.clone()));
                details.insert(
                    "selected_option".into(),
                    Value::from(*selected_option_index),
                );
                details.insert("request_generation".into(), Value::from(generation));
                self.event(
                    "approval_required",
                    op,
                    format!("Command requires approval: {}", command),
                    details,
                    vec![format!("Command: {}", command)],
                    Some(RequestInfo {
                        prompt: command.clone(),
                        options: vec!["approve".into(), "deny".into()],
                        question_type: None,
                    }),
                )
            }
            AntigravityState::FileEditConfirmation {
                file_path,
                selected_option_index,
                ..
            } => {
                let mut details = Details::new();
                details.insert("file".into(), Value::String(file_path.clone()));
                details.insert(
                    "selected_option".into(),
                    Value::from(*selected_option_index),
                );
                details.insert("request_generation".into(), Value::from(generation));
                self.event(
                    "approval_required",
                    Operation::Edit,
                    format!("File edit requires approval: {}", file_path),
                    details,
                    vec![format!("File: {}", file_path)],
                    Some(RequestInfo {
                        prompt: format!("Allow edit to {}?", file_path),
                        options: vec!["approve".into(), "deny".into()],
                        question_type: None,
                    }),
                )
            }
            AntigravityState::WorkspaceTrust { directory } => {
                let mut details = Details::new();
                details.insert("directory".into(), Value::String(directory.clone()));
                details.insert("request_generation".into(), Value::from(generation));
                self.event(
                    "approval_required",
                    Operation::Other,
                    format!("Workspace trust required for: {}", directory),
                    details,
                    vec![format!("Trust requested for directory {}", directory)],
                    Some(RequestInfo {
                        prompt: format!("Trust directory {}?", directory),
                        options: vec!["approve".into(), "deny".into()],
                        question_type: None,
                    }),
                )
            }
            AntigravityState::UserQuestion {
                question,
                form,
                details,
            } => {
                let mut d = question_details_map(question, *form, details);
                d.insert("request_generation".into(), Value::from(generation));
                self.event(
                    "input_required",
                    Operation::Other,
                    question.clone(),
                    d,
                    vec![question.clone()],
                    Some(RequestInfo {
                        prompt: question.clone(),
                        options: details.answer_options(),
                        question_type: Some(details.question_type()),
                    }),
                )
            }
            _ => unreachable!("request_event called with non-blocking state"),
        }
    }

    /// A blocking state was detected: refresh the pending request if it is the same one
    /// (redraw / cursor movement), otherwise expire the old request and open a new one.
    fn open_request(
        &mut self,
        detected: AntigravityState,
        now: DateTime<Utc>,
        out: &mut Vec<AdapterOutput>,
    ) {
        if let Some(pending) = self.pending.as_mut()
            && pending.state.same_request(&detected)
        {
            pending.state = detected.clone();
            self.lifecycle = AntigravityLifecycle::BlockedOnPermission { state: detected };
            return;
        }

        // Just-answered dialog still being processed by Antigravity: not a new request yet.
        if let Some(consumed) = &self.last_consumed
            && consumed.state.same_request(&detected)
            && now < consumed.grace_until
        {
            return;
        }
        self.last_consumed = None;

        self.expire_pending();
        self.request_generation += 1;
        let generation = self.request_generation;
        self.lifecycle = AntigravityLifecycle::BlockedOnPermission {
            state: detected.clone(),
        };
        let ev = self.request_event(&detected, generation);
        let agent_seq = match &ev {
            AdapterOutput::Event(raw) => raw.agent_seq,
            _ => 0,
        };
        self.pending = Some(PendingRequest {
            generation,
            agent_seq,
            state: detected,
        });
        out.push(ev);
    }

    /// Phase two of a write-in answer: the text-entry control is on screen for the question
    /// we opened it for, so type the validated text. Returns true when handled.
    fn try_complete_write_in(&mut self, detected: &AntigravityState, now: DateTime<Utc>) -> bool {
        let Some(pw) = self.pending_write_in.as_ref() else {
            return false;
        };
        if now > pw.deadline {
            self.pending_write_in = None;
            return false;
        }
        let AntigravityState::UserQuestion {
            question,
            details: QuestionDetails::FreeText { .. },
            ..
        } = detected
        else {
            return false;
        };
        if question != &pw.question {
            self.pending_write_in = None;
            return false;
        }
        let pw = self.pending_write_in.take().expect("checked above");
        if validate_text_input(&pw.text).is_err() {
            return false;
        }
        let input = match encode_question_response(
            &RequestResponse::TextInput { text: pw.text },
            &QuestionDetails::FreeText {
                current_text: match detected {
                    AntigravityState::UserQuestion {
                        details: QuestionDetails::FreeText { current_text },
                        ..
                    } => current_text.clone(),
                    _ => String::new(),
                },
            },
        ) {
            Ok(QuestionInput::Submit(bytes)) => bytes,
            _ => return false,
        };
        if self.transport.send_input(&input).is_err() {
            self.lifecycle = AntigravityLifecycle::Exited {
                exit_code: Some(-1),
            };
            return true;
        }
        // The entry control echoes the typed text progressively; that is not a new request.
        self.last_consumed = Some(ConsumedRequest {
            state: detected.clone(),
            grace_until: now + Duration::seconds(CONSUMED_GRACE_SECS),
        });
        self.lifecycle = AntigravityLifecycle::Working {
            details: "Write-in answer delivered to Antigravity".into(),
        };
        true
    }

    /// Process newly detected semantic state transitions and emit canonical AgentDesk events.
    fn handle_state_transition(
        &mut self,
        detected: AntigravityState,
        now: DateTime<Utc>,
        out: &mut Vec<AdapterOutput>,
    ) {
        if detected.is_blocking() {
            if self.try_complete_write_in(&detected, now) {
                return;
            }
            self.open_request(detected, now, out);
            return;
        }

        // Any non-blocking state means the previous request is no longer answerable.
        self.expire_pending();

        match detected {
            AntigravityState::Working { ref details } => {
                let should_emit = match &self.lifecycle {
                    AntigravityLifecycle::Working { details: prev } => prev != details,
                    _ => true,
                };
                if should_emit {
                    self.lifecycle = AntigravityLifecycle::Working {
                        details: details.clone(),
                    };
                    let ev = self.event(
                        "progress",
                        Operation::Other,
                        details.clone(),
                        Details::new(),
                        vec![],
                        None,
                    );
                    out.push(ev);
                }
            }

            AntigravityState::Completed { ref message } => {
                self.pending_write_in = None;
                if !matches!(self.lifecycle, AntigravityLifecycle::Completed { .. }) {
                    self.lifecycle = AntigravityLifecycle::Completed {
                        message: message.clone(),
                    };
                    let ev = self.event(
                        "task_completed",
                        Operation::Other,
                        message.clone(),
                        Details::new(),
                        vec![message.clone()],
                        None,
                    );
                    out.push(ev);
                }
            }

            AntigravityState::FatalError { ref error } => {
                self.pending_write_in = None;
                if !matches!(self.lifecycle, AntigravityLifecycle::Exited { .. }) {
                    self.lifecycle = AntigravityLifecycle::Exited {
                        exit_code: Some(-1),
                    };
                    let ev = self.event(
                        "command_failed",
                        Operation::Other,
                        error.clone(),
                        Details::new(),
                        vec![error.clone()],
                        None,
                    );
                    out.push(ev);
                }
            }

            AntigravityState::IdlePrompt => {
                self.pending_write_in = None;
                if self.lifecycle != AntigravityLifecycle::Running {
                    self.lifecycle = AntigravityLifecycle::Running;
                }
            }

            AntigravityState::Initializing => {}

            _ => unreachable!("blocking states handled above"),
        }
    }

    /// Drain all available PTY bytes into the screen; returns true if anything arrived.
    fn drain_transport(&mut self) -> bool {
        let mut received = false;
        while let Ok(Some(chunk)) = self.transport.try_recv() {
            received = true;
            self.screen.process_bytes(&chunk.bytes);
        }
        received
    }

    /// Encode a validated response for a live blocking state into keystrokes.
    fn encode_for_state(
        &self,
        live: &AntigravityState,
        response: &RequestResponse,
    ) -> Result<QuestionInput, RespondError> {
        match live {
            AntigravityState::UserQuestion { details, .. } => {
                encode_question_response(response, details)
            }
            AntigravityState::CommandConfirmation { .. }
            | AntigravityState::FileEditConfirmation { .. }
            | AntigravityState::WorkspaceTrust { .. } => {
                let decision = match response {
                    RequestResponse::Approve => Decision::Approve,
                    RequestResponse::Deny => Decision::Deny,
                    _ => return Err(RespondError::InvalidResponse),
                };
                Ok(QuestionInput::Submit(encode_decision(decision, live)))
            }
            _ => Err(RespondError::NotBlocked),
        }
    }
}

impl<T: PtyTransport> Adapter for AntigravityPtyAdapter<T> {
    fn agents(&self) -> &[AgentInfo] {
        &self.agents
    }

    fn poll(&mut self, now: DateTime<Utc>) -> Vec<AdapterOutput> {
        let mut out = std::mem::take(&mut self.buffered_outputs);

        // Initial session start notification on the first poll
        if self.lifecycle == AntigravityLifecycle::Uninitialized {
            self.lifecycle = AntigravityLifecycle::Starting;
            let msg = format!("Antigravity session {} started", self.task_id);
            let ev = self.event(
                "started",
                Operation::Other,
                msg,
                Details::new(),
                vec![],
                None,
            );
            out.push(ev);
        }

        // Drain incoming PTY byte chunks and update terminal screen grid
        let received_bytes = self.drain_transport();

        // Analyze screen snapshot only when new terminal bytes were received
        if received_bytes {
            let snapshot = self.screen.snapshot();
            let detected = detect_state(&snapshot);

            if detected != self.current_state {
                self.current_state = detected.clone();
                self.handle_state_transition(detected, now, &mut out);
            }
        }

        if let Some(pw) = &self.pending_write_in
            && now > pw.deadline
        {
            self.pending_write_in = None;
        }

        // A dialog that survived the post-answer grace period is a live request again
        // (e.g. Antigravity rejected or ignored the keystrokes).
        if self.pending.is_none()
            && self.pending_write_in.is_none()
            && self.current_state.is_blocking()
            && let Some(consumed) = &self.last_consumed
            && now >= consumed.grace_until
        {
            let state = self.current_state.clone();
            self.open_request(state, now, &mut out);
        }

        // Check if process terminated / PTY stream closed
        if !self.transport.is_alive()
            && !matches!(self.lifecycle, AntigravityLifecycle::Exited { .. })
        {
            let was_completed = matches!(self.lifecycle, AntigravityLifecycle::Completed { .. });
            self.lifecycle = AntigravityLifecycle::Exited { exit_code: None };
            self.expire_pending();
            self.pending_write_in = None;

            if !was_completed {
                let ev = self.event(
                    "adapter_error",
                    Operation::Other,
                    "Antigravity process disconnected or PTY closed".into(),
                    Details::new(),
                    vec![],
                    None,
                );
                out.push(ev);
            }
        }

        out
    }

    fn next_due(&self) -> Option<DateTime<Utc>> {
        match self.lifecycle {
            AntigravityLifecycle::Exited { .. } => None,
            AntigravityLifecycle::BlockedOnPermission { .. } => None,
            _ => {
                if !self.buffered_outputs.is_empty() {
                    Some(Utc::now())
                } else {
                    None
                }
            }
        }
    }

    fn respond(
        &mut self,
        task_id: &TaskId,
        decision: Decision,
        now: DateTime<Utc>,
    ) -> Result<(), RespondError> {
        let resp = RequestResponse::from_decision(decision);
        self.respond_with_response(task_id, None, &resp, now)
    }

    fn respond_with_response(
        &mut self,
        task_id: &TaskId,
        request_seq: Option<u64>,
        response: &RequestResponse,
        now: DateTime<Utc>,
    ) -> Result<(), RespondError> {
        // 1. Task/session identity
        if &self.task_id != task_id {
            return Err(RespondError::NoSuchTask);
        }

        // 2. PTY transport must be alive (checked before anything else is derived)
        if !self.transport.is_alive() {
            self.expire_pending();
            return Err(RespondError::Disconnected);
        }

        // 3. A request must be pending (distinguish consumed / expired / never)
        let Some(pending) = self.pending.clone() else {
            return Err(match self.last_outcome {
                Some(RequestOutcome::Consumed) => RespondError::AlreadyConsumed,
                Some(RequestOutcome::Expired) => RespondError::StateMismatch,
                None => RespondError::NotBlocked,
            });
        };

        // 4. The response must target this pending request, not a superseded one
        if let Some(seq) = request_seq
            && seq != pending.agent_seq
        {
            return Err(RespondError::WrongRequest);
        }

        // 5. The live TUI must still show the same request
        self.drain_transport();
        let live = detect_state(&self.screen.snapshot());
        if !pending.state.same_request(&live) {
            self.current_state = live;
            self.expire_pending();
            return Err(RespondError::StateMismatch);
        }

        // 6. Response validity for this request type, encoded against live cursor state
        let input = self.encode_for_state(&live, response)?;

        // 7. Deliver validated keystrokes
        let (bytes, write_in) = match input {
            QuestionInput::Submit(bytes) => (bytes, None),
            QuestionInput::OpenWriteIn {
                bytes,
                pending_text,
            } => (bytes, Some(pending_text)),
        };
        self.transport.send_input(&bytes).map_err(|_| {
            self.lifecycle = AntigravityLifecycle::Exited {
                exit_code: Some(-1),
            };
            self.pending = None;
            self.last_outcome = Some(RequestOutcome::Expired);
            RespondError::Disconnected
        })?;

        // 8. Mark consumed
        self.pending = None;
        self.last_outcome = Some(RequestOutcome::Consumed);
        self.last_consumed = Some(ConsumedRequest {
            state: live.clone(),
            grace_until: now + Duration::seconds(CONSUMED_GRACE_SECS),
        });
        if let (Some(text), AntigravityState::UserQuestion { question, .. }) = (write_in, &live) {
            self.pending_write_in = Some(PendingWriteIn {
                question: question.clone(),
                text,
                deadline: now + Duration::seconds(10),
            });
            self.lifecycle = AntigravityLifecycle::Working {
                details: "Opening write-in answer entry".into(),
            };
        } else {
            self.lifecycle = AntigravityLifecycle::Working {
                details: format!(
                    "Response delivered to Antigravity (generation {})",
                    pending.generation
                ),
            };
        }

        Ok(())
    }

    fn is_finished(&self) -> bool {
        matches!(self.lifecycle, AntigravityLifecycle::Exited { .. })
    }
}
