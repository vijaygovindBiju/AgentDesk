//! Scenario file format for the simulated agent. JSON, validated on load.
//!
//! A scenario is a set of agents and tasks; each task is a sequence of steps
//! that produce raw events and log lines on a virtual timeline. The same
//! scenario and seed always produce the same output.

use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use agentdesk_core::classify;
use agentdesk_model::{AgentId, Category, Details, Operation, TaskId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub agents: Vec<AgentSpec>,
    pub tasks: Vec<TaskSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpec {
    pub agent_id: AgentId,
    pub name: String,
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskSpec {
    pub task_id: TaskId,
    pub agent_id: AgentId,
    pub title: String,
    pub operation: Operation,
    /// When the task starts, relative to scenario start.
    #[serde(default)]
    pub start_offset_ms: u64,
    pub steps: Vec<Step>,
}

/// One unit of scripted behaviour. Timing fields are virtual milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Step {
    /// Emit a `started` event.
    Started {
        #[serde(default)]
        message: Option<String>,
    },
    /// Emit `count` `progress` events, one every `interval_ms` (+ random
    /// jitter up to `jitter_ms`). `template` may use `{i}` and `{n}`.
    /// Each event also carries its message as a log line.
    Progress {
        count: u32,
        interval_ms: u64,
        #[serde(default)]
        jitter_ms: u64,
        template: String,
    },
    /// Emit raw log lines that are not events, one every `interval_ms`.
    Log {
        lines: Vec<String>,
        #[serde(default)]
        interval_ms: u64,
    },
    /// Silence.
    Wait { ms: u64 },
    /// Emit a request event and block until `respond` is called.
    /// Approve continues with the next step; deny emits `on_deny_kind`
    /// (default `cancelled_by_user`) and ends the task.
    Request {
        #[serde(default = "default_request_kind")]
        kind: String,
        prompt: String,
        #[serde(default = "default_options")]
        options: Vec<String>,
        #[serde(default)]
        message: Option<String>,
        #[serde(default = "default_on_deny_kind")]
        on_deny_kind: String,
    },
    /// Emit an arbitrary event. If `kind` classifies as `completed` or
    /// `error` the task ends here.
    Event {
        kind: String,
        message: String,
        #[serde(default)]
        details: Details,
        #[serde(default)]
        log_lines: Vec<String>,
        #[serde(default)]
        delay_ms: u64,
    },
}

fn default_request_kind() -> String {
    "approval_required".into()
}
fn default_options() -> Vec<String> {
    vec!["approve".into(), "deny".into()]
}
fn default_on_deny_kind() -> String {
    "cancelled_by_user".into()
}

/// True if an event of this kind ends a task.
pub fn is_terminal_kind(kind: &str) -> bool {
    matches!(classify(kind).category, Category::Completed | Category::Error)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioError {
    Parse(String),
    Io(String),
    DuplicateAgent(AgentId),
    DuplicateTask(TaskId),
    UnknownAgent { task_id: TaskId, agent_id: AgentId },
    EmptyTask(TaskId),
    ZeroCount { task_id: TaskId, step: usize },
    EmptyOptions { task_id: TaskId, step: usize },
    StepAfterTerminal { task_id: TaskId, step: usize },
    RequestKindNotRequest { task_id: TaskId, step: usize, kind: String },
    OnDenyKindNotTerminal { task_id: TaskId, step: usize, kind: String },
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ScenarioError {}

impl Scenario {
    pub fn from_json(json: &str) -> Result<Self, ScenarioError> {
        let s: Scenario = serde_json::from_str(json).map_err(|e| ScenarioError::Parse(e.to_string()))?;
        s.validate()?;
        Ok(s)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, ScenarioError> {
        let text = std::fs::read_to_string(path).map_err(|e| ScenarioError::Io(e.to_string()))?;
        Self::from_json(&text)
    }

    /// The committed default scenario (`scenarios/default.json`).
    pub fn default_scenario() -> Self {
        Self::from_json(include_str!("../scenarios/default.json")).expect("default scenario is valid")
    }

    pub fn validate(&self) -> Result<(), ScenarioError> {
        let mut agents = HashSet::new();
        for a in &self.agents {
            if !agents.insert(a.agent_id.as_str()) {
                return Err(ScenarioError::DuplicateAgent(a.agent_id.clone()));
            }
        }
        let mut tasks = HashSet::new();
        for t in &self.tasks {
            let tid = || t.task_id.clone();
            if !tasks.insert(t.task_id.as_str()) {
                return Err(ScenarioError::DuplicateTask(tid()));
            }
            if !agents.contains(t.agent_id.as_str()) {
                return Err(ScenarioError::UnknownAgent { task_id: tid(), agent_id: t.agent_id.clone() });
            }
            if t.steps.is_empty() {
                return Err(ScenarioError::EmptyTask(tid()));
            }
            let mut terminated = false;
            for (i, s) in t.steps.iter().enumerate() {
                if terminated {
                    return Err(ScenarioError::StepAfterTerminal { task_id: tid(), step: i });
                }
                match s {
                    Step::Progress { count: 0, .. } => return Err(ScenarioError::ZeroCount { task_id: tid(), step: i }),
                    Step::Request { kind, options, on_deny_kind, .. } => {
                        if classify(kind).category != Category::Request {
                            return Err(ScenarioError::RequestKindNotRequest { task_id: tid(), step: i, kind: kind.clone() });
                        }
                        if options.is_empty() {
                            return Err(ScenarioError::EmptyOptions { task_id: tid(), step: i });
                        }
                        if !is_terminal_kind(on_deny_kind) {
                            return Err(ScenarioError::OnDenyKindNotTerminal { task_id: tid(), step: i, kind: on_deny_kind.clone() });
                        }
                    }
                    Step::Event { kind, .. } if is_terminal_kind(kind) => terminated = true,
                    _ => {}
                }
            }
        }
        Ok(())
    }
}
