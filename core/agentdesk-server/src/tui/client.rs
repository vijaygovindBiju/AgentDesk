//! Client interface between TUI event loop and AgentDesk Core / Server.
//! Supports both direct in-process CoreTask channel bridge and external WebSocket connection.

use std::path::{Path, PathBuf};

use tokio::sync::mpsc;

use agentdesk_core::{ClientId, CoreCommand, SinkError, TransportSink};
use agentdesk_model::{
    Body, Decision, EventId, EventLogs, EventPush, EventRef, GetEventLogs, Message,
    RawAgentEvent, RequestResponse, RespondRequest, ScoreUpdate, Snapshot, StateUpdate,
};

use crate::pairing::{PairingManager, render_qr_terminal};
use crate::tui::state::PairingMode;

#[derive(Debug)]
pub enum TuiEvent {
    Snapshot(Snapshot),
    Event(EventPush),
    ScoreUpdate(ScoreUpdate),
    StateUpdate(StateUpdate),
    Logs(EventLogs),
    PhoneConnected(bool, Option<String>, Option<String>), // connected, transport, device_name
    PairingSessionCreated {
        session_id: String,
        uri: String,
        qr: String,
        instance_id: String,
        expires_at: chrono::DateTime<chrono::Utc>,
    },
    PairingComplete {
        device_name: String,
        device_id: String,
    },
    PairingFailed(String),
    PromptReceived(String),
    AdapterRespond {
        event_id: EventId,
        decision: Decision,
        response: Option<RequestResponse>,
    },
}

/// Outbound transport sink for in-process TUI client
pub struct TuiConnectionSink {
    sender: mpsc::Sender<TuiEvent>,
}

impl TuiConnectionSink {
    pub fn new(sender: mpsc::Sender<TuiEvent>) -> Self {
        Self { sender }
    }
}

impl TransportSink for TuiConnectionSink {
    fn send(&mut self, message: &Message) -> Result<usize, SinkError> {
        let event = match &message.body {
            Body::Snapshot(s) => TuiEvent::Snapshot(s.clone()),
            Body::Event(e) => TuiEvent::Event(e.clone()),
            Body::ScoreUpdate(s) => TuiEvent::ScoreUpdate(s.clone()),
            Body::StateUpdate(s) => TuiEvent::StateUpdate(s.clone()),
            Body::EventLogs(l) => TuiEvent::Logs(l.clone()),
            Body::StartTask(t) => TuiEvent::PromptReceived(t.prompt.clone()),
            Body::RespondRequest(r) => TuiEvent::AdapterRespond {
                event_id: r.event_id,
                decision: r.decision,
                response: Some(RequestResponse::from_respond_request(r)),
            },
            _ => return Ok(0),
        };

        match self.sender.try_send(event) {
            Ok(()) => Ok(1),
            Err(mpsc::error::TrySendError::Full(_)) => Err(SinkError::ChannelFull),
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

/// Abstract connection handle for the TUI to talk to the core runtime
#[derive(Clone)]
pub enum TuiBridge {
    InProcess {
        core_sender: mpsc::Sender<CoreCommand>,
        client_id: ClientId,
        config_dir: PathBuf,
    },
    WebSocket {
        outbound_tx: mpsc::Sender<Message>,
        config_dir: PathBuf,
    },
}

impl TuiBridge {
    pub async fn send_response(&self, req: RespondRequest) -> Result<(), String> {
        match self {
            TuiBridge::InProcess {
                core_sender,
                client_id,
                ..
            } => {
                let msg = Message::push(Body::RespondRequest(req));
                core_sender
                    .send(CoreCommand::Client {
                        client_id: *client_id,
                        message: msg,
                    })
                    .await
                    .map_err(|e| format!("Failed sending response to core: {e}"))
            }
            TuiBridge::WebSocket { outbound_tx, .. } => {
                let msg = Message::push(Body::RespondRequest(req));
                outbound_tx
                    .send(msg)
                    .await
                    .map_err(|e| format!("Failed sending response to websocket: {e}"))
            }
        }
    }

    pub async fn get_logs(&self, event_id: EventId, offset: i64, limit: u32) -> Result<(), String> {
        let msg = Message::push(Body::GetEventLogs(GetEventLogs {
            event_id,
            offset,
            limit,
        }));
        match self {
            TuiBridge::InProcess {
                core_sender,
                client_id,
                ..
            } => core_sender
                .send(CoreCommand::Client {
                    client_id: *client_id,
                    message: msg,
                })
                .await
                .map_err(|e| format!("Failed requesting logs: {e}")),
            TuiBridge::WebSocket { outbound_tx, .. } => outbound_tx
                .send(msg)
                .await
                .map_err(|e| format!("Failed requesting logs: {e}")),
        }
    }

    pub async fn dismiss(&self, event_id: EventId) -> Result<(), String> {
        let msg = Message::push(Body::Dismiss(EventRef { event_id }));
        match self {
            TuiBridge::InProcess {
                core_sender,
                client_id,
                ..
            } => core_sender
                .send(CoreCommand::Client {
                    client_id: *client_id,
                    message: msg,
                })
                .await
                .map_err(|e| format!("Failed dismissing event: {e}")),
            TuiBridge::WebSocket { outbound_tx, .. } => outbound_tx
                .send(msg)
                .await
                .map_err(|e| format!("Failed dismissing event: {e}")),
        }
    }

    pub async fn send_adapter_event(&self, event: RawAgentEvent) -> Result<(), String> {
        match self {
            TuiBridge::InProcess { core_sender, .. } => {
                core_sender
                    .send(CoreCommand::Adapter(agentdesk_core::AdapterOutput::Event(event)))
                    .await
                    .map_err(|e| format!("Failed sending adapter event: {e}"))
            }
            TuiBridge::WebSocket { outbound_tx, .. } => {
                let msg = Message::push(Body::RawEvent(event));
                outbound_tx
                    .send(msg)
                    .await
                    .map_err(|e| format!("Failed sending adapter event to websocket: {e}"))
            }
        }
    }

    pub async fn send_adapter_response(&self, outcome: agentdesk_core::AdapterOutcome) -> Result<(), String> {
        match self {
            TuiBridge::InProcess { core_sender, .. } => {
                core_sender
                    .send(CoreCommand::AdapterResponse(outcome))
                    .await
                    .map_err(|e| format!("Failed sending adapter response: {e}"))
            }
            TuiBridge::WebSocket { outbound_tx, .. } => {
                let msg = Message::push(Body::AdapterOutcome(outcome));
                outbound_tx
                    .send(msg)
                    .await
                    .map_err(|e| format!("Failed sending adapter outcome to websocket: {e}"))
            }
        }
    }

    pub fn generate_pairing_session(
        &self,
        _mode: PairingMode,
        config_dir: &Path,
        _port: u16,
        relay_url: Option<String>,
        fingerprint: Option<String>,
    ) -> Result<(String, String, String, String, chrono::DateTime<chrono::Utc>, String), String> {
        let reg = crate::device::DeviceRegistry::load_or_create(config_dir)
            .map_err(|e| format!("Failed loading device registry: {e}"))?;
        let instance_id = reg.instance_id.clone();
        let fp = fingerprint.unwrap_or_else(|| "none".into());

        let resolved_relay = crate::tunnel::resolve_or_ensure_relay_url(config_dir, relay_url);

        let pairing_manager = PairingManager::with_dir(config_dir.to_path_buf());
        let (session, _payload, uri) = pairing_manager.create_session(
            instance_id.clone(),
            fp,
            Some(resolved_relay.clone()),
            Some(crate::pairing::PAIRING_DEFAULT_DURATION_SECS),
            chrono::Utc::now(),
        );

        let qr = render_qr_terminal(&uri).map_err(|e| format!("QR rendering failed: {e}"))?;

        Ok((session.session_id, uri, qr, instance_id, session.expires_at, resolved_relay))
    }
}
