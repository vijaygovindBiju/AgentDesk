use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use rcgen::generate_simple_self_signed;
use rustls::ServerConfig as RustlsServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use uuid::Uuid;

pub const DEFAULT_RELAY_PORT: u16 = 8766;
/// Minimum length of the per-instance host key presented by laptop daemons.
pub const MIN_HOST_KEY_LEN: usize = 32;

fn is_loopback_bind(addr: &str) -> bool {
    matches!(addr.trim().trim_matches(['[', ']']), "127.0.0.1" | "::1" | "localhost")
}

fn hash_host_key(key: &str) -> String {
    compute_sha256_fingerprint(key.as_bytes())
}

pub fn compute_sha256_fingerprint(der: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(der);
    let hash = hasher.finalize();
    hash.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(":")
}

pub struct RelayTlsIdentity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
    pub fingerprint: String,
}

impl RelayTlsIdentity {
    pub fn generate() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let subject_alt_names = vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "0.0.0.0".to_string(),
        ];
        let cert = generate_simple_self_signed(subject_alt_names)?;
        let cert_der = cert.cert.der().to_vec();
        let key_der = cert.signing_key.serialize_der();
        let fingerprint = compute_sha256_fingerprint(&cert_der);
        Ok(Self {
            cert_der,
            key_der,
            fingerprint,
        })
    }

    pub fn load_from_paths(
        cert_path: &Path,
        key_path: &Path,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let cert_bytes = fs::read(cert_path)?;
        let key_bytes = fs::read(key_path)?;
        let fingerprint = compute_sha256_fingerprint(&cert_bytes);
        Ok(Self {
            cert_der: cert_bytes,
            key_der: key_bytes,
            fingerprint,
        })
    }

    pub fn build_tls_acceptor(&self) -> Result<TlsAcceptor, Box<dyn std::error::Error + Send + Sync>> {
        let cert_der = CertificateDer::from(self.cert_der.clone());
        let key_der = PrivateKeyDer::try_from(self.key_der.clone())
            .map_err(|e| format!("invalid private key DER: {e}"))?;

        let server_config = RustlsServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der], key_der)?;

        Ok(TlsAcceptor::from(Arc::new(server_config)))
    }
}

#[derive(Debug, Clone)]
pub struct RelayConfig {
    pub bind_addr: String,
    pub port: u16,
    pub auth_token: Option<String>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub insecure_dev: bool,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0".into(),
            port: DEFAULT_RELAY_PORT,
            auth_token: None,
            tls_cert: None,
            tls_key: None,
            insecure_dev: false,
        }
    }
}

impl RelayConfig {
    pub fn insecure(bind_addr: impl Into<String>, port: u16) -> Self {
        Self {
            bind_addr: bind_addr.into(),
            port,
            auth_token: None,
            tls_cert: None,
            tls_key: None,
            insecure_dev: true,
        }
    }
}

#[derive(Debug, Default)]
pub struct RelayMetrics {
    pub active_hosts: AtomicU64,
    pub active_clients: AtomicU64,
    pub forwarded_messages: AtomicU64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthInfo {
    pub device_id: String,
    pub device_token: String,
    #[serde(default)]
    pub revoked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingAuthInfo {
    pub session_id: String,
    pub pairing_secret: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum RelayControlAction {
    #[serde(rename = "sync_devices")]
    SyncDevices { devices: Vec<DeviceAuthInfo> },
    #[serde(rename = "sync_pairing")]
    SyncPairing { session: Option<PairingAuthInfo> },
    #[serde(rename = "authorize_device")]
    AuthorizeDevice { device_id: String, device_token: String },
    #[serde(rename = "revoke_device")]
    RevokeDevice { device_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayControlMessage {
    #[serde(rename = "type")]
    pub msg_type: String, // "relay_control"
    #[serde(flatten)]
    pub action: RelayControlAction,
}

struct ClientSession {
    tx: mpsc::Sender<WsMessage>,
    device_id: Option<String>,
}

struct SessionEntry {
    host_tx: Option<mpsc::Sender<WsMessage>>,
    /// SHA-256 of the per-instance host key. The first host to register an
    /// instance binds it; later hosts must present the same key (TOFU).
    host_key_hash: Option<String>,
    clients: HashMap<String, ClientSession>,
    authorized_devices: HashMap<String, DeviceAuthInfo>,
    active_pairing: Option<PairingAuthInfo>,
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
    Pair,
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
        if self.config.insecure_dev && !is_loopback_bind(&self.config.bind_addr) {
            return Err(format!(
                "--insecure-dev relay must bind to loopback (got '{}'); terminate TLS in front of it for public use",
                self.config.bind_addr
            )
            .into());
        }
        let addr = format!("{}:{}", self.config.bind_addr, self.config.port);
        let listener = TcpListener::bind(&addr).await?;
        let local_addr = listener.local_addr()?;

        let tls_acceptor = if self.config.insecure_dev {
            eprintln!(
                "===================================================================\n\
                 * WARNING: RELAY --insecure-dev IS ENABLED!                      *\n\
                 * WebSocket transport is unencrypted plain ws://.                 *\n\
                 * Intended ONLY for local development testing.                    *\n\
                 ==================================================================="
            );
            None
        } else {
            let identity = if let (Some(cert_path), Some(key_path)) =
                (&self.config.tls_cert, &self.config.tls_key)
            {
                RelayTlsIdentity::load_from_paths(cert_path, key_path)?
            } else {
                RelayTlsIdentity::generate()?
            };
            let acceptor = identity.build_tls_acceptor()?;
            eprintln!("AgentDesk Relay TLS enabled (fingerprint: {})", identity.fingerprint);
            Some(acceptor)
        };

        let scheme = if tls_acceptor.is_some() { "wss" } else { "ws" };
        eprintln!("AgentDesk Relay listening on {}://{}", scheme, local_addr);

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
                    let acceptor = tls_acceptor.clone();
                    tokio::spawn(async move {
                        if let Some(acceptor) = acceptor {
                            match acceptor.accept(stream).await {
                                Ok(tls_stream) => {
                                    if let Err(e) = server.handle_peer(tls_stream, peer_addr).await {
                                        eprintln!("Relay peer error ({}): {}", peer_addr, e);
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Relay TLS handshake error ({}): {}", peer_addr, e);
                                }
                            }
                        } else {
                            if let Err(e) = server.handle_peer(stream, peer_addr).await {
                                eprintln!("Relay peer error ({}): {}", peer_addr, e);
                            }
                        }
                    });
                }
            }
        }

        Ok(())
    }

    pub async fn handle_peer<S>(
        &self,
        stream: S,
        peer_addr: SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let mut role = None;
        let mut instance_id = None;
        let mut client_device_id = None;
        let mut client_token = None;
        let mut pairing_session_id = None;
        let mut pairing_secret = None;
        let mut host_key = None;

        let expected_token = self.config.auth_token.clone();

        #[allow(clippy::result_large_err)]
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
                    "pair" => PeerRole::Pair,
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
                    if expected_token.as_ref().is_some_and(|exp| {
                        params.get("token").map(|s| s.as_str()).unwrap_or("") != exp
                    }) {
                        let mut err = ErrorResponse::new(Some("unauthorized".into()));
                        *err.status_mut() = StatusCode::UNAUTHORIZED;
                        return Err(err);
                    }
                    match params.get("host_key") {
                        Some(k) if k.len() >= MIN_HOST_KEY_LEN => host_key = Some(k.clone()),
                        _ => {
                            let mut err = ErrorResponse::new(Some("missing or weak host_key".into()));
                            *err.status_mut() = StatusCode::UNAUTHORIZED;
                            return Err(err);
                        }
                    }
                }

                role = Some(parsed_role);
                instance_id = Some(inst);
                client_device_id = params.get("device_id").cloned();
                client_token = params.get("token").cloned();
                pairing_session_id = params.get("sess").cloned();
                pairing_secret = params.get("sec").cloned();

                Ok(resp)
            },
        )
        .await?;

        let role = role.ok_or("unresolved role")?;
        let instance_id = instance_id.ok_or("unresolved instance_id")?;

        let (mut ws_sink, mut ws_stream) = ws_stream.split();
        let (out_tx, mut out_rx) = mpsc::channel::<WsMessage>(256);

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
                let presented_hash = hash_host_key(host_key.as_deref().unwrap_or_default());

                // Register host (rejecting instance hijack attempts)
                let previous_host = {
                    let mut lock = self.sessions.lock().unwrap();
                    let entry = lock.entry(instance_id.clone()).or_insert_with(|| SessionEntry {
                        host_tx: None,
                        host_key_hash: None,
                        clients: HashMap::new(),
                        authorized_devices: HashMap::new(),
                        active_pairing: None,
                    });
                    match &entry.host_key_hash {
                        Some(bound) if *bound != presented_hash => Err(()),
                        _ => {
                            entry.host_key_hash = Some(presented_hash.clone());
                            Ok(entry.host_tx.replace(out_tx.clone()))
                        }
                    }
                };
                let previous_host = match previous_host {
                    Ok(prev) => prev,
                    Err(()) => {
                        eprintln!("Rejected host for instance '{}' from {}: host_key mismatch", instance_id, peer_addr);
                        let _ = out_tx
                            .send(WsMessage::Close(Some(CloseFrame {
                                code: CloseCode::from(4003),
                                reason: "instance bound to a different host key".into(),
                            })))
                            .await;
                        let _ = out_handle.await;
                        return Ok(());
                    }
                };
                // A reconnecting host supersedes its stale trunk (no duplicate sessions).
                if let Some(prev) = previous_host {
                    let _ = prev.try_send(WsMessage::Close(Some(CloseFrame {
                        code: CloseCode::from(4000),
                        reason: "superseded by reconnecting host".into(),
                    })));
                }
                self.metrics.active_hosts.fetch_add(1, Ordering::SeqCst);
                eprintln!(
                    "Host connected: instance '{}' from {}",
                    instance_id, peer_addr
                );

                // Inbound loop: frames from host -> forwarded to clients or handled as control
                while let Some(msg_res) = ws_stream.next().await {
                    match msg_res {
                        Ok(msg) => {
                            if msg.is_close() {
                                break;
                            }
                            if msg.is_ping() {
                                let _ = out_tx.send(WsMessage::Pong(msg.into_data())).await;
                                continue;
                            }
                            if msg.is_pong() {
                                continue;
                            }

                            if let WsMessage::Text(ref text) = msg {
                                // Check if host sent a relay control command
                                if let Ok(ctrl) = serde_json::from_str::<RelayControlMessage>(text) {
                                    let mut lock = self.sessions.lock().unwrap();
                                    if let Some(entry) = lock.get_mut(&instance_id) {
                                        match ctrl.action {
                                            RelayControlAction::SyncDevices { devices } => {
                                                entry.authorized_devices.clear();
                                                for d in devices {
                                                    entry.authorized_devices.insert(d.device_id.clone(), d);
                                                }
                                            }
                                            RelayControlAction::SyncPairing { session } => {
                                                entry.active_pairing = session;
                                            }
                                            RelayControlAction::AuthorizeDevice { device_id, device_token } => {
                                                entry.authorized_devices.insert(
                                                    device_id.clone(),
                                                    DeviceAuthInfo {
                                                        device_id,
                                                        device_token,
                                                        revoked: false,
                                                    },
                                                );
                                            }
                                            RelayControlAction::RevokeDevice { device_id } => {
                                                if let Some(d) = entry.authorized_devices.get_mut(&device_id) {
                                                    d.revoked = true;
                                                }
                                                // Immediately disconnect any active client with this revoked device_id
                                                let mut drop_conns = Vec::new();
                                                for (conn_id, client) in &entry.clients {
                                                    if client.device_id.as_deref() == Some(&device_id) {
                                                        drop_conns.push(conn_id.clone());
                                                        let _ = client.tx.try_send(WsMessage::Close(Some(CloseFrame {
                                                            code: CloseCode::from(4001),
                                                            reason: "device revoked".into(),
                                                        })));
                                                    }
                                                }
                                                for conn_id in drop_conns {
                                                    entry.clients.remove(&conn_id);
                                                }
                                            }
                                        }
                                    }
                                    continue;
                                }
                            }

                            // Forward regular message to all active clients for this instance
                            let client_txs: Vec<mpsc::Sender<WsMessage>> = {
                                let lock = self.sessions.lock().unwrap();
                                lock.get(&instance_id)
                                    .map(|e| e.clients.values().map(|c| c.tx.clone()).collect())
                                    .unwrap_or_default()
                            };

                            for tx in client_txs {
                                if tx.send(msg.clone()).await.is_ok() {
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

                let client_txs: Vec<mpsc::Sender<WsMessage>> = {
                    let mut lock = self.sessions.lock().unwrap();
                    match lock.get_mut(&instance_id) {
                        // Only tear down if this trunk is still the active one.
                        Some(entry) if entry.host_tx.as_ref().is_some_and(|tx| tx.same_channel(&out_tx)) => {
                            entry.host_tx = None;
                            entry.clients.drain().map(|(_, c)| c.tx).collect()
                        }
                        _ => Vec::new(),
                    }
                };

                // Notify all clients that host went offline
                for tx in client_txs {
                    let _ = tx
                        .send(WsMessage::Close(Some(CloseFrame {
                            code: CloseCode::from(4004),
                            reason: "host daemon disconnected".into(),
                        })))
                        .await;
                }
            }
            PeerRole::Client | PeerRole::Pair => {
                // 1. Verify host is online
                let (host_tx, auth_rejection) = {
                    let lock = self.sessions.lock().unwrap();
                    match lock.get(&instance_id) {
                        Some(entry) if entry.host_tx.is_some() => {
                            let mut rejection = None;
                            if role == PeerRole::Pair || pairing_session_id.is_some() {
                                if let Some(ref p) = entry.active_pairing {
                                    let sess = pairing_session_id.as_deref().unwrap_or_default();
                                    let sec = pairing_secret.as_deref().unwrap_or_default();
                                    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
                                    if p.session_id != sess || p.pairing_secret != sec || now > p.expires_at {
                                        rejection = Some((4001, "invalid or expired pairing credentials"));
                                    }
                                }
                            } else if let (Some(dev_id), Some(tok)) = (&client_device_id, &client_token) {
                                match entry.authorized_devices.get(dev_id) {
                                    Some(auth) if auth.revoked => {
                                        rejection = Some((4001, "device revoked"));
                                    }
                                    Some(auth) if auth.device_token != *tok => {
                                        rejection = Some((4001, "invalid device token"));
                                    }
                                    Some(_) => {}
                                    None if !entry.authorized_devices.is_empty() => {
                                        rejection = Some((4001, "device not authorized"));
                                    }
                                    None => {}
                                }
                            }
                            (entry.host_tx.clone(), rejection)
                        }
                        _ => (None, None),
                    }
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

                if let Some((code, reason)) = auth_rejection {
                    let _ = out_tx
                        .send(WsMessage::Close(Some(CloseFrame {
                            code: CloseCode::from(code),
                            reason: reason.into(),
                        })))
                        .await;
                    out_handle.await?;
                    return Ok(());
                }

                self.metrics.active_clients.fetch_add(1, Ordering::SeqCst);
                let conn_id = Uuid::new_v4().to_string();
                eprintln!(
                    "Client connected for instance '{}' (conn: {}, device: {:?}) from {}",
                    instance_id, conn_id, client_device_id, peer_addr
                );

                // Register client session
                {
                    let mut lock = self.sessions.lock().unwrap();
                    if let Some(entry) = lock.get_mut(&instance_id) {
                        entry.clients.insert(
                            conn_id.clone(),
                            ClientSession {
                                tx: out_tx.clone(),
                                device_id: client_device_id.clone(),
                            },
                        );
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
                                let _ = out_tx.send(WsMessage::Pong(msg.into_data())).await;
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

                // Cleanup client only; host remains online!
                eprintln!("Client disconnected for instance '{}' (conn: {})", instance_id, conn_id);
                self.metrics.active_clients.fetch_sub(1, Ordering::SeqCst);

                {
                    let mut lock = self.sessions.lock().unwrap();
                    if let Some(entry) = lock.get_mut(&instance_id) {
                        entry.clients.remove(&conn_id);
                    }
                }
            }
        }

        out_handle.abort();
        Ok(())
    }
}
