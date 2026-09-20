//! Deterministic, time-driven scripted agent. Implements `Adapter`.
//!
//! All output is produced from `poll(now)`, in due-time order (ties broken
//! by task order), so the emitted sequence is independent of how finely the
//! driver polls. The only randomness is progress jitter from a seeded RNG,
//! drawn in emission order.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use agentdesk_core::{Adapter, AdapterOutput, RespondError};
use agentdesk_model::{
    AdapterKind, AgentId, AgentInfo, Decision, Details, RawAgentEvent, RequestInfo, TaskId,
};

use crate::scenario::{Scenario, Step, TaskSpec, is_terminal_kind};

#[derive(Debug, Clone)]
struct Blocked {
    prompt: String,
    on_deny_kind: String,
}

#[derive(Debug)]
struct TaskState {
    spec: TaskSpec,
    step: usize,
    /// Index within a multi-piece step (progress tick, log line).
    piece: usize,
    next_due: Option<DateTime<Utc>>,
    blocked: Option<Blocked>,
    /// Set by a deny; emits the on-deny event on the next poll.
    pending_deny: Option<Blocked>,
    finished: bool,
}

pub struct Simulator {
    agents: Vec<AgentInfo>,
    tasks: Vec<TaskState>,
    agent_seq: HashMap<AgentId, u64>,
    rng: StdRng,
}

impl Simulator {
    pub fn new(scenario: Scenario, seed: u64, start: DateTime<Utc>) -> Self {
        let agents = scenario
            .agents
            .iter()
            .map(|a| AgentInfo {
                agent_id: a.agent_id.clone(),
                name: a.name.clone(),
                project: a.project.clone(),
                adapter_kind: AdapterKind::Simulator,
            })
            .collect();
        let tasks = scenario
            .tasks
            .into_iter()
            .map(|spec| TaskState {
                next_due: Some(start + Duration::milliseconds(spec.start_offset_ms as i64)),
                spec,
                step: 0,
                piece: 0,
                blocked: None,
                pending_deny: None,
                finished: false,
            })
            .collect();
        Simulator {
            agents,
            tasks,
            agent_seq: HashMap::new(),
            rng: StdRng::seed_from_u64(seed),
        }
    }

    /// Tasks currently waiting on `respond`.
    pub fn blocked_tasks(&self) -> Vec<TaskId> {
        self.tasks
            .iter()
            .filter(|t| t.blocked.is_some())
            .map(|t| t.spec.task_id.clone())
            .collect()
    }

    fn next_agent_seq(&mut self, agent_id: &str) -> u64 {
        let c = self.agent_seq.entry(agent_id.to_string()).or_insert(0);
        *c += 1;
        *c
    }

    fn event(
        &mut self,
        ti: usize,
        kind: &str,
        message: String,
        details: Details,
        log_lines: Vec<String>,
        request: Option<RequestInfo>,
    ) -> AdapterOutput {
        let spec = &self.tasks[ti].spec;
        let (agent_id, task_id, operation) =
            (spec.agent_id.clone(), spec.task_id.clone(), spec.operation);
        let agent_seq = self.next_agent_seq(&agent_id);
        AdapterOutput::Event(RawAgentEvent {
            agent_id,
            agent_seq,
            task_id: Some(task_id),
            kind: kind.to_string(),
            operation,
            message,
            details,
            log_lines,
            request,
        })
    }

    fn jitter(&mut self, jitter_ms: u64) -> Duration {
        if jitter_ms == 0 {
            return Duration::zero();
        }
        Duration::milliseconds(self.rng.random_range(0..=jitter_ms) as i64)
    }

    /// Emit the piece that is due for task `ti` and schedule the next one.
    fn emit(&mut self, ti: usize, out: &mut Vec<AdapterOutput>) {
        let due = self.tasks[ti].next_due.expect("emit only for due tasks");

        if let Some(b) = self.tasks[ti].pending_deny.take() {
            let details =
                Details::from([("task".to_string(), self.tasks[ti].spec.title.clone().into())]);
            out.push(self.event(
                ti,
                &b.on_deny_kind,
                format!("Denied: {}", b.prompt),
                details,
                vec![],
                None,
            ));
            self.finish(ti);
            return;
        }

        let step = match self.tasks[ti].spec.steps.get(self.tasks[ti].step).cloned() {
            Some(s) => s,
            None => return self.finish(ti),
        };
        let title = self.tasks[ti].spec.title.clone();
        let task_detail = || Details::from([("task".to_string(), title.clone().into())]);

        match step {
            Step::Started { message } => {
                out.push(self.event(
                    ti,
                    "started",
                    message.unwrap_or_else(|| title.clone()),
                    task_detail(),
                    vec![],
                    None,
                ));
                self.advance_step(ti);
            }
            Step::Progress {
                count,
                interval_ms,
                jitter_ms,
                template,
            } => {
                let i = self.tasks[ti].piece + 1;
                let msg = template
                    .replace("{i}", &i.to_string())
                    .replace("{n}", &count.to_string());
                out.push(self.event(ti, "progress", msg.clone(), Details::new(), vec![msg], None));
                let delay = Duration::milliseconds(interval_ms as i64) + self.jitter(jitter_ms);
                self.tasks[ti].next_due = Some(due + delay);
                self.tasks[ti].piece += 1;
                if self.tasks[ti].piece as u32 >= count {
                    self.advance_step(ti);
                }
            }
            Step::Log { lines, interval_ms } => {
                let text = lines[self.tasks[ti].piece].clone();
                out.push(AdapterOutput::Line {
                    agent_id: self.tasks[ti].spec.agent_id.clone(),
                    text,
                });
                self.tasks[ti].next_due = Some(due + Duration::milliseconds(interval_ms as i64));
                self.tasks[ti].piece += 1;
                if self.tasks[ti].piece >= lines.len() {
                    self.advance_step(ti);
                }
            }
            Step::Wait { ms } => {
                self.tasks[ti].next_due = Some(due + Duration::milliseconds(ms as i64));
                self.advance_step(ti);
            }
            Step::Request {
                kind,
                prompt,
                options,
                message,
                on_deny_kind,
            } => {
                let req = RequestInfo {
                    prompt: prompt.clone(),
                    options,
                    question_type: None,
                };
                out.push(self.event(
                    ti,
                    &kind,
                    message.unwrap_or_else(|| prompt.clone()),
                    task_detail(),
                    vec![],
                    Some(req),
                ));
                self.tasks[ti].blocked = Some(Blocked {
                    prompt,
                    on_deny_kind,
                });
                self.tasks[ti].next_due = None;
                self.advance_step(ti);
            }
            Step::Event {
                kind,
                message,
                details,
                log_lines,
                delay_ms,
            } => {
                out.push(self.event(ti, &kind, message, details, log_lines, None));
                if is_terminal_kind(&kind) {
                    self.finish(ti);
                } else {
                    self.tasks[ti].next_due = Some(due + Duration::milliseconds(delay_ms as i64));
                    self.advance_step(ti);
                }
            }
        }
    }

    fn advance_step(&mut self, ti: usize) {
        let t = &mut self.tasks[ti];
        t.step += 1;
        t.piece = 0;
        if t.step >= t.spec.steps.len() && t.blocked.is_none() {
            self.finish(ti);
        }
    }

    fn finish(&mut self, ti: usize) {
        let t = &mut self.tasks[ti];
        t.finished = true;
        t.next_due = None;
        t.blocked = None;
    }

    fn next_due_task(&self, now: DateTime<Utc>) -> Option<usize> {
        self.tasks
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.next_due.filter(|d| *d <= now).map(|d| (d, i)))
            .min()
            .map(|(_, i)| i)
    }
}

impl Adapter for Simulator {
    fn agents(&self) -> &[AgentInfo] {
        &self.agents
    }

    fn poll(&mut self, now: DateTime<Utc>) -> Vec<AdapterOutput> {
        let mut out = Vec::new();
        while let Some(ti) = self.next_due_task(now) {
            self.emit(ti, &mut out);
        }
        out
    }

    fn next_due(&self) -> Option<DateTime<Utc>> {
        self.tasks.iter().filter_map(|t| t.next_due).min()
    }

    fn respond(
        &mut self,
        task_id: &TaskId,
        decision: Decision,
        now: DateTime<Utc>,
    ) -> Result<(), RespondError> {
        let t = self
            .tasks
            .iter_mut()
            .find(|t| &t.spec.task_id == task_id)
            .ok_or(RespondError::NoSuchTask)?;
        let b = t.blocked.take().ok_or(RespondError::NotBlocked)?;
        match decision {
            Decision::Approve => {
                if t.step >= t.spec.steps.len() {
                    t.finished = true;
                } else {
                    t.next_due = Some(now);
                }
            }
            Decision::Deny => {
                t.pending_deny = Some(b);
                t.next_due = Some(now);
            }
        }
        Ok(())
    }

    fn is_finished(&self) -> bool {
        self.tasks.iter().all(|t| t.finished)
    }
}
