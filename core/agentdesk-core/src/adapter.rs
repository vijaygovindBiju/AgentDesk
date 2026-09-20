//! The seam between a coding agent and AgentDesk. Every agent — simulated
//! or real — is driven through this trait so the rest of the pipeline never
//! sees agent-specific data. See docs/ARCHITECTURE.md "Adapter".
//!
//! Adapters are passive and time-driven: the owner (core task or bench)
//! calls `poll(now)` to collect everything due, and `next_due()` to learn
//! how far to sleep or advance a virtual clock. This keeps simulation
//! deterministic and keeps the adapter free of threads and timers.

use chrono::{DateTime, Utc};

use agentdesk_model::{AgentId, AgentInfo, Decision, RawAgentEvent, TaskId};

#[derive(Debug, Clone, PartialEq)]
pub enum AdapterOutput {
    /// A raw event; its `log_lines` are also appended to the agent's log.
    Event(RawAgentEvent),
    /// A raw log line that is not itself an event (pure noise).
    Line { agent_id: AgentId, text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RespondError {
    NoSuchTask,
    /// The task exists but is not waiting for a decision.
    NotBlocked,
}

pub trait Adapter: Send {
    /// Agents this adapter speaks for. Stable for the adapter's lifetime.
    fn agents(&self) -> &[AgentInfo];

    /// Everything that became due at or before `now`, in emission order.
    fn poll(&mut self, now: DateTime<Utc>) -> Vec<AdapterOutput>;

    /// When the next output becomes due, or `None` if the adapter is
    /// finished or blocked waiting on `respond`.
    fn next_due(&self) -> Option<DateTime<Utc>>;

    /// Deliver a human decision to a task blocked on a request. `now` is
    /// when the decision arrived; the task resumes from that instant.
    fn respond(
        &mut self,
        task_id: &TaskId,
        decision: Decision,
        now: DateTime<Utc>,
    ) -> Result<(), RespondError>;

    /// True once no further output can ever be produced.
    fn is_finished(&self) -> bool;
}
