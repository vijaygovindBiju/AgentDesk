//! Task Tracker and Watchdog: tracks open tasks and escalates long-running tasks.
//! See docs/ARCHITECTURE.md, docs/SYSTEM_DESIGN.md "Task lifecycle",
//! and docs/DATA_MODEL.md.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

use agentdesk_model::{AgentId, Category, Event, EventId, Operation, TaskId};

/// Per-operation expected duration table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThresholdTable {
    pub build: Duration,
    pub test: Duration,
    pub install: Duration,
    pub analyze: Duration,
    pub edit: Duration,
    pub other: Duration,
}

impl Default for ThresholdTable {
    fn default() -> Self {
        ThresholdTable {
            build: Duration::minutes(5),
            test: Duration::minutes(3),
            install: Duration::minutes(10),
            analyze: Duration::minutes(2),
            edit: Duration::minutes(2),
            other: Duration::minutes(5),
        }
    }
}

impl ThresholdTable {
    pub fn expected(&self, op: Operation) -> Duration {
        match op {
            Operation::Build => self.build,
            Operation::Test => self.test,
            Operation::Install => self.install,
            Operation::Analyze => self.analyze,
            Operation::Edit => self.edit,
            Operation::Other => self.other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTask {
    pub task_id: TaskId,
    pub agent_id: AgentId,
    pub operation: Operation,
    pub started_at: DateTime<Utc>,
    pub latest_working_event: Option<EventId>,
    pub working_events: Vec<EventId>,
    pub escalation_level: u8,
}

/// Escalation triggered by watchdog on a tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscalationAction {
    pub task_id: TaskId,
    pub event_id: EventId,
    pub level: u8,
}

pub struct TaskTracker {
    thresholds: ThresholdTable,
    open_tasks: HashMap<TaskId, OpenTask>,
}

impl Default for TaskTracker {
    fn default() -> Self {
        Self::new(ThresholdTable::default())
    }
}

impl TaskTracker {
    pub fn new(thresholds: ThresholdTable) -> Self {
        TaskTracker {
            thresholds,
            open_tasks: HashMap::new(),
        }
    }

    pub fn thresholds(&self) -> &ThresholdTable {
        &self.thresholds
    }

    pub fn open_task(&self, task_id: &str) -> Option<&OpenTask> {
        self.open_tasks.get(task_id)
    }

    pub fn open_tasks_len(&self) -> usize {
        self.open_tasks.len()
    }

    /// Observe a newly processed event.
    /// If the event closes the task (`Completed` or `Error`), removes it from open tasks
    /// and returns the working events that should be marked `superseded`.
    pub fn observe_event(&mut self, event: &Event) -> Option<Vec<EventId>> {
        let task_id = match &event.task_id {
            Some(tid) if !tid.trim().is_empty() => tid.clone(),
            _ => return None, // Event without task_id is never tracked
        };

        match event.category {
            Category::Completed | Category::Error => {
                // Closes the task
                if let Some(task) = self.open_tasks.remove(&task_id) {
                    Some(task.working_events)
                } else {
                    None
                }
            }
            Category::Working => {
                let task = self
                    .open_tasks
                    .entry(task_id.clone())
                    .or_insert_with(|| OpenTask {
                        task_id: task_id.clone(),
                        agent_id: event.agent_id.clone(),
                        operation: event.operation,
                        started_at: event.ts,
                        latest_working_event: None,
                        working_events: Vec::new(),
                        escalation_level: 0,
                    });
                task.latest_working_event = Some(event.event_id);
                task.working_events.push(event.event_id);
                None
            }
            Category::Request => {
                // Opens or updates task without working event
                self.open_tasks
                    .entry(task_id.clone())
                    .or_insert_with(|| OpenTask {
                        task_id,
                        agent_id: event.agent_id.clone(),
                        operation: event.operation,
                        started_at: event.ts,
                        latest_working_event: None,
                        working_events: Vec::new(),
                        escalation_level: 0,
                    });
                None
            }
        }
    }

    /// Tick-driven watchdog check: compares elapsed time of each open task against
    /// threshold and 2x threshold. Returns escalation actions.
    pub fn tick(&mut self, now: DateTime<Utc>) -> Vec<EscalationAction> {
        let mut escalations = Vec::new();

        for task in self.open_tasks.values_mut() {
            let target_event_id = match task.latest_working_event {
                Some(id) => id,
                None => continue, // Task with no working event never escalates
            };

            let elapsed = now - task.started_at;
            let expected = self.thresholds.expected(task.operation);

            if elapsed >= expected * 2 && task.escalation_level < 2 {
                task.escalation_level = 2;
                escalations.push(EscalationAction {
                    task_id: task.task_id.clone(),
                    event_id: target_event_id,
                    level: 2,
                });
            } else if elapsed >= expected && task.escalation_level < 1 {
                task.escalation_level = 1;
                escalations.push(EscalationAction {
                    task_id: task.task_id.clone(),
                    event_id: target_event_id,
                    level: 1,
                });
            }
        }
        escalations.sort_by(|a, b| a.task_id.cmp(&b.task_id));
        escalations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentdesk_model::{Details, LogRange, SCHEMA_VERSION, Severity};
    use uuid::Uuid;

    fn make_event(
        task_id: Option<&str>,
        category: Category,
        operation: Operation,
        ts: DateTime<Utc>,
    ) -> Event {
        Event {
            schema_version: SCHEMA_VERSION,
            event_id: Uuid::new_v4(),
            seq: 1,
            agent_seq: 1,
            agent_id: "agent".into(),
            agent_name: "Agent".into(),
            project: "Project".into(),
            task_id: task_id.map(|s| s.to_string()),
            ts,
            category,
            severity: Severity::Routine,
            kind: "kind".into(),
            operation,
            summary: "summary".into(),
            message: "msg".into(),
            details: Details::new(),
            log_range: LogRange {
                start: 0,
                end: 0,
                pinned: false,
            },
            request: None,
        }
    }

    #[test]
    fn tracker_escalates_at_expected_and_2x_expected_never_third_time() {
        let thresholds = ThresholdTable {
            build: Duration::seconds(10),
            test: Duration::seconds(10),
            install: Duration::seconds(10),
            analyze: Duration::seconds(10),
            edit: Duration::seconds(10),
            other: Duration::seconds(10),
        };
        let mut tracker = TaskTracker::new(thresholds);
        let start: DateTime<Utc> = "2026-09-17T12:00:00Z".parse().unwrap();

        let ev_work = make_event(Some("task-1"), Category::Working, Operation::Build, start);
        let work_id = ev_work.event_id;
        tracker.observe_event(&ev_work);

        // At 5s: under 10s threshold => no escalation
        let esc = tracker.tick(start + Duration::seconds(5));
        assert!(esc.is_empty());

        // At 10s: reached 1x threshold => escalation level 1
        let esc = tracker.tick(start + Duration::seconds(10));
        assert_eq!(esc.len(), 1);
        assert_eq!(esc[0].level, 1);
        assert_eq!(esc[0].event_id, work_id);

        // At 15s: still level 1, no duplicate escalation
        let esc = tracker.tick(start + Duration::seconds(15));
        assert!(esc.is_empty());

        // At 20s: reached 2x threshold => escalation level 2
        let esc = tracker.tick(start + Duration::seconds(20));
        assert_eq!(esc.len(), 1);
        assert_eq!(esc[0].level, 2);
        assert_eq!(esc[0].event_id, work_id);

        // At 30s: capped at level 2, never escalates a third time
        let esc = tracker.tick(start + Duration::seconds(30));
        assert!(esc.is_empty());
    }

    #[test]
    fn closing_event_supersedes_and_stops_escalation() {
        let thresholds = ThresholdTable {
            build: Duration::seconds(10),
            ..Default::default()
        };
        let mut tracker = TaskTracker::new(thresholds);
        let start: DateTime<Utc> = "2026-09-17T12:00:00Z".parse().unwrap();

        let w1 = make_event(Some("task-1"), Category::Working, Operation::Build, start);
        let w2 = make_event(
            Some("task-1"),
            Category::Working,
            Operation::Build,
            start + Duration::seconds(2),
        );
        tracker.observe_event(&w1);
        tracker.observe_event(&w2);

        assert_eq!(tracker.open_tasks_len(), 1);

        // Completed event closes task and returns working events to supersede
        let comp = make_event(
            Some("task-1"),
            Category::Completed,
            Operation::Build,
            start + Duration::seconds(5),
        );
        let superseded = tracker.observe_event(&comp).expect("task was open");
        assert_eq!(superseded, vec![w1.event_id, w2.event_id]);
        assert_eq!(tracker.open_tasks_len(), 0);

        // Subsequent ticks produce no escalations because task is closed
        let esc = tracker.tick(start + Duration::seconds(50));
        assert!(esc.is_empty());
    }

    #[test]
    fn task_without_working_never_escalates() {
        let thresholds = ThresholdTable {
            build: Duration::seconds(10),
            ..Default::default()
        };
        let mut tracker = TaskTracker::new(thresholds);
        let start: DateTime<Utc> = "2026-09-17T12:00:00Z".parse().unwrap();

        // Request event opens the task, but has no working event
        let req = make_event(Some("task-req"), Category::Request, Operation::Build, start);
        tracker.observe_event(&req);

        let esc = tracker.tick(start + Duration::seconds(25));
        assert!(
            esc.is_empty(),
            "task without working event must not escalate"
        );
    }

    #[test]
    fn event_without_task_id_never_tracked() {
        let mut tracker = TaskTracker::default();
        let ev = make_event(None, Category::Working, Operation::Build, Utc::now());
        assert!(tracker.observe_event(&ev).is_none());
        assert_eq!(tracker.open_tasks_len(), 0);
    }
}
