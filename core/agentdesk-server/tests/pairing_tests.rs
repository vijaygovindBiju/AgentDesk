//! Integration tests for QR pairing protocol and device authentication.
//! See Section 37: TESTING — RUST (PAIRING & SECURITY).

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

use agentdesk_core::{Clock, CoreTask, LogStoreConfig, ThresholdTable, VirtualClock};
use agentdesk_model::close_code;
use agentdesk_model::{
    Body, Message, PairRequest, PairResponse, PipelineMode, SCHEMA_VERSION,
};
use agentdesk_server::{
    PairingManager, Server, ServerConfig, generate_token,
};

async fn setup_pairing_test_server() -> (Server, String, PairingManager, Arc<VirtualClock>, std::path::PathBuf) {
    let clock = Arc::new(VirtualClock::at_epoch());
    let token = generate_token();
    let temp_dir = std::env::temp_dir().join(format!("agentdesk_pair_test_{}", generate_token()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let (tx_core, rx_core) = mpsc::channel(256);
    let (tx_adapter, mut rx_adapter) = mpsc::channel(64);
    tokio::spawn(async move {
        while let Some(_cmd) = rx_adapter.recv().await {}
    });

    let core = CoreTask::with_seed(
        PipelineMode::Agentdesk,
        clock.clone(),
        42,
        LogStoreConfig::default(),
        ThresholdTable::default(),
        Some(tx_adapter),
    );
    tokio::spawn(core.run(rx_core));

    let config = ServerConfig {
        bind_addr: "127.0.0.1".into(),
        port: 0,
        token: token.clone(),
        insecure_dev: true,
        pipeline_mode: PipelineMode::Agentdesk,
        outbound_capacity: 128,
        debug_logging: false,
        config_dir: Some(temp_dir.clone()),
        relay_url: None,
    };

    let server = Server::bind(config, tx_core, clock.clone()).await.unwrap();
    let pairing_mgr = server.pairing_manager().clone();
    (server, token, pairing_mgr, clock, temp_dir)
}

#[tokio::test]
async fn test_valid_pairing_and_subsequent_connection() {
    let (server, _master_token, pairing_mgr, clock, _dir) = setup_pairing_test_server().await;
    let server_arc = Arc::new(server);
    let s_clone = server_arc.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // 1. Generate pairing session on laptop
    let instance_id = server_arc.device_registry().read().await.instance_id.clone();
    let (session, payload, uri) = pairing_mgr.create_session(
        instance_id.clone(),
        "AA:BB:CC".into(),
        Some("wss://relay.agentdesk.net".into()),
        Some(180),
        clock.now(),
    );

    // Verify permanent token and LAN addresses are NOT in payload or URI
    assert!(!uri.contains(&_master_token));
    assert!(!uri.contains("&lan="));
    assert!(uri.contains("&relay="));
    assert_ne!(payload.pairing_secret, _master_token);

    // 2. Phone connects and initiates pairing with PairRequest
    let ws_url = format!("ws://{}", server_arc.local_addr());
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();

    let pair_msg = Message::push(Body::PairRequest(PairRequest {
        session_id: session.session_id.clone(),
        pairing_secret: payload.pairing_secret.clone(),
        device_id: "test-phone-1".into(),
        device_name: "Pixel 9 Pro".into(),
        client_version: "0.1.0".into(),
        schema_version: SCHEMA_VERSION,
    }));
    ws.send(WsMessage::Text(serde_json::to_string(&pair_msg).unwrap().into()))
        .await
        .unwrap();

    // 3. Receive PairResponse
    let resp_frame = ws.next().await.unwrap().unwrap();
    let dev_token = if let WsMessage::Text(t) = resp_frame {
        let msg: Message = serde_json::from_str(&t).unwrap();
        match msg.body {
            Body::PairResponse(PairResponse {
                instance_id: returned_inst,
                device_token,
                schema_version,
            }) => {
                assert_eq!(returned_inst, instance_id);
                assert_eq!(schema_version, SCHEMA_VERSION);
                assert_eq!(device_token.len(), 64);
                device_token
            }
            other => panic!("Expected PairResponse, got {:?}", other),
        }
    } else {
        panic!("Expected text frame");
    };

    // 4. Now connect as the paired device using Hello and the newly provisioned device_token
    let (mut ws2, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let hello_msg = Message::push(Body::Hello(agentdesk_model::Hello {
        token: dev_token.clone(),
        device_id: "test-phone-1".into(),
        client_version: "0.1.0".into(),
        schema_version: SCHEMA_VERSION,
    }));
    ws2.send(WsMessage::Text(serde_json::to_string(&hello_msg).unwrap().into()))
        .await
        .unwrap();

    // Should receive Welcome frame
    let welcome_frame = ws2.next().await.unwrap().unwrap();
    if let WsMessage::Text(t) = welcome_frame {
        let msg: Message = serde_json::from_str(&t).unwrap();
        assert!(matches!(msg.body, Body::Welcome(_)));
    } else {
        panic!("Expected Welcome frame");
    }

    server_arc.shutdown();
}

#[tokio::test]
async fn test_pairing_replay_is_rejected() {
    let (server, _master_token, pairing_mgr, clock, _dir) = setup_pairing_test_server().await;
    let server_arc = Arc::new(server);
    let s_clone = server_arc.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let instance_id = server_arc.device_registry().read().await.instance_id.clone();
    let (session, payload, _) = pairing_mgr.create_session(
        instance_id,
        "AA:BB".into(),
        None,
        Some(180),
        clock.now(),
    );

    let ws_url = format!("ws://{}", server_arc.local_addr());

    // First attempt: Phone A pairs successfully
    let (mut ws_a, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let pair_msg_a = Message::push(Body::PairRequest(PairRequest {
        session_id: session.session_id.clone(),
        pairing_secret: payload.pairing_secret.clone(),
        device_id: "phone-a".into(),
        device_name: "Phone A".into(),
        client_version: "0.1.0".into(),
        schema_version: SCHEMA_VERSION,
    }));
    ws_a.send(WsMessage::Text(serde_json::to_string(&pair_msg_a).unwrap().into()))
        .await
        .unwrap();
    let resp = ws_a.next().await.unwrap().unwrap();
    assert!(matches!(resp, WsMessage::Text(_)));

    // Second attempt: Phone B tries to use the same QR / pairing secret -> MUST BE REJECTED!
    let (mut ws_b, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let pair_msg_b = Message::push(Body::PairRequest(PairRequest {
        session_id: session.session_id.clone(),
        pairing_secret: payload.pairing_secret.clone(),
        device_id: "phone-b".into(),
        device_name: "Phone B".into(),
        client_version: "0.1.0".into(),
        schema_version: SCHEMA_VERSION,
    }));
    ws_b.send(WsMessage::Text(serde_json::to_string(&pair_msg_b).unwrap().into()))
        .await
        .unwrap();

    let close_msg = ws_b.next().await.unwrap().unwrap();
    if let WsMessage::Close(Some(frame)) = close_msg {
        assert_eq!(u16::from(frame.code), close_code::UNAUTHORIZED);
        assert!(frame.reason.contains("replayed") || frame.reason.contains("rejected"));
    } else {
        panic!("Expected 4001 close frame on replay attempt");
    }

    server_arc.shutdown();
}

#[tokio::test]
async fn test_pairing_expired_is_rejected() {
    let (server, _master_token, pairing_mgr, clock, _dir) = setup_pairing_test_server().await;
    let server_arc = Arc::new(server);
    let s_clone = server_arc.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let instance_id = server_arc.device_registry().read().await.instance_id.clone();
    let (session, payload, _) = pairing_mgr.create_session(
        instance_id,
        "AA:BB".into(),
        None,
        Some(10), // 10 seconds validity
        clock.now(),
    );

    // Advance virtual clock past expiry
    clock.advance(chrono::Duration::seconds(20));

    let ws_url = format!("ws://{}", server_arc.local_addr());
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();

    let pair_msg = Message::push(Body::PairRequest(PairRequest {
        session_id: session.session_id.clone(),
        pairing_secret: payload.pairing_secret.clone(),
        device_id: "phone-expired".into(),
        device_name: "Phone Expired".into(),
        client_version: "0.1.0".into(),
        schema_version: SCHEMA_VERSION,
    }));
    ws.send(WsMessage::Text(serde_json::to_string(&pair_msg).unwrap().into()))
        .await
        .unwrap();

    let close_msg = ws.next().await.unwrap().unwrap();
    if let WsMessage::Close(Some(frame)) = close_msg {
        assert_eq!(u16::from(frame.code), close_code::UNAUTHORIZED);
        assert!(frame.reason.contains("expired") || frame.reason.contains("rejected"));
    } else {
        panic!("Expected 4001 close frame on expired pairing");
    }

    server_arc.shutdown();
}

#[tokio::test]
async fn test_revoked_device_is_rejected() {
    let (server, _master_token, pairing_mgr, clock, _dir) = setup_pairing_test_server().await;
    let server_arc = Arc::new(server);
    let s_clone = server_arc.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let instance_id = server_arc.device_registry().read().await.instance_id.clone();
    let (session, payload, _) = pairing_mgr.create_session(
        instance_id,
        "AA:BB".into(),
        None,
        Some(180),
        clock.now(),
    );

    let ws_url = format!("ws://{}", server_arc.local_addr());

    // Pair phone
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let pair_msg = Message::push(Body::PairRequest(PairRequest {
        session_id: session.session_id.clone(),
        pairing_secret: payload.pairing_secret.clone(),
        device_id: "phone-to-revoke".into(),
        device_name: "Phone Revoke".into(),
        client_version: "0.1.0".into(),
        schema_version: SCHEMA_VERSION,
    }));
    ws.send(WsMessage::Text(serde_json::to_string(&pair_msg).unwrap().into()))
        .await
        .unwrap();

    let resp = ws.next().await.unwrap().unwrap();
    let dev_token = if let WsMessage::Text(t) = resp {
        let m: Message = serde_json::from_str(&t).unwrap();
        match m.body {
            Body::PairResponse(p) => p.device_token,
            _ => panic!("Expected pair response"),
        }
    } else {
        panic!("Expected text");
    };

    // Verify it connects before revocation
    let (mut ws_ok, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let hello_ok = Message::push(Body::Hello(agentdesk_model::Hello {
        token: dev_token.clone(),
        device_id: "phone-to-revoke".into(),
        client_version: "0.1.0".into(),
        schema_version: SCHEMA_VERSION,
    }));
    ws_ok.send(WsMessage::Text(serde_json::to_string(&hello_ok).unwrap().into()))
        .await
        .unwrap();
    let welcome = ws_ok.next().await.unwrap().unwrap();
    assert!(matches!(welcome, WsMessage::Text(_)));

    // Now laptop revokes this device
    {
        let reg_lock = server_arc.device_registry();
        let mut reg = reg_lock.write().await;
        assert!(reg.revoke_device("phone-to-revoke"));
    }

    // Try connecting with revoked device -> MUST BE REJECTED (4001)
    let (mut ws_revoked, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let hello_revoked = Message::push(Body::Hello(agentdesk_model::Hello {
        token: dev_token.clone(),
        device_id: "phone-to-revoke".into(),
        client_version: "0.1.0".into(),
        schema_version: SCHEMA_VERSION,
    }));
    ws_revoked
        .send(WsMessage::Text(serde_json::to_string(&hello_revoked).unwrap().into()))
        .await
        .unwrap();

    let close_frame = ws_revoked.next().await.unwrap().unwrap();
    if let WsMessage::Close(Some(frame)) = close_frame {
        assert_eq!(u16::from(frame.code), close_code::UNAUTHORIZED);
    } else {
        panic!("Expected 4001 close frame for revoked device");
    }

    server_arc.shutdown();
}
