//! Agent identity as known to the daemon. See docs/DATA_MODEL.md "Agent".

use serde::{Deserialize, Serialize};

use crate::event::AgentId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    Simulator,
    ClaudeCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub agent_id: AgentId,
    /// Human label, e.g. "Backend Agent".
    pub name: String,
    /// Human label, e.g. "Hybrid".
    pub project: String,
    pub adapter_kind: AdapterKind,
}
