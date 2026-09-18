//! Per-connection WebSocket handling and transport sink.
//! See docs/COMMUNICATION.md and P6.1 - P6.4.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::frame::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

use agentdesk_core::{ClientId, Clock, CoreCommand, SinkError, TransportSink};
use agentdesk_model::close_code;
use agentdesk_model::{
    Body, ErrorReply, Message, PipelineMode, TransportMode, Welcome, SCHEMA_VERSION,
};

use crate::logging;
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
        let json_bytes =
            serde_json::to_vec(message).map_err(|e| SinkError::Io(e.to_string()))?;
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
}

/// Accept and manage a single client WebSocket connection lifecycle.
pub async fn handle_connection(stream: TcpStream, params: ConnectionParams) {
    let peer_addr = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "unknown".into());

    logging::info(format!(
        "Accepted connection from {} (client_id: {})",
        peer_addr, params.client_id
    ));

    let mut ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            logging::info(format!(
                "WebSocket handshake failed from {}: {}",
                peer_addr, e
            ));
            return;
        }
    };

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
            logging::info(format!("Connection closed by {} before handshake", peer_addr));
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

    // Parse Hello message
    let hello = match serde_json::from_str::<Message>(&text_content) {
        Ok(Message {
            body: Body::Hello(h),
            ..
        }) => h,
        _ => {
            logging::info(format!(
                "First frame from {} was not hello, closing 4001",
                peer_addr
            ));
            let _ = ws_stream
                .send(WsMessage::Close(Some(CloseFrame {
                    code: CloseCode::from(close_code::UNAUTHORIZED),
                    reason: "first frame must be hello".into(),
                })))
                .await;
            return;
        }
    };

    // Verify token in constant time
    if !verify_token(&hello.token, &params.expected_token) {
        logging::info(format!(
            "Authentication failed for {} (device: {}), closing 4001",
            peer_addr, hello.device_id
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
    if hello.schema_version != SCHEMA_VERSION {
        logging::info(format!(
            "Schema mismatch for {} (got {}, expected {}), closing 4002",
            peer_addr, hello.schema_version, SCHEMA_VERSION
        ));
        let _ = ws_stream
            .send(WsMessage::Close(Some(CloseFrame {
                code: CloseCode::from(close_code::SCHEMA_MISMATCH),
                reason: format!(
                    "schema version mismatch: client {}, server {}",
                    hello.schema_version, SCHEMA_VERSION
                )
                .into(),
            })))
            .await;
        return;
    }

    logging::info(format!(
        "Authenticated client {} (device: {}, client_version: {})",
        params.client_id, hello.device_id, hello.client_version
    ));

    // Handshake passed: establish bounded outbound channel & sink
    let (outbound_tx, mut outbound_rx) =
        mpsc::channel::<Message>(params.outbound_capacity.max(1));
    let slow_client_notify = Arc::new(tokio::sync::Notify::new());
    let is_slow = Arc::new(AtomicBool::new(false));

    let sink = ServerConnectionSink::new(
        outbound_tx,
        slow_client_notify.clone(),
        is_slow.clone(),
    );

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
    let _ = params
        .core_sender
        .send(CoreCommand::Disconnect {
            client_id: params.client_id,
        })
        .await;

    logging::info(format!("Client {} disconnected", params.client_id));
}
