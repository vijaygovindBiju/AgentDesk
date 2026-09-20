//! Log Store: bounded per-agent ring buffer and pinned log windows for
//! attention events. See docs/ARCHITECTURE.md and docs/DATA_MODEL.md.

use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, Utc};

use agentdesk_model::{AgentId, EventId, EventLogs, LOG_OFFSET_TAIL, LogLine, LogRange};

pub const DEFAULT_RING_CAPACITY: usize = 10_000;
pub const DEFAULT_PIN_BEFORE: u64 = 200;
pub const DEFAULT_PIN_AFTER: u64 = 50;
pub const DEFAULT_PAGE_CAP: u32 = 500;

#[derive(Debug, Clone)]
pub struct LogStoreConfig {
    pub ring_capacity: usize,
    pub pin_before: u64,
    pub pin_after: u64,
    pub page_cap: u32,
}

impl Default for LogStoreConfig {
    fn default() -> Self {
        LogStoreConfig {
            ring_capacity: DEFAULT_RING_CAPACITY,
            pin_before: DEFAULT_PIN_BEFORE,
            pin_after: DEFAULT_PIN_AFTER,
            page_cap: DEFAULT_PAGE_CAP,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedLogWindow {
    pub event_id: EventId,
    pub start_offset: u64,
    pub lines: Vec<LogLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogPage {
    pub offset: u64,
    pub total: u64,
    pub evicted: bool,
    pub lines: Vec<LogLine>,
}

#[derive(Debug)]
struct AgentBuffer {
    capacity: usize,
    next_offset: u64,
    lines: VecDeque<LogLine>,
}

impl AgentBuffer {
    fn new(capacity: usize) -> Self {
        AgentBuffer {
            capacity,
            next_offset: 0,
            lines: VecDeque::with_capacity(capacity.min(1024)),
        }
    }

    fn append(&mut self, text: String, ts: DateTime<Utc>) -> LogLine {
        let offset = self.next_offset;
        self.next_offset += 1;
        let line = LogLine { offset, ts, text };
        if self.lines.len() == self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(line.clone());
        line
    }

    fn oldest_offset(&self) -> u64 {
        self.lines
            .front()
            .map(|l| l.offset)
            .unwrap_or(self.next_offset)
    }

    fn page(&self, offset: u64, limit: u32, page_cap: u32) -> LogPage {
        let limit = limit.min(page_cap);
        let total = self.lines.len() as u64;
        let oldest = self.oldest_offset();

        if limit == 0 {
            return LogPage {
                offset,
                total,
                evicted: offset < oldest,
                lines: vec![],
            };
        }

        if self.lines.is_empty() {
            return LogPage {
                offset: self.next_offset,
                total: 0,
                evicted: offset < oldest,
                lines: vec![],
            };
        }

        if offset < oldest {
            let req_end = offset.saturating_add(limit as u64);
            if req_end <= oldest {
                // Completely evicted
                LogPage {
                    offset,
                    total,
                    evicted: true,
                    lines: vec![],
                }
            } else {
                // Partially evicted: return what remains starting from `oldest`
                let remaining_limit = (req_end - oldest) as usize;
                let take_count = remaining_limit.min(self.lines.len());
                let lines: Vec<LogLine> = self.lines.iter().take(take_count).cloned().collect();
                LogPage {
                    offset: oldest,
                    total,
                    evicted: true,
                    lines,
                }
            }
        } else if offset >= self.next_offset {
            LogPage {
                offset: self.next_offset,
                total,
                evicted: false,
                lines: vec![],
            }
        } else {
            let start_idx = (offset - oldest) as usize;
            let lines: Vec<LogLine> = self
                .lines
                .iter()
                .skip(start_idx)
                .take(limit as usize)
                .cloned()
                .collect();
            LogPage {
                offset,
                total,
                evicted: false,
                lines,
            }
        }
    }

    fn tail(&self, limit: u32, page_cap: u32) -> LogPage {
        let limit = limit.min(page_cap);
        let total = self.lines.len() as u64;

        if limit == 0 || self.lines.is_empty() {
            return LogPage {
                offset: self.next_offset,
                total,
                evicted: false,
                lines: vec![],
            };
        }

        let count = (limit as usize).min(self.lines.len());
        let skip_idx = self.lines.len() - count;
        let start_offset = self.lines[skip_idx].offset;
        let lines: Vec<LogLine> = self.lines.iter().skip(skip_idx).cloned().collect();

        LogPage {
            offset: start_offset,
            total,
            evicted: false,
            lines,
        }
    }

    fn slice_range(&self, start: u64, end: u64) -> (u64, Vec<LogLine>) {
        let oldest = self.oldest_offset();
        let clamped_start = start.max(oldest);
        let clamped_end = end.min(self.next_offset);

        if clamped_start >= clamped_end || self.lines.is_empty() {
            return (clamped_start, vec![]);
        }

        let start_idx = (clamped_start - oldest) as usize;
        let count = (clamped_end - clamped_start) as usize;
        let lines = self
            .lines
            .iter()
            .skip(start_idx)
            .take(count)
            .cloned()
            .collect();
        (clamped_start, lines)
    }
}

pub struct LogStore {
    config: LogStoreConfig,
    agents: HashMap<AgentId, AgentBuffer>,
    pinned_windows: HashMap<EventId, PinnedLogWindow>,
    event_agents: HashMap<EventId, AgentId>,
}

impl LogStore {
    pub fn new(config: LogStoreConfig) -> Self {
        LogStore {
            config,
            agents: HashMap::new(),
            pinned_windows: HashMap::new(),
            event_agents: HashMap::new(),
        }
    }

    pub fn config(&self) -> &LogStoreConfig {
        &self.config
    }

    fn buffer_mut(&mut self, agent_id: &str) -> &mut AgentBuffer {
        let cap = self.config.ring_capacity;
        self.agents
            .entry(agent_id.to_string())
            .or_insert_with(|| AgentBuffer::new(cap))
    }

    fn buffer(&self, agent_id: &str) -> Option<&AgentBuffer> {
        self.agents.get(agent_id)
    }

    /// Monotonically increasing next offset for `agent_id`.
    pub fn next_offset(&self, agent_id: &str) -> u64 {
        self.agents
            .get(agent_id)
            .map(|b| b.next_offset)
            .unwrap_or(0)
    }

    /// Oldest retrievable offset in the ring buffer for `agent_id`.
    pub fn oldest_offset(&self, agent_id: &str) -> u64 {
        self.agents
            .get(agent_id)
            .map(|b| b.oldest_offset())
            .unwrap_or(0)
    }

    /// Number of lines currently retained in the ring buffer for `agent_id`.
    pub fn ring_len(&self, agent_id: &str) -> usize {
        self.agents
            .get(agent_id)
            .map(|b| b.lines.len())
            .unwrap_or(0)
    }

    /// Append a single line for `agent_id`.
    pub fn append(&mut self, agent_id: &str, text: String, ts: DateTime<Utc>) -> LogLine {
        self.buffer_mut(agent_id).append(text, ts)
    }

    /// Append multiple lines for `agent_id`.
    pub fn append_lines(
        &mut self,
        agent_id: &str,
        lines: &[String],
        ts: DateTime<Utc>,
    ) -> Vec<LogLine> {
        let buf = self.buffer_mut(agent_id);
        lines
            .iter()
            .map(|line| buf.append(line.clone(), ts))
            .collect()
    }

    /// Associate an event with an agent so unpinned log requests find the agent buffer.
    pub fn register_event(&mut self, event_id: EventId, agent_id: AgentId) {
        self.event_agents.insert(event_id, agent_id);
    }

    /// Page lines directly for an agent.
    pub fn page(&self, agent_id: &str, offset: u64, limit: u32) -> LogPage {
        match self.buffer(agent_id) {
            Some(b) => b.page(offset, limit, self.config.page_cap),
            None => LogPage {
                offset: 0,
                total: 0,
                evicted: false,
                lines: vec![],
            },
        }
    }

    /// Tail lines directly for an agent.
    pub fn tail(&self, agent_id: &str, limit: u32) -> LogPage {
        match self.buffer(agent_id) {
            Some(b) => b.tail(limit, self.config.page_cap),
            None => LogPage {
                offset: 0,
                total: 0,
                evicted: false,
                lines: vec![],
            },
        }
    }

    /// Pin a window of lines around `range` for `event_id`.
    /// Window default: `[range.start - 200, range.end + 50]`, clamped at buffer bounds.
    pub fn pin(&mut self, event_id: EventId, agent_id: &str, range: LogRange) {
        self.event_agents.insert(event_id, agent_id.to_string());

        let target_start = range.start.saturating_sub(self.config.pin_before);
        let target_end = range.end.saturating_add(self.config.pin_after);

        let (start_offset, lines) = match self.buffer(agent_id) {
            Some(b) => b.slice_range(target_start, target_end),
            None => (target_start, vec![]),
        };

        self.pinned_windows.insert(
            event_id,
            PinnedLogWindow {
                event_id,
                start_offset,
                lines,
            },
        );
    }

    pub fn is_pinned(&self, event_id: &EventId) -> bool {
        self.pinned_windows.contains_key(event_id)
    }

    pub fn pinned_window(&self, event_id: &EventId) -> Option<&PinnedLogWindow> {
        self.pinned_windows.get(event_id)
    }

    /// Retrieve event logs per COMMUNICATION.md.
    /// Returns `None` if the event is unknown.
    pub fn get_event_logs(&self, event_id: &EventId, offset: i64, limit: u32) -> Option<EventLogs> {
        let limit = limit.min(self.config.page_cap);

        if let Some(pinned) = self.pinned_windows.get(event_id) {
            let total = pinned.lines.len() as u64;

            if limit == 0 {
                let off = if offset == LOG_OFFSET_TAIL {
                    pinned.start_offset + total
                } else {
                    offset.max(0) as u64
                };
                return Some(EventLogs {
                    event_id: *event_id,
                    offset: off,
                    total,
                    evicted: false,
                    lines: vec![],
                });
            }

            if offset == LOG_OFFSET_TAIL {
                if pinned.lines.is_empty() {
                    return Some(EventLogs {
                        event_id: *event_id,
                        offset: pinned.start_offset,
                        total: 0,
                        evicted: false,
                        lines: vec![],
                    });
                }
                let count = (limit as usize).min(pinned.lines.len());
                let skip = pinned.lines.len() - count;
                let start_off = pinned.lines[skip].offset;
                let lines = pinned.lines[skip..].to_vec();
                return Some(EventLogs {
                    event_id: *event_id,
                    offset: start_off,
                    total,
                    evicted: false,
                    lines,
                });
            }

            let req_off = offset.max(0) as u64;
            let pin_start = pinned.start_offset;
            let pin_end = pin_start + total;

            if req_off < pin_start {
                let req_end = req_off.saturating_add(limit as u64);
                if req_end <= pin_start {
                    Some(EventLogs {
                        event_id: *event_id,
                        offset: req_off,
                        total,
                        evicted: true,
                        lines: vec![],
                    })
                } else {
                    let take = ((req_end - pin_start) as usize).min(pinned.lines.len());
                    let lines = pinned.lines[..take].to_vec();
                    Some(EventLogs {
                        event_id: *event_id,
                        offset: pin_start,
                        total,
                        evicted: true,
                        lines,
                    })
                }
            } else if req_off >= pin_end {
                Some(EventLogs {
                    event_id: *event_id,
                    offset: pin_end,
                    total,
                    evicted: false,
                    lines: vec![],
                })
            } else {
                let start_idx = (req_off - pin_start) as usize;
                let count = (limit as usize).min(pinned.lines.len() - start_idx);
                let lines = pinned.lines[start_idx..start_idx + count].to_vec();
                Some(EventLogs {
                    event_id: *event_id,
                    offset: req_off,
                    total,
                    evicted: false,
                    lines,
                })
            }
        } else {
            // Not pinned: check if event is known to an agent buffer
            let agent_id = self.event_agents.get(event_id)?;
            let page = if offset == LOG_OFFSET_TAIL {
                self.tail(agent_id, limit)
            } else {
                self.page(agent_id, offset.max(0) as u64, limit)
            };
            Some(EventLogs {
                event_id: *event_id,
                offset: page.offset,
                total: page.total,
                evicted: page.evicted,
                lines: page.lines,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn ts() -> DateTime<Utc> {
        "2026-09-17T10:00:00Z".parse().unwrap()
    }

    #[test]
    fn eviction_keeps_offsets_increasing() {
        let mut store = LogStore::new(LogStoreConfig {
            ring_capacity: 5,
            pin_before: 2,
            pin_after: 2,
            page_cap: 10,
        });

        for i in 0..12 {
            store.append("agent-1", format!("line {i}"), ts());
        }

        assert_eq!(store.next_offset("agent-1"), 12);
        assert_eq!(store.oldest_offset("agent-1"), 7);
        assert_eq!(store.ring_len("agent-1"), 5);

        // Lines in ring are 7..12
        let page = store.page("agent-1", 7, 5);
        assert_eq!(page.offset, 7);
        assert_eq!(page.lines.len(), 5);
        assert_eq!(page.lines[0].offset, 7);
        assert_eq!(page.lines[4].offset, 11);
        assert!(!page.evicted);
    }

    #[test]
    fn tail_returns_true_start_offset() {
        let mut store = LogStore::new(LogStoreConfig {
            ring_capacity: 100,
            ..Default::default()
        });

        for i in 0..10 {
            store.append("a", format!("line {i}"), ts());
        }

        let tail = store.tail("a", 3);
        assert_eq!(tail.offset, 7);
        assert_eq!(tail.lines.len(), 3);
        assert_eq!(tail.lines[0].text, "line 7");
        assert_eq!(tail.lines[2].text, "line 9");
        assert_eq!(tail.total, 10);
    }

    #[test]
    fn evicted_true_when_appropriate() {
        let mut store = LogStore::new(LogStoreConfig {
            ring_capacity: 5,
            ..Default::default()
        });

        for i in 0..10 {
            store.append("a", format!("line {i}"), ts());
        }

        // Oldest is 5. Requesting offset 0..3: completely evicted
        let page = store.page("a", 0, 3);
        assert!(page.evicted);
        assert!(page.lines.is_empty());

        // Requesting offset 3..7: partially evicted, returns remaining 5..7
        let page = store.page("a", 3, 4);
        assert!(page.evicted);
        assert_eq!(page.offset, 5);
        assert_eq!(page.lines.len(), 2);
        assert_eq!(page.lines[0].offset, 5);
        assert_eq!(page.lines[1].offset, 6);
    }

    #[test]
    fn pinned_window_survives_full_rotation() {
        let mut store = LogStore::new(LogStoreConfig {
            ring_capacity: 10,
            pin_before: 3,
            pin_after: 2,
            page_cap: 50,
        });

        for i in 0..10 {
            store.append("agent", format!("line {i}"), ts());
        }

        let event_id = Uuid::new_v4();
        // Pin around lines 4..6. Pin window before 3, after 2 => [1, 8]
        store.pin(
            event_id,
            "agent",
            LogRange {
                start: 4,
                end: 6,
                pinned: true,
            },
        );

        assert!(store.is_pinned(&event_id));

        // Now push 100 more lines to completely rotate and evict from ring buffer
        for i in 10..110 {
            store.append("agent", format!("line {i}"), ts());
        }

        assert_eq!(store.oldest_offset("agent"), 100);

        // Ring buffer no longer has lines 1..8
        let direct_page = store.page("agent", 1, 5);
        assert!(direct_page.evicted);
        assert!(direct_page.lines.is_empty());

        // But get_event_logs on the pinned event still returns the lines!
        let logs = store.get_event_logs(&event_id, 1, 10).unwrap();
        assert_eq!(logs.offset, 1);
        assert!(!logs.lines.is_empty());
        assert_eq!(logs.lines[0].text, "line 1");

        // Tail request on pinned window
        let tail_logs = store.get_event_logs(&event_id, LOG_OFFSET_TAIL, 3).unwrap();
        assert_eq!(tail_logs.lines.len(), 3);
        assert_eq!(tail_logs.lines.last().unwrap().offset, 7);
    }

    #[test]
    fn window_clamps_at_buffer_start_and_current_end() {
        let mut store = LogStore::new(LogStoreConfig {
            ring_capacity: 100,
            pin_before: 200,
            pin_after: 50,
            page_cap: 500,
        });

        // Only 5 lines exist
        for i in 0..5 {
            store.append("agent", format!("line {i}"), ts());
        }

        let event_id = Uuid::new_v4();
        // Request pin at [2, 3] with pin_before 200 (clamps at 0) and pin_after 50 (clamps at 5)
        store.pin(
            event_id,
            "agent",
            LogRange {
                start: 2,
                end: 3,
                pinned: true,
            },
        );

        let pinned = store.pinned_window(&event_id).unwrap();
        assert_eq!(pinned.start_offset, 0);
        assert_eq!(pinned.lines.len(), 5);
        assert_eq!(pinned.lines[0].offset, 0);
        assert_eq!(pinned.lines[4].offset, 4);
    }

    #[test]
    fn limit_capped_and_zero_limit_ok() {
        let mut store = LogStore::new(LogStoreConfig {
            ring_capacity: 100,
            page_cap: 5,
            ..Default::default()
        });

        for i in 0..20 {
            store.append("a", format!("line {i}"), ts());
        }

        // Limit requested is 100, but cap is 5
        let page = store.page("a", 0, 100);
        assert_eq!(page.lines.len(), 5);

        // Limit 0 returns empty ok
        let zero_page = store.page("a", 0, 0);
        assert!(zero_page.lines.is_empty());
        assert_eq!(zero_page.total, 20);

        let event_id = Uuid::new_v4();
        store.register_event(event_id, "a".into());
        let zero_event_logs = store.get_event_logs(&event_id, 0, 0).unwrap();
        assert!(zero_event_logs.lines.is_empty());
        assert_eq!(zero_event_logs.total, 20);
    }

    #[test]
    fn memory_bounded_by_configuration_3x_capacity() {
        let capacity = 100;
        let mut store = LogStore::new(LogStoreConfig {
            ring_capacity: capacity,
            ..Default::default()
        });

        for i in 0..(capacity * 3) {
            store.append("agent", format!("line {i}"), ts());
        }

        assert_eq!(store.ring_len("agent"), capacity);
        assert_eq!(store.next_offset("agent"), (capacity * 3) as u64);
        assert_eq!(store.oldest_offset("agent"), (capacity * 2) as u64);
    }

    #[test]
    fn unknown_event_returns_none() {
        let store = LogStore::new(LogStoreConfig::default());
        assert!(store.get_event_logs(&Uuid::new_v4(), 0, 10).is_none());
    }
}
