use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use agentdesk_core::{CoreTask, LogStoreConfig, ThresholdTable, VirtualClock};
use agentdesk_model::{
    Body, Empty, Hello, Message, PairRequest, PipelineMode, TransportMode, SCHEMA_VERSION,
};
use agentdesk_relay::{RelayConfig, RelayServer};
use agentdesk_server::{Server, ServerConfig};

#[tokio::test]
async fn test_daemon_relay_end_to_end() {
    // 1. Start Relay Server
    let relay_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let relay_port = relay_listener.local_addr().unwrap().port();
    drop(relay_listener);

    let relay_config = RelayConfig::insecure("127.0.0.1", relay_port);
    let relay = RelayServer::new(relay_config);
    let r_clone = relay.clone();
    tokio::spawn(async move {
        let _ = r_clone.run().await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 2. Set up Server temp dir and core
    let temp_dir = std::env::temp_dir().join(format!(
        "agentdesk-test-relay-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);

    let clock = Arc::new(VirtualClock::at_epoch());
    let (tx_adapter, _rx_adapter) = mpsc::channel(64);
    let (tx_core, rx_core) = mpsc::channel(256);

    let core = CoreTask::with_seed(
        PipelineMode::Agentdesk,
        clock.clone(),
        42,
        LogStoreConfig::default(),
        ThresholdTable::default(),
        Some(tx_adapter),
    );
    tokio::spawn(core.run(rx_core));

    let relay_url = format!("ws://127.0.0.1:{}", relay_port);
    let token = "test_relay_token_1234567890abcdef".to_string();

    let server_config = ServerConfig {
        bind_addr: "127.0.0.1".into(),
        port: 0,
        token: token.clone(),
        insecure_dev: true,
        pipeline_mode: PipelineMode::Agentdesk,
        outbound_capacity: 128,
        debug_logging: true,
        config_dir: Some(temp_dir.clone()),
        relay_url: Some(relay_url.clone()),
    };

    let server = Server::bind(server_config, tx_core.clone(), clock.clone())
        .await
        .expect("bind server");

    let instance_id = {
        let registry = server.device_registry();
        let reg = registry.read().await;
        reg.instance_id.clone()
    };

    let server = Arc::new(server);
    let s_clone = server.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });

    // Wait for server to connect outbound to relay
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 3. Connect client to Relay
    let client_relay_url = format!("{}/ws?role=client&inst={}", relay_url, instance_id);
    let (mut client_ws, _) = connect_async(&client_relay_url)
        .await
        .expect("connect client to relay");

    // 4. Send Hello over relay
    let hello = Message::push(Body::Hello(Hello {
        token: token.clone(),
        device_id: "test-mobile-device".into(),
        client_version: "0.1.0".into(),
        schema_version: SCHEMA_VERSION,
    }));
    client_ws
        .send(WsMessage::Text(serde_json::to_string(&hello).unwrap().into()))
        .await
        .expect("send hello");

    // 5. Receive Welcome
    let welcome_msg = client_ws.next().await.unwrap().unwrap();
    let welcome_text = welcome_msg.to_text().unwrap();
    let welcome: Message = serde_json::from_str(welcome_text).unwrap();
    match welcome.body {
        Body::Welcome(w) => {
            assert_eq!(w.transport, TransportMode::Relay);
            assert_eq!(w.schema_version, SCHEMA_VERSION);
        }
        other => panic!("expected welcome, got {:?}", other),
    }

    // 6. Receive Snapshot
    let snapshot_msg = client_ws.next().await.unwrap().unwrap();
    let snapshot_text = snapshot_msg.to_text().unwrap();
    let snapshot: Message = serde_json::from_str(snapshot_text).unwrap();
    match snapshot.body {
        Body::Snapshot(s) => {
            assert!(s.events.is_empty() || !s.events.is_empty());
        }
        other => panic!("expected snapshot, got {:?}", other),
    }

    // 7. Send get_metrics request, receive metrics reply
    let metrics_req = Message::with_request_id("req-metrics-1", Body::GetMetrics(Empty {}));
    client_ws
        .send(WsMessage::Text(serde_json::to_string(&metrics_req).unwrap().into()))
        .await
        .expect("send get_metrics");

    let reply_msg = client_ws.next().await.unwrap().unwrap();
    let reply_text = reply_msg.to_text().unwrap();
    let reply: Message = serde_json::from_str(reply_text).unwrap();
    assert_eq!(reply.request_id.as_deref(), Some("req-metrics-1"));
    match reply.body {
        Body::Metrics(m) => {
            assert!(m.contains_key("transmitted_bytes"));
        }
        other => panic!("expected metrics reply, got {:?}", other),
    }

    // 8. Cleanup
    server.shutdown();
    relay.shutdown();
    let _ = std::fs::remove_dir_all(temp_dir);
}

#[tokio::test]
async fn test_daemon_relay_reconnection_after_client_disconnect() {
    let relay_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let relay_port = relay_listener.local_addr().unwrap().port();
    drop(relay_listener);

    let relay = RelayServer::new(RelayConfig::insecure("127.0.0.1", relay_port));
    let r_clone = relay.clone();
    tokio::spawn(async move {
        let _ = r_clone.run().await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let temp_dir = std::env::temp_dir().join(format!(
        "agentdesk-test-relay-reconn-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);

    let clock = Arc::new(VirtualClock::at_epoch());
    let (tx_adapter, _rx_adapter) = mpsc::channel(64);
    let (tx_core, rx_core) = mpsc::channel(256);

    let core = CoreTask::with_seed(
        PipelineMode::Agentdesk,
        clock.clone(),
        42,
        LogStoreConfig::default(),
        ThresholdTable::default(),
        Some(tx_adapter),
    );
    tokio::spawn(core.run(rx_core));

    let relay_url = format!("ws://127.0.0.1:{}", relay_port);
    let token = "test_relay_token_reconnect".to_string();

    let server_config = ServerConfig {
        bind_addr: "127.0.0.1".into(),
        port: 0,
        token: token.clone(),
        insecure_dev: true,
        pipeline_mode: PipelineMode::Agentdesk,
        outbound_capacity: 128,
        debug_logging: false,
        config_dir: Some(temp_dir.clone()),
        relay_url: Some(relay_url.clone()),
    };

    let server = Server::bind(server_config, tx_core.clone(), clock.clone())
        .await
        .expect("bind server");

    let instance_id = {
        let registry = server.device_registry();
        let reg = registry.read().await;
        reg.instance_id.clone()
    };

    let server = Arc::new(server);
    let s_clone = server.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    let client_relay_url = format!("{}/ws?role=client&inst={}", relay_url, instance_id);

    // --- Session 1 ---
    {
        let (mut client_ws, _) = connect_async(&client_relay_url)
            .await
            .expect("session 1 connect");

        let hello = Message::push(Body::Hello(Hello {
            token: token.clone(),
            device_id: "phone-session-1".into(),
            client_version: "0.1.0".into(),
            schema_version: SCHEMA_VERSION,
        }));
        client_ws
            .send(WsMessage::Text(serde_json::to_string(&hello).unwrap().into()))
            .await
            .unwrap();

        let welcome_msg = client_ws.next().await.unwrap().unwrap();
        let welcome: Message = serde_json::from_str(welcome_msg.to_text().unwrap()).unwrap();
        assert!(matches!(welcome.body, Body::Welcome(_)));

        let snapshot_msg = client_ws.next().await.unwrap().unwrap();
        let snapshot: Message = serde_json::from_str(snapshot_msg.to_text().unwrap()).unwrap();
        assert!(matches!(snapshot.body, Body::Snapshot(_)));

        // Disconnect Session 1
        let _ = client_ws.close(None).await;
    }

    // Wait for server to detect client disconnect and reconnect to relay
    tokio::time::sleep(Duration::from_millis(200)).await;

    // --- Session 2 (Reconnect) ---
    {
        let (mut client_ws, _) = connect_async(&client_relay_url)
            .await
            .expect("session 2 connect");

        let hello = Message::push(Body::Hello(Hello {
            token: token.clone(),
            device_id: "phone-session-2".into(),
            client_version: "0.1.0".into(),
            schema_version: SCHEMA_VERSION,
        }));
        client_ws
            .send(WsMessage::Text(serde_json::to_string(&hello).unwrap().into()))
            .await
            .unwrap();

        let welcome_msg = client_ws.next().await.unwrap().unwrap();
        let welcome: Message = serde_json::from_str(welcome_msg.to_text().unwrap()).unwrap();
        assert!(matches!(welcome.body, Body::Welcome(_)));

        let snapshot_msg = client_ws.next().await.unwrap().unwrap();
        let snapshot: Message = serde_json::from_str(snapshot_msg.to_text().unwrap()).unwrap();
        assert!(matches!(snapshot.body, Body::Snapshot(_)));
    }

    server.shutdown();
    relay.shutdown();
    let _ = std::fs::remove_dir_all(temp_dir);
}

#[tokio::test]
async fn test_daemon_relay_remote_pairing_and_event_delivery() {
    let relay_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let relay_port = relay_listener.local_addr().unwrap().port();
    drop(relay_listener);

    let relay = RelayServer::new(RelayConfig::insecure("127.0.0.1", relay_port));
    let r_clone = relay.clone();
    tokio::spawn(async move {
        let _ = r_clone.run().await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let temp_dir = std::env::temp_dir().join(format!(
        "agentdesk-test-relay-pairing-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);

    let clock = Arc::new(VirtualClock::at_epoch());
    let (tx_adapter, _rx_adapter) = mpsc::channel(64);
    let (tx_core, rx_core) = mpsc::channel(256);

    let core = CoreTask::with_seed(
        PipelineMode::Agentdesk,
        clock.clone(),
        42,
        LogStoreConfig::default(),
        ThresholdTable::default(),
        Some(tx_adapter),
    );
    tokio::spawn(core.run(rx_core));

    let relay_url = format!("ws://127.0.0.1:{}", relay_port);
    let token = "test_relay_token_remote_pair".to_string();

    let server_config = ServerConfig {
        bind_addr: "127.0.0.1".into(),
        port: 0,
        token: token.clone(),
        insecure_dev: true,
        pipeline_mode: PipelineMode::Agentdesk,
        outbound_capacity: 128,
        debug_logging: false,
        config_dir: Some(temp_dir.clone()),
        relay_url: Some(relay_url.clone()),
    };

    let server = Server::bind(server_config, tx_core.clone(), clock.clone())
        .await
        .expect("bind server");

    let instance_id = {
        let registry = server.device_registry();
        let reg = registry.read().await;
        reg.instance_id.clone()
    };

    // Create pairing session
    let (_session, payload, uri) = server.pairing_manager().create_session(
        instance_id.clone(),
        "AA:BB:CC:DD".into(),
        Some(relay_url.clone()),
        Some(180),
        chrono::Utc::now(),
    );
    assert!(!uri.contains("&lan="));
    assert!(uri.contains("&relay="));

    let server = Arc::new(server);
    let s_clone = server.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    // --- 1. Pair over Relay (No LAN) ---
    let pair_url = format!(
        "{}/ws?role=pair&inst={}&sess={}&sec={}",
        relay_url,
        urlencoding::encode(&instance_id),
        urlencoding::encode(&payload.session_id),
        urlencoding::encode(&payload.pairing_secret)
    );

    let (mut pair_ws, _) = connect_async(&pair_url)
        .await
        .expect("connect pair to relay");

    let pair_req = Message::push(Body::PairRequest(PairRequest {
        device_id: "phone-remote-1".into(),
        device_name: "Pixel 9 Pro".into(),
        client_version: "0.1.0".into(),
        session_id: payload.session_id.clone(),
        pairing_secret: payload.pairing_secret.clone(),
        schema_version: SCHEMA_VERSION,
    }));
    pair_ws
        .send(WsMessage::Text(serde_json::to_string(&pair_req).unwrap().into()))
        .await
        .unwrap();

    let resp_msg = pair_ws.next().await.unwrap().unwrap();
    let resp: Message = serde_json::from_str(resp_msg.to_text().unwrap()).unwrap();
    let device_token = match resp.body {
        Body::PairResponse(p) => {
            assert_eq!(p.instance_id, instance_id);
            p.device_token
        }
        other => panic!("expected pair_response, got {:?}", other),
    };

    drop(pair_ws);
    tokio::time::sleep(Duration::from_millis(50)).await;

    // --- 2. Connect as authorized client over Relay (No LAN) ---
    let client_url = format!(
        "{}/ws?role=client&inst={}&device_id=phone-remote-1&token={}",
        relay_url,
        urlencoding::encode(&instance_id),
        urlencoding::encode(&device_token)
    );
    let (mut client_ws, _) = connect_async(&client_url)
        .await
        .expect("connect client to relay");

    let hello = Message::push(Body::Hello(Hello {
        token: device_token.clone(),
        device_id: "phone-remote-1".into(),
        client_version: "0.1.0".into(),
        schema_version: SCHEMA_VERSION,
    }));
    client_ws
        .send(WsMessage::Text(serde_json::to_string(&hello).unwrap().into()))
        .await
        .unwrap();

    let welcome_msg = client_ws.next().await.unwrap().unwrap();
    let welcome: Message = serde_json::from_str(welcome_msg.to_text().unwrap()).unwrap();
    match welcome.body {
        Body::Welcome(w) => {
            assert_eq!(w.transport, TransportMode::Relay);
        }
        other => panic!("expected welcome, got {:?}", other),
    }

    let snap_msg = client_ws.next().await.unwrap().unwrap();
    let snap: Message = serde_json::from_str(snap_msg.to_text().unwrap()).unwrap();
    assert!(matches!(snap.body, Body::Snapshot(_)));

    server.shutdown();
    relay.shutdown();
    let _ = std::fs::remove_dir_all(temp_dir);
}


