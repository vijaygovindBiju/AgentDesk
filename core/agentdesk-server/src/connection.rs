//! Per-connection WebSocket handling and transport sink.
//! See docs/COMMUNICATION.md and P6.1 - P6.4.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::tungstenite::protocol::frame::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use agentdesk_core::{ClientId, Clock, CoreCommand, SinkError, TransportSink};
use agentdesk_model::close_code;
use agentdesk_model::{
    Body, ErrorReply, Message, PairResponse, PipelineMode, SCHEMA_VERSION, TransportMode, Welcome,
};

use crate::device::DeviceRegistry;
use crate::logging;
use crate::pairing::PairingManager;
use crate::status::StatusTracker;
use crate::token::verify_token;

/// Default outbound bounded channel capacity per connection.
pub const DEFAULT_OUTBOUND_CAPACITY: usize = 1024;

/// Outbound transport sink bound to a single WebSocket client connection.
pub struct ServerConnectionSink {
    sender: mpsc::Sender<Message>,
    slow_client_notify: Arc<tokio::sync::Notify>,
    is_slow: Arc<AtomicBool>,
}

impl ServerConnectionSink {
    pub fn new(
        sender: mpsc::Sender<Message>,
        slow_client_notify: Arc<tokio::sync::Notify>,
        is_slow: Arc<AtomicBool>,
    ) -> Self {
        Self {
            sender,
            slow_client_notify,
            is_slow,
        }
    }
}

impl TransportSink for ServerConnectionSink {
    fn send(&mut self, message: &Message) -> Result<usize, SinkError> {
        let json_bytes = serde_json::to_vec(message).map_err(|e| SinkError::Io(e.to_string()))?;
        let len = json_bytes.len();

        match self.sender.try_send(message.clone()) {
            Ok(()) => Ok(len),
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.is_slow.store(true, Ordering::SeqCst);
                self.slow_client_notify.notify_one();
                Err(SinkError::ChannelFull)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(SinkError::Closed),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Parameters for handling an incoming client connection.
pub struct ConnectionParams {
    pub client_id: ClientId,
    pub expected_token: Arc<String>,
    pub pipeline_mode: PipelineMode,
    pub transport_mode: TransportMode,
    pub clock: Arc<dyn Clock>,
    pub core_sender: mpsc::Sender<CoreCommand>,
    pub outbound_capacity: usize,
    pub debug_logging: bool,
    pub device_registry: Arc<tokio::sync::RwLock<DeviceRegistry>>,
    pub pairing_manager: PairingManager,
    pub config_dir: PathBuf,
    pub status_tracker: Arc<StatusTracker>,
}

use tokio_tungstenite::WebSocketStream;

/// Accept and manage a single client WebSocket connection lifecycle.
pub async fn handle_connection<S>(stream: S, peer_addr: String, params: ConnectionParams)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    logging::info(format!(
        "Accepted connection from {} (client_id: {})",
        peer_addr, params.client_id
    ));

    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            logging::info(format!(
                "WebSocket handshake failed from {}: {}",
                peer_addr, e
            ));
            return;
        }
    };

    handle_ws_connection(ws_stream, peer_addr, params).await;
}

pub async fn handle_ws_connection<S>(
    mut ws_stream: WebSocketStream<S>,
    peer_addr: String,
    params: ConnectionParams,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    // 1. Handshake: First frame MUST be `hello`
    let first_frame = match ws_stream.next().await {
        Some(Ok(msg)) => msg,
        Some(Err(e)) => {
            logging::info(format!(
                "Failed reading first frame from {}: {}",
                peer_addr, e
            ));
            return;
        }
        None => {
            logging::info(format!(
                "Connection closed by {} before handshake",
                peer_addr
            ));
            return;
        }
    };

    let text_content = match first_frame {
        WsMessage::Text(t) => t,
        _ => {
            logging::info(format!(
                "First frame from {} was not text, closing 4001",
                peer_addr
            ));
            let _ = ws_stream
                .send(WsMessage::Close(Some(CloseFrame {
                    code: CloseCode::from(close_code::UNAUTHORIZED),
                    reason: "first frame must be text hello".into(),
                })))
                .await;
            return;
        }
    };

    // Parse first message: can be Hello or PairRequest
    let first_msg = match serde_json::from_str::<Message>(&text_content) {
        Ok(m) => m,
        Err(e) => {
            logging::info(format!(
                "First frame from {} was not valid JSON message ({}): '{}', closing 4001",
                peer_addr, e, text_content
            ));
            let _ = ws_stream
                .send(WsMessage::Close(Some(CloseFrame {
                    code: CloseCode::from(close_code::UNAUTHORIZED),
                    reason: "first frame must be hello or pair_request".into(),
                })))
                .await;
            return;
        }
    };

    let hello = match first_msg.body {
        Body::PairRequest(pair_req) => {
            if pair_req.schema_version != SCHEMA_VERSION {
                logging::info(format!(
                    "Pairing schema mismatch from {} (got {}, expected {}), closing 4002",
                    peer_addr, pair_req.schema_version, SCHEMA_VERSION
                ));
                let _ = ws_stream
                    .send(WsMessage::Close(Some(CloseFrame {
                        code: CloseCode::from(close_code::SCHEMA_MISMATCH),
                        reason: format!(
                            "schema version mismatch: client {}, server {}",
                            pair_req.schema_version, SCHEMA_VERSION
                        )
                        .into(),
                    })))
                    .await;
                return;
            }

            match params.pairing_manager.validate_and_consume(
                &pair_req.session_id,
                &pair_req.pairing_secret,
                params.clock.now(),
            ) {
                Ok(session) => {
                    let mut reg = params.device_registry.write().await;
                    let dev_token = reg.register_device(
                        pair_req.device_id.clone(),
                        pair_req.device_name.clone(),
                    );
                    let _ = reg.save(&params.config_dir);
                    params.pairing_manager.record_paired_device(
                        &pair_req.session_id,
                        &pair_req.device_id,
                        &pair_req.device_name,
                    );
                    logging::info(format!(
                        "Successfully paired device '{}' ({}) from {}",
                        pair_req.device_id, pair_req.device_name, peer_addr
                    ));

                    let resp = Message::push(Body::PairResponse(PairResponse {
                        instance_id: session.instance_id,
                        device_token: dev_token,
                        schema_version: SCHEMA_VERSION,
                    }));
                    if let Ok(text) = serde_json::to_string(&resp) {
                        let _ = ws_stream.send(WsMessage::Text(text.into())).await;
                    }
                    let _ = ws_stream
                        .send(WsMessage::Close(Some(CloseFrame {
                            code: CloseCode::Normal,
                            reason: "pairing complete".into(),
                        })))
                        .await;
                    return;
                }
                Err(e) => {
                    logging::info(format!(
                        "Pairing rejected from {} for session {}: {}",
                        peer_addr, pair_req.session_id, e
                    ));
                    let _ = ws_stream
                        .send(WsMessage::Close(Some(CloseFrame {
                            code: CloseCode::from(close_code::UNAUTHORIZED),
                            reason: format!("pairing rejected: {}", e).into(),
                        })))
                        .await;
                    return;
                }
            }
        }
        Body::Hello(h) => {
            // Verify token in constant time: either master token or valid non-revoked device token
            let master_ok = verify_token(&h.token, &params.expected_token);
            let device_ok = if !master_ok {
                let reg = params.device_registry.read().await;
                reg.is_authorized(&h.device_id, &h.token)
            } else {
                true
            };

            if !device_ok {
                logging::info(format!(
                    "Authentication failed for {} (device: {}), closing 4001",
                    peer_addr, h.device_id
                ));
                let _ = ws_stream
                    .send(WsMessage::Close(Some(CloseFrame {
                        code: CloseCode::from(close_code::UNAUTHORIZED),
                        reason: "invalid authentication token".into(),
                    })))
                    .await;
                return;
            }

            // Verify schema version
            if h.schema_version != SCHEMA_VERSION {
                logging::info(format!(
                    "Schema mismatch for {} (got {}, expected {}), closing 4002",
                    peer_addr, h.schema_version, SCHEMA_VERSION
                ));
                let _ = ws_stream
                    .send(WsMessage::Close(Some(CloseFrame {
                        code: CloseCode::from(close_code::SCHEMA_MISMATCH),
                        reason: format!(
                            "schema version mismatch: client {}, server {}",
                            h.schema_version, SCHEMA_VERSION
                        )
                        .into(),
                    })))
                    .await;
                return;
            }

            h
        }
        _ => {
            logging::info(format!(
                "First frame from {} was not hello or pair_request, closing 4001",
                peer_addr
            ));
            let _ = ws_stream
                .send(WsMessage::Close(Some(CloseFrame {
                    code: CloseCode::from(close_code::UNAUTHORIZED),
                    reason: "first frame must be hello or pair_request".into(),
                })))
                .await;
            return;
        }
    };

    logging::info(format!(
        "Authenticated client {} (device: {}, client_version: {})",
        params.client_id, hello.device_id, hello.client_version
    ));

    let transport_str = match params.transport_mode {
        TransportMode::Relay => "Relay (Outbound WSS)",
        TransportMode::Tls => "Direct TLS (Loopback)",
        TransportMode::InsecureDev => "Insecure Dev (Loopback ws)",
    };
    let dev_name = {
        let reg = params.device_registry.read().await;
        reg.devices
            .get(&hello.device_id)
            .map(|d| d.device_name.clone())
            .unwrap_or_else(|| hello.device_id.clone())
    };
    params.status_tracker.register_client(
        params.client_id,
        hello.device_id.clone(),
        dev_name,
        transport_str,
    );

    // Handshake passed: establish bounded outbound channel & sink
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<Message>(params.outbound_capacity.max(1));
    let slow_client_notify = Arc::new(tokio::sync::Notify::new());
    let is_slow = Arc::new(AtomicBool::new(false));

    let sink = ServerConnectionSink::new(outbound_tx, slow_client_notify.clone(), is_slow.clone());

    // Register sink with core
    if params
        .core_sender
        .send(CoreCommand::Connect {
            client_id: params.client_id,
            sink: Box::new(sink),
        })
        .await
        .is_err()
    {
        logging::info(format!(
            "Core task inbox closed, disconnecting client {}",
            params.client_id
        ));
        return;
    }

    // Send Welcome frame
    let welcome_msg = Message::push(Body::Welcome(Welcome {
        daemon_version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version: SCHEMA_VERSION,
        pipeline_mode: params.pipeline_mode,
        transport: params.transport_mode,
        server_time: params.clock.now(),
    }));

    let welcome_json = match serde_json::to_string(&welcome_msg) {
        Ok(s) => s,
        Err(e) => {
            logging::info(format!("Failed serializing welcome: {}", e));
            return;
        }
    };

    if let Err(e) = ws_stream.send(WsMessage::Text(welcome_json.into())).await {
        logging::info(format!(
            "Failed sending welcome to client {}: {}",
            params.client_id, e
        ));
        let _ = params
            .core_sender
            .send(CoreCommand::Disconnect {
                client_id: params.client_id,
            })
            .await;
        return;
    }

    // Immediately trigger snapshot push from core
    let _ = params
        .core_sender
        .send(CoreCommand::SendSnapshot {
            client_id: params.client_id,
        })
        .await;

    // Split stream into sender and receiver
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Loop until disconnect or slow-client abort
    loop {
        tokio::select! {
            // Slow client detected
            _ = slow_client_notify.notified() => {
                logging::info(format!("Slow client overflow on client {}, closing 4003", params.client_id));
                let _ = ws_sender.send(WsMessage::Close(Some(CloseFrame {
                    code: CloseCode::from(close_code::SLOW_CLIENT),
                    reason: "outbound channel full (slow client)".into(),
                }))).await;
                break;
            }

            // Outbound message to client
            outbound = outbound_rx.recv() => {
                match outbound {
                    Some(msg) => {
                        match serde_json::to_string(&msg) {
                            Ok(text) => {
                                if params.debug_logging {
                                    logging::debug(format!("Outbound -> client {}: {}", params.client_id, text));
                                }
                                if let Err(e) = ws_sender.send(WsMessage::Text(text.into())).await {
                                    logging::info(format!("Write error on client {}: {}", params.client_id, e));
                                    break;
                                }
                            }
                            Err(e) => {
                                logging::info(format!("Failed serializing outbound message: {}", e));
                            }
                        }
                    }
                    None => {
                        // Outbound channel closed by core
                        break;
                    }
                }
            }

            // Inbound message from client
            inbound = ws_receiver.next() => {
                match inbound {
                    Some(Ok(WsMessage::Text(text))) => {
                        if params.debug_logging {
                            logging::debug(format!("Inbound <- client {}: {}", params.client_id, text));
                        }
                        params.status_tracker.update_last_seen(params.client_id);

                        match serde_json::from_str::<Message>(&text) {
                            Ok(msg) => {
                                // Valid message, route to CoreTask
                                if let Err(e) = params.core_sender.send(CoreCommand::Client {
                                    client_id: params.client_id,
                                    message: msg,
                                }).await {
                                    logging::info(format!("Failed dispatching client message to core: {}", e));
                                    break;
                                }
                            }
                            Err(parse_err) => {
                                // P6.3 / P6.T3: Malformed JSON => error reply, connection stays open
                                logging::info(format!("Malformed message from client {}: {}", params.client_id, parse_err));

                                let req_id = serde_json::from_str::<serde_json::Value>(&text)
                                    .ok()
                                    .and_then(|v| {
                                        v.get("request_id")
                                            .and_then(|r| r.as_str().map(ToString::to_string))
                                    });

                                let error_reply = Message {
                                    request_id: req_id,
                                    body: Body::Error(ErrorReply {
                                        code: "malformed_frame".into(),
                                        message: format!("Malformed message frame: {parse_err}"),
                                    }),
                                };

                                if let Ok(err_text) = serde_json::to_string(&error_reply) {
                                    #[allow(clippy::collapsible_if)]
                                    if let Err(e) = ws_sender.send(WsMessage::Text(err_text.into())).await {
                                        logging::info(format!("Error sending error reply to client {}: {}", params.client_id, e));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(WsMessage::Ping(data))) => {
                        if let Err(e) = ws_sender.send(WsMessage::Pong(data)).await {
                            logging::info(format!("Ping reply error on client {}: {}", params.client_id, e));
                            break;
                        }
                    }
                    Some(Ok(WsMessage::Close(frame))) => {
                        logging::info(format!(
                            "Client {} sent close frame: {:?}",
                            params.client_id, frame
                        ));
                        break;
                    }
                    Some(Err(e)) => {
                        logging::info(format!("Read error on client {}: {}", params.client_id, e));
                        break;
                    }
                    None => {
                        // Connection closed
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    // Cleanup: unregister sink from core
    params.status_tracker.unregister_client(params.client_id);
    let _ = params
        .core_sender
        .send(CoreCommand::Disconnect {
            client_id: params.client_id,
        })
        .await;

    logging::info(format!("Client {} disconnected", params.client_id));
}

/// Persistent host trunk connection to the AgentDesk relay.
/// Multiplexes communication between the AgentDesk core pipeline and multiple remote phone clients.
pub async fn handle_relay_trunk_connection<S>(
    ws_stream: WebSocketStream<S>,
    relay_endpoint: String,
    params: ConnectionParams,
    _instance_id: String,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    logging::info(format!("Established relay trunk connection to {}", relay_endpoint));

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    let (ws_out_tx, mut ws_out_rx) = mpsc::channel::<WsMessage>(params.outbound_capacity.max(1));

    let ws_out_handle = tokio::spawn(async move {
        while let Some(msg) = ws_out_rx.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // 1. Sync current device authorizations to relay
    {
        let reg = params.device_registry.read().await;
        let devices: Vec<agentdesk_relay::DeviceAuthInfo> = reg
            .list_devices()
            .into_iter()
            .map(|d| agentdesk_relay::DeviceAuthInfo {
                device_id: d.device_id.clone(),
                device_token: d.token.clone(),
                revoked: d.revoked,
            })
            .collect();

        let sync_msg = agentdesk_relay::RelayControlMessage {
            msg_type: "relay_control".into(),
            action: agentdesk_relay::RelayControlAction::SyncDevices { devices },
        };
        if let Ok(json) = serde_json::to_string(&sync_msg) {
            let _ = ws_out_tx.send(WsMessage::Text(json.into())).await;
        }
    }

    // 2. Sync active pairing session if one exists
    if let Some(session) = crate::pairing::load_session_file(&params.config_dir) {
        if !session.used && session.expires_at > params.clock.now() {
            let sync_pairing = agentdesk_relay::RelayControlMessage {
                msg_type: "relay_control".into(),
                action: agentdesk_relay::RelayControlAction::SyncPairing {
                    session: Some(agentdesk_relay::PairingAuthInfo {
                        session_id: session.session_id,
                        pairing_secret: session.pairing_secret,
                        expires_at: session.expires_at.timestamp(),
                    }),
                },
            };
            if let Ok(json) = serde_json::to_string(&sync_pairing) {
                let _ = ws_out_tx.send(WsMessage::Text(json.into())).await;
            }
        }
    }

    // 3. Register sink with core task so core broadcast messages reach the relay
    let (core_sink_tx, mut core_sink_rx) = mpsc::channel::<Message>(params.outbound_capacity.max(1));
    let ws_out_tx_core = ws_out_tx.clone();
    let core_pump_handle = tokio::spawn(async move {
        while let Some(msg) = core_sink_rx.recv().await {
            if let Ok(text) = serde_json::to_string(&msg) {
                if ws_out_tx_core.send(WsMessage::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    let slow_notify = Arc::new(tokio::sync::Notify::new());
    let is_slow = Arc::new(AtomicBool::new(false));
    let sink = ServerConnectionSink::new(core_sink_tx, slow_notify, is_slow);

    if params
        .core_sender
        .send(CoreCommand::Connect {
            client_id: params.client_id,
            sink: Box::new(sink),
        })
        .await
        .is_err()
    {
        logging::info("Core task inbox closed while registering relay trunk");
        ws_out_handle.abort();
        core_pump_handle.abort();
        return;
    }

    // 4. Inbound loop: handle frames from relay (from any remote client)
    while let Some(msg_res) = ws_receiver.next().await {
        let msg = match msg_res {
            Ok(m) => m,
            Err(e) => {
                logging::info(format!("Relay trunk read error: {}", e));
                break;
            }
        };

        if msg.is_close() {
            logging::info("Relay trunk closed by remote server");
            break;
        }
        if msg.is_ping() {
            let _ = ws_out_tx.send(WsMessage::Pong(msg.into_data().into())).await;
            continue;
        }
        if msg.is_pong() {
            continue;
        }

        let text = match msg {
            WsMessage::Text(t) => t,
            _ => continue,
        };

        let parsed_msg: Message = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                logging::info(format!("Failed parsing relay message: {}", e));
                continue;
            }
        };

        match parsed_msg.body {
            Body::PairRequest(pair_req) => {
                if pair_req.schema_version != SCHEMA_VERSION {
                    let err = Message::push(Body::Error(ErrorReply {
                        code: "schema_mismatch".into(),
                        message: format!(
                            "schema version mismatch: client {}, server {}",
                            pair_req.schema_version, SCHEMA_VERSION
                        ),
                    }));
                    if let Ok(t) = serde_json::to_string(&err) {
                        let _ = ws_out_tx.send(WsMessage::Text(t.into())).await;
                    }
                    continue;
                }

                match params.pairing_manager.validate_and_consume(
                    &pair_req.session_id,
                    &pair_req.pairing_secret,
                    params.clock.now(),
                ) {
                    Ok(session) => {
                        let (dev_token, dev_id, dev_name) = {
                            let mut reg = params.device_registry.write().await;
                            let tok = reg.register_device(
                                pair_req.device_id.clone(),
                                pair_req.device_name.clone(),
                            );
                            let _ = reg.save(&params.config_dir);
                            (tok, pair_req.device_id, pair_req.device_name)
                        };
                        params.pairing_manager.record_paired_device(
                            &pair_req.session_id,
                            &dev_id,
                            &dev_name,
                        );

                        // Authorize device on relay
                        let auth_msg = agentdesk_relay::RelayControlMessage {
                            msg_type: "relay_control".into(),
                            action: agentdesk_relay::RelayControlAction::AuthorizeDevice {
                                device_id: dev_id.clone(),
                                device_token: dev_token.clone(),
                            },
                        };
                        if let Ok(json) = serde_json::to_string(&auth_msg) {
                            let _ = ws_out_tx.send(WsMessage::Text(json.into())).await;
                        }

                        logging::info(format!(
                            "Successfully paired device '{}' ({}) via relay",
                            dev_id, dev_name
                        ));

                        let resp = Message::push(Body::PairResponse(PairResponse {
                            instance_id: session.instance_id,
                            device_token: dev_token,
                            schema_version: SCHEMA_VERSION,
                        }));
                        if let Ok(t) = serde_json::to_string(&resp) {
                            let _ = ws_out_tx.send(WsMessage::Text(t.into())).await;
                        }
                    }
                    Err(e) => {
                        logging::info(format!(
                            "Pairing rejected via relay for session {}: {}",
                            pair_req.session_id, e
                        ));
                        let err = Message::push(Body::Error(ErrorReply {
                            code: "unauthorized".into(),
                            message: format!("pairing rejected: {}", e),
                        }));
                        if let Ok(t) = serde_json::to_string(&err) {
                            let _ = ws_out_tx.send(WsMessage::Text(t.into())).await;
                        }
                    }
                }
            }
            Body::Hello(h) => {
                let master_ok = verify_token(&h.token, &params.expected_token);
                let device_ok = if !master_ok {
                    let reg = params.device_registry.read().await;
                    reg.is_authorized(&h.device_id, &h.token)
                } else {
                    true
                };

                if !device_ok {
                    logging::info(format!("Unauthorized client '{}' via relay", h.device_id));
                    let err = Message::push(Body::Error(ErrorReply {
                        code: "unauthorized".into(),
                        message: "invalid authentication token or device revoked".into(),
                    }));
                    if let Ok(t) = serde_json::to_string(&err) {
                        let _ = ws_out_tx.send(WsMessage::Text(t.into())).await;
                    }
                    continue;
                }

                if h.schema_version != SCHEMA_VERSION {
                    let err = Message::push(Body::Error(ErrorReply {
                        code: "schema_mismatch".into(),
                        message: format!(
                            "schema version mismatch: client {}, server {}",
                            h.schema_version, SCHEMA_VERSION
                        ),
                    }));
                    if let Ok(t) = serde_json::to_string(&err) {
                        let _ = ws_out_tx.send(WsMessage::Text(t.into())).await;
                    }
                    continue;
                }

                logging::info(format!(
                    "Authenticated client '{}' (client_version: {}) via relay",
                    h.device_id, h.client_version
                ));

                let dev_name = {
                    let reg = params.device_registry.read().await;
                    reg.devices
                        .get(&h.device_id)
                        .map(|d| d.device_name.clone())
                        .unwrap_or_else(|| h.device_id.clone())
                };
                params.status_tracker.register_client(
                    params.client_id,
                    h.device_id.clone(),
                    dev_name,
                    "Relay",
                );

                // Send Welcome
                let welcome = Message::push(Body::Welcome(Welcome {
                    daemon_version: env!("CARGO_PKG_VERSION").to_string(),
                    schema_version: SCHEMA_VERSION,
                    pipeline_mode: params.pipeline_mode,
                    transport: TransportMode::Relay,
                    server_time: params.clock.now(),
                }));
                if let Ok(t) = serde_json::to_string(&welcome) {
                    let _ = ws_out_tx.send(WsMessage::Text(t.into())).await;
                }

                // Immediately trigger snapshot
                let _ = params
                    .core_sender
                    .send(CoreCommand::SendSnapshot {
                        client_id: params.client_id,
                    })
                    .await;
            }
            _ => {
                // Forward inbound messages (e.g. InputResponse, GetMetrics, GetSnapshot) to core task
                let _ = params
                    .core_sender
                    .send(CoreCommand::Client {
                        client_id: params.client_id,
                        message: parsed_msg,
                    })
                    .await;
            }
        }
    }

    // 5. Cleanup when relay trunk disconnects
    params.status_tracker.unregister_client(params.client_id);
    let _ = params
        .core_sender
        .send(CoreCommand::Disconnect {
            client_id: params.client_id,
        })
        .await;

    ws_out_handle.abort();
    core_pump_handle.abort();
    logging::info("Relay trunk connection closed");
}

