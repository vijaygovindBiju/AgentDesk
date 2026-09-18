//! Report structures for the measurement bench (P5.3).
//! See docs/TODO.md P5.3 and docs/PROJECT.md "Success criteria".

use serde::{Deserialize, Serialize};

use agentdesk_core::Metrics;
use agentdesk_sim::{GroundTruthEscalation, ScenarioGroundTruth};

use crate::client::{ClientCounters, TapPolicy};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingleModeResult {
    pub mode: String,
    pub laptop_metrics: Metrics,
    pub client_counters: ClientCounters,
    pub transmitted_events: u64,
    pub transmitted_bytes: usize,
    pub surfaced_events: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReductionSection {
    pub raw_lines_events: u64,
    pub raw_lines_bytes: usize,
    pub raw_events_events: u64,
    pub raw_events_bytes: usize,
    pub agentdesk_surfaced_events: u64,
    pub agentdesk_transmitted_events: u64,
    pub agentdesk_transmitted_bytes: usize,
    pub event_reduction_vs_raw_events_ratio: f64,
    pub event_reduction_vs_raw_events_pct: f64,
    pub event_reduction_vs_raw_lines_ratio: f64,
    pub event_reduction_vs_raw_lines_pct: f64,
    pub byte_reduction_vs_raw_events_ratio: f64,
    pub byte_reduction_vs_raw_events_pct: f64,
    pub byte_reduction_vs_raw_lines_ratio: f64,
    pub byte_reduction_vs_raw_lines_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryCoverage {
    pub expected: usize,
    pub surfaced: usize,
    pub preserved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedEscalation {
    pub task_id: String,
    pub highest_level: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationCoverage {
    pub expected: Vec<GroundTruthEscalation>,
    pub observed: Vec<ObservedEscalation>,
    pub preserved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageSection {
    pub requests: CategoryCoverage,
    pub errors: CategoryCoverage,
    pub completed_important: CategoryCoverage,
    pub completed_routine: CategoryCoverage,
    pub escalations: EscalationCoverage,
    pub duplicates: u64,
    pub silent_loss_detected: bool,
    pub all_passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiModeReport {
    pub scenario: String,
    pub seed: u64,
    pub timestamp: String,
    pub tap_policy: TapPolicy,
    pub modes: MultiModeMap,
    pub reduction: ReductionSection,
    pub coverage: CoverageSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiModeMap {
    pub raw_lines: SingleModeResult,
    pub raw_events: SingleModeResult,
    pub agentdesk: SingleModeResult,
}

impl MultiModeReport {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scenario_name: String,
        seed: u64,
        tap_policy: TapPolicy,
        timestamp: String,
        raw_lines: SingleModeResult,
        raw_events: SingleModeResult,
        agentdesk: SingleModeResult,
        ground_truth: ScenarioGroundTruth,
        surfaced_requests: usize,
        surfaced_errors: usize,
        surfaced_completed_important: usize,
        surfaced_completed_routine: usize,
        observed_escalations: Vec<ObservedEscalation>,
        silent_loss_detected: bool,
    ) -> Self {
        // Compute reduction metrics
        let raw_lines_events = raw_lines.surfaced_events;
        let raw_lines_bytes = raw_lines.transmitted_bytes;
        let raw_events_events = raw_events.transmitted_events;
        let raw_events_bytes = raw_events.transmitted_bytes;
        let agentdesk_surfaced_events = agentdesk.surfaced_events;
        let agentdesk_transmitted_events = agentdesk.transmitted_events;
        let agentdesk_transmitted_bytes = agentdesk.transmitted_bytes;

        let event_reduction_vs_raw_events_ratio = if agentdesk_surfaced_events > 0 {
            raw_events_events as f64 / agentdesk_surfaced_events as f64
        } else {
            0.0
        };
        let event_reduction_vs_raw_events_pct = if raw_events_events > 0 {
            (1.0 - (agentdesk_surfaced_events as f64 / raw_events_events as f64)) * 100.0
        } else {
            0.0
        };

        let event_reduction_vs_raw_lines_ratio = if agentdesk_surfaced_events > 0 {
            raw_lines_events as f64 / agentdesk_surfaced_events as f64
        } else {
            0.0
        };
        let event_reduction_vs_raw_lines_pct = if raw_lines_events > 0 {
            (1.0 - (agentdesk_surfaced_events as f64 / raw_lines_events as f64)) * 100.0
        } else {
            0.0
        };

        let byte_reduction_vs_raw_events_ratio = if agentdesk_transmitted_bytes > 0 {
            raw_events_bytes as f64 / agentdesk_transmitted_bytes as f64
        } else {
            0.0
        };
        let byte_reduction_vs_raw_events_pct = if raw_events_bytes > 0 {
            (1.0 - (agentdesk_transmitted_bytes as f64 / raw_events_bytes as f64)) * 100.0
        } else {
            0.0
        };

        let byte_reduction_vs_raw_lines_ratio = if agentdesk_transmitted_bytes > 0 {
            raw_lines_bytes as f64 / agentdesk_transmitted_bytes as f64
        } else {
            0.0
        };
        let byte_reduction_vs_raw_lines_pct = if raw_lines_bytes > 0 {
            (1.0 - (agentdesk_transmitted_bytes as f64 / raw_lines_bytes as f64)) * 100.0
        } else {
            0.0
        };

        let reduction = ReductionSection {
            raw_lines_events,
            raw_lines_bytes,
            raw_events_events,
            raw_events_bytes,
            agentdesk_surfaced_events,
            agentdesk_transmitted_events,
            agentdesk_transmitted_bytes,
            event_reduction_vs_raw_events_ratio,
            event_reduction_vs_raw_events_pct,
            event_reduction_vs_raw_lines_ratio,
            event_reduction_vs_raw_lines_pct,
            byte_reduction_vs_raw_events_ratio,
            byte_reduction_vs_raw_events_pct,
            byte_reduction_vs_raw_lines_ratio,
            byte_reduction_vs_raw_lines_pct,
        };

        // Compute coverage metrics
        let requests_preserved = surfaced_requests == ground_truth.requests.len();
        let errors_preserved = surfaced_errors == ground_truth.errors.len();
        let completed_important_preserved =
            surfaced_completed_important >= ground_truth.completed_important.len();

        let escalations_preserved = ground_truth.escalations.iter().all(|exp| {
            observed_escalations
                .iter()
                .any(|obs| obs.task_id == exp.task_id && obs.highest_level >= exp.level)
        });

        let all_passed = requests_preserved
            && errors_preserved
            && completed_important_preserved
            && escalations_preserved
            && !silent_loss_detected
            && agentdesk.client_counters.duplicates == 0
            && agentdesk_surfaced_events < raw_events_events
            && raw_events_events <= raw_lines_events;

        let coverage = CoverageSection {
            requests: CategoryCoverage {
                expected: ground_truth.requests.len(),
                surfaced: surfaced_requests,
                preserved: requests_preserved,
            },
            errors: CategoryCoverage {
                expected: ground_truth.errors.len(),
                surfaced: surfaced_errors,
                preserved: errors_preserved,
            },
            completed_important: CategoryCoverage {
                expected: ground_truth.completed_important.len(),
                surfaced: surfaced_completed_important,
                preserved: completed_important_preserved,
            },
            completed_routine: CategoryCoverage {
                expected: ground_truth.completed_routine.len(),
                surfaced: surfaced_completed_routine,
                preserved: true,
            },
            escalations: EscalationCoverage {
                expected: ground_truth.escalations,
                observed: observed_escalations,
                preserved: escalations_preserved,
            },
            duplicates: agentdesk.client_counters.duplicates,
            silent_loss_detected,
            all_passed,
        };

        MultiModeReport {
            scenario: scenario_name,
            seed,
            timestamp,
            tap_policy,
            modes: MultiModeMap {
                raw_lines,
                raw_events,
                agentdesk,
            },
            reduction,
            coverage,
        }
    }
}
