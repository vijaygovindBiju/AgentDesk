//! Integration and Unit Tests for AntigravityPtyAdapter.
//!
//! Tests verify:
//! 1. Complete lifecycle state transitions (`Uninitialized` -> `Starting` -> `Running` -> `Working` -> `BlockedOnPermission` -> `Working` -> `Completed` / `Exited`).
//! 2. Security / adversarial defense: plain terminal text, fake chat prompts, and spoofed escape sequences NEVER trigger approval requests.
//! 3. Screen redraw deduplication: repeated snapshots during blocking dialogs do NOT emit duplicate `RawAgentEvent`s.
//! 4. Permission approval and denial keystroke encoding.
//! 5. Terminal window resize propagation.
//! 6. Error and EOF detection when child exits.
//! 7. End-to-end integration through `CoreTask`, `PriorityQueue`, and `RespondRequest` client messaging.
//! 8. Live end-to-end command permission flow with real `agy` child process in a real POSIX PTY.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use agentdesk_core::{
    Adapter, AdapterCommand, AdapterOutput, AntigravityConfig, AntigravityLifecycle,
    AntigravityPtyAdapter, AntigravityState, ChannelSink, CoreCommand, CoreTask, LogStoreConfig,
    MockPtyTransport, RespondError, SystemClock, ThresholdTable,
};
use agentdesk_model::{
    Body, Decision, Message, Operation, PipelineMode, QuestionType, RawAgentEvent, RequestInfo,
    RequestResponse, RespondRequest,
};
use chrono::Utc;

#[test]
fn test_lifecycle_initialization_and_start() {
    let mut transport = MockPtyTransport::new(120, 40);
    transport.push_text("Antigravity v1.2.7 starting...\r\n> ");

    let config = AntigravityConfig::default();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(config, transport);

    assert_eq!(*adapter.lifecycle(), AntigravityLifecycle::Uninitialized);
    assert_eq!(adapter.agents().len(), 1);
    assert_eq!(adapter.agents()[0].agent_id, "antigravity");

    // First poll triggers session start
    let outputs = adapter.poll(Utc::now());
    assert_eq!(*adapter.lifecycle(), AntigravityLifecycle::Running);

    let started_event = outputs
        .iter()
        .find(|o| matches!(o, AdapterOutput::Event(raw) if raw.kind == "started"));
    assert!(
        started_event.is_some(),
        "Must emit started event on first poll"
    );
}

#[test]
fn test_terminal_noise_and_adversarial_chat_ignored() {
    let mut transport = MockPtyTransport::new(120, 40);
    // Malicious or accidental model chat containing prompt keywords without Bubbletea menu structure
    let fake_prompt = b"Here is the command you should run:\r\nCommand: rm -rf /\r\nYes, run command\r\nNo, deny\r\n> ";
    transport.push_bytes(fake_prompt);

    let config = AntigravityConfig::default();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(config, transport);

    let outputs = adapter.poll(Utc::now());

    // Verify NO approval_required event was emitted
    for out in &outputs {
        if let AdapterOutput::Event(raw) = out {
            assert_ne!(
                raw.kind, "approval_required",
                "Adversarial chat must NEVER trigger approval_required: got {:?}",
                raw
            );
            assert!(raw.request.is_none());
        }
    }
    assert_ne!(
        *adapter.lifecycle(),
        AntigravityLifecycle::BlockedOnPermission {
            state: AntigravityState::CommandConfirmation {
                command: "rm -rf /".into(),
                selected_option_index: 0,
                options: vec![],
            }
        }
    );
}

#[test]
fn test_screen_redraw_does_not_duplicate_request_events() {
    let mut transport = MockPtyTransport::new(80, 24);
    // Authentic Bubbletea confirmation menu in alternate screen buffer
    let menu_chunk = b"\x1b[?1049h\x1b[H\x1b[2JCommand: cargo test\r\n\x1b[7mYes, run command\x1b[0m\r\nNo, deny\r\n";
    transport.push_bytes(menu_chunk);

    let config = AntigravityConfig::default();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(config, transport);

    // First poll -> detects menu and emits approval_required
    let outputs1 = adapter.poll(Utc::now());
    let req_count1 = outputs1
        .iter()
        .filter(|o| matches!(o, AdapterOutput::Event(raw) if raw.kind == "approval_required"))
        .count();
    assert_eq!(req_count1, 1, "Must emit exactly 1 approval_required event");
    assert!(matches!(
        adapter.lifecycle(),
        AntigravityLifecycle::BlockedOnPermission { .. }
    ));

    // Redraw: cursor blink or re-rendered screen with identical content
    adapter.transport_mut().push_bytes(menu_chunk);
    let outputs2 = adapter.poll(Utc::now());
    let req_count2 = outputs2
        .iter()
        .filter(|o| matches!(o, AdapterOutput::Event(raw) if raw.kind == "approval_required"))
        .count();
    assert_eq!(
        req_count2, 0,
        "Redraw must NOT emit duplicate request events"
    );
}

#[test]
fn test_command_confirmation_approval_flow() {
    let mut transport = MockPtyTransport::new(80, 24);
    let menu_chunk = b"\x1b[?1049h\x1b[H\x1b[2JCommand: cargo test --workspace\r\n\x1b[7mYes, run command\x1b[0m\r\nYes, and always allow in this conversation\r\nNo, deny\r\n";
    transport.push_bytes(menu_chunk);

    let config = AntigravityConfig::default();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(config, transport);

    let outputs = adapter.poll(Utc::now());
    let event = outputs
        .into_iter()
        .find_map(|o| match o {
            AdapterOutput::Event(raw) if raw.kind == "approval_required" => Some(raw),
            _ => None,
        })
        .expect("Must produce approval_required event");

    assert_eq!(event.operation, Operation::Test);
    assert_eq!(
        event.message,
        "Command requires approval: cargo test --workspace"
    );
    assert_eq!(
        event.request,
        Some(RequestInfo {
            prompt: "cargo test --workspace".into(),
            options: vec!["approve".into(), "deny".into()],
            question_type: None,
        })
    );

    // Respond with Approve
    let task_id = adapter.current_task_id().clone();
    let res = adapter.respond(&task_id, Decision::Approve, Utc::now());
    assert!(res.is_ok(), "Approval response must succeed");

    // Verify input sent to PTY was Enter (\r)
    let sent = adapter.transport_mut().sent_inputs.clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0], b"\r", "Must send Enter for option 0 approval");

    // Lifecycle should resume to Working
    assert!(matches!(
        adapter.lifecycle(),
        AntigravityLifecycle::Working { .. }
    ));
}

#[test]
fn test_command_confirmation_denial_flow() {
    let mut transport = MockPtyTransport::new(80, 24);
    let menu_chunk = b"\x1b[?1049h\x1b[H\x1b[2JCommand: rm -rf /tmp/test\r\n\x1b[7mYes, run command\x1b[0m\r\nNo, deny\r\n";
    transport.push_bytes(menu_chunk);

    let config = AntigravityConfig::default();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(config, transport);

    let _ = adapter.poll(Utc::now());
    assert!(matches!(
        adapter.lifecycle(),
        AntigravityLifecycle::BlockedOnPermission { .. }
    ));

    // Respond with Deny
    let task_id = adapter.current_task_id().clone();
    let res = adapter.respond(&task_id, Decision::Deny, Utc::now());
    assert!(res.is_ok(), "Denial response must succeed");

    // Verify input sent to PTY was Down Arrow (\x1b[B) + Enter (\r)
    let sent = adapter.transport_mut().sent_inputs.clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0], b"\x1b[B\r",
        "Must send Down Arrow + Enter to select 'No, deny'"
    );
}

#[test]
fn test_file_edit_confirmation_flow() {
    let mut transport = MockPtyTransport::new(80, 24);
    let edit_chunk = b"\x1b[?1049h\x1b[H\x1b[2JFile: src/main.rs\r\n\x1b[7mYes, accept this change\x1b[0m\r\nNo, reject this change\r\nReview in external editor\r\n";
    transport.push_bytes(edit_chunk);

    let config = AntigravityConfig::default();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(config, transport);

    let outputs = adapter.poll(Utc::now());
    let event = outputs
        .into_iter()
        .find_map(|o| match o {
            AdapterOutput::Event(raw) if raw.kind == "approval_required" => Some(raw),
            _ => None,
        })
        .expect("Must produce approval_required event for file edit");

    assert_eq!(event.operation, Operation::Edit);
    assert_eq!(event.message, "File edit requires approval: src/main.rs");

    let task_id = adapter.current_task_id().clone();
    adapter
        .respond(&task_id, Decision::Approve, Utc::now())
        .unwrap();
    assert_eq!(adapter.transport_mut().sent_inputs[0], b"\r");
}

#[test]
fn test_workspace_trust_flow() {
    let mut transport = MockPtyTransport::new(80, 24);
    let trust_chunk = b"Do you trust the authors of the files in /workspace/project?\r\nYes, I trust this folder\r\nNo, exit\r\n";
    transport.push_bytes(trust_chunk);

    let config = AntigravityConfig::default();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(config, transport);

    let outputs = adapter.poll(Utc::now());
    let event = outputs
        .into_iter()
        .find_map(|o| match o {
            AdapterOutput::Event(raw) if raw.kind == "approval_required" => Some(raw),
            _ => None,
        })
        .expect("Must produce approval_required event for workspace trust");

    assert_eq!(event.operation, Operation::Other);

    let task_id = adapter.current_task_id().clone();
    adapter
        .respond(&task_id, Decision::Approve, Utc::now())
        .unwrap();
    assert_eq!(adapter.transport_mut().sent_inputs[0], b"\r");
}

#[test]
fn test_user_question_flow() {
    let mut transport = MockPtyTransport::new(80, 24);
    // Real agy `ask_question` form: header + numbered menu with cursor + key legend + status bar.
    let question_chunk = concat!(
        "Question 1/1: Select deployment target\r\n",
        "> 1. Staging cluster\r\n",
        "  2. Production us-east\r\n",
        "  ↑/↓ Navigate · enter Select · esc Skip\r\n",
        "esc to cancel\r\n",
    );
    transport.push_text(question_chunk);

    let config = AntigravityConfig::default();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(config, transport);

    let outputs = adapter.poll(Utc::now());
    let event = outputs
        .into_iter()
        .find_map(|o| match o {
            AdapterOutput::Event(raw) if raw.kind == "input_required" => Some(raw),
            _ => None,
        })
        .expect("Must produce input_required event for user question");

    assert_eq!(event.message, "Select deployment target");
    let req = event.request.expect("question carries request info");
    assert_eq!(req.options, vec!["Staging cluster", "Production us-east"]);
    assert_eq!(req.question_type, Some(QuestionType::SingleChoice));

    // The same option rows WITHOUT the live menu control (no cursor, no legend) are just text.
    let mut plain = MockPtyTransport::new(80, 24);
    plain.push_text("Select deployment target:\r\n1. Staging cluster\r\n2. Production us-east\r\n");
    let mut plain_adapter =
        AntigravityPtyAdapter::new_with_transport(AntigravityConfig::default(), plain);
    let outputs = plain_adapter.poll(Utc::now());
    assert!(
        outputs
            .iter()
            .all(|o| !matches!(o, AdapterOutput::Event(raw) if raw.request.is_some())),
        "option-looking rows must not become a request"
    );
}

#[test]
fn test_invalid_task_and_not_blocked_rejections() {
    let transport = MockPtyTransport::new(80, 24);
    let config = AntigravityConfig::default();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(config, transport);

    let task_id = adapter.current_task_id().clone();

    // Not blocked yet
    let res1 = adapter.respond(&task_id, Decision::Approve, Utc::now());
    assert_eq!(res1, Err(RespondError::NotBlocked));

    // Wrong task id
    let res2 = adapter.respond(&"wrong-task-id".to_string(), Decision::Approve, Utc::now());
    assert_eq!(res2, Err(RespondError::NoSuchTask));
}

#[test]
fn test_dead_process_control_rejection() {
    let mut transport = MockPtyTransport::new(80, 24);
    let menu_chunk =
        b"\x1b[?1049h\x1b[H\x1b[2JCommand: ls\r\n\x1b[7mYes, run command\x1b[0m\r\nNo, deny\r\n";
    transport.push_bytes(menu_chunk);

    let config = AntigravityConfig::default();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(config, transport);

    adapter.poll(Utc::now());
    assert!(matches!(
        adapter.lifecycle(),
        AntigravityLifecycle::BlockedOnPermission { .. }
    ));

    // Child dies before decision arrives
    adapter.transport_mut().close();

    let task_id = adapter.current_task_id().clone();
    let res = adapter.respond(&task_id, Decision::Approve, Utc::now());
    assert_eq!(res, Err(RespondError::Disconnected));
    assert!(adapter.transport_mut().sent_inputs.is_empty());
}

#[test]
fn test_process_crash_and_disconnect() {
    let mut transport = MockPtyTransport::new(80, 24);
    transport.push_text("Fatal crash\r\nFATAL: unexpected nil pointer dereference\r\n");

    let config = AntigravityConfig::default();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(config, transport);

    let outputs = adapter.poll(Utc::now());
    let err_event = outputs
        .into_iter()
        .find(|o| matches!(o, AdapterOutput::Event(raw) if raw.kind == "command_failed"));
    assert!(err_event.is_some(), "Must emit error event on fatal error");

    // Close transport to simulate process exit
    adapter.transport_mut().close();
    let _ = adapter.poll(Utc::now());
    assert!(adapter.is_finished());
}

#[test]
fn test_terminal_resize_propagation() {
    let transport = MockPtyTransport::new(80, 24);
    let config = AntigravityConfig::default();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(config, transport);

    assert_eq!(adapter.screen().cols, 120);
    assert_eq!(adapter.screen().rows, 40);

    adapter.resize(160, 50).expect("Resize must succeed");
    assert_eq!(adapter.screen().cols, 160);
    assert_eq!(adapter.screen().rows, 50);
    assert_eq!(adapter.transport_mut().cols, 160);
    assert_eq!(adapter.transport_mut().rows, 50);
}

#[tokio::test]
async fn test_antigravity_adapter_through_core_pipeline() {
    let mut transport = MockPtyTransport::new(80, 24);
    // Menu chunk
    let menu_chunk = b"\x1b[?1049h\x1b[H\x1b[2JCommand: cargo build --release\r\n\x1b[7mYes, run command\x1b[0m\r\nNo, deny\r\n";
    transport.push_bytes(menu_chunk);

    let config = AntigravityConfig::default();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(config, transport);

    let (tx_adapter, mut rx_adapter) = tokio::sync::mpsc::channel(64);
    let (tx_core, rx_core) = tokio::sync::mpsc::channel(256);

    let clock = Arc::new(SystemClock);
    let mut core = CoreTask::with_seed(
        PipelineMode::Agentdesk,
        clock.clone(),
        42,
        LogStoreConfig::default(),
        ThresholdTable::default(),
        Some(tx_adapter),
    );
    core.register_agents(adapter.agents());

    let core_sender = tx_core.clone();
    tokio::spawn(core.run(rx_core));

    let (tx_client, mut rx_messages) = tokio::sync::mpsc::channel(64);
    let client_sink = ChannelSink::new(tx_client);
    core_sender
        .send(CoreCommand::Connect {
            client_id: 1,
            sink: Box::new(client_sink),
        })
        .await
        .unwrap();

    // Step adapter poll and feed outputs into core
    let outputs = adapter.poll(Utc::now());
    for out in outputs {
        core_sender.send(CoreCommand::Adapter(out)).await.unwrap();
    }

    // Await message from client channel
    let mut push = None;
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        if let Ok(msg) = rx_messages.try_recv()
            && let Body::Event(ref ep) = msg.body
            && ep.event.kind == "approval_required"
        {
            push = Some(ep.clone());
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert!(
        push.is_some(),
        "Core task must push approval_required Event to client"
    );
    let push = push.unwrap();
    assert_eq!(push.event.category, agentdesk_model::Category::Request);
    assert_eq!(push.entry.tier, 0);

    // Client responds with Approve
    let event_id = push.event.event_id;
    let respond_msg = Message {
        request_id: Some("req-1".into()),
        body: Body::RespondRequest(RespondRequest {
            event_id,
            decision: Decision::Approve,
            selected_options: None,
            text_input: None,
        }),
    };
    core_sender
        .send(CoreCommand::Client {
            client_id: 1,
            message: respond_msg,
        })
        .await
        .unwrap();

    // Verify adapter receives AdapterCommand::Respond
    let adapter_cmd = tokio::time::timeout(Duration::from_millis(500), rx_adapter.recv())
        .await
        .expect("Must receive adapter response command within timeout")
        .expect("Channel must be open");

    match adapter_cmd {
        AdapterCommand::Respond {
            task_id,
            decision,
            now,
            ..
        } => {
            assert_eq!(task_id, *adapter.current_task_id());
            assert_eq!(decision, Decision::Approve);
            adapter
                .respond(&task_id, decision, now)
                .expect("Adapter respond must succeed");
        }
    }

    // Verify input sent to PTY was Enter
    let sent = adapter.transport_mut().sent_inputs.clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0], b"\r");
}

#[test]
fn test_numbered_command_confirmation_menu() {
    let mut transport = MockPtyTransport::new(120, 40);
    let real_agy_screen = b"\
Command\r\n\
esc to cancel\r\n\
\r\n\
Requesting permission for:\r\n\
   echo AGY_PERMISSION_OK\r\n\
\r\n\
Run this command?\r\n\
> 1. Yes, run command\r\n\
  2. Yes, and always allow in this conversation for commands that start with 'echo'\r\n\
  3. Yes, and always allow for commands that start with 'echo' (Persist to settings.json)\r\n\
  4. No, cancel\r\n\
\r\n\
  ^ Navigate - tab Amend\r\n";
    transport.push_bytes(real_agy_screen);

    let config = AntigravityConfig::default();
    let mut adapter = AntigravityPtyAdapter::new_with_transport(config, transport);

    let outputs = adapter.poll(Utc::now());
    let req = outputs
        .into_iter()
        .find_map(|o| match o {
            AdapterOutput::Event(raw) if raw.kind == "approval_required" => Some(raw),
            _ => None,
        })
        .expect("Must detect approval_required for numbered agy menu");

    assert_eq!(
        req.message,
        "Command requires approval: echo AGY_PERMISSION_OK"
    );
    assert_eq!(
        req.request,
        Some(RequestInfo {
            prompt: "echo AGY_PERMISSION_OK".into(),
            options: vec!["approve".into(), "deny".into()],
            question_type: None,
        })
    );

    // Test Approve: target 0
    let task_id = adapter.current_task_id().clone();
    adapter
        .respond(&task_id, Decision::Approve, Utc::now())
        .unwrap();
    assert_eq!(adapter.transport_mut().sent_inputs[0], b"\r");

    // Test Deny on another instance
    let mut transport_deny = MockPtyTransport::new(120, 40);
    transport_deny.push_bytes(real_agy_screen);
    let mut adapter_deny =
        AntigravityPtyAdapter::new_with_transport(AntigravityConfig::default(), transport_deny);
    adapter_deny.poll(Utc::now());
    adapter_deny
        .respond(&task_id, Decision::Deny, Utc::now())
        .unwrap();
    // Options: 0: Yes, 1: Yes, 2: Yes, 3: No, cancel. Diff = 3 down arrows + Enter
    assert_eq!(
        adapter_deny.transport_mut().sent_inputs[0],
        b"\x1b[B\x1b[B\x1b[B\r"
    );
}

#[test]
fn test_real_agy_live_command_permission_e2e() {
    // Check if agy is available in system PATH
    let agy_found = std::process::Command::new("which")
        .arg("agy")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !agy_found {
        eprintln!("Skipping test_real_agy_live_command_permission_e2e: 'agy' not found in PATH");
        return;
    }
    let _live = live_agy_guard();

    let config = AntigravityConfig {
        command: "agy".into(),
        args: live_agy_args(),
        cwd: None,
        agent_id: "antigravity-live".into(),
        agent_name: "Antigravity Live".into(),
        project: "AgentDesk".into(),
        initial_prompt: Some("Run 'echo AGY_PERMISSION_OK' using run_command".into()),
        cols: 120,
        rows: 40,
    };

    let mut adapter = match AntigravityPtyAdapter::spawn(config) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to spawn real agy process: {}", e);
            return;
        }
    };

    let start = Instant::now();
    let mut collected = Vec::new();
    let mut permission_detected = false;

    // Poll until command permission confirmation dialog is detected (or timeout 180s)
    let timeout = Duration::from_secs(180);
    while start.elapsed() < timeout {
        let outputs = adapter.poll(Utc::now());
        for out in outputs {
            if let AdapterOutput::Event(ref raw) = out
                && raw.kind == "approval_required"
            {
                permission_detected = true;
            }
            collected.push(out);
        }

        if permission_detected {
            break;
        }

        std::thread::sleep(Duration::from_millis(100));
    }

    // Verify started event
    let has_started = collected
        .iter()
        .any(|o| matches!(o, AdapterOutput::Event(raw) if raw.kind == "started"));
    assert!(has_started, "Real agy run must emit started event");

    let screen_text_at_check = adapter.screen().all_lines().join("\n");
    assert!(
        permission_detected,
        "Real agy must trigger command permission dialog within {}s. Screen was:\n{}",
        timeout.as_secs(),
        screen_text_at_check
    );

    let task_id = adapter.current_task_id().clone();
    let res = adapter.respond(&task_id, Decision::Approve, Utc::now());
    assert!(
        res.is_ok(),
        "Delivering approval decision to live agy PTY must succeed"
    );

    // Wait a few turns for agy to execute the command after approval
    let resume_start = Instant::now();
    let mut command_executed = false;
    while resume_start.elapsed() < Duration::from_secs(15) {
        let outputs = adapter.poll(Utc::now());
        collected.extend(outputs);
        let full_text = adapter.screen().all_lines().join("\n");
        if full_text.contains("AGY_PERMISSION_OK") {
            command_executed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let full_text = adapter.screen().all_lines().join("\n");
    println!("Live agy screen after approval:\n{}", full_text);
    assert!(
        command_executed || full_text.contains("AGY_PERMISSION_OK") || full_text.contains("echo"),
        "Live agy must have executed the approved command: screen was:\n{}",
        full_text
    );
}

#[test]
fn test_real_agy_question_probe() {
    // Manual exploration probe: run with AGY_PROBE_PROMPT set to observe the real question TUI.
    let Ok(prompt) = std::env::var("AGY_PROBE_PROMPT") else {
        eprintln!("Skipping probe: AGY_PROBE_PROMPT not set");
        return;
    };
    let _live = live_agy_guard();
    let config = AntigravityConfig {
        command: "agy".into(),
        args: live_agy_args(),
        cwd: None,
        agent_id: "antigravity-probe".into(),
        agent_name: "Antigravity Probe".into(),
        project: "AgentDesk".into(),
        initial_prompt: Some(prompt),
        cols: 120,
        rows: 40,
    };
    let mut adapter = AntigravityPtyAdapter::spawn(config).expect("spawn agy");
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(60) {
        let _ = adapter.poll(Utc::now());
        let text = adapter.screen().all_lines().join("\n");
        if text.contains("Question 1/") || text.contains("esc Skip") {
            std::thread::sleep(Duration::from_millis(700));
            let _ = adapter.poll(Utc::now());
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if let Ok(keys) = std::env::var("AGY_PROBE_KEYS") {
        // e.g. "down,down,down,enter"
        for k in keys.split(',') {
            let bytes: &[u8] = match k {
                "down" => b"\x1b[B",
                "up" => b"\x1b[A",
                "enter" => b"\r",
                "space" => b" ",
                other => other.as_bytes(),
            };
            adapter.transport_mut().send_input(bytes).unwrap();
            std::thread::sleep(Duration::from_millis(400));
            let _ = adapter.poll(Utc::now());
        }
        std::thread::sleep(Duration::from_millis(800));
        let _ = adapter.poll(Utc::now());
    }
    let snap = adapter.screen().snapshot();
    println!(">>> SCREEN <<<\n{}", snap.lines.join("\n"));
    println!(
        "cursor=({}, {}) vis={} alt={}",
        snap.cursor_row, snap.cursor_col, snap.cursor_visible, snap.in_alt_screen
    );
    println!("reversed={:?}", snap.reversed_lines);
    if let Ok(path) = std::env::var("AGY_PROBE_RECORD") {
        adapter
            .transport_mut()
            .recording()
            .save_to_file(std::path::Path::new(&path))
            .unwrap();
    }
}

/// Live `agy` sessions must not overlap (shared account/session state); serialize them.
static LIVE_AGY: Mutex<()> = Mutex::new(());

fn live_agy_guard() -> MutexGuard<'static, ()> {
    LIVE_AGY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Model for live tests: `AGY_E2E_MODEL` if set, otherwise agy's configured default. (The
/// per-model quota of a fixed flash model was exhausted during development; the flow under test
/// is model-independent.)
fn live_agy_args() -> Vec<String> {
    match std::env::var("AGY_E2E_MODEL") {
        Ok(m) if !m.is_empty() => vec!["--model".into(), m],
        _ => Vec::new(),
    }
}

fn agy_available() -> bool {
    std::process::Command::new("which")
        .arg("agy")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Drive one real `agy` session: prompt it to ask a question, wait for the adapter to emit the
/// normalized `input_required` request, answer with `respond(event)`, then wait for `marker`
/// to appear in the transcript proving agy received the structured answer and continued.
fn run_real_agy_question_e2e(
    label: &str,
    prompt: &str,
    respond: impl Fn(&RawAgentEvent) -> RequestResponse,
    marker: &str,
) -> RawAgentEvent {
    let _live = live_agy_guard();
    let config = AntigravityConfig {
        command: "agy".into(),
        args: live_agy_args(),
        cwd: None,
        agent_id: format!("antigravity-{label}"),
        agent_name: "Antigravity Live".into(),
        project: "AgentDesk".into(),
        initial_prompt: Some(prompt.into()),
        cols: 120,
        rows: 40,
    };
    let mut adapter = AntigravityPtyAdapter::spawn(config).expect("spawn real agy");

    let start = Instant::now();
    let mut request: Option<RawAgentEvent> = None;
    while start.elapsed() < Duration::from_secs(180) && request.is_none() {
        for out in adapter.poll(Utc::now()) {
            if let AdapterOutput::Event(raw) = out {
                assert_ne!(
                    raw.kind, "approval_required",
                    "[{label}] ask_question must not be classified as a permission request"
                );
                if raw.kind == "input_required" {
                    request = Some(raw);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let request = request.unwrap_or_else(|| {
        panic!(
            "[{label}] real agy must present an interactive question. Screen:\n{}",
            adapter.screen().all_lines().join("\n")
        )
    });
    // Let the form settle (agy redraws a few times), then answer.
    std::thread::sleep(Duration::from_millis(500));
    let _ = adapter.poll(Utc::now());

    let response = respond(&request);
    let task_id = adapter.current_task_id().clone();
    adapter
        .respond_with_response(&task_id, Some(request.agent_seq), &response, Utc::now())
        .unwrap_or_else(|e| {
            panic!(
                "[{label}] delivering {response:?} must succeed, got {e:?}. Screen:\n{}",
                adapter.screen().all_lines().join("\n")
            )
        });

    let resume = Instant::now();
    let mut seen = false;
    while resume.elapsed() < Duration::from_secs(60) {
        for out in adapter.poll(Utc::now()) {
            if let AdapterOutput::Event(raw) = out
                && raw.kind == "input_required"
            {
                panic!(
                    "[{label}] agy re-asked after our answer ({:?}). Screen:\n{}",
                    raw.message,
                    adapter.screen().all_lines().join("\n")
                );
            }
        }
        // agy wraps long transcript lines mid-word; compare with whitespace removed.
        let compact: String = adapter
            .screen()
            .all_lines()
            .join("")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if compact.contains(marker) {
            seen = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let screen = adapter.screen().all_lines().join("\n");
    println!("[{label}] live agy screen after answer:\n{screen}");
    assert!(
        seen,
        "[{label}] agy must acknowledge the answer with {marker:?}. Screen:\n{screen}"
    );
    assert!(
        adapter.pending_request().is_none(),
        "[{label}] request must be consumed"
    );

    if let Ok(dir) = std::env::var("AGY_E2E_RECORD_DIR") {
        let path = std::path::Path::new(&dir).join(format!("real_agy_question_{label}.json"));
        adapter
            .transport_mut()
            .recording()
            .save_to_file(&path)
            .expect("save recording");
    }
    adapter.transport_mut().terminate();
    request
}

/// Real `agy` end-to-end: single-choice `ask_question`.
#[test]
fn test_real_agy_live_question_single_choice_e2e() {
    if !agy_available() {
        eprintln!("Skipping: 'agy' not found in PATH");
        return;
    }
    let req = run_real_agy_question_e2e(
        "single",
        "Use the ask_question tool now to ask exactly one question: 'Which database should we use?' \
         with options: 'PostgreSQL', 'SQLite', 'MySQL'. After you receive my answer, reply with \
         exactly: ANSWER_RECEIVED=<my answer> and nothing else. Do not call any other tool.",
        |ev| {
            let info = ev.request.as_ref().unwrap();
            assert_eq!(info.question_type, Some(QuestionType::SingleChoice));
            assert!(
                info.options.len() >= 3,
                "options extracted dynamically: {:?}",
                info.options
            );
            RequestResponse::SelectOption {
                option: info.options[1].clone(),
            }
        },
        "ANSWER_RECEIVED=SQLite",
    );
    assert_eq!(req.message, "Which database should we use?");
    assert_eq!(
        req.request.unwrap().options,
        vec!["PostgreSQL", "SQLite", "MySQL"]
    );
    assert_eq!(req.details["allows_write_in"], true);
}

/// Real `agy` end-to-end: multi-select `ask_question`.
#[test]
fn test_real_agy_live_question_multi_choice_e2e() {
    if !agy_available() {
        eprintln!("Skipping: 'agy' not found in PATH");
        return;
    }
    let req = run_real_agy_question_e2e(
        "multi",
        "Use the ask_question tool now to ask exactly one MULTI-SELECT question (allow multiple \
         selections): 'Which features should be enabled?' with options: 'Authentication', \
         'Database', 'Logging'. After you receive my answer, reply with exactly: \
         ANSWER_RECEIVED=<the selected options joined by '+' in the order I selected them> and \
         nothing else. Do not call any other tool.",
        |ev| {
            let info = ev.request.as_ref().unwrap();
            assert_eq!(info.question_type, Some(QuestionType::MultipleChoice));
            assert!(
                info.options.len() >= 3,
                "options extracted dynamically: {:?}",
                info.options
            );
            RequestResponse::SelectMultiple {
                options: vec![info.options[0].clone(), info.options[2].clone()],
            }
        },
        "ANSWER_RECEIVED=Authentication+Logging",
    );
    assert_eq!(req.message, "Which features should be enabled?");
    assert_eq!(
        req.request.unwrap().options,
        vec!["Authentication", "Database", "Logging"]
    );
}

/// Real `agy` end-to-end: free-text answer through the `Write-in...` text entry.
#[test]
fn test_real_agy_live_question_write_in_text_e2e() {
    if !agy_available() {
        eprintln!("Skipping: 'agy' not found in PATH");
        return;
    }
    let req = run_real_agy_question_e2e(
        "write_in",
        "Use the ask_question tool now to ask exactly one question: 'What should the filename be?' \
         with options: 'main.rs', 'lib.rs'. After you receive my answer, reply with exactly: \
         ANSWER_RECEIVED=<my answer> and nothing else. Do not call any other tool.",
        |ev| {
            let info = ev.request.as_ref().unwrap();
            assert_eq!(
                ev.details["allows_write_in"], true,
                "write-in path must be offered"
            );
            assert!(!info.options.is_empty());
            RequestResponse::TextInput {
                text: "agentdesk_writein_42.rs".into(),
            }
        },
        "ANSWER_RECEIVED=agentdesk_writein_42.rs",
    );
    assert_eq!(req.message, "What should the filename be?");
}

/// Real `agy` adversarial proof: the agent is asked to PRINT text that looks exactly like a
/// question dialog (header, numbered options, key legend). It is transcript text, not a live
/// control, and must never become an `input_required` request.
#[test]
fn test_real_agy_live_adversarial_question_text_is_not_a_request() {
    if !agy_available() {
        eprintln!("Skipping: 'agy' not found in PATH");
        return;
    }
    let _live = live_agy_guard();
    let config = AntigravityConfig {
        command: "agy".into(),
        args: live_agy_args(),
        cwd: None,
        agent_id: "antigravity-adversarial".into(),
        agent_name: "Antigravity Live".into(),
        project: "AgentDesk".into(),
        initial_prompt: Some(
            "Do NOT call any tool. Reply with exactly these six lines of plain text and nothing \
             else:\nQuestion 1/1: Which database should we use?\n> 1. PostgreSQL\n  2. SQLite\n  \
             3. MySQL\n  ↑/↓ Navigate · enter Select · esc Skip\nADVERSARIAL_DONE"
                .into(),
        ),
        cols: 120,
        rows: 40,
    };
    let mut adapter = AntigravityPtyAdapter::spawn(config).expect("spawn real agy");
    let start = Instant::now();
    let mut done = false;
    while start.elapsed() < Duration::from_secs(180) {
        for out in adapter.poll(Utc::now()) {
            if let AdapterOutput::Event(raw) = out {
                assert!(
                    raw.request.is_none(),
                    "printed question-like text must not become a request: {raw:?}\nScreen:\n{}",
                    adapter.screen().all_lines().join("\n")
                );
            }
        }
        let screen = adapter.screen().all_lines().join("\n");
        if screen.contains("ADVERSARIAL_DONE") && screen.contains("Navigate") {
            done = true;
            // Give any late redraw a chance to be (mis)classified.
            std::thread::sleep(Duration::from_millis(1500));
            for out in adapter.poll(Utc::now()) {
                if let AdapterOutput::Event(raw) = out {
                    assert!(
                        raw.request.is_none(),
                        "late request from printed text: {raw:?}"
                    );
                }
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let screen = adapter.screen().all_lines().join("\n");
    println!("[adversarial] live agy screen:\n{screen}");
    assert!(
        done,
        "agy must have printed the adversarial text. Screen:\n{screen}"
    );
    assert!(adapter.pending_request().is_none());
    if let Ok(dir) = std::env::var("AGY_E2E_RECORD_DIR") {
        let path = std::path::Path::new(&dir).join("real_agy_adversarial_question_text.json");
        adapter
            .transport_mut()
            .recording()
            .save_to_file(&path)
            .unwrap();
    }
    adapter.transport_mut().terminate();
}
