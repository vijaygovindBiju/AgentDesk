//! Print a summary of a scenario run: `cargo run -p agentdesk-sim --example stats [scenario.json] [seed]`.

use std::collections::BTreeMap;

use chrono::Duration;

use agentdesk_core::{classify, AdapterOutput, Clock, VirtualClock};
use agentdesk_model::Decision;
use agentdesk_sim::{run_to_end, Scenario, Simulator};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let scenario = match args.get(1) {
        Some(p) => Scenario::load(p).expect("load scenario"),
        None => Scenario::default_scenario(),
    };
    let seed: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(42);

    let clock = VirtualClock::at_epoch();
    let start = clock.now();
    let mut sim = Simulator::new(scenario.clone(), seed, start);
    let timeline = run_to_end(&mut sim, &clock, Duration::seconds(5), |_| Decision::Approve);

    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_cat: BTreeMap<String, usize> = BTreeMap::new();
    let mut lines = 0usize;
    let mut bytes = 0usize;
    for (_, o) in &timeline {
        match o {
            AdapterOutput::Event(e) => {
                *by_kind.entry(e.kind.clone()).or_default() += 1;
                *by_cat.entry(format!("{:?}", classify(&e.kind).category)).or_default() += 1;
                bytes += serde_json::to_string(e).unwrap().len();
                lines += e.log_lines.len();
            }
            AdapterOutput::Line { text, .. } => {
                lines += 1;
                bytes += text.len();
            }
        }
    }
    let events: usize = by_kind.values().sum();
    let end = timeline.last().map(|(t, _)| *t).unwrap_or(start);

    println!("scenario: {} (seed {seed})", scenario.name);
    println!("virtual duration: {}", end - start);
    println!("raw events: {events}   raw log lines: {lines}   approx raw bytes: {bytes}");
    println!("by category: {by_cat:?}");
    println!("by kind:");
    for (k, n) in by_kind {
        println!("  {k:<22} {n}");
    }
}
