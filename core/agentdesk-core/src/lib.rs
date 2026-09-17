//! AgentDesk core: adapter seam, classification, and (in later phases) the
//! event pipeline, priority queue, log store, task tracker and metrics.
//! See docs/ARCHITECTURE.md.

pub mod adapter;
pub mod classifier;
pub mod clock;

pub use adapter::{Adapter, AdapterOutput, RespondError};
pub use classifier::{classify, Classification, Matched, Rule, RULES};
pub use clock::{Clock, SystemClock, VirtualClock};
