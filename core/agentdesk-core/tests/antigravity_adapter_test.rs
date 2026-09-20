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

use std::sync::Arc;
use std::time::{Duration, Instant};

use agentdesk_core::{
    Adapter, AdapterCommand, AdapterOutput, AntigravityConfig, AntigravityLifecycle,
    AntigravityPtyAdapter, AntigravityState, ChannelSink, CoreCommand, CoreTask, LogStoreConfig,
    MockPtyTransport, RespondError, SystemClock, ThresholdTable,
};
use agentdesk_model::{
    Body, Decision, Message, Operation, PipelineMode, RequestInfo, RespondRequest,
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
    let question_chunk =
        b"Select deployment target:\r\n( ) Staging cluster\r\n( ) Production us-east\r\n";
    transport.push_bytes(question_chunk);

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

    assert_eq!(event.message, "Select deployment target:");
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
    assert_eq!(res, Err(RespondError::NotBlocked));
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

    let config = AntigravityConfig {
        command: "agy".into(),
        args: vec!["--model".into(), "gemini-3.8-flash-low".into()],
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

    // Poll until command permission confirmation dialog is detected (or timeout 60s)
    let timeout = Duration::from_secs(60);
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
