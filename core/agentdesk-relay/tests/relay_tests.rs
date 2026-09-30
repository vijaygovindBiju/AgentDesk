use std::net::TcpListener;
use std::time::Duration;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use agentdesk_relay::{
    DeviceAuthInfo, PairingAuthInfo, RelayConfig, RelayControlAction, RelayControlMessage, RelayServer,
};

fn get_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[tokio::test]
async fn test_relay_rejection_when_host_offline() {
    let port = get_free_port();
    let server = RelayServer::new(RelayConfig::insecure("127.0.0.1", port));

    let s_clone = server.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect client without host
    let client_url = format!("ws://127.0.0.1:{}/ws?role=client&inst=test-inst-1", port);
    let (mut client_ws, _) = connect_async(client_url).await.unwrap();

    let msg = client_ws.next().await.unwrap().unwrap();
    assert!(msg.is_close());
    if let WsMessage::Close(Some(frame)) = msg {
        assert_eq!(u16::from(frame.code), 4004);
        assert!(frame.reason.contains("host instance not online"));
    } else {
        panic!("expected close frame");
    }

    server.shutdown();
}

#[tokio::test]
async fn test_relay_bidirectional_frame_forwarding() {
    let port = get_free_port();
    let server = RelayServer::new(RelayConfig::insecure("127.0.0.1", port));

    let s_clone = server.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 1. Host connects
    let host_url = format!("ws://127.0.0.1:{}/ws?role=host&inst=inst-duplex-1&host_key=test-host-key-0123456789abcdef0123456789", port);
    let (mut host_ws, _) = connect_async(host_url).await.unwrap();

    // 2. Client connects
    let client_url = format!("ws://127.0.0.1:{}/ws?role=client&inst=inst-duplex-1", port);
    let (mut client_ws, _) = connect_async(client_url).await.unwrap();

    // 3. Client sends to host
    client_ws
        .send(WsMessage::Text("{\"type\":\"hello_from_client\"}".into()))
        .await
        .unwrap();

    let host_received = host_ws.next().await.unwrap().unwrap();
    assert_eq!(
        host_received.to_text().unwrap(),
        "{\"type\":\"hello_from_client\"}"
    );

    // 4. Host sends to client
    host_ws
        .send(WsMessage::Text("{\"type\":\"welcome_from_host\"}".into()))
        .await
        .unwrap();

    let client_received = client_ws.next().await.unwrap().unwrap();
    assert_eq!(
        client_received.to_text().unwrap(),
        "{\"type\":\"welcome_from_host\"}"
    );

    server.shutdown();
}

#[tokio::test]
async fn test_relay_host_disconnect_closes_client() {
    let port = get_free_port();
    let server = RelayServer::new(RelayConfig::insecure("127.0.0.1", port));

    let s_clone = server.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let host_url = format!("ws://127.0.0.1:{}/ws?role=host&inst=inst-disc-1&host_key=test-host-key-0123456789abcdef0123456789", port);
    let (host_ws, _) = connect_async(host_url).await.unwrap();

    let client_url = format!("ws://127.0.0.1:{}/ws?role=client&inst=inst-disc-1", port);
    let (mut client_ws, _) = connect_async(client_url).await.unwrap();

    // Drop host connection
    drop(host_ws);

    let msg = client_ws.next().await.unwrap().unwrap();
    assert!(msg.is_close());
    if let WsMessage::Close(Some(frame)) = msg {
        assert_eq!(u16::from(frame.code), 4004);
        assert!(frame.reason.contains("host daemon disconnected"));
    }

    server.shutdown();
}

#[tokio::test]
async fn test_relay_client_disconnect_keeps_host_online() {
    let port = get_free_port();
    let server = RelayServer::new(RelayConfig::insecure("127.0.0.1", port));

    let s_clone = server.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let host_url = format!("ws://127.0.0.1:{}/ws?role=host&inst=inst-keepalive-1&host_key=test-host-key-0123456789abcdef0123456789", port);
    let (mut host_ws, _) = connect_async(host_url).await.unwrap();

    // Session 1 connects and disconnects
    {
        let client_url = format!("ws://127.0.0.1:{}/ws?role=client&inst=inst-keepalive-1", port);
        let (mut client_ws, _) = connect_async(client_url).await.unwrap();
        client_ws.close(None).await.unwrap();
    }

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Session 2 connects to the SAME host without host having to reconnect
    {
        let client_url = format!("ws://127.0.0.1:{}/ws?role=client&inst=inst-keepalive-1", port);
        let (mut client_ws, _) = connect_async(client_url).await.unwrap();

        client_ws
            .send(WsMessage::Text("{\"type\":\"hello_session_2\"}".into()))
            .await
            .unwrap();

        let received = host_ws.next().await.unwrap().unwrap();
        assert_eq!(received.to_text().unwrap(), "{\"type\":\"hello_session_2\"}");
    }

    server.shutdown();
}

#[tokio::test]
async fn test_relay_authenticates_and_rejects_unauthorized_device() {
    let port = get_free_port();
    let server = RelayServer::new(RelayConfig::insecure("127.0.0.1", port));

    let s_clone = server.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Host connects and syncs authorized devices
    let host_url = format!("ws://127.0.0.1:{}/ws?role=host&inst=inst-auth-1&host_key=test-host-key-0123456789abcdef0123456789", port);
    let (mut host_ws, _) = connect_async(host_url).await.unwrap();

    let sync_msg = RelayControlMessage {
        msg_type: "relay_control".into(),
        action: RelayControlAction::SyncDevices {
            devices: vec![DeviceAuthInfo {
                device_id: "valid-phone".into(),
                device_token: "secret-token-123".into(),
                revoked: false,
            }],
        },
    };
    host_ws
        .send(WsMessage::Text(serde_json::to_string(&sync_msg).unwrap().into()))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // 1. Unknown device is rejected
    let bad_client_url = format!(
        "ws://127.0.0.1:{}/ws?role=client&inst=inst-auth-1&device_id=intruder&token=bad",
        port
    );
    let (mut bad_ws, _) = connect_async(bad_client_url).await.unwrap();
    let msg = bad_ws.next().await.unwrap().unwrap();
    assert!(msg.is_close());
    if let WsMessage::Close(Some(frame)) = msg {
        assert_eq!(u16::from(frame.code), 4001);
        assert!(frame.reason.contains("device not authorized"));
    }

    // 2. Wrong token for valid device is rejected
    let wrong_token_url = format!(
        "ws://127.0.0.1:{}/ws?role=client&inst=inst-auth-1&device_id=valid-phone&token=wrong-token",
        port
    );
    let (mut wrong_ws, _) = connect_async(wrong_token_url).await.unwrap();
    let msg = wrong_ws.next().await.unwrap().unwrap();
    assert!(msg.is_close());
    if let WsMessage::Close(Some(frame)) = msg {
        assert_eq!(u16::from(frame.code), 4001);
        assert!(frame.reason.contains("invalid device token"));
    }

    // 3. Valid device connects successfully
    let good_url = format!(
        "ws://127.0.0.1:{}/ws?role=client&inst=inst-auth-1&device_id=valid-phone&token=secret-token-123",
        port
    );
    let (mut good_ws, _) = connect_async(good_url).await.unwrap();
    good_ws
        .send(WsMessage::Text("{\"type\":\"hello_from_valid_device\"}".into()))
        .await
        .unwrap();

    let host_msg = host_ws.next().await.unwrap().unwrap();
    assert_eq!(host_msg.to_text().unwrap(), "{\"type\":\"hello_from_valid_device\"}");

    // 4. Host revokes device -> client is disconnected immediately
    let revoke_msg = RelayControlMessage {
        msg_type: "relay_control".into(),
        action: RelayControlAction::RevokeDevice {
            device_id: "valid-phone".into(),
        },
    };
    host_ws
        .send(WsMessage::Text(serde_json::to_string(&revoke_msg).unwrap().into()))
        .await
        .unwrap();

    let disconnect_msg = good_ws.next().await.unwrap().unwrap();
    assert!(disconnect_msg.is_close());
    if let WsMessage::Close(Some(frame)) = disconnect_msg {
        assert_eq!(u16::from(frame.code), 4001);
        assert!(frame.reason.contains("device revoked"));
    }

    server.shutdown();
}

#[tokio::test]
async fn test_relay_pairing_validation() {
    let port = get_free_port();
    let server = RelayServer::new(RelayConfig::insecure("127.0.0.1", port));

    let s_clone = server.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Host connects and syncs pairing session
    let host_url = format!("ws://127.0.0.1:{}/ws?role=host&inst=inst-pair-1&host_key=test-host-key-0123456789abcdef0123456789", port);
    let (mut host_ws, _) = connect_async(host_url).await.unwrap();

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let sync_msg = RelayControlMessage {
        msg_type: "relay_control".into(),
        action: RelayControlAction::SyncPairing {
            session: Some(PairingAuthInfo {
                session_id: "sess-valid-1".into(),
                pairing_secret: "secret-hex-12345678".into(),
                expires_at: now_secs + 300,
            }),
        },
    };
    host_ws
        .send(WsMessage::Text(serde_json::to_string(&sync_msg).unwrap().into()))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Bad secret is rejected
    let bad_pair_url = format!(
        "ws://127.0.0.1:{}/ws?role=pair&inst=inst-pair-1&sess=sess-valid-1&sec=wrong-secret",
        port
    );
    let (mut bad_ws, _) = connect_async(bad_pair_url).await.unwrap();
    let msg = bad_ws.next().await.unwrap().unwrap();
    assert!(msg.is_close());
    if let WsMessage::Close(Some(frame)) = msg {
        assert_eq!(u16::from(frame.code), 4001);
    }

    // Good pairing connects and forwards to host
    let good_pair_url = format!(
        "ws://127.0.0.1:{}/ws?role=pair&inst=inst-pair-1&sess=sess-valid-1&sec=secret-hex-12345678",
        port
    );
    let (mut good_ws, _) = connect_async(good_pair_url).await.unwrap();
    good_ws
        .send(WsMessage::Text("{\"type\":\"pair_request\"}".into()))
        .await
        .unwrap();

    let host_msg = host_ws.next().await.unwrap().unwrap();
    assert_eq!(host_msg.to_text().unwrap(), "{\"type\":\"pair_request\"}");

    server.shutdown();
}

#[tokio::test]
async fn test_relay_rejects_host_without_host_key() {
    let port = get_free_port();
    let server = RelayServer::new(RelayConfig::insecure("127.0.0.1", port));
    let s_clone = server.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let url = format!("ws://127.0.0.1:{}/ws?role=host&inst=inst-nokey", port);
    assert!(connect_async(url).await.is_err());
    let weak = format!("ws://127.0.0.1:{}/ws?role=host&inst=inst-nokey&host_key=short", port);
    assert!(connect_async(weak).await.is_err());

    server.shutdown();
}

#[tokio::test]
async fn test_relay_rejects_instance_hijack_and_allows_same_host_reconnect() {
    let port = get_free_port();
    let server = RelayServer::new(RelayConfig::insecure("127.0.0.1", port));
    let s_clone = server.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let good = format!(
        "ws://127.0.0.1:{}/ws?role=host&inst=inst-bind-1&host_key=legit-host-key-0123456789abcdef012345",
        port
    );
    let (mut host_ws, _) = connect_async(&good).await.unwrap();

    // An attacker who knows the instance id but not the host key is rejected.
    let evil = format!(
        "ws://127.0.0.1:{}/ws?role=host&inst=inst-bind-1&host_key=attacker-key-0123456789abcdef0123456",
        port
    );
    let (mut evil_ws, _) = connect_async(evil).await.unwrap();
    match evil_ws.next().await.unwrap().unwrap() {
        WsMessage::Close(Some(frame)) => assert_eq!(u16::from(frame.code), 4003),
        other => panic!("expected close, got {other:?}"),
    }

    // The legitimate laptop reconnecting (e.g. after a Wi-Fi change) supersedes its stale trunk.
    let (mut host_ws2, _) = connect_async(&good).await.unwrap();
    match tokio::time::timeout(Duration::from_secs(2), host_ws.next()).await.unwrap() {
        Some(Ok(WsMessage::Close(Some(frame)))) => assert_eq!(u16::from(frame.code), 4000),
        other => panic!("expected supersede close, got {other:?}"),
    }

    // The new trunk is live: a client reaches it.
    let client_url = format!("ws://127.0.0.1:{}/ws?role=client&inst=inst-bind-1", port);
    let (mut client_ws, _) = connect_async(client_url).await.unwrap();
    client_ws.send(WsMessage::Text("{\"type\":\"ping\"}".into())).await.unwrap();
    let got = tokio::time::timeout(Duration::from_secs(2), host_ws2.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(got.to_text().unwrap(), "{\"type\":\"ping\"}");

    server.shutdown();
}

#[tokio::test]
async fn test_insecure_relay_refuses_non_loopback_bind() {
    let server = RelayServer::new(RelayConfig::insecure("0.0.0.0", get_free_port()));
    assert!(server.run().await.is_err());
}
