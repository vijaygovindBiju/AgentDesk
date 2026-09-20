//! Real-Agent Adapter implementing the Agent Client Protocol (ACP) v1.
//!
//! Bridges live coding agents (such as Gemini CLI `gemini --acp`, Devin, OpenCode)
//! with the AgentDesk `Adapter` interface over standard I/O (JSON-RPC 2.0).
//!
//! Terminal text (unformatted stdout/stderr) is strictly forwarded as bounded log
//! lines (`AdapterOutput::Line`). Attention categories are generated solely from
//! documented structured ACP protocol events:
//! - `session/request_permission` -> `Request` (`approval_required`)
//! - `session/prompt` completion -> `Completed` (`task_completed`)
//! - JSON-RPC errors, non-zero exit codes, crash/EOF -> `Error` (`command_failed` / `adapter_error`)
//! - Turn start and `tool_call` updates -> `Working` (`started` / `progress`)

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use agentdesk_model::{
    AdapterKind, AgentId, AgentInfo, Decision, Details, Operation, RawAgentEvent, RequestInfo,
    TaskId,
};

use crate::adapter::{Adapter, AdapterOutput, RespondError};

/// Message received from the ACP transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportMessage {
    Stdout(String),
    Stderr(String),
    Eof,
}

/// Abstract transport for ACP communication. Allows real child process execution
/// or deterministic in-memory test fixtures.
pub trait AcpTransport: Send {
    /// Send a single line (JSON-RPC message) to the agent.
    fn send_line(&mut self, line: &str) -> std::io::Result<()>;
    /// Try reading an incoming message without blocking.
    fn try_recv(&mut self) -> Result<Option<TransportMessage>, std::io::Error>;
    /// Check if the underlying agent process/stream is still alive.
    fn is_alive(&self) -> bool;
    /// Terminate the underlying process/stream.
    fn close(&mut self);
}

/// Real child process transport over stdio.
pub struct ProcessTransport {
    stdin: Option<ChildStdin>,
    child: Option<Arc<Mutex<Child>>>,
    rx: Receiver<TransportMessage>,
    alive: Arc<AtomicBool>,
    eof_sent: bool,
}

impl ProcessTransport {
    pub fn spawn(
        command: &str,
        args: &[String],
        cwd: Option<&PathBuf>,
    ) -> std::io::Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let (tx, rx) = channel();
        let alive = Arc::new(AtomicBool::new(true));

        // Background reader thread for stdout
        if let Some(out) = stdout {
            let tx_out = tx.clone();
            let alive_out = alive.clone();
            thread::Builder::new()
                .name("acp-stdout".into())
                .spawn(move || {
                    let reader = BufReader::new(out);
                    for line in reader.lines() {
                        match line {
                            Ok(l) => {
                                if tx_out.send(TransportMessage::Stdout(l)).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    alive_out.store(false, Ordering::SeqCst);
                    let _ = tx_out.send(TransportMessage::Eof);
                })?;
        }

        // Background reader thread for stderr
        if let Some(err) = stderr {
            let tx_err = tx;
            thread::Builder::new()
                .name("acp-stderr".into())
                .spawn(move || {
                    let reader = BufReader::new(err);
                    for line in reader.lines() {
                        match line {
                            Ok(l) => {
                                if tx_err.send(TransportMessage::Stderr(l)).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                })?;
        }

        Ok(Self {
            stdin,
            child: Some(Arc::new(Mutex::new(child))),
            rx,
            alive,
            eof_sent: false,
        })
    }
}

impl AcpTransport for ProcessTransport {
    fn send_line(&mut self, line: &str) -> std::io::Result<()> {
        if !self.is_alive() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Agent process is no longer running",
            ));
        }
        if let Some(stdin) = self.stdin.as_mut() {
            stdin.write_all(line.as_bytes())?;
            if !line.ends_with('\n') {
                stdin.write_all(b"\n")?;
            }
            stdin.flush()?;
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Process stdin closed",
            ))
        }
    }

    fn try_recv(&mut self) -> Result<Option<TransportMessage>, std::io::Error> {
        match self.rx.try_recv() {
            Ok(msg) => {
                if msg == TransportMessage::Eof {
                    self.eof_sent = true;
                }
                Ok(Some(msg))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                self.alive.store(false, Ordering::SeqCst);
                if !self.eof_sent {
                    self.eof_sent = true;
                    Ok(Some(TransportMessage::Eof))
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn is_alive(&self) -> bool {
        if !self.alive.load(Ordering::SeqCst) {
            return false;
        }
        if let Some(ref child_mutex) = self.child {
            if let Ok(mut child) = child_mutex.lock() {
                match child.try_wait() {
                    Ok(Some(_status)) => {
                        self.alive.store(false, Ordering::SeqCst);
                        false
                    }
                    Ok(None) => true,
                    Err(_) => {
                        self.alive.store(false, Ordering::SeqCst);
                        false
                    }
                }
            } else {
                false
            }
        } else {
            false
        }
    }

    fn close(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        self.stdin.take(); // Close stdin to signal EOF to child
        if let Some(child_mutex) = self.child.take()
            && let Ok(mut child) = child_mutex.lock()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for ProcessTransport {
    fn drop(&mut self) {
        self.close();
    }
}

/// Deterministic in-memory fake transport for reproducible adapter tests.
pub struct MockAcpTransport {
    pub incoming: VecDeque<TransportMessage>,
    pub sent_lines: Vec<String>,
    pub alive: bool,
}

impl MockAcpTransport {
    pub fn new() -> Self {
        Self {
            incoming: VecDeque::new(),
            sent_lines: Vec::new(),
            alive: true,
        }
    }

    pub fn push_stdout(&mut self, text: impl Into<String>) {
        self.incoming.push_back(TransportMessage::Stdout(text.into()));
    }

    pub fn push_stderr(&mut self, text: impl Into<String>) {
        self.incoming.push_back(TransportMessage::Stderr(text.into()));
    }

    pub fn push_eof(&mut self) {
        self.incoming.push_back(TransportMessage::Eof);
    }
}

impl Default for MockAcpTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl AcpTransport for MockAcpTransport {
    fn send_line(&mut self, line: &str) -> std::io::Result<()> {
        if !self.alive {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Mock process dead",
            ));
        }
        self.sent_lines.push(line.trim_end_matches('\n').to_string());
        Ok(())
    }

    fn try_recv(&mut self) -> Result<Option<TransportMessage>, std::io::Error> {
        Ok(self.incoming.pop_front())
    }

    fn is_alive(&self) -> bool {
        self.alive
    }

    fn close(&mut self) {
        self.alive = false;
    }
}

/// Configuration for ACP adapter initialization and execution.
#[derive(Debug, Clone)]
pub struct AcpConfig {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub agent_id: AgentId,
    pub agent_name: String,
    pub project: String,
    pub initial_prompt: String,
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            command: "gemini".into(),
            args: vec!["--skip-trust".into(), "--acp".into()],
            cwd: None,
            agent_id: "gemini-cli".into(),
            agent_name: "Gemini CLI".into(),
            project: "AgentDesk".into(),
            initial_prompt: "Review workspace status.".into(),
        }
    }
}

/// Permission option offered in `session/request_permission`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpPermissionOption {
    #[serde(rename = "optionId")]
    pub option_id: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

/// Content inside an ACP tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpToolCallContent {
    #[serde(rename = "type", default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub content: Option<AcpTextContent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpTextContent {
    #[serde(rename = "type", default)]
    pub text_type: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}

/// Tool call details within `session/request_permission` or `session/update`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpToolCall {
    #[serde(rename = "toolCallId", default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub content: Option<Vec<AcpToolCallContent>>,
}

/// Params of `session/request_permission`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpPermissionParams {
    #[serde(rename = "sessionId", default)]
    pub session_id: Option<String>,
    #[serde(rename = "toolCall", default)]
    pub tool_call: Option<AcpToolCall>,
    #[serde(default)]
    pub options: Vec<AcpPermissionOption>,
}

/// Internal state machine for the ACP adapter lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpState {
    Uninitialized,
    Initializing,
    CreatingSession,
    Prompting,
    BlockedOnPermission {
        req_id: Value,
        options: Vec<AcpPermissionOption>,
    },
    Finished,
}

/// Real-agent adapter implementing the Agent Client Protocol (ACP).
pub struct AcpAdapter<T: AcpTransport> {
    config: AcpConfig,
    agents: Vec<AgentInfo>,
    state: AcpState,
    agent_seq: u64,
    task_id: Option<TaskId>,
    transport: T,
    buffered_outputs: Vec<AdapterOutput>,
}

impl AcpAdapter<ProcessTransport> {
    /// Spawn a real agent process (e.g. Gemini CLI) using `ProcessTransport`.
    pub fn spawn(config: AcpConfig) -> std::io::Result<Self> {
        let transport = ProcessTransport::spawn(&config.command, &config.args, config.cwd.as_ref())?;
        Ok(Self::new_with_transport(config, transport))
    }
}

impl<T: AcpTransport> AcpAdapter<T> {
    pub fn new_with_transport(config: AcpConfig, transport: T) -> Self {
        let agent = AgentInfo {
            agent_id: config.agent_id.clone(),
            name: config.agent_name.clone(),
            project: config.project.clone(),
            adapter_kind: AdapterKind::Acp,
        };
        Self {
            config,
            agents: vec![agent],
            state: AcpState::Uninitialized,
            agent_seq: 0,
            task_id: None,
            transport,
            buffered_outputs: Vec::new(),
        }
    }

    pub fn state(&self) -> &AcpState {
        &self.state
    }

    pub fn current_task_id(&self) -> Option<&TaskId> {
        self.task_id.as_ref()
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    fn send_initialize(&mut self) -> Result<(), std::io::Error> {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": 1,
                "clientInfo": {
                    "name": "AgentDesk",
                    "version": "0.1.0"
                }
            }
        });
        let line = serde_json::to_string(&req).map_err(std::io::Error::other)?;
        self.transport.send_line(&line)?;
        self.state = AcpState::Initializing;
        Ok(())
    }

    fn send_session_new(&mut self) -> Result<(), std::io::Error> {
        let cwd_str = self
            .config
            .cwd
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".into());
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session/new",
            "params": {
                "cwd": cwd_str,
                "mcpServers": []
            }
        });
        let line = serde_json::to_string(&req).map_err(std::io::Error::other)?;
        self.transport.send_line(&line)?;
        self.state = AcpState::CreatingSession;
        Ok(())
    }

    fn send_session_prompt(&mut self, session_id: &str) -> Result<(), std::io::Error> {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "session/prompt",
            "params": {
                "sessionId": session_id,
                "prompt": [
                    {
                        "type": "text",
                        "text": self.config.initial_prompt
                    }
                ]
            }
        });
        let line = serde_json::to_string(&req).map_err(std::io::Error::other)?;
        self.transport.send_line(&line)?;
        self.state = AcpState::Prompting;
        Ok(())
    }

    fn map_tool_kind(kind: Option<&str>) -> Operation {
        match kind {
            Some("edit") | Some("delete") | Some("move") => Operation::Edit,
            Some("read") | Some("search") => Operation::Analyze,
            Some("execute") => Operation::Build,
            _ => Operation::Other,
        }
    }

    fn process_jsonrpc(&mut self, val: Value, raw_line: String, out: &mut Vec<AdapterOutput>) {
        // 1. Is it a request initiated by the agent?
        if let Some(method) = val.get("method").and_then(|m| m.as_str()) {
            if method == "session/request_permission" {
                let req_id = val.get("id").cloned().unwrap_or(Value::Null);
                if let Some(params_val) = val.get("params")
                    && let Ok(params) = serde_json::from_value::<AcpPermissionParams>(params_val.clone())
                {
                    let title = params
                        .tool_call
                        .as_ref()
                        .and_then(|t| t.title.clone())
                        .or_else(|| {
                            params.tool_call.as_ref().and_then(|t| {
                                t.content.as_ref().and_then(|c| {
                                    c.first()
                                        .and_then(|item| item.content.as_ref())
                                        .and_then(|tc| tc.text.clone())
                                })
                            })
                        })
                        .unwrap_or_else(|| "Tool approval requested".into());

                    let op = Self::map_tool_kind(
                        params.tool_call.as_ref().and_then(|t| t.kind.as_deref()),
                    );

                    let mut details = Details::new();
                    if let Some(ref tc) = params.tool_call {
                        if let Some(ref tid) = tc.tool_call_id {
                            details.insert("tool_call_id".into(), Value::String(tid.clone()));
                        }
                        if let Some(ref k) = tc.kind {
                            details.insert("kind".into(), Value::String(k.clone()));
                        }
                        if let Some(ref t) = tc.title {
                            details.insert("title".into(), Value::String(t.clone()));
                        }
                    }

                    let request_info = RequestInfo {
                        prompt: title.clone(),
                        options: vec!["approve".into(), "deny".into()],
                    };

                    self.agent_seq += 1;
                    self.state = AcpState::BlockedOnPermission {
                        req_id,
                        options: params.options,
                    };

                    out.push(AdapterOutput::Event(RawAgentEvent {
                        agent_id: self.config.agent_id.clone(),
                        agent_seq: self.agent_seq,
                        task_id: self.task_id.clone(),
                        kind: "approval_required".into(),
                        operation: op,
                        message: title,
                        details,
                        log_lines: vec![raw_line],
                        request: Some(request_info),
                    }));
                    return;
                }
            }

            // 2. Is it a notification from the agent?
            if method == "session/update" {
                if let Some(update) = val.get("params").and_then(|p| p.get("update")) {
                    let update_type = update
                        .get("sessionUpdate")
                        .and_then(|u| u.as_str())
                        .unwrap_or("");

                    match update_type {
                        "agent_message_chunk" | "agent_thought_chunk" => {
                            if let Some(text) = update
                                .get("content")
                                .and_then(|c| c.get("text"))
                                .and_then(|t| t.as_str())
                            {
                                for line in text.lines() {
                                    out.push(AdapterOutput::Line {
                                        agent_id: self.config.agent_id.clone(),
                                        text: line.to_string(),
                                    });
                                }
                            }
                        }
                        "tool_call" => {
                            let title = update
                                .get("title")
                                .and_then(|t| t.as_str())
                                .unwrap_or("tool execution");
                            let status = update
                                .get("status")
                                .and_then(|s| s.as_str())
                                .unwrap_or("");
                            let kind = update.get("kind").and_then(|k| k.as_str());
                            let op = Self::map_tool_kind(kind);

                            if status == "in_progress" {
                                self.agent_seq += 1;
                                out.push(AdapterOutput::Event(RawAgentEvent {
                                    agent_id: self.config.agent_id.clone(),
                                    agent_seq: self.agent_seq,
                                    task_id: self.task_id.clone(),
                                    kind: "progress".into(),
                                    operation: op,
                                    message: format!("Tool executing: {}", title),
                                    details: Details::new(),
                                    log_lines: vec![raw_line],
                                    request: None,
                                }));
                            } else {
                                out.push(AdapterOutput::Line {
                                    agent_id: self.config.agent_id.clone(),
                                    text: format!("Tool call [{}]: {}", status, title),
                                });
                            }
                        }
                        _ => {
                            // Any other session update logged purely as text
                            out.push(AdapterOutput::Line {
                                agent_id: self.config.agent_id.clone(),
                                text: raw_line,
                            });
                        }
                    }
                }
                return;
            }

            // Other unknown notifications/methods logged purely as noise
            out.push(AdapterOutput::Line {
                agent_id: self.config.agent_id.clone(),
                text: raw_line,
            });
            return;
        }

        // 3. Is it a response to one of our client requests?
        if let Some(id_num) = val.get("id").and_then(|i| i.as_i64()) {
            if let Some(err) = val.get("error") {
                let err_msg = err
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("ACP error response");
                self.agent_seq += 1;
                self.state = AcpState::Finished;
                out.push(AdapterOutput::Event(RawAgentEvent {
                    agent_id: self.config.agent_id.clone(),
                    agent_seq: self.agent_seq,
                    task_id: self.task_id.clone(),
                    kind: "command_failed".into(),
                    operation: Operation::Other,
                    message: format!("ACP error on request {}: {}", id_num, err_msg),
                    details: Details::new(),
                    log_lines: vec![raw_line],
                    request: None,
                }));
                return;
            }

            if let Some(res) = val.get("result") {
                match id_num {
                    1 => {
                        // Response to initialize -> send session/new
                        if let Err(e) = self.send_session_new() {
                            self.agent_seq += 1;
                            self.state = AcpState::Finished;
                            out.push(AdapterOutput::Event(RawAgentEvent {
                                agent_id: self.config.agent_id.clone(),
                                agent_seq: self.agent_seq,
                                task_id: self.task_id.clone(),
                                kind: "adapter_error".into(),
                                operation: Operation::Other,
                                message: format!("Failed to send session/new: {}", e),
                                details: Details::new(),
                                log_lines: vec![],
                                request: None,
                            }));
                        }
                    }
                    2 => {
                        // Response to session/new -> extract sessionId and send prompt
                        if let Some(sid) = res.get("sessionId").and_then(|s| s.as_str()) {
                            let session_id = sid.to_string();
                            self.task_id = Some(session_id.clone());
                            self.agent_seq += 1;
                            out.push(AdapterOutput::Event(RawAgentEvent {
                                agent_id: self.config.agent_id.clone(),
                                agent_seq: self.agent_seq,
                                task_id: self.task_id.clone(),
                                kind: "started".into(),
                                operation: Operation::Other,
                                message: format!("Session {} started", session_id),
                                details: Details::new(),
                                log_lines: vec![raw_line],
                                request: None,
                            }));

                            if let Err(e) = self.send_session_prompt(&session_id) {
                                self.agent_seq += 1;
                                self.state = AcpState::Finished;
                                out.push(AdapterOutput::Event(RawAgentEvent {
                                    agent_id: self.config.agent_id.clone(),
                                    agent_seq: self.agent_seq,
                                    task_id: self.task_id.clone(),
                                    kind: "adapter_error".into(),
                                    operation: Operation::Other,
                                    message: format!("Failed to send session/prompt: {}", e),
                                    details: Details::new(),
                                    log_lines: vec![],
                                    request: None,
                                }));
                            }
                        }
                    }
                    3 => {
                        // Response to session/prompt -> completed
                        let stop_reason = res
                            .get("stopReason")
                            .and_then(|s| s.as_str())
                            .unwrap_or("end_turn");
                        self.agent_seq += 1;
                        self.state = AcpState::Finished;
                        out.push(AdapterOutput::Event(RawAgentEvent {
                            agent_id: self.config.agent_id.clone(),
                            agent_seq: self.agent_seq,
                            task_id: self.task_id.clone(),
                            kind: "task_completed".into(),
                            operation: Operation::Other,
                            message: format!("Prompt completed ({})", stop_reason),
                            details: Details::new(),
                            log_lines: vec![raw_line],
                            request: None,
                        }));
                    }
                    _ => {
                        out.push(AdapterOutput::Line {
                            agent_id: self.config.agent_id.clone(),
                            text: raw_line,
                        });
                    }
                }
                return;
            }
        }

        // Catch-all pure log line
        out.push(AdapterOutput::Line {
            agent_id: self.config.agent_id.clone(),
            text: raw_line,
        });
    }
}

impl<T: AcpTransport> Adapter for AcpAdapter<T> {
    fn agents(&self) -> &[AgentInfo] {
        &self.agents
    }

    fn poll(&mut self, _now: DateTime<Utc>) -> Vec<AdapterOutput> {
        let mut out = std::mem::take(&mut self.buffered_outputs);

        // Start initialization handshake on the very first poll
        if self.state == AcpState::Uninitialized
            && let Err(e) = self.send_initialize()
        {
            self.agent_seq += 1;
                self.state = AcpState::Finished;
                out.push(AdapterOutput::Event(RawAgentEvent {
                    agent_id: self.config.agent_id.clone(),
                    agent_seq: self.agent_seq,
                    task_id: self.task_id.clone(),
                    kind: "adapter_error".into(),
                    operation: Operation::Other,
                    message: format!("Failed to send initialize to ACP agent: {}", e),
                    details: Details::new(),
                    log_lines: vec![],
                    request: None,
                }));
                return out;
        }

        while let Ok(Some(msg)) = self.transport.try_recv() {
            match msg {
                TransportMessage::Stderr(line) => {
                    out.push(AdapterOutput::Line {
                        agent_id: self.config.agent_id.clone(),
                        text: line,
                    });
                }
                TransportMessage::Stdout(line) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Value>(trimmed) {
                        Ok(json_val) => {
                            self.process_jsonrpc(json_val, line, &mut out);
                        }
                        Err(_) => {
                            // Non-JSON terminal output is forwarded purely as a log line
                            out.push(AdapterOutput::Line {
                                agent_id: self.config.agent_id.clone(),
                                text: line,
                            });
                        }
                    }
                }
                TransportMessage::Eof => {
                    if self.state != AcpState::Finished {
                        self.agent_seq += 1;
                        self.state = AcpState::Finished;
                        out.push(AdapterOutput::Event(RawAgentEvent {
                            agent_id: self.config.agent_id.clone(),
                            agent_seq: self.agent_seq,
                            task_id: self.task_id.clone(),
                            kind: "adapter_error".into(),
                            operation: Operation::Other,
                            message: "Agent process disconnected or closed stream".into(),
                            details: Details::new(),
                            log_lines: vec![],
                            request: None,
                        }));
                    }
                }
            }
        }

        out
    }

    fn next_due(&self) -> Option<DateTime<Utc>> {
        match self.state {
            AcpState::Finished => None,
            AcpState::BlockedOnPermission { .. } => None,
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
        if let Some(ref current_task) = self.task_id {
            if current_task != task_id {
                return Err(RespondError::NoSuchTask);
            }
        } else {
            return Err(RespondError::NoSuchTask);
        }

        // Reject if the agent process is dead or disconnected
        if !self.transport.is_alive() {
            if self.state != AcpState::Finished {
                self.agent_seq += 1;
                self.state = AcpState::Finished;
                self.buffered_outputs.push(AdapterOutput::Event(RawAgentEvent {
                    agent_id: self.config.agent_id.clone(),
                    agent_seq: self.agent_seq,
                    task_id: self.task_id.clone(),
                    kind: "adapter_error".into(),
                    operation: Operation::Other,
                    message: "Cannot deliver response: agent process is no longer running".into(),
                    details: Details::new(),
                    log_lines: vec![],
                    request: None,
                }));
            }
            return Err(RespondError::NotBlocked);
        }

        let (req_id, options) = match &self.state {
            AcpState::BlockedOnPermission { req_id, options } => {
                (req_id.clone(), options.clone())
            }
            _ => return Err(RespondError::NotBlocked),
        };

        let response_body = match decision {
            Decision::Approve => {
                let opt_id = options
                    .iter()
                    .find(|o| {
                        o.kind.as_deref() == Some("allow_once")
                            || o.kind.as_deref() == Some("allow_always")
                            || o.option_id == "proceed_once"
                            || o.name.as_deref().unwrap_or("").to_lowercase().contains("allow")
                            || o.name.as_deref().unwrap_or("").to_lowercase().contains("proceed")
                    })
                    .map(|o| o.option_id.clone())
                    .unwrap_or_else(|| {
                        options
                            .first()
                            .map(|o| o.option_id.clone())
                            .unwrap_or_else(|| "proceed_once".into())
                    });

                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "outcome": {
                            "outcome": "selected",
                            "optionId": opt_id
                        }
                    }
                })
            }
            Decision::Deny => {
                let reject_opt = options.iter().find(|o| {
                    o.kind.as_deref() == Some("reject_once")
                        || o.kind.as_deref() == Some("reject_always")
                        || o.option_id == "cancel"
                        || o.name.as_deref().unwrap_or("").to_lowercase().contains("cancel")
                        || o.name.as_deref().unwrap_or("").to_lowercase().contains("reject")
                        || o.name.as_deref().unwrap_or("").to_lowercase().contains("deny")
                });

                if let Some(opt) = reject_opt {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "result": {
                            "outcome": {
                                "outcome": "selected",
                                "optionId": opt.option_id
                            }
                        }
                    })
                } else {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "result": {
                            "outcome": {
                                "outcome": "cancelled"
                            }
                        }
                    })
                }
            }
        };

        let line = serde_json::to_string(&response_body)
            .map_err(|_| RespondError::NotBlocked)?;

        self.transport.send_line(&line).map_err(|_| {
            self.state = AcpState::Finished;
            RespondError::NotBlocked
        })?;

        // Transition back to Prompting awaiting further updates or turn completion
        self.state = AcpState::Prompting;
        Ok(())
    }

    fn is_finished(&self) -> bool {
        self.state == AcpState::Finished
    }
}
