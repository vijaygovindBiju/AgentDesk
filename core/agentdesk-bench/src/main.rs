//! Command-line measurement bench binary (P5.1).
//! See docs/TODO.md P5.1, P5.3.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

use agentdesk_bench::{TapPolicy, run_all_modes, run_bench_mode};
use agentdesk_model::PipelineMode;
use agentdesk_sim::Scenario;

struct CliArgs {
    scenario_path: Option<PathBuf>,
    seed: u64,
    mode: PipelineMode,
    all_modes: bool,
    tap_policy: TapPolicy,
    output_path: Option<PathBuf>,
}

fn print_usage() {
    eprintln!(
        r#"Usage: agentdesk-bench [OPTIONS]

Options:
  --scenario <PATH>      Path to JSON scenario file (defaults to embedded default scenario)
  --seed <U64>           RNG seed for simulation (default: 42)
  --mode <MODE>          Pipeline mode: raw_lines | raw_events | agentdesk (default: agentdesk)
  --all-modes            Run all three modes and output a comparison report
  --tap-policy <POLICY>  Scripted tap policy: default | none (default: default)
  --output <PATH>        Write output JSON report to file instead of stdout
  -h, --help             Print help information
"#
    );
}

fn parse_args() -> Result<CliArgs, String> {
    let mut args = env::args().skip(1);
    let mut scenario_path = None;
    let mut seed = 42;
    let mut mode = PipelineMode::Agentdesk;
    let mut all_modes = false;
    let mut tap_policy = TapPolicy::Default;
    let mut output_path = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                let val = args.next().ok_or("Missing value for --scenario")?;
                scenario_path = Some(PathBuf::from(val));
            }
            "--seed" => {
                let val = args.next().ok_or("Missing value for --seed")?;
                seed = val
                    .parse::<u64>()
                    .map_err(|e| format!("Invalid seed: {e}"))?;
            }
            "--mode" => {
                let val = args.next().ok_or("Missing value for --mode")?;
                mode = match val.as_str() {
                    "raw_lines" => PipelineMode::RawLines,
                    "raw_events" => PipelineMode::RawEvents,
                    "agentdesk" => PipelineMode::Agentdesk,
                    other => return Err(format!("Unknown mode: {other}")),
                };
            }
            "--all-modes" => {
                all_modes = true;
            }
            "--tap-policy" => {
                let val = args.next().ok_or("Missing value for --tap-policy")?;
                tap_policy = match val.as_str() {
                    "default" => TapPolicy::Default,
                    "none" => TapPolicy::None,
                    other => return Err(format!("Unknown tap policy: {other}")),
                };
            }
            "--output" => {
                let val = args.next().ok_or("Missing value for --output")?;
                output_path = Some(PathBuf::from(val));
            }
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                return Err(format!("Unknown argument: {other}"));
            }
        }
    }

    Ok(CliArgs {
        scenario_path,
        seed,
        mode,
        all_modes,
        tap_policy,
        output_path,
    })
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(err) => {
            eprintln!("Error: {err}\n");
            print_usage();
            process::exit(1);
        }
    };

    let scenario = match args.scenario_path {
        Some(path) => match Scenario::load(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to load scenario from {}: {e}", path.display());
                process::exit(1);
            }
        },
        None => Scenario::default_scenario(),
    };

    let json_output = if args.all_modes {
        let report = run_all_modes(&scenario, args.seed, args.tap_policy);
        serde_json::to_string_pretty(&report).expect("failed to serialize multi-mode report")
    } else {
        let single = run_bench_mode(&scenario, args.seed, args.mode, args.tap_policy);
        serde_json::to_string_pretty(&single.result)
            .expect("failed to serialize single-mode result")
    };

    if let Some(out_path) = args.output_path {
        if let Some(parent) = out_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Err(e) = fs::write(&out_path, &json_output) {
            eprintln!("Failed to write report to {}: {e}", out_path.display());
            process::exit(1);
        }
        eprintln!("Report written to {}", out_path.display());
    } else {
        println!("{json_output}");
    }
}
