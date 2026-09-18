//! Synchronous event pipeline tying together the processor, event store,
//! priority queue, log store, and metrics.
//! See docs/ARCHITECTURE.md and docs/SYSTEM_DESIGN.md "Data flow".

use chrono::{DateTime, Utc};

use agentdesk_model::{AgentInfo, Event, QueueEntry};

use crate::adapter::AdapterOutput;
use crate::event_store::EventStore;
use crate::log_store::{LogStore, LogStoreConfig};
use crate::metrics::Metrics;
use crate::processor::{EventProcessor, ProcessError};
use crate::queue::PriorityQueue;

pub struct Pipeline {
    pub processor: EventProcessor,
    pub event_store: EventStore,
    pub queue: PriorityQueue,
    pub log_store: LogStore,
    pub metrics: Metrics,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new(LogStoreConfig::default())
    }
}

impl Pipeline {
    pub fn new(log_config: LogStoreConfig) -> Self {
        Pipeline {
            processor: EventProcessor::new(),
            event_store: EventStore::new(),
            queue: PriorityQueue::new(),
            log_store: LogStore::new(log_config),
            metrics: Metrics::new(),
        }
    }

    pub fn register_agents(&mut self, agents: &[AgentInfo]) {
        self.processor.register_agents(agents);
    }

    /// Handle one piece of adapter output (a raw line or a raw event).
    pub fn handle_output(
        &mut self,
        output: AdapterOutput,
        now: DateTime<Utc>,
    ) -> Result<Option<(Event, QueueEntry)>, ProcessError> {
        match output {
            AdapterOutput::Line { agent_id, text } => {
                self.processor.process_line(
                    &agent_id,
                    text,
                    now,
                    &mut self.log_store,
                    &mut self.metrics,
                );
                Ok(None)
            }
            AdapterOutput::Event(raw) => {
                let pair = self.processor.process_raw_event(
                    raw,
                    now,
                    &mut self.event_store,
                    &mut self.queue,
                    &mut self.log_store,
                    &mut self.metrics,
                )?;
                Ok(Some(pair))
            }
        }
    }
}
