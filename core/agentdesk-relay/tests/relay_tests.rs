use std::net::TcpListener;
use std::time::Duration;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use agentdesk_relay::{RelayConfig, RelayServer};

fn get_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[tokio::test]
async fn test_relay_rejection_when_host_offline() {
    let port = get_free_port();
    let server = RelayServer::new(RelayConfig {
        bind_addr: "127.0.0.1".into(),
        port,
        auth_token: None,
    });

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
    let server = RelayServer::new(RelayConfig {
        bind_addr: "127.0.0.1".into(),
        port,
        auth_token: None,
    });

    let s_clone = server.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 1. Host connects
    let host_url = format!("ws://127.0.0.1:{}/ws?role=host&inst=inst-duplex-1", port);
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
    let server = RelayServer::new(RelayConfig {
        bind_addr: "127.0.0.1".into(),
        port,
        auth_token: None,
    });

    let s_clone = server.clone();
    tokio::spawn(async move {
        let _ = s_clone.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let host_url = format!("ws://127.0.0.1:{}/ws?role=host&inst=inst-disc-1", port);
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
