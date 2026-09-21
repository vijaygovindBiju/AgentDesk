use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

pub const DEFAULT_RELAY_PORT: u16 = 8766;

#[derive(Debug, Clone)]
pub struct RelayConfig {
    pub bind_addr: String,
    pub port: u16,
    pub auth_token: Option<String>,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0".into(),
            port: DEFAULT_RELAY_PORT,
            auth_token: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct RelayMetrics {
    pub active_hosts: AtomicU64,
    pub active_clients: AtomicU64,
    pub forwarded_messages: AtomicU64,
}

struct SessionEntry {
    host_tx: Option<mpsc::Sender<WsMessage>>,
    client_tx: Option<mpsc::Sender<WsMessage>>,
}

#[derive(Clone)]
pub struct RelayServer {
    config: RelayConfig,
    sessions: Arc<Mutex<HashMap<String, SessionEntry>>>,
    metrics: Arc<RelayMetrics>,
    shutdown_notify: Arc<tokio::sync::Notify>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PeerRole {
    Host,
    Client,
}

impl RelayServer {
    pub fn new(config: RelayConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            metrics: Arc::new(RelayMetrics::default()),
            shutdown_notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub fn metrics(&self) -> Arc<RelayMetrics> {
        self.metrics.clone()
    }

    pub fn shutdown(&self) {
        self.shutdown_notify.notify_waiters();
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let addr = format!("{}:{}", self.config.bind_addr, self.config.port);
        let listener = TcpListener::bind(&addr).await?;
        let local_addr = listener.local_addr()?;
        eprintln!("AgentDesk Relay listening on ws://{}", local_addr);

        loop {
            tokio::select! {
                _ = self.shutdown_notify.notified() => {
                    eprintln!("Relay shutdown signaled");
                    break;
                }
                accept_res = listener.accept() => {
                    let (stream, peer_addr) = match accept_res {
                        Ok(pair) => pair,
                        Err(e) => {
                            eprintln!("Relay accept error: {}", e);
                            continue;
                        }
                    };

                    let server = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = server.handle_peer(stream, peer_addr).await {
                            eprintln!("Relay peer error ({}): {}", peer_addr, e);
                        }
                    });
                }
            }
        }

        Ok(())
    }

    async fn handle_peer(
        &self,
        stream: TcpStream,
        peer_addr: SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut role = None;
        let mut instance_id = None;

        let expected_token = self.config.auth_token.clone();

        let ws_stream = tokio_tungstenite::accept_hdr_async(
            stream,
            |req: &Request, resp: Response| {
                let query = req.uri().query().unwrap_or_default();
                let params: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
                    .into_owned()
                    .collect();

                let r_str = match params.get("role") {
                    Some(r) => r.as_str(),
                    None => {
                        let mut err = ErrorResponse::new(Some("missing role query parameter".into()));
                        *err.status_mut() = StatusCode::BAD_REQUEST;
                        return Err(err);
                    }
                };

                let parsed_role = match r_str {
                    "host" => PeerRole::Host,
                    "client" => PeerRole::Client,
                    _ => {
                        let mut err = ErrorResponse::new(Some("invalid role parameter".into()));
                        *err.status_mut() = StatusCode::BAD_REQUEST;
                        return Err(err);
                    }
                };

                let inst = match params.get("inst") {
                    Some(i) if !i.trim().is_empty() => i.trim().to_string(),
                    _ => {
                        let mut err = ErrorResponse::new(Some("missing inst query parameter".into()));
                        *err.status_mut() = StatusCode::BAD_REQUEST;
                        return Err(err);
                    }
                };

                if parsed_role == PeerRole::Host {
                    if let Some(ref exp) = expected_token {
                        let provided = params.get("token").map(|s| s.as_str()).unwrap_or("");
                        if provided != exp {
                            let mut err = ErrorResponse::new(Some("unauthorized".into()));
                            *err.status_mut() = StatusCode::UNAUTHORIZED;
                            return Err(err);
                        }
                    }
                }

                role = Some(parsed_role);
                instance_id = Some(inst);
                Ok(resp)
            },
        )
        .await?;

        let role = role.ok_or("unresolved role")?;
        let instance_id = instance_id.ok_or("unresolved instance_id")?;

        let (mut ws_sink, mut ws_stream) = ws_stream.split();
        let (out_tx, mut out_rx) = mpsc::channel::<WsMessage>(128);

        // Outbound pump
        let out_handle = tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                if ws_sink.send(msg).await.is_err() {
                    break;
                }
            }
        });

        match role {
            PeerRole::Host => {
                self.metrics.active_hosts.fetch_add(1, Ordering::SeqCst);
                eprintln!(
                    "Host connected: instance '{}' from {}",
                    instance_id, peer_addr
                );

                // Register host
                {
                    let mut lock = self.sessions.lock().unwrap();
                    let entry = lock.entry(instance_id.clone()).or_insert_with(|| SessionEntry {
                        host_tx: None,
                        client_tx: None,
                    });
                    entry.host_tx = Some(out_tx.clone());
                }

                // Inbound loop: frames from host -> forwarded to client
                while let Some(msg_res) = ws_stream.next().await {
                    match msg_res {
                        Ok(msg) => {
                            if msg.is_close() {
                                break;
                            }
                            if msg.is_ping() {
                                let _ = out_tx.send(WsMessage::Pong(msg.into_data().into())).await;
                                continue;
                            }
                            if msg.is_pong() {
                                continue;
                            }

                            // Forward to client if connected
                            let client_tx = {
                                let lock = self.sessions.lock().unwrap();
                                lock.get(&instance_id).and_then(|e| e.client_tx.clone())
                            };
                            if let Some(tx) = client_tx {
                                if tx.send(msg).await.is_ok() {
                                    self.metrics.forwarded_messages.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Host ws error ({}): {}", peer_addr, e);
                            break;
                        }
                    }
                }

                // Cleanup host
                eprintln!("Host disconnected: instance '{}'", instance_id);
                self.metrics.active_hosts.fetch_sub(1, Ordering::SeqCst);

                let client_tx = {
                    let mut lock = self.sessions.lock().unwrap();
                    if let Some(entry) = lock.get_mut(&instance_id) {
                        entry.host_tx = None;
                        entry.client_tx.clone()
                    } else {
                        None
                    }
                };

                // Notify client that host went offline
                if let Some(tx) = client_tx {
                    let _ = tx
                        .send(WsMessage::Close(Some(CloseFrame {
                            code: CloseCode::from(4004),
                            reason: "host daemon disconnected".into(),
                        })))
                        .await;
                }
            }
            PeerRole::Client => {
                // Check if host is online
                let host_tx = {
                    let lock = self.sessions.lock().unwrap();
                    lock.get(&instance_id).and_then(|e| e.host_tx.clone())
                };

                let Some(host_tx) = host_tx else {
                    let _ = out_tx
                        .send(WsMessage::Close(Some(CloseFrame {
                            code: CloseCode::from(4004),
                            reason: "host instance not online on relay".into(),
                        })))
                        .await;
                    out_handle.await?;
                    return Ok(());
                };

                self.metrics.active_clients.fetch_add(1, Ordering::SeqCst);
                eprintln!(
                    "Client connected for instance '{}' from {}",
                    instance_id, peer_addr
                );

                // Register client
                {
                    let mut lock = self.sessions.lock().unwrap();
                    if let Some(entry) = lock.get_mut(&instance_id) {
                        entry.client_tx = Some(out_tx.clone());
                    }
                }

                // Inbound loop: frames from client -> forwarded to host
                while let Some(msg_res) = ws_stream.next().await {
                    match msg_res {
                        Ok(msg) => {
                            if msg.is_close() {
                                break;
                            }
                            if msg.is_ping() {
                                let _ = out_tx.send(WsMessage::Pong(msg.into_data().into())).await;
                                continue;
                            }
                            if msg.is_pong() {
                                continue;
                            }

                            // Forward to host
                            if host_tx.send(msg).await.is_ok() {
                                self.metrics.forwarded_messages.fetch_add(1, Ordering::Relaxed);
                            } else {
                                // Host died
                                break;
                            }
                        }
                        Err(e) => {
                            eprintln!("Client ws error ({}): {}", peer_addr, e);
                            break;
                        }
                    }
                }

                // Cleanup client
                eprintln!("Client disconnected for instance '{}'", instance_id);
                self.metrics.active_clients.fetch_sub(1, Ordering::SeqCst);

                let host_tx = {
                    let mut lock = self.sessions.lock().unwrap();
                    if let Some(entry) = lock.get_mut(&instance_id) {
                        entry.client_tx = None;
                        entry.host_tx.clone()
                    } else {
                        None
                    }
                };

                if let Some(tx) = host_tx {
                    let _ = tx
                        .send(WsMessage::Close(Some(CloseFrame {
                            code: CloseCode::from(1000),
                            reason: "client session ended".into(),
                        })))
                        .await;
                }
            }
        }

        out_handle.abort();
        Ok(())
    }
}
