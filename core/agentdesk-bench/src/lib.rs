//! AgentDesk measurement bench library.
//! See docs/TODO.md Phase 5 and docs/SYSTEM_DESIGN.md "Pipeline modes".

pub mod client;
pub mod report;
pub mod runner;

pub use client::{ClientCounters, FakeClient, FakeClientSink, TapPolicy};
pub use report::{
    CategoryCoverage, CoverageSection, EscalationCoverage, MultiModeReport, ObservedEscalation,
    ReductionSection, SingleModeResult,
};
pub use runner::{ModeRunOutput, run_all_modes, run_bench_mode};
