//! The seam between a coding agent and AgentDesk. Every agent — simulated
//! or real — is driven through this trait so the rest of the pipeline never
//! sees agent-specific data. See docs/ARCHITECTURE.md "Adapter".
//!
//! Adapters are passive and time-driven: the owner (core task or bench)
//! calls `poll(now)` to collect everything due, and `next_due()` to learn
//! how far to sleep or advance a virtual clock. This keeps simulation
//! deterministic and keeps the adapter free of threads and timers.

use chrono::{DateTime, Utc};

use agentdesk_model::{AgentId, AgentInfo, Decision, RawAgentEvent, RequestResponse, TaskId};

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
    /// The request is stale or already consumed.
    AlreadyConsumed,
    /// The current TUI state does not match the active request.
    StateMismatch,
    /// The response is invalid for the current request type (e.g. invalid option).
    InvalidResponse,
    /// PTY transport is disconnected or closed.
    Disconnected,
    /// The response targets a request that has been superseded by a newer one.
    WrongRequest,
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

    /// Deliver a normalized human response (which may include option selections or text input).
    /// `request_seq` is the `agent_seq` of the request event being answered, when known; adapters
    /// that track pending requests use it to reject responses aimed at a superseded request.
    /// Defaults to delegating to `respond` with `Decision::Approve` or `Decision::Deny`.
    fn respond_with_response(
        &mut self,
        task_id: &TaskId,
        request_seq: Option<u64>,
        response: &RequestResponse,
        now: DateTime<Utc>,
    ) -> Result<(), RespondError> {
        let _ = request_seq;
        let decision = match response {
            RequestResponse::Deny => Decision::Deny,
            _ => Decision::Approve,
        };
        self.respond(task_id, decision, now)
    }

    /// True once no further output can ever be produced.
    fn is_finished(&self) -> bool;
}
