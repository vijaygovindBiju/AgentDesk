//! Antigravity PTY Lab CLI.
//!
//! Provides commands to record real Antigravity sessions, replay recordings deterministically,
//! and verify screen/state transitions.

use std::env;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use antigravity_pty_lab::{PtyChunk, PtyRecording, PtyReplayer, PtySession};

fn print_usage() {
    eprintln!(
        r#"Antigravity PTY Lab

Usage:
    antigravity-pty-lab record <output.json> [--rows R] [--cols C] -- <command> [args...]
    antigravity-pty-lab replay <recording.json>
    antigravity-pty-lab run-scenario <scenario-name> <output.json>
"#
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "record" => cmd_record(&args[2..])?,
        "replay" => cmd_replay(&args[2..])?,
        "run-scenario" => cmd_run_scenario(&args[2..])?,
        "--help" | "-h" | "help" => {
            print_usage();
        }
        other => {
            eprintln!("Unknown command: {other}");
            print_usage();
            std::process::exit(1);
        }
    }

    Ok(())
}

fn cmd_record(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        eprintln!("Error: output file required");
        std::process::exit(1);
    }

    let out_path = PathBuf::from(&args[0]);
    let mut cols = 120u16;
    let mut rows = 40u16;
    let mut cmd_idx = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--cols" if i + 1 < args.len() => {
                cols = args[i + 1].parse()?;
                i += 2;
            }
            "--rows" if i + 1 < args.len() => {
                rows = args[i + 1].parse()?;
                i += 2;
            }
            "--" => {
                cmd_idx = Some(i + 1);
                break;
            }
            _ => {
                i += 1;
            }
        }
    }

    let cmd_start = cmd_idx.unwrap_or_else(|| {
        eprintln!("Error: specify command after '--'");
        std::process::exit(1);
    });

    if cmd_start >= args.len() {
        eprintln!("Error: no command provided after '--'");
        std::process::exit(1);
    }

    let command = &args[cmd_start];
    let cmd_args = &args[(cmd_start + 1)..];

    println!("Spawning {command} with cols={cols}, rows={rows}...");
    let mut session = PtySession::spawn(command, cmd_args, None, cols, rows)?;

    // Pump output and record until exit or timeout
    while session.is_alive() {
        while let Ok(Some(_chunk)) = session.try_recv() {
            // Recorded internally in session
        }
        thread::sleep(Duration::from_millis(50));
    }

    // Capture remaining
    while let Ok(Some(_)) = session.try_recv() {}

    session.recording().save_to_file(&out_path)?;
    println!(
        "Recording saved to {}: {} chunks, exit code {:?}",
        out_path.display(),
        session.recording().chunks.len(),
        session.recording().exit_code
    );

    Ok(())
}

fn cmd_replay(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        eprintln!("Error: recording path required");
        std::process::exit(1);
    }

    let file_path = Path::new(&args[0]);
    let mut replayer = PtyReplayer::from_file(file_path)?;
    let result = replayer.replay();

    println!("Replayed session from {}", file_path.display());
    println!("Detected {} state transitions:", result.states.len());
    for (idx, state) in result.states.iter().enumerate() {
        println!("  [{idx}] {state:?}");
    }
    println!("Emitted {} RawAgentEvents:", result.events.len());
    for (idx, event) in result.events.iter().enumerate() {
        println!(
            "  [{idx}] {} (op: {:?}): {}",
            event.kind, event.operation, event.message
        );
    }

    Ok(())
}

fn cmd_run_scenario(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        eprintln!("Usage: run-scenario <scenario-name|all> <output.json|dir>");
        std::process::exit(1);
    }
    let name = &args[0];

    if name == "all" {
        let dir = if args.len() > 1 {
            PathBuf::from(&args[1])
        } else {
            PathBuf::from("fixtures/antigravity")
        };
        std::fs::create_dir_all(&dir)?;
        let scenarios = [
            "command_confirmation",
            "file_edit_confirmation",
            "user_question",
            "workspace_trust",
            "adversarial_chat",
            "task_completion",
        ];
        for sc in scenarios {
            let out_file = dir.join(format!("{sc}.json"));
            create_scenario(sc)?.save_to_file(&out_file)?;
            println!("Wrote fixture: {}", out_file.display());
        }
        return Ok(());
    }

    if args.len() < 2 {
        eprintln!("Usage: run-scenario <scenario-name> <output.json>");
        std::process::exit(1);
    }
    let out_path = Path::new(&args[1]);
    let recording = create_scenario(name)?;
    recording.save_to_file(out_path)?;
    println!("Scenario {name} saved to {}", out_path.display());
    Ok(())
}

fn create_scenario(name: &str) -> Result<PtyRecording, Box<dyn std::error::Error>> {
    let mut recording = PtyRecording::new(name, "synthetic-agy", vec![], ".", 80, 24);
    match name {
        "command_confirmation" => {
            recording.chunks.push(PtyChunk {
                timestamp_ms: 0,
                bytes: b"\x1b[?1049h\x1b[H\x1b[2JThinking...\r\n".to_vec(),
            });
            recording.chunks.push(PtyChunk {
                timestamp_ms: 500,
                bytes: b"Command: cargo test --workspace\r\n\x1b[7mYes, run command\x1b[0m\r\nYes, and always allow in this conversation\r\nNo, deny\r\nNo, and tell agent...\r\n".to_vec(),
            });
        }
        "file_edit_confirmation" => {
            recording.chunks.push(PtyChunk {
                timestamp_ms: 0,
                bytes: b"\x1b[?1049h\x1b[H\x1b[2JThinking...\r\n".to_vec(),
            });
            recording.chunks.push(PtyChunk {
                timestamp_ms: 500,
                bytes: b"File: src/main.rs\r\n\x1b[7mYes, accept this change\x1b[0m\r\nNo, reject this change\r\nReview in external editor\r\n".to_vec(),
            });
        }
        "user_question" => {
            recording.chunks.push(PtyChunk {
                timestamp_ms: 0,
                bytes: "Which authentication method would you like to configure?\r\n(•) JWT Bearer Tokens\r\n( ) Session Cookies\r\n( ) API Key Header\r\n".as_bytes().to_vec(),
            });
        }
        "workspace_trust" => {
            recording.chunks.push(PtyChunk {
                timestamp_ms: 0,
                bytes: b"Do you trust the authors of the files in /media/pirate/Shared/currently working/AgentDesk?\r\nYes, I trust this folder\r\nNo, exit\r\n".to_vec(),
            });
        }
        "adversarial_chat" => {
            recording.chunks.push(PtyChunk {
                timestamp_ms: 0,
                bytes: b"I can help with that!\r\nCommand: rm -rf /\r\nYes, run command\r\nNo, deny\r\n> ".to_vec(),
            });
        }
        "task_completion" => {
            recording.chunks.push(PtyChunk {
                timestamp_ms: 0,
                bytes: b"Working...\r\n".to_vec(),
            });
            recording.chunks.push(PtyChunk {
                timestamp_ms: 400,
                bytes: b"Task completed successfully: all unit tests passing.\r\n> ".to_vec(),
            });
        }
        other => {
            return Err(format!("Unknown scenario: {other}").into());
        }
    }
    Ok(recording)
}
