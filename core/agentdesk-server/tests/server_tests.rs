//! Phase 6 integration tests.
//! Validates P6.T1 - P6.T6 and the Phase 6 exit criteria.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

use agentdesk_core::{
    Adapter, AdapterCommand, Clock, CoreCommand, CoreTask, LogStoreConfig, ThresholdTable,
    VirtualClock,
};
use agentdesk_model::close_code;
use agentdesk_model::{
    Body, Category, Decision, Empty, EventPush, EventRef, GetEventLogs,
    Hello, Message, Operation, PipelineMode, RequestInfo, RespondRequest, TransportMode,
    SCHEMA_VERSION,
};
use agentdesk_server::{
    generate_token, is_loopback_addr, set_log_capture, set_log_level, validate_bind_security,
    LogCapture, LogLevel, Server, ServerConfig,
};
use agentdesk_sim::{Scenario, Simulator};

/// Helper to spin up a test server with CoreTask in memory.
async fn setup_test_server(
    mode: PipelineMode,
    outbound_capacity: usize,
) -> (Server, String, mpsc::Sender<CoreCommand>, Arc<VirtualClock>) {
    let clock = Arc::new(VirtualClock::at_epoch());
    let token = generate_token();

    let (tx_core, rx_core) = mpsc::channel(256);
    let (tx_adapter, _rx_adapter) = mpsc::channel(64);

    let core = CoreTask::with_seed(
        mode,
        clock.clone(),
        42,
        LogStoreConfig::default(),
        ThresholdTable::default(),
        Some(tx_adapter),
    );

    tokio::spawn(core.run(rx_core));

    let config = ServerConfig {
        bind_addr: "127.0.0.1".into(),
        port: 0, // OS assigned port
        token: token.clone(),
        insecure_dev: true,
        pipeline_mode: mode,
        outbound_capacity,
        debug_logging: false,
    };

    let server = Server::bind(config, tx_core.clone(), clock.clone())
        .await
        .expect("bind server");

    (server, token, tx_core, clock)
}

/// Helper to connect and send hello.
async fn connect_and_hello(
    server_addr: std::net::SocketAddr,
    token: &str,
    schema_version: u32,
) -> (
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Option<u16>,
) {
    let url = format!("ws://{}", server_addr);
    let (mut ws, _) = connect_async(&url).await.expect("connect to websocket");

    let hello_msg = Message::push(Body::Hello(Hello {
        token: token.to_string(),
        device_id: "test-device".into(),
        client_version: "0.1.0".into(),
        schema_version,
    }));

    let json = serde_json::to_string(&hello_msg).unwrap();
    ws.send(WsMessage::Text(json.into())).await.expect("send hello");

    // Read next frame: either Welcome or Close frame
    match ws.next().await {
        Some(Ok(WsMessage::Text(_))) => (ws, None),
        Some(Ok(WsMessage::Close(Some(frame)))) => {
            let code: u16 = frame.code.into();
            (ws, Some(code))
        }
        Some(Ok(WsMessage::Close(None))) => (ws, Some(1000)),
        other => panic!("Unexpected response to hello: {:?}", other),
    }
}

/// P6.T1: Handshake tests (correct token, wrong token, non-hello first frame, schema mismatch).
#[tokio::test]
async fn p6_t1_handshake_tests() {
    let (server, token, _core_tx, _clock) = setup_test_server(PipelineMode::Agentdesk, 128).await;
    let server_arc = Arc::new(server);
    let s_clone = server_arc.clone();
    let server_handle = tokio::spawn(async move {
        let _ = s_clone.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // 1. Correct token -> receives Welcome then Snapshot
    {
        let url = format!("ws://{}", server_arc.local_addr());
        let (mut ws, _) = connect_async(&url).await.expect("connect");

        let hello = Message::push(Body::Hello(Hello {
            token: token.clone(),
            device_id: "device-1".into(),
            client_version: "1.0.0".into(),
            schema_version: SCHEMA_VERSION,
        }));
        ws.send(WsMessage::Text(serde_json::to_string(&hello).unwrap().into()))
            .await
            .unwrap();

        // Expect Welcome frame
        let welcome_frame = ws.next().await.unwrap().unwrap();
        if let WsMessage::Text(t) = welcome_frame {
            let msg: Message = serde_json::from_str(&t).unwrap();
            match msg.body {
                Body::Welcome(w) => {
                    assert_eq!(w.schema_version, SCHEMA_VERSION);
                    assert_eq!(w.pipeline_mode, PipelineMode::Agentdesk);
                    assert_eq!(w.transport, TransportMode::InsecureDev);
                }
                other => panic!("Expected welcome, got {:?}", other),
            }
        } else {
            panic!("Expected text welcome frame");
        }

        // Expect Snapshot frame immediately after Welcome
        let snapshot_frame = ws.next().await.unwrap().unwrap();
        if let WsMessage::Text(t) = snapshot_frame {
            let msg: Message = serde_json::from_str(&t).unwrap();
            match msg.body {
                Body::Snapshot(_) => {} // Snapshot received!
                other => panic!("Expected snapshot, got {:?}", other),
            }
        } else {
            panic!("Expected text snapshot frame");
        }
    }

    // 2. Wrong token -> closes with code 4001
    {
        let (_ws, close_code) =
            connect_and_hello(server_arc.local_addr(), "wrong_invalid_token", SCHEMA_VERSION).await;
        assert_eq!(close_code, Some(close_code::UNAUTHORIZED));
    }

    // 3. Non-hello first frame -> closes with code 4001
    {
        let url = format!("ws://{}", server_arc.local_addr());
        let (mut ws, _) = connect_async(&url).await.expect("connect");

        let non_hello = Message::with_request_id("r-bad", Body::GetMetrics(Empty {}));
        ws.send(WsMessage::Text(serde_json::to_string(&non_hello).unwrap().into()))
            .await
            .unwrap();

        let resp = ws.next().await.unwrap().unwrap();
        match resp {
            WsMessage::Close(Some(frame)) => {
                let code: u16 = frame.code.into();
                assert_eq!(code, close_code::UNAUTHORIZED);
            }
            other => panic!("Expected close 4001, got {:?}", other),
        }
    }

    // 4. Schema mismatch -> closes with code 4002
    {
        let (_ws, close_code) =
            connect_and_hello(server_arc.local_addr(), &token, SCHEMA_VERSION + 100).await;
        assert_eq!(close_code, Some(close_code::SCHEMA_MISMATCH));
    }

    server_arc.shutdown();
    let _ = server_handle.await;
}

/// P6.T2: Every request type gets exactly one reply with the same request_id.
#[tokio::test]
async fn p6_t2_every_request_type_gets_matching_reply() {
    let (server, token, core_tx, _clock) = setup_test_server(PipelineMode::Agentdesk, 128).await;
    let server_arc = Arc::new(server);
    let s_clone = server_arc.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Connect and authenticate
    let (mut ws, close) = connect_and_hello(server_arc.local_addr(), &token, SCHEMA_VERSION).await;
    assert!(close.is_none());

    // Drain initial snapshot
    let _ = ws.next().await.unwrap().unwrap();

    // Push a test event into Core via AdapterOutput so event-related requests have a valid target
    let raw_event = agentdesk_model::RawAgentEvent {
        agent_id: "agent-1".into(),
        agent_seq: 1,
        task_id: Some("task-1".into()),
        kind: "approval_required".into(),
        operation: Operation::Other,
        message: "Agent requesting permission".into(),
        details: BTreeMap::new(),
        log_lines: vec!["line 1".into(), "line 2".into()],
        request: Some(RequestInfo {
            prompt: "Allow disk write?".into(),
            options: vec!["approve".into(), "deny".into()],
        }),
    };
    core_tx
        .send(CoreCommand::Adapter(agentdesk_core::AdapterOutput::Event(raw_event)))
        .await
        .unwrap();

    // Read the pushed event
    let event_frame = ws.next().await.unwrap().unwrap();
    let target_event_id = if let WsMessage::Text(t) = event_frame {
        let msg: Message = serde_json::from_str(&t).unwrap();
        match msg.body {
            Body::Event(EventPush { event, .. }) => event.event_id,
            other => panic!("Expected event push, got {:?}", other),
        }
    } else {
        panic!("Expected text frame");
    };

    // Helper to send request and assert exactly one reply with same request_id
    async fn send_and_expect_reply<F>(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        req_id: &str,
        body: Body,
        assert_body: F,
    ) where
        F: FnOnce(Body),
    {
        let msg = Message::with_request_id(req_id, body);
        ws.send(WsMessage::Text(serde_json::to_string(&msg).unwrap().into()))
            .await
            .unwrap();

        loop {
            let reply_frame = ws.next().await.unwrap().unwrap();
            if let WsMessage::Text(t) = reply_frame {
                let reply: Message = serde_json::from_str(&t).unwrap();
                if reply.request_id.as_deref() == Some(req_id) {
                    assert_body(reply.body);
                    break;
                }
            } else {
                panic!("Expected text reply frame");
            }
        }
    }

    // 1. get_metrics
    send_and_expect_reply(
        &mut ws,
        "req-metrics",
        Body::GetMetrics(Empty {}),
        |b| match b {
            Body::Metrics(m) => assert!(m.contains_key("transmitted_bytes")),
            other => panic!("Expected metrics reply, got {:?}", other),
        },
    )
    .await;

    // 2. get_event_details (also marks seen)
    send_and_expect_reply(
        &mut ws,
        "req-details",
        Body::GetEventDetails(EventRef {
            event_id: target_event_id,
        }),
        |b| match b {
            Body::EventDetails(EventPush { event, entry }) => {
                assert_eq!(event.event_id, target_event_id);
                assert_eq!(entry.state, agentdesk_model::EntryState::Seen);
            }
            other => panic!("Expected event_details reply, got {:?}", other),
        },
    )
    .await;

    // Drain score_update / state_update if triggered by mark_seen
    // 3. get_event_logs
    send_and_expect_reply(
        &mut ws,
        "req-logs",
        Body::GetEventLogs(GetEventLogs {
            event_id: target_event_id,
            offset: -1,
            limit: 10,
        }),
        |b| match b {
            Body::EventLogs(logs) => {
                assert_eq!(logs.event_id, target_event_id);
                assert!(!logs.lines.is_empty());
            }
            other => panic!("Expected event_logs reply, got {:?}", other),
        },
    )
    .await;

    // 4. ack
    send_and_expect_reply(
        &mut ws,
        "req-ack",
        Body::Ack(EventRef {
            event_id: target_event_id,
        }),
        |b| match b {
            Body::CommandResult(res) => assert!(res.ok),
            other => panic!("Expected command_result for ack, got {:?}", other),
        },
    )
    .await;

    // 5. dismiss
    send_and_expect_reply(
        &mut ws,
        "req-dismiss",
        Body::Dismiss(EventRef {
            event_id: target_event_id,
        }),
        |b| match b {
            Body::CommandResult(res) => assert!(res.ok),
            other => panic!("Expected command_result for dismiss, got {:?}", other),
        },
    )
    .await;

    // 6. respond_request
    send_and_expect_reply(
        &mut ws,
        "req-respond",
        Body::RespondRequest(RespondRequest {
            event_id: target_event_id,
            decision: Decision::Approve,
        }),
        |b| match b {
            Body::CommandResult(res) => assert!(res.ok, "Expected ok, got: {:?}", res),
            other => panic!("Expected command_result for respond_request, got {:?}", other),
        },
    )
    .await;

    server_arc.shutdown();
}

/// P6.T3: Malformed JSON => error reply, connection stays open.
#[tokio::test]
async fn p6_t3_malformed_json_error_connection_kept() {
    let (server, token, _core_tx, _clock) = setup_test_server(PipelineMode::Agentdesk, 128).await;
    let server_arc = Arc::new(server);
    let s_clone = server_arc.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let (mut ws, close) = connect_and_hello(server_arc.local_addr(), &token, SCHEMA_VERSION).await;
    assert!(close.is_none());

    // Drain initial snapshot
    let _ = ws.next().await.unwrap().unwrap();

    // Send malformed JSON frame
    let malformed = r#"{"request_id": "r-malformed", "type": "unknown_invalid_type", "payload": 123"#;
    ws.send(WsMessage::Text(malformed.into())).await.unwrap();

    // Read reply: should be Error reply
    let reply_frame = ws.next().await.unwrap().unwrap();
    if let WsMessage::Text(t) = reply_frame {
        let reply: Message = serde_json::from_str(&t).unwrap();
        match reply.body {
            Body::Error(err) => {
                assert_eq!(err.code, "malformed_frame");
            }
            other => panic!("Expected error reply, got {:?}", other),
        }
    } else {
        panic!("Expected text frame");
    }

    // Now send a valid request on the SAME connection to assert it stayed open
    let valid_req = Message::with_request_id("r-subsequent", Body::GetMetrics(Empty {}));
    ws.send(WsMessage::Text(serde_json::to_string(&valid_req).unwrap().into()))
        .await
        .unwrap();

    let subsequent_reply = ws.next().await.unwrap().unwrap();
    if let WsMessage::Text(t) = subsequent_reply {
        let reply: Message = serde_json::from_str(&t).unwrap();
        assert_eq!(reply.request_id.as_deref(), Some("r-subsequent"));
        assert!(matches!(reply.body, Body::Metrics(_)));
    } else {
        panic!("Expected metrics reply");
    }

    server_arc.shutdown();
}

/// P6.T4: Slow client => 4003, core keeps processing (assert later events still reach a second client).
#[tokio::test]
async fn p6_t4_slow_client_overflow_closes_4003_and_core_continues() {
    // Start server with small capacity (2)
    let (server, token, core_tx, _clock) = setup_test_server(PipelineMode::Agentdesk, 2).await;
    let server_arc = Arc::new(server);
    let s_clone = server_arc.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // 1. Connect Client 1 (the slow client)
    let (mut ws_slow, close1) =
        connect_and_hello(server_arc.local_addr(), &token, SCHEMA_VERSION).await;
    assert!(close1.is_none());

    // Client 1 does NOT read from its socket. We blast 10 events through core to overflow capacity (2).
    for i in 1..=10 {
        let raw = agentdesk_model::RawAgentEvent {
            agent_id: "agent-1".into(),
            agent_seq: i,
            task_id: Some(format!("task-{}", i)),
            kind: "tool_use".into(),
            operation: Operation::Other,
            message: format!("Message {}", i),
            details: BTreeMap::new(),
            request: None,
            log_lines: vec![],
        };
        core_tx
            .send(CoreCommand::Adapter(agentdesk_core::AdapterOutput::Event(raw)))
            .await
            .unwrap();
    }

    // Give time for channel overflow to register on Client 1
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Client 1 should observe a close frame with code 4003 (SLOW_CLIENT)
    let mut saw_4003 = false;
    while let Some(msg) = ws_slow.next().await {
        match msg {
            Ok(WsMessage::Close(Some(frame))) => {
                let code: u16 = frame.code.into();
                if code == close_code::SLOW_CLIENT {
                    saw_4003 = true;
                    break;
                }
            }
            Ok(WsMessage::Close(None)) => break,
            Err(_) => break,
            _ => {}
        }
    }
    assert!(saw_4003, "Slow client must be closed with 4003");

    // 2. Connect Client 2 (second client) after Client 1 disconnected
    let (mut ws_second, close2) =
        connect_and_hello(server_arc.local_addr(), &token, SCHEMA_VERSION).await;
    assert!(close2.is_none());

    // Client 2 receives snapshot
    let snapshot_frame = ws_second.next().await.unwrap().unwrap();
    assert!(matches!(snapshot_frame, WsMessage::Text(_)));

    // 3. Blast later events through core and verify they reach Client 2
    let later_raw = agentdesk_model::RawAgentEvent {
        agent_id: "agent-1".into(),
        agent_seq: 100,
        task_id: Some("task-100".into()),
        kind: "tool_use".into(),
        operation: Operation::Other,
        message: "Later event reached second client".into(),
        details: BTreeMap::new(),
        request: None,
        log_lines: vec![],
    };
    core_tx
        .send(CoreCommand::Adapter(agentdesk_core::AdapterOutput::Event(later_raw)))
        .await
        .unwrap();

    let later_event_frame = ws_second.next().await.unwrap().unwrap();
    if let WsMessage::Text(t) = later_event_frame {
        let msg: Message = serde_json::from_str(&t).unwrap();
        match msg.body {
            Body::Event(EventPush { event, .. }) => {
                assert_eq!(event.message, "Later event reached second client");
            }
            other => panic!("Expected later event push, got {:?}", other),
        }
    } else {
        panic!("Expected text frame for later event");
    }

    server_arc.shutdown();
}

/// P6.T5: --insecure-dev with a non-loopback bind is rejected at startup.
#[test]
fn p6_t5_insecure_dev_with_non_loopback_rejected() {
    assert!(is_loopback_addr("127.0.0.1"));
    assert!(is_loopback_addr("::1"));
    assert!(is_loopback_addr("localhost"));
    assert!(!is_loopback_addr("0.0.0.0"));
    assert!(!is_loopback_addr("192.168.1.1"));

    // Rejected combinations
    assert!(validate_bind_security("0.0.0.0", "valid_token", true).is_err());
    assert!(validate_bind_security("192.168.1.10", "valid_token", true).is_err());
    assert!(validate_bind_security("10.0.0.1", "valid_token", true).is_err());

    // Allowed combinations
    assert!(validate_bind_security("127.0.0.1", "valid_token", true).is_ok());

    // Empty token on non-loopback rejected
    assert!(validate_bind_security("0.0.0.0", "", false).is_err());
}

/// P6.T6: Captured logs at default level contain no token.
#[tokio::test]
async fn p6_t6_captured_logs_contain_no_token() {
    let capture = LogCapture::new();
    set_log_capture(Some(capture.clone()));
    set_log_level(LogLevel::Info);

    let (server, token, _core_tx, _clock) = setup_test_server(PipelineMode::Agentdesk, 128).await;
    let server_arc = Arc::new(server);
    let s_clone = server_arc.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Normal client flow
    let (_ws, _) = connect_and_hello(server_arc.local_addr(), &token, SCHEMA_VERSION).await;

    // Bad token client flow
    let bad_token = "bad_attacker_secret_token_value_987654321";
    let (_ws_bad, _) = connect_and_hello(server_arc.local_addr(), bad_token, SCHEMA_VERSION).await;

    server_arc.shutdown();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let lines = capture.lines();
    assert!(!lines.is_empty(), "Logs should have been captured");

    for line in &lines {
        assert!(
            !line.contains(&token),
            "Log line contains server token: {}",
            line
        );
        assert!(
            !line.contains(bad_token),
            "Log line contains client token: {}",
            line
        );
    }

    set_log_capture(None);
}

/// Phase 6 Exit Criteria:
/// A test client can connect over loopback, receive snapshot and pushes,
/// page logs, approve a request, and observe the simulator continue.
#[tokio::test]
async fn phase6_exit_criteria_end_to_end() {
    let clock = Arc::new(VirtualClock::at_epoch());
    let token = generate_token();

    let scenario = Scenario::default_scenario();
    let mut sim = Simulator::new(scenario, 42, clock.now());

    let (tx_adapter, mut rx_adapter) = mpsc::channel(64);
    let (tx_core, rx_core) = mpsc::channel(256);

    let mut core = CoreTask::with_seed(
        PipelineMode::Agentdesk,
        clock.clone(),
        42,
        LogStoreConfig::default(),
        ThresholdTable::default(),
        Some(tx_adapter),
    );
    core.register_agents(sim.agents());

    tokio::spawn(core.run(rx_core));

    let config = ServerConfig {
        bind_addr: "127.0.0.1".into(),
        port: 0,
        token: token.clone(),
        insecure_dev: true,
        pipeline_mode: PipelineMode::Agentdesk,
        outbound_capacity: 1024,
        debug_logging: false,
    };

    let server = Server::bind(config, tx_core.clone(), clock.clone())
        .await
        .expect("bind server");
    let server_arc = Arc::new(server);
    let s_clone = server_arc.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // 1. Connect over loopback & authenticate
    let (ws, close) = connect_and_hello(server_arc.local_addr(), &token, SCHEMA_VERSION).await;
    assert!(close.is_none());

    // Split WebSocket stream so receiving happens concurrently with driving simulator
    let (mut ws_sink, mut ws_stream) = ws.split();
    let (inbound_tx, mut inbound_rx) = mpsc::channel::<Message>(1000);

    let reader_handle = tokio::spawn(async move {
        while let Some(Ok(WsMessage::Text(text))) = ws_stream.next().await {
            #[allow(clippy::collapsible_if)]
            if let Ok(msg) = serde_json::from_str::<Message>(&text) {
                if inbound_tx.send(msg).await.is_err() {
                    break;
                }
            }
        }
    });

    // 2. Receive Snapshot
    let snapshot_msg = tokio::time::timeout(Duration::from_secs(2), inbound_rx.recv())
        .await
        .expect("timeout waiting for snapshot")
        .expect("channel closed");
    assert!(matches!(snapshot_msg.body, Body::Snapshot(_)));

    // 3. Drive simulator until a Request event appears
    let mut request_event_id = None;
    let mut poll_time = clock.now();

    while request_event_id.is_none() {
        let outputs = sim.poll(poll_time);
        for out in outputs {
            tx_core.send(CoreCommand::Adapter(out)).await.unwrap();
        }

        // Drain available messages from client inbound channel
        while let Ok(msg) = inbound_rx.try_recv() {
            #[allow(clippy::collapsible_if)]
            if let Body::Event(EventPush { event, .. }) = msg.body {
                if event.category == Category::Request {
                    request_event_id = Some(event.event_id);
                    break;
                }
            }
        }

        tokio::task::yield_now().await;
        poll_time += chrono::Duration::seconds(5);
        clock.set(poll_time);
    }

    let req_id = request_event_id.expect("A request event must have been emitted");

    // 4. Page logs for the request event
    let log_req = Message::with_request_id(
        "req-logs-1",
        Body::GetEventLogs(GetEventLogs {
            event_id: req_id,
            offset: -1,
            limit: 50,
        }),
    );
    ws_sink
        .send(WsMessage::Text(serde_json::to_string(&log_req).unwrap().into()))
        .await
        .unwrap();

    let mut event_logs_received = false;
    while let Some(msg) = tokio::time::timeout(Duration::from_secs(2), inbound_rx.recv())
        .await
        .expect("timeout waiting for log reply")
    {
        if msg.request_id.as_deref() == Some("req-logs-1") {
            match msg.body {
                Body::EventLogs(logs) => {
                    assert_eq!(logs.event_id, req_id);
                    event_logs_received = true;
                    break;
                }
                other => panic!("Expected event_logs, got {:?}", other),
            }
        }
    }
    assert!(event_logs_received, "Event logs must be received");

    // 5. Approve the request via respond_request
    let approve_req = Message::with_request_id(
        "req-approve-1",
        Body::RespondRequest(RespondRequest {
            event_id: req_id,
            decision: Decision::Approve,
        }),
    );
    ws_sink
        .send(WsMessage::Text(serde_json::to_string(&approve_req).unwrap().into()))
        .await
        .unwrap();

    let mut approve_reply_ok = false;
    while let Some(msg) = tokio::time::timeout(Duration::from_secs(2), inbound_rx.recv())
        .await
        .expect("timeout waiting for approve reply")
    {
        if msg.request_id.as_deref() == Some("req-approve-1") {
            match msg.body {
                Body::CommandResult(res) => {
                    assert!(res.ok, "CommandResult must be ok: {:?}", res.error);
                    approve_reply_ok = true;
                    break;
                }
                other => panic!("Expected command_result, got {:?}", other),
            }
        }
    }
    assert!(approve_reply_ok, "Approve reply must be ok");

    // 6. Deliver adapter response to simulator and observe simulator continue
    let cmd = rx_adapter.recv().await.expect("Adapter command should be sent");
    match cmd {
        AdapterCommand::Respond {
            task_id,
            decision,
            now,
        } => {
            sim.respond(&task_id, decision, now).expect("Simulator unblocked");
        }
    }

    // Advance clock and poll simulator: verify task progresses after approval
    clock.advance(chrono::Duration::seconds(5));
    let subsequent_outputs = sim.poll(clock.now());
    assert!(
        !subsequent_outputs.is_empty(),
        "Simulator must produce further outputs after request approval"
    );

    server_arc.shutdown();
    reader_handle.abort();
}
