//! AgentDesk Terminal User Interface (TUI)
//! Production-quality attention and control surface for interactive coding agents.
//! Built with Ratatui and Crossterm.

pub mod app;
pub mod client;
pub mod clipboard;
pub mod key_encoder;
pub mod preferences;
pub mod state;
pub mod terminal_session;
pub mod theme;
pub mod views;

pub use theme::Theme;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

use agentdesk_core::{
    AcpAdapter, AcpConfig, Adapter, AdapterCommand, AntigravityConfig,
    AntigravityPtyAdapter, Clock, CoreCommand, CoreTask, LogStoreConfig, SystemClock,
    ThresholdTable,
};
use agentdesk_model::{
    Body, Hello, Message, PipelineMode, RequestResponse, SCHEMA_VERSION,
};
use agentdesk_sim::{Scenario, Simulator};

use crate::config::{
    load_service_config, service_env_path,
};
use crate::server::{Server, ServerConfig};
use crate::tls::TlsIdentity;
use crate::token::{default_config_dir, load_or_generate_token, token_path};

use self::app::TuiApp;
use self::client::{TuiBridge, TuiConnectionSink, TuiEvent};
use self::state::TuiState;

/// Check if a daemon process is currently accepting connections on 127.0.0.1
fn is_daemon_listening(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(80),
    ).is_ok()
}

/// Main entry point to run the AgentDesk TUI.
pub async fn run_tui(custom_config_dir: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    crate::logging::set_suppress_stderr(true);
    let config_dir = custom_config_dir.unwrap_or_else(default_config_dir);
    let env_path = service_env_path(&config_dir);
    let service_config = load_service_config(&env_path).unwrap_or_default();
    let token_file = token_path(&config_dir);
    let token = load_or_generate_token(&token_file)?;

    let resolved_relay = crate::tunnel::resolve_or_ensure_relay_url(&config_dir, service_config.relay_url.clone());

    if is_daemon_listening(service_config.port) {
        // --- Option A: Attach to existing running daemon (e.g. systemd service) ---
        run_attached_websocket(config_dir, service_config.port, crate::config::LOCAL_DAEMON_BIND.into(), token, Some(resolved_relay)).await
    } else {
        // --- Option B: Launch in-process daemon with TUI attached directly ---
        let mut cfg = service_config;
        cfg.relay_url = Some(resolved_relay);
        run_in_process_daemon(config_dir, cfg, token).await
    }
}

/// Connect to loopback WebSocket daemon, trying TLS with pinned cert then plain ws
async fn connect_loopback_ws(
    port: u16,
    tls_id: Option<&TlsIdentity>,
) -> Result<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(id) = tls_id {
        let client_config = id.build_client_config();
        let wss_url = format!("wss://127.0.0.1:{}/ws", port);
        match tokio_tungstenite::connect_async_tls_with_config(
            &wss_url,
            None,
            false,
            Some(tokio_tungstenite::Connector::Rustls(client_config)),
        ).await {
            Ok((stream, _)) => return Ok(stream),
            Err(e) => {
                crate::logging::info(format!("Loopback TLS WebSocket connection to {} failed ({}), trying plain ws...", wss_url, e));
            }
        }
    }

    let ws_url = format!("ws://127.0.0.1:{}/ws", port);
    let (stream, _) = tokio_tungstenite::connect_async(&ws_url).await?;
    Ok(stream)
}

/// Attach to an existing daemon running on the local port over WebSocket
async fn run_attached_websocket(
    config_dir: PathBuf,
    port: u16,
    bind_addr: String,
    token: String,
    relay_url: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let tls_id = TlsIdentity::load_or_generate(&config_dir).ok();
    let fingerprint = tls_id.as_ref().map(|t| t.fingerprint.clone());

    // Connect to loopback WebSocket (with TLS support)
    let ws_stream = match connect_loopback_ws(port, tls_id.as_ref()).await {
        Ok(res) => res,
        Err(e) => {
            return Err(format!(
                "Daemon is listening on port {}, but failed to connect over WebSocket: {}. If another process is holding this port, terminate it.",
                port, e
            ).into());
        }
    };

    let (tx_events, rx_events) = mpsc::channel::<TuiEvent>(256);
    let (tx_outbound, mut rx_outbound) = mpsc::channel::<Message>(64);

    let tx_inbound = tx_events.clone();
    let token_clone = token.clone();
    let tls_id_clone = tls_id.clone();

    // Outbound & Inbound connection task with automatic reconnect
    tokio::spawn(async move {
        let mut initial_stream = Some(ws_stream);
        loop {
            let ws_stream = if let Some(s) = initial_stream.take() {
                s
            } else {
                match connect_loopback_ws(port, tls_id_clone.as_ref()).await {
                    Ok(stream) => stream,
                    Err(_) => {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                }
            };

            let (mut ws_write, mut ws_read) = ws_stream.split();

            // Send Hello handshake
            let hello = Message::push(Body::Hello(Hello {
                token: token_clone.clone(),
                device_id: "agentdesk-tui".into(),
                client_version: env!("CARGO_PKG_VERSION").into(),
                schema_version: SCHEMA_VERSION,
            }));
            if let Ok(hello_json) = serde_json::to_string(&hello) {
                if ws_write.send(WsMessage::Text(hello_json.into())).await.is_err() {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
            }

            loop {
                tokio::select! {
                    msg_res = ws_read.next() => {
                        match msg_res {
                            Some(Ok(WsMessage::Text(text))) => {
                                if let Ok(msg) = serde_json::from_str::<Message>(&text) {
                                    let tui_ev = match msg.body {
                                        Body::Snapshot(s) => Some(TuiEvent::Snapshot(s)),
                                        Body::Event(e) => Some(TuiEvent::Event(e)),
                                        Body::ScoreUpdate(s) => Some(TuiEvent::ScoreUpdate(s)),
                                        Body::StateUpdate(s) => Some(TuiEvent::StateUpdate(s)),
                                        Body::EventLogs(l) => Some(TuiEvent::Logs(l)),
                                        Body::StartTask(t) => Some(TuiEvent::PromptReceived {
                                            prompt: t.prompt,
                                            workspace: t.workspace,
                                            agent_id: t.agent_id,
                                            mode: t.mode,
                                        }),
                                        Body::RespondRequest(r) => Some(TuiEvent::AdapterRespond {
                                            event_id: r.event_id,
                                            decision: r.decision,
                                            response: Some(RequestResponse::from_respond_request(&r)),
                                        }),
                                        _ => None,
                                    };
                                    if let Some(ev) = tui_ev {
                                        let _ = tx_inbound.send(ev).await;
                                    }
                                }
                            }
                            Some(Ok(WsMessage::Close(_))) | Some(Err(_)) | None => break,
                            _ => {}
                        }
                    }
                    out_msg = rx_outbound.recv() => {
                        match out_msg {
                            Some(msg) => {
                                if let Ok(text) = serde_json::to_string(&msg) {
                                    if ws_write.send(WsMessage::Text(text.into())).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            None => return, // TUI exited and dropped outbound channel
                        }
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });

    let bridge = TuiBridge::WebSocket {
        outbound_tx: tx_outbound,
        config_dir: config_dir.clone(),
    };

    let mut state = TuiState::new(port, bind_addr, relay_url, fingerprint);
    state.is_attached_mode = true;
    let detected = crate::discovery::AgentDiscovery::discover();
    let prefs = crate::tui::preferences::load_preferences(&config_dir);
    state.init_workspace_selection(detected, prefs);

    let app = TuiApp::new(state, bridge, rx_events, config_dir);
    app.run().await
}

/// Run daemon in-process and attach TUI directly via channels
async fn run_in_process_daemon(
    config_dir: PathBuf,
    service_config: crate::config::ServiceConfig,
    token: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let clock = Arc::new(SystemClock);
    let start_time = clock.now();

    // Adapter setup (Antigravity PTY, ACP, or deterministic simulator)
    let mut adapter: Box<dyn Adapter> = if service_config.antigravity {
        let agent_cmd = service_config.agent.as_deref().unwrap_or("agy");
        let parts: Vec<String> = agent_cmd
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        let command = parts.first().cloned().unwrap_or_else(|| "agy".into());
        let args = if parts.len() > 1 { parts[1..].to_vec() } else { Vec::new() };

        let agy_config = AntigravityConfig {
            command,
            args,
            cwd: None,
            agent_id: "antigravity".into(),
            agent_name: "Antigravity".into(),
            project: "AgentDesk".into(),
            initial_prompt: None,
            cols: 120,
            rows: 40,
        };
        match AntigravityPtyAdapter::spawn(agy_config) {
            Ok(a) => Box::new(a),
            Err(_) => Box::new(Simulator::new(Scenario::empty(), 42, start_time)),
        }
    } else if let Some(ref agent_cmd) = service_config.agent {
        let parts: Vec<String> = agent_cmd
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        if !parts.is_empty() {
            let command = parts[0].clone();
            let args = parts[1..].to_vec();
            let acp_config = AcpConfig {
                command,
                args,
                cwd: None,
                agent_id: "acp-agent".into(),
                agent_name: "ACP Agent".into(),
                project: "AgentDesk".into(),
                initial_prompt: "Explain this project".into(),
            };
            match AcpAdapter::spawn(acp_config) {
                Ok(a) => Box::new(a),
                Err(_) => Box::new(Simulator::new(Scenario::empty(), 42, start_time)),
            }
        } else {
            Box::new(Simulator::new(Scenario::empty(), 42, start_time))
        }
    } else {
        Box::new(Simulator::new(Scenario::empty(), 42, start_time))
    };

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
    let mut all_agents = adapter.agents().to_vec();
    if !all_agents.iter().any(|a| a.agent_id == "antigravity") {
        all_agents.push(agentdesk_model::AgentInfo {
            agent_id: "antigravity".into(),
            name: "Antigravity CLI".into(),
            project: "AgentDesk".into(),
            adapter_kind: agentdesk_model::AdapterKind::AntigravityPty,
        });
    }
    core.register_agents(&all_agents);

    let core_sender = tx_core.clone();
    tokio::spawn(core.run(rx_core));

    let server_config = ServerConfig {
        bind_addr: crate::config::LOCAL_DAEMON_BIND.into(),
        port: service_config.port,
        token: token.clone(),
        insecure_dev: false,
        pipeline_mode: PipelineMode::Agentdesk,
        outbound_capacity: crate::connection::DEFAULT_OUTBOUND_CAPACITY,
        debug_logging: false,
        config_dir: Some(config_dir.clone()),
        relay_url: service_config.relay_url.clone(),
    };

    let server = match Server::bind(server_config, core_sender.clone(), clock.clone()).await {
        Ok(s) => s,
        Err(e) => {
            return Err(format!(
                "Failed to bind AgentDesk server on 127.0.0.1:{}: {}. Terminate any existing AgentDesk process first.",
                service_config.port, e
            ).into());
        }
    };
    let fingerprint = server.fingerprint().map(|f| f.to_string());

    // Spawn loopback accept loop (local TUI attach) and the outbound relay trunk
    tokio::spawn(async move {
        let _ = server.run().await;
    });

    // Attach TUI via in-process sink
    let (tx_events, rx_events) = mpsc::channel::<TuiEvent>(256);
    let tui_events_tx = tx_events.clone();
    let sink = Box::new(TuiConnectionSink::new(tx_events));
    let tui_client_id = 9999;

    let _ = core_sender.send(CoreCommand::Connect {
        client_id: tui_client_id,
        sink,
    }).await;

    let _ = core_sender.send(CoreCommand::SendSnapshot {
        client_id: tui_client_id,
    }).await;

    // Spawn ticker
    let tick_sender = core_sender.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            if tick_sender.send(CoreCommand::Tick).await.is_err() {
                break;
            }
        }
    });

    // Adapter driving loop
    let adapter_sender = core_sender.clone();
    tokio::spawn(async move {
        loop {
            let now = chrono::Utc::now();
            let outputs = adapter.poll(now);
            for out in outputs {
                if adapter_sender.send(CoreCommand::Adapter(out)).await.is_err() {
                    return;
                }
            }

            tokio::select! {
                adapter_cmd = rx_adapter.recv() => {
                    match adapter_cmd {
                        Some(AdapterCommand::Respond { event_id, decision, response, .. }) => {
                            // Forward response to TUI so it can inject keystrokes into the visible terminal shell
                            let _ = tui_events_tx.send(TuiEvent::AdapterRespond {
                                event_id,
                                decision,
                                response,
                            }).await;
                        }
                        Some(AdapterCommand::StartTask { prompt, workspace, agent_id, mode, .. }) => {
                            let _ = tui_events_tx.send(TuiEvent::PromptReceived {
                                prompt,
                                workspace,
                                agent_id,
                                mode,
                            }).await;
                        }
                        None => {}
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {}
            }
        }
    });

    let _ = core_sender.send(CoreCommand::SendSnapshot {
        client_id: tui_client_id,
    }).await;

    let bridge = TuiBridge::InProcess {
        core_sender: core_sender.clone(),
        client_id: tui_client_id,
        config_dir: config_dir.clone(),
    };

    let mut state = TuiState::new(
        service_config.port,
        crate::config::LOCAL_DAEMON_BIND.into(),
        service_config.relay_url,
        fingerprint,
    );
    let detected = crate::discovery::AgentDiscovery::discover();
    let prefs = crate::tui::preferences::load_preferences(&config_dir);
    state.init_workspace_selection(detected, prefs);

    let app = TuiApp::new(state, bridge, rx_events, config_dir);
    app.run().await
}
