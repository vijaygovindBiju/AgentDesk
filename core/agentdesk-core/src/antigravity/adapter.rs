//! Real-Agent Pseudo-Terminal (PTY) Adapter for Antigravity (`agy`).
//!
//! Bridges interactive Bubbletea terminal sessions into the normalized AgentDesk
//! adapter architecture. The phone client NEVER sees raw PTY byte streams or escape sequences;
//! all terminal activity is parsed into a VT100 2D screen grid on the host machine,
//! semantically classified into canonical AgentDesk events, and decisions from the
//! phone are encoded into deterministic terminal keystrokes.

use std::io;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::Value;

use agentdesk_model::{
    AdapterKind, AgentId, AgentInfo, Decision, Details, Operation, RawAgentEvent, RequestInfo,
    TaskId,
};

use crate::adapter::{Adapter, AdapterOutput, RespondError};
use crate::antigravity::input_encoder::encode_decision;
use crate::antigravity::pty::{PtySession, PtyTransport};
use crate::antigravity::screen::Screen;
use crate::antigravity::state_machine::{
    AntigravityState, classify_command_operation, detect_state,
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
    /// Agent is halted on an interactive confirmation menu (e.g. command approval).
    BlockedOnPermission { state: AntigravityState },
    /// Agent has completed its goal / turn.
    Completed { message: String },
    /// Process exited or fatal error.
    Exited { exit_code: Option<i32> },
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

    pub fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        self.screen.resize(cols, rows);
        self.transport.resize(cols, rows)
    }

    fn next_agent_seq(&mut self) -> u64 {
        self.agent_seq += 1;
        self.agent_seq
    }

    /// Process newly detected semantic state transitions and emit canonical AgentDesk events.
    fn handle_state_transition(
        &mut self,
        detected: AntigravityState,
        out: &mut Vec<AdapterOutput>,
    ) {
        match detected {
            AntigravityState::CommandConfirmation {
                ref command,
                selected_option_index,
                ..
            } => {
                if !matches!(
                    self.lifecycle,
                    AntigravityLifecycle::BlockedOnPermission { .. }
                ) {
                    self.lifecycle = AntigravityLifecycle::BlockedOnPermission {
                        state: detected.clone(),
                    };
                    let op = classify_command_operation(command);
                    let seq = self.next_agent_seq();
                    let mut details = Details::new();
                    details.insert("command".into(), Value::String(command.clone()));
                    details.insert("selected_option".into(), Value::from(selected_option_index));

                    out.push(AdapterOutput::Event(RawAgentEvent {
                        agent_id: self.config.agent_id.clone(),
                        agent_seq: seq,
                        task_id: Some(self.task_id.clone()),
                        kind: "approval_required".into(),
                        operation: op,
                        message: format!("Command requires approval: {}", command),
                        details,
                        log_lines: vec![format!("Command: {}", command)],
                        request: Some(RequestInfo {
                            prompt: command.clone(),
                            options: vec!["approve".into(), "deny".into()],
                        }),
                    }));
                }
            }

            AntigravityState::FileEditConfirmation {
                ref file_path,
                selected_option_index,
                ..
            } => {
                if !matches!(
                    self.lifecycle,
                    AntigravityLifecycle::BlockedOnPermission { .. }
                ) {
                    self.lifecycle = AntigravityLifecycle::BlockedOnPermission {
                        state: detected.clone(),
                    };
                    let seq = self.next_agent_seq();
                    let mut details = Details::new();
                    details.insert("file".into(), Value::String(file_path.clone()));
                    details.insert("selected_option".into(), Value::from(selected_option_index));

                    out.push(AdapterOutput::Event(RawAgentEvent {
                        agent_id: self.config.agent_id.clone(),
                        agent_seq: seq,
                        task_id: Some(self.task_id.clone()),
                        kind: "approval_required".into(),
                        operation: Operation::Edit,
                        message: format!("File edit requires approval: {}", file_path),
                        details,
                        log_lines: vec![format!("File: {}", file_path)],
                        request: Some(RequestInfo {
                            prompt: format!("Allow edit to {}?", file_path),
                            options: vec!["approve".into(), "deny".into()],
                        }),
                    }));
                }
            }

            AntigravityState::WorkspaceTrust { ref directory } => {
                if !matches!(
                    self.lifecycle,
                    AntigravityLifecycle::BlockedOnPermission { .. }
                ) {
                    self.lifecycle = AntigravityLifecycle::BlockedOnPermission {
                        state: detected.clone(),
                    };
                    let seq = self.next_agent_seq();
                    let mut details = Details::new();
                    details.insert("directory".into(), Value::String(directory.clone()));

                    out.push(AdapterOutput::Event(RawAgentEvent {
                        agent_id: self.config.agent_id.clone(),
                        agent_seq: seq,
                        task_id: Some(self.task_id.clone()),
                        kind: "approval_required".into(),
                        operation: Operation::Other,
                        message: format!("Workspace trust required for: {}", directory),
                        details,
                        log_lines: vec![format!("Trust requested for directory {}", directory)],
                        request: Some(RequestInfo {
                            prompt: format!("Trust directory {}?", directory),
                            options: vec!["approve".into(), "deny".into()],
                        }),
                    }));
                }
            }

            AntigravityState::UserQuestion {
                ref question,
                ref options,
            } => {
                if !matches!(
                    self.lifecycle,
                    AntigravityLifecycle::BlockedOnPermission { .. }
                ) {
                    self.lifecycle = AntigravityLifecycle::BlockedOnPermission {
                        state: detected.clone(),
                    };
                    let seq = self.next_agent_seq();
                    out.push(AdapterOutput::Event(RawAgentEvent {
                        agent_id: self.config.agent_id.clone(),
                        agent_seq: seq,
                        task_id: Some(self.task_id.clone()),
                        kind: "input_required".into(),
                        operation: Operation::Other,
                        message: question.clone(),
                        details: Details::new(),
                        log_lines: vec![question.clone()],
                        request: Some(RequestInfo {
                            prompt: question.clone(),
                            options: options.clone(),
                        }),
                    }));
                }
            }

            AntigravityState::Working { ref details } => {
                let should_emit = match &self.lifecycle {
                    AntigravityLifecycle::Working { details: prev } => prev != details,
                    _ => true,
                };
                if should_emit {
                    self.lifecycle = AntigravityLifecycle::Working {
                        details: details.clone(),
                    };
                    let seq = self.next_agent_seq();
                    out.push(AdapterOutput::Event(RawAgentEvent {
                        agent_id: self.config.agent_id.clone(),
                        agent_seq: seq,
                        task_id: Some(self.task_id.clone()),
                        kind: "progress".into(),
                        operation: Operation::Other,
                        message: details.clone(),
                        details: Details::new(),
                        log_lines: vec![],
                        request: None,
                    }));
                }
            }

            AntigravityState::Completed { ref message } => {
                if !matches!(self.lifecycle, AntigravityLifecycle::Completed { .. }) {
                    self.lifecycle = AntigravityLifecycle::Completed {
                        message: message.clone(),
                    };
                    let seq = self.next_agent_seq();
                    out.push(AdapterOutput::Event(RawAgentEvent {
                        agent_id: self.config.agent_id.clone(),
                        agent_seq: seq,
                        task_id: Some(self.task_id.clone()),
                        kind: "task_completed".into(),
                        operation: Operation::Other,
                        message: message.clone(),
                        details: Details::new(),
                        log_lines: vec![message.clone()],
                        request: None,
                    }));
                }
            }

            AntigravityState::FatalError { ref error } => {
                if !matches!(self.lifecycle, AntigravityLifecycle::Exited { .. }) {
                    self.lifecycle = AntigravityLifecycle::Exited {
                        exit_code: Some(-1),
                    };
                    let seq = self.next_agent_seq();
                    out.push(AdapterOutput::Event(RawAgentEvent {
                        agent_id: self.config.agent_id.clone(),
                        agent_seq: seq,
                        task_id: Some(self.task_id.clone()),
                        kind: "command_failed".into(),
                        operation: Operation::Other,
                        message: error.clone(),
                        details: Details::new(),
                        log_lines: vec![error.clone()],
                        request: None,
                    }));
                }
            }

            AntigravityState::IdlePrompt => {
                if self.lifecycle != AntigravityLifecycle::Running {
                    self.lifecycle = AntigravityLifecycle::Running;
                }
            }

            AntigravityState::Initializing => {}
        }
    }
}

impl<T: PtyTransport> Adapter for AntigravityPtyAdapter<T> {
    fn agents(&self) -> &[AgentInfo] {
        &self.agents
    }

    fn poll(&mut self, _now: DateTime<Utc>) -> Vec<AdapterOutput> {
        let mut out = std::mem::take(&mut self.buffered_outputs);

        // Initial session start notification on the first poll
        if self.lifecycle == AntigravityLifecycle::Uninitialized {
            self.lifecycle = AntigravityLifecycle::Starting;
            let seq = self.next_agent_seq();
            out.push(AdapterOutput::Event(RawAgentEvent {
                agent_id: self.config.agent_id.clone(),
                agent_seq: seq,
                task_id: Some(self.task_id.clone()),
                kind: "started".into(),
                operation: Operation::Other,
                message: format!("Antigravity session {} started", self.task_id),
                details: Details::new(),
                log_lines: vec![],
                request: None,
            }));
        }

        let mut received_bytes = false;

        // Drain incoming PTY byte chunks and update terminal screen grid
        while let Ok(Some(chunk)) = self.transport.try_recv() {
            received_bytes = true;
            self.screen.process_bytes(&chunk.bytes);
        }

        // Analyze screen snapshot only when new terminal bytes were received
        if received_bytes {
            let snapshot = self.screen.snapshot();
            let detected = detect_state(&snapshot);

            if detected != self.current_state {
                self.current_state = detected.clone();
                self.handle_state_transition(detected, &mut out);
            }
        }

        // Check if process terminated / PTY stream closed
        if !self.transport.is_alive()
            && !matches!(self.lifecycle, AntigravityLifecycle::Exited { .. })
        {
            let was_completed = matches!(self.lifecycle, AntigravityLifecycle::Completed { .. });
            self.lifecycle = AntigravityLifecycle::Exited { exit_code: None };

            if !was_completed {
                let seq = self.next_agent_seq();
                out.push(AdapterOutput::Event(RawAgentEvent {
                    agent_id: self.config.agent_id.clone(),
                    agent_seq: seq,
                    task_id: Some(self.task_id.clone()),
                    kind: "adapter_error".into(),
                    operation: Operation::Other,
                    message: "Antigravity process disconnected or PTY closed".into(),
                    details: Details::new(),
                    log_lines: vec![],
                    request: None,
                }));
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
        _now: DateTime<Utc>,
    ) -> Result<(), RespondError> {
        if &self.task_id != task_id {
            return Err(RespondError::NoSuchTask);
        }

        if !self.transport.is_alive() {
            return Err(RespondError::NotBlocked);
        }

        let blocked_state = match &self.lifecycle {
            AntigravityLifecycle::BlockedOnPermission { state } => state.clone(),
            _ => return Err(RespondError::NotBlocked),
        };

        // Translate decision to verified terminal keystrokes
        let input_bytes = encode_decision(decision, &blocked_state);

        self.transport.send_input(&input_bytes).map_err(|_| {
            self.lifecycle = AntigravityLifecycle::Exited {
                exit_code: Some(-1),
            };
            RespondError::NotBlocked
        })?;

        // Transition back to Working while Antigravity processes the input
        self.lifecycle = AntigravityLifecycle::Working {
            details: format!("Decision {:?} delivered to Antigravity", decision),
        };

        Ok(())
    }

    fn is_finished(&self) -> bool {
        matches!(self.lifecycle, AntigravityLifecycle::Exited { .. })
    }
}
