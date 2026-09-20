//! Comprehensive deterministic tests for the ACP Real-Agent Adapter.
//! Verifies:
//! - Full lifecycle (initialize -> session/new -> prompt -> progress -> completion)
//! - Permission approval and denial feedback flows
//! - Dead-process control rejection
//! - Invalid task rejection
//! - Unexpected process crash / EOF disconnect
//! - JSON-RPC error handling
//! - Bounded log forwarding with strict immunity to terminal-text scraping
//! - Full integration through the AgentDesk EventProcessor and PriorityQueue

use chrono::Utc;
use serde_json::json;

use agentdesk_core::{
    classify, AcpAdapter, AcpConfig, AcpState, AcpTransport, Adapter, AdapterOutput,
    EventProcessor, LogStore, LogStoreConfig, MockAcpTransport, PriorityQueue, RespondError,
};
use agentdesk_model::{Category, Decision, Operation, Severity};

fn make_test_config() -> AcpConfig {
    AcpConfig {
        command: "gemini".into(),
        args: vec!["--skip-trust".into(), "--acp".into()],
        cwd: None,
        agent_id: "test-agent".into(),
        agent_name: "Test Agent".into(),
        project: "AgentDesk".into(),
        initial_prompt: "Test prompt".into(),
    }
}

#[test]
fn test_happy_path_initialization_and_turn() {
    let transport = MockAcpTransport::new();
    let config = make_test_config();
    let mut adapter = AcpAdapter::new_with_transport(config, transport);

    let now = Utc::now();

    // 1. First poll triggers initialize request
    let out = adapter.poll(now);
    assert!(out.is_empty());
    assert_eq!(*adapter.state(), AcpState::Initializing);
    assert_eq!(adapter.transport_mut().sent_lines.len(), 1);
    let init_sent: serde_json::Value =
        serde_json::from_str(&adapter.transport_mut().sent_lines[0]).unwrap();
    assert_eq!(init_sent["method"], "initialize");

    // 2. Agent replies with initialize response
    adapter.transport_mut().push_stdout(
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": 1,
                "agentInfo": {"name": "gemini-cli", "version": "0.52.0"}
            }
        })
        .to_string(),
    );

    let out = adapter.poll(now);
    assert!(out.is_empty());
    assert_eq!(*adapter.state(), AcpState::CreatingSession);
    assert_eq!(adapter.transport_mut().sent_lines.len(), 2);
    let sess_new_sent: serde_json::Value =
        serde_json::from_str(&adapter.transport_mut().sent_lines[1]).unwrap();
    assert_eq!(sess_new_sent["method"], "session/new");

    // 3. Agent replies with session/new response
    adapter.transport_mut().push_stdout(
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "sessionId": "session-12345"
            }
        })
        .to_string(),
    );

    let out = adapter.poll(now);
    assert_eq!(out.len(), 1);
    match &out[0] {
        AdapterOutput::Event(raw) => {
            assert_eq!(raw.kind, "started");
            assert_eq!(raw.task_id.as_deref(), Some("session-12345"));
            assert_eq!(classify(&raw.kind).category, Category::Working);
        }
        _ => panic!("Expected RawAgentEvent for started"),
    }
    assert_eq!(*adapter.state(), AcpState::Prompting);
    assert_eq!(adapter.current_task_id().map(|s| s.as_str()), Some("session-12345"));

    // Verify session/prompt was sent
    assert_eq!(adapter.transport_mut().sent_lines.len(), 3);
    let prompt_sent: serde_json::Value =
        serde_json::from_str(&adapter.transport_mut().sent_lines[2]).unwrap();
    assert_eq!(prompt_sent["method"], "session/prompt");
    assert_eq!(prompt_sent["params"]["sessionId"], "session-12345");

    // 4. Agent emits progress update and thought chunk
    adapter.transport_mut().push_stdout(
        json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "session-12345",
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "call-1",
                    "title": "cargo test",
                    "kind": "execute",
                    "status": "in_progress"
                }
            }
        })
        .to_string(),
    );
    adapter.transport_mut().push_stdout(
        json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "session-12345",
                "update": {
                    "sessionUpdate": "agent_thought_chunk",
                    "content": {"type": "text", "text": "Analyzing the test results..."}
                }
            }
        })
        .to_string(),
    );

    let out = adapter.poll(now);
    assert_eq!(out.len(), 2);
    // First should be progress event (tool_call in_progress)
    match &out[0] {
        AdapterOutput::Event(raw) => {
            assert_eq!(raw.kind, "progress");
            assert_eq!(raw.operation, Operation::Build);
            assert_eq!(classify(&raw.kind).category, Category::Working);
        }
        _ => panic!("Expected progress event"),
    }
    // Second should be text line from thought chunk
    match &out[1] {
        AdapterOutput::Line { agent_id, text } => {
            assert_eq!(agent_id, "test-agent");
            assert_eq!(text, "Analyzing the test results...");
        }
        _ => panic!("Expected Line output for thought"),
    }

    // 5. Agent sends completion response to prompt (id: 3)
    adapter.transport_mut().push_stdout(
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "result": {
                "stopReason": "end_turn"
            }
        })
        .to_string(),
    );

    let out = adapter.poll(now);
    assert_eq!(out.len(), 1);
    match &out[0] {
        AdapterOutput::Event(raw) => {
            assert_eq!(raw.kind, "task_completed");
            assert_eq!(classify(&raw.kind).category, Category::Completed);
            assert_eq!(classify(&raw.kind).severity, Severity::Important);
        }
        _ => panic!("Expected task_completed event"),
    }

    assert!(adapter.is_finished());
    assert_eq!(adapter.next_due(), None);
}

#[test]
fn test_permission_approval_flow() {
    let transport = MockAcpTransport::new();
    let config = make_test_config();
    let mut adapter = AcpAdapter::new_with_transport(config, transport);
    let now = Utc::now();

    // Fast-forward through init
    adapter.poll(now);
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": 1}}).to_string(),
    );
    adapter.poll(now);
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "sess-perm"}}).to_string(),
    );
    adapter.poll(now);

    let task_id = "sess-perm".to_string();

    // Agent requests permission
    adapter.transport_mut().push_stdout(
        json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "session/request_permission",
            "params": {
                "sessionId": "sess-perm",
                "toolCall": {
                    "toolCallId": "call-perm-1",
                    "title": "rm -rf target/temp",
                    "kind": "delete",
                    "status": "pending"
                },
                "options": [
                    {"optionId": "opt-allow", "kind": "allow_once", "name": "Allow delete"},
                    {"optionId": "opt-deny", "kind": "reject_once", "name": "Cancel delete"}
                ]
            }
        })
        .to_string(),
    );

    let out = adapter.poll(now);
    assert_eq!(out.len(), 1);
    match &out[0] {
        AdapterOutput::Event(raw) => {
            assert_eq!(raw.kind, "approval_required");
            assert_eq!(raw.operation, Operation::Edit);
            let cls = classify(&raw.kind);
            assert_eq!(cls.category, Category::Request);
            assert_eq!(cls.severity, Severity::Critical);
            assert_eq!(cls.summary, "Approval Required");
            assert!(raw.request.is_some());
            let req = raw.request.as_ref().unwrap();
            assert_eq!(req.prompt, "rm -rf target/temp");
            assert_eq!(req.options, vec!["approve", "deny"]);
        }
        _ => panic!("Expected approval_required event"),
    }

    // While blocked, next_due is None
    assert_eq!(adapter.next_due(), None);

    // Human approves via respond
    let res = adapter.respond(&task_id, Decision::Approve, now);
    assert!(res.is_ok());

    // Verify sent JSON-RPC response selected the allow option
    let sent = adapter.transport_mut().sent_lines.last().unwrap();
    let sent_json: serde_json::Value = serde_json::from_str(sent).unwrap();
    assert_eq!(sent_json["id"], 99);
    assert_eq!(sent_json["result"]["outcome"]["outcome"], "selected");
    assert_eq!(sent_json["result"]["outcome"]["optionId"], "opt-allow");

    // Agent completes
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 3, "result": {"stopReason": "end_turn"}}).to_string(),
    );
    let out = adapter.poll(now);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0], AdapterOutput::Event(agentdesk_model::RawAgentEvent {
        agent_id: "test-agent".into(),
        agent_seq: 3,
        task_id: Some("sess-perm".into()),
        kind: "task_completed".into(),
        operation: Operation::Other,
        message: "Prompt completed (end_turn)".into(),
        details: agentdesk_model::Details::new(),
        log_lines: vec![json!({"jsonrpc": "2.0", "id": 3, "result": {"stopReason": "end_turn"}}).to_string()],
        request: None,
    }));
    assert!(adapter.is_finished());
}

#[test]
fn test_permission_denial_flow() {
    let transport = MockAcpTransport::new();
    let config = make_test_config();
    let mut adapter = AcpAdapter::new_with_transport(config, transport);
    let now = Utc::now();

    adapter.poll(now);
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": 1}}).to_string(),
    );
    adapter.poll(now);
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "sess-deny"}}).to_string(),
    );
    adapter.poll(now);

    let task_id = "sess-deny".to_string();

    adapter.transport_mut().push_stdout(
        json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "session/request_permission",
            "params": {
                "sessionId": "sess-deny",
                "toolCall": {"title": "Dangerous shell action", "kind": "execute"},
                "options": [
                    {"optionId": "opt-1", "kind": "allow_once", "name": "Allow"},
                    {"optionId": "opt-2", "kind": "reject_once", "name": "Reject"}
                ]
            }
        })
        .to_string(),
    );

    let out = adapter.poll(now);
    assert_eq!(out.len(), 1);

    // Deny the request
    let res = adapter.respond(&task_id, Decision::Deny, now);
    assert!(res.is_ok());

    let sent = adapter.transport_mut().sent_lines.last().unwrap();
    let sent_json: serde_json::Value = serde_json::from_str(sent).unwrap();
    assert_eq!(sent_json["id"], 42);
    assert_eq!(sent_json["result"]["outcome"]["outcome"], "selected");
    assert_eq!(sent_json["result"]["outcome"]["optionId"], "opt-2");
}

#[test]
fn test_dead_process_control_rejection() {
    let transport = MockAcpTransport::new();
    let config = make_test_config();
    let mut adapter = AcpAdapter::new_with_transport(config, transport);
    let now = Utc::now();

    adapter.poll(now);
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": 1}}).to_string(),
    );
    adapter.poll(now);
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "sess-dead"}}).to_string(),
    );
    adapter.poll(now);

    adapter.transport_mut().push_stdout(
        json!({
            "jsonrpc": "2.0",
            "id": 100,
            "method": "session/request_permission",
            "params": {
                "sessionId": "sess-dead",
                "toolCall": {"title": "Need input"},
                "options": [{"optionId": "o1", "kind": "allow_once", "name": "Yes"}]
            }
        })
        .to_string(),
    );
    adapter.poll(now);

    // Process dies / crashes
    adapter.transport_mut().close();

    // Calling respond on dead process safely rejects
    let res = adapter.respond(&"sess-dead".into(), Decision::Approve, now);
    assert_eq!(res, Err(RespondError::NotBlocked));

    // Next poll cleans up with adapter_error
    adapter.transport_mut().push_eof();
    let out = adapter.poll(now);
    assert_eq!(out.len(), 1);
    match &out[0] {
        AdapterOutput::Event(raw) => {
            assert_eq!(raw.kind, "adapter_error");
            assert_eq!(classify(&raw.kind).category, Category::Error);
        }
        _ => panic!("Expected adapter_error"),
    }
}

#[test]
fn test_invalid_task_and_not_blocked_rejections() {
    let transport = MockAcpTransport::new();
    let config = make_test_config();
    let mut adapter = AcpAdapter::new_with_transport(config, transport);
    let now = Utc::now();

    adapter.poll(now);
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": 1}}).to_string(),
    );
    adapter.poll(now);
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "sess-valid"}}).to_string(),
    );
    adapter.poll(now);

    // Not blocked on permission right now
    assert_eq!(
        adapter.respond(&"sess-valid".into(), Decision::Approve, now),
        Err(RespondError::NotBlocked)
    );

    // Invalid task id
    assert_eq!(
        adapter.respond(&"sess-non-existent".into(), Decision::Approve, now),
        Err(RespondError::NoSuchTask)
    );
}

#[test]
fn test_process_crash_and_disconnect() {
    let transport = MockAcpTransport::new();
    let config = make_test_config();
    let mut adapter = AcpAdapter::new_with_transport(config, transport);
    let now = Utc::now();

    adapter.poll(now);
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": 1}}).to_string(),
    );
    adapter.poll(now);
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "sess-crash"}}).to_string(),
    );
    adapter.poll(now);

    // Process sends EOF midway
    adapter.transport_mut().push_eof();
    let out = adapter.poll(now);
    assert_eq!(out.len(), 1);
    match &out[0] {
        AdapterOutput::Event(raw) => {
            assert_eq!(raw.kind, "adapter_error");
            assert_eq!(classify(&raw.kind).category, Category::Error);
            assert_eq!(classify(&raw.kind).severity, Severity::Critical);
        }
        _ => panic!("Expected adapter_error"),
    }
    assert!(adapter.is_finished());
}

#[test]
fn test_jsonrpc_error_handling() {
    let transport = MockAcpTransport::new();
    let config = make_test_config();
    let mut adapter = AcpAdapter::new_with_transport(config, transport);
    let now = Utc::now();

    adapter.poll(now);
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": 1}}).to_string(),
    );
    adapter.poll(now);
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "sess-err"}}).to_string(),
    );
    adapter.poll(now);

    // Prompt fails with JSON-RPC error
    adapter.transport_mut().push_stdout(
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "error": {
                "code": -32000,
                "message": "Model quota exhausted: try again later"
            }
        })
        .to_string(),
    );

    let out = adapter.poll(now);
    assert_eq!(out.len(), 1);
    match &out[0] {
        AdapterOutput::Event(raw) => {
            assert_eq!(raw.kind, "command_failed");
            assert_eq!(classify(&raw.kind).category, Category::Error);
            assert!(raw.message.contains("Model quota exhausted"));
        }
        _ => panic!("Expected command_failed event"),
    }
    assert!(adapter.is_finished());
}

#[test]
fn test_arbitrary_terminal_text_logged_only_never_scraped() {
    let transport = MockAcpTransport::new();
    let config = make_test_config();
    let mut adapter = AcpAdapter::new_with_transport(config, transport);
    let now = Utc::now();

    adapter.poll(now);

    // Send diverse noisy lines on stderr and stdout containing words that
    // a naive regex scraper might mistake for events or errors
    adapter.transport_mut().push_stderr("WARNING: low disk space");
    adapter.transport_mut().push_stderr("FATAL ERROR: cannot connect to debugger");
    adapter.transport_mut().push_stderr("Approval required for sudo access!");
    adapter.transport_mut().push_stdout("Build failed in 3.42s");
    adapter.transport_mut().push_stdout("Error: 15 tests failed, 2 passed");
    adapter.transport_mut().push_stdout("Finished with error code 1");
    adapter.transport_mut().push_stdout("not valid json at all");

    let out = adapter.poll(now);

    // Every single line MUST be emitted as AdapterOutput::Line, NEVER an Event!
    assert_eq!(out.len(), 7);
    for item in out {
        match item {
            AdapterOutput::Line { agent_id, text } => {
                assert_eq!(agent_id, "test-agent");
                assert!(!text.is_empty());
            }
            AdapterOutput::Event(raw) => {
                panic!("Security violation: Terminal text was scraped into event: {:?}", raw);
            }
        }
    }
}

#[test]
fn test_acp_adapter_through_core_pipeline() {
    let transport = MockAcpTransport::new();
    let config = make_test_config();
    let mut adapter = AcpAdapter::new_with_transport(config, transport);
    let now = Utc::now();

    // Set up core pipeline components
    let mut log_store = LogStore::new(LogStoreConfig::default());
    let mut event_store = agentdesk_core::EventStore::new();
    let mut processor = EventProcessor::new();
    processor.register_agent(adapter.agents()[0].clone());
    let mut queue = PriorityQueue::new();
    let mut metrics = agentdesk_core::Metrics::default();

    // Initialize adapter
    adapter.poll(now);
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": 1}}).to_string(),
    );
    adapter.poll(now);
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "sess-pipeline"}}).to_string(),
    );

    let outputs = adapter.poll(now);
    for out in outputs {
        match out {
            AdapterOutput::Line { agent_id, text } => {
                processor.process_line(&agent_id, text, now, &mut log_store, &mut metrics);
            }
            AdapterOutput::Event(raw) => {
                let _ = processor.process_raw_event(
                    raw,
                    now,
                    &mut event_store,
                    &mut queue,
                    &mut log_store,
                    &mut metrics,
                ).unwrap();
            }
        }
    }

    // Now trigger permission request
    adapter.transport_mut().push_stdout(
        json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "session/request_permission",
            "params": {
                "sessionId": "sess-pipeline",
                "toolCall": {"title": "Modify database schema", "kind": "edit"},
                "options": [{"optionId": "opt-ok", "kind": "allow_once", "name": "Proceed"}]
            }
        })
        .to_string(),
    );

    let outputs = adapter.poll(now);
    for out in outputs {
        match out {
            AdapterOutput::Line { agent_id, text } => {
                processor.process_line(&agent_id, text, now, &mut log_store, &mut metrics);
            }
            AdapterOutput::Event(raw) => {
                let (event, _) = processor.process_raw_event(
                    raw,
                    now,
                    &mut event_store,
                    &mut queue,
                    &mut log_store,
                    &mut metrics,
                ).unwrap();
                assert_eq!(event.category, Category::Request);
                assert_eq!(event.severity, Severity::Critical);
                assert_eq!(event.summary, "Approval Required");
                // Verify log window was pinned for the request event
                assert!(event.log_range.pinned);
            }
        }
    }

    // In queue, Request entry must be top tier (Tier 0)
    let snapshot = queue.ordered_snapshot();
    assert!(!snapshot.is_empty());
    assert_eq!(snapshot[0].tier, 0);

    // Human approves
    let res = adapter.respond(&"sess-pipeline".into(), Decision::Approve, now);
    assert!(res.is_ok());

    // Agent completes
    adapter.transport_mut().push_stdout(
        json!({"jsonrpc": "2.0", "id": 3, "result": {"stopReason": "end_turn"}}).to_string(),
    );

    let outputs = adapter.poll(now);
    for out in outputs {
        match out {
            AdapterOutput::Line { agent_id, text } => {
                processor.process_line(&agent_id, text, now, &mut log_store, &mut metrics);
            }
            AdapterOutput::Event(raw) => {
                let (event, _) = processor.process_raw_event(
                    raw,
                    now,
                    &mut event_store,
                    &mut queue,
                    &mut log_store,
                    &mut metrics,
                ).unwrap();
                assert_eq!(event.category, Category::Completed);
            }
        }
    }

    assert!(adapter.is_finished());
}

#[test]
fn test_real_process_transport_stdio_lifecycle() {
    let script = r#"
import sys, json

sys.stderr.write("PYTHON STARTED\n")
sys.stderr.flush()

while True:
    line = sys.stdin.readline()
    if not line:
        break
    sys.stderr.write(f"PYTHON RECV: {line}\n")
    sys.stderr.flush()
    req = json.loads(line)
    req_id = req.get("id")
    method = req.get("method")
    if method == "initialize":
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": {"protocolVersion": 1}}) + "\n")
        sys.stdout.flush()
    elif method == "session/new":
        sys.stderr.write("Creating new test session...\n")
        sys.stderr.flush()
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": {"sessionId": "proc-sess-1"}}) + "\n")
        sys.stdout.flush()
    elif method == "session/prompt":
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "proc-sess-1", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Result text"}}}})+ "\n")
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": {"stopReason": "end_turn"}}) + "\n")
        sys.stdout.flush()
        break
"#;

    let config = AcpConfig {
        command: "python3".into(),
        args: vec!["-u".into(), "-c".into(), script.into()],
        cwd: None,
        agent_id: "proc-agent".into(),
        agent_name: "Process Agent".into(),
        project: "AgentDesk".into(),
        initial_prompt: "Run test".into(),
    };

    let mut adapter = AcpAdapter::spawn(config).expect("Failed to spawn process");
    let start = std::time::Instant::now();

    // Poll until finished or timeout
    let mut collected = Vec::new();
    while !adapter.is_finished() && start.elapsed() < std::time::Duration::from_secs(5) {
        let outputs = adapter.poll(Utc::now());
        collected.extend(outputs);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    assert!(adapter.is_finished());

    // Verify we received both stderr line and stdout events
    let has_stderr = collected.iter().any(|o| matches!(o, AdapterOutput::Line { text, .. } if text.contains("Creating new test session")));
    let has_started = collected.iter().any(|o| matches!(o, AdapterOutput::Event(raw) if raw.kind == "started"));
    let has_completed = collected.iter().any(|o| matches!(o, AdapterOutput::Event(raw) if raw.kind == "task_completed"));

    assert!(has_stderr, "Must have received stderr line");
    assert!(has_started, "Must have received started event");
    assert!(has_completed, "Must have received task_completed event");
}

#[test]
fn test_gemini_live_e2e() {
    if std::process::Command::new("which").arg("gemini").output().map(|o| o.status.success()).unwrap_or(false) {
        let config = AcpConfig {
            command: "gemini".into(),
            args: vec!["--skip-trust".into(), "--acp".into()],
            cwd: None,
            agent_id: "gemini-live".into(),
            agent_name: "Gemini CLI Live".into(),
            project: "AgentDesk".into(),
            initial_prompt: "What is 1+1? Output only the single digit 2.".into(),
        };

        let mut adapter = match AcpAdapter::spawn(config) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("Skipping live test: could not spawn gemini: {}", e);
                return;
            }
        };

        let start = std::time::Instant::now();
        let mut collected = Vec::new();
        // Allow up to 30s for live API inference
        while !adapter.is_finished() && start.elapsed() < std::time::Duration::from_secs(30) {
            let outputs = adapter.poll(Utc::now());
            collected.extend(outputs);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        assert!(adapter.is_finished(), "Live Gemini adapter did not finish in 30s");

        let has_started = collected.iter().any(|o| matches!(o, AdapterOutput::Event(raw) if raw.kind == "started"));
        let has_completed = collected.iter().any(|o| matches!(o, AdapterOutput::Event(raw) if raw.kind == "task_completed"));

        assert!(has_started, "Live run must have emitted started event");
        assert!(has_completed, "Live run must have emitted task_completed event");
    } else {
        eprintln!("Skipping test_gemini_live_e2e: gemini executable not found in PATH");
    }
}
