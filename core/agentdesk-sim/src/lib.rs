//! AgentDesk simulated coding agent: a deterministic, scripted `Adapter`
//! that produces realistic noisy and important events for testing and
//! measurement. See docs/ARCHITECTURE.md "Adapter (trait) and Simulator".

pub mod driver;
pub mod scenario;
pub mod simulator;

pub use driver::{Timeline, run_to_end};
pub use scenario::{
    AgentSpec, GroundTruthEscalation, GroundTruthEvent, Scenario, ScenarioError,
    ScenarioGroundTruth, Step, TaskSpec,
};
pub use simulator::Simulator;
