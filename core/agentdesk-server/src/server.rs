//! WebSocket server listener and connection acceptance.
//! See docs/SECURITY.md and P6.1, P6.6, P8.1, P8.2.

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;

use agentdesk_core::{Clock, CoreCommand};
use agentdesk_model::{PipelineMode, TransportMode};

use crate::connection::{
    ConnectionParams, DEFAULT_OUTBOUND_CAPACITY, handle_connection, handle_relay_trunk_connection,
};
use crate::device::DeviceRegistry;
use crate::logging;
use crate::pairing::PairingManager;
use crate::status::StatusTracker;
use crate::tls::TlsIdentity;
use crate::token::{default_config_dir, validate_bind_security};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_addr: String,
    pub port: u16,
    pub token: String,
    pub insecure_dev: bool,
    pub pipeline_mode: PipelineMode,
    pub outbound_capacity: usize,
    pub debug_logging: bool,
    pub config_dir: Option<PathBuf>,
    pub relay_url: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1".into(),
            port: 8765,
            token: String::new(),
            insecure_dev: false,
            pipeline_mode: PipelineMode::Agentdesk,
            outbound_capacity: DEFAULT_OUTBOUND_CAPACITY,
            debug_logging: false,
            config_dir: None,
            relay_url: Some(crate::config::DEFAULT_RELAY_URL.to_string()),
        }
    }
}

impl ServerConfig {
    pub fn validate(&self) -> Result<(), String> {
        validate_bind_security(&self.bind_addr, &self.token, self.insecure_dev)
    }

    pub fn transport_mode(&self) -> TransportMode {
        if self.insecure_dev {
            TransportMode::InsecureDev
        } else {
            TransportMode::Tls
        }
    }
}

pub struct Server {
    listener: TcpListener,
    local_addr: SocketAddr,
    config: ServerConfig,
    core_sender: mpsc::Sender<CoreCommand>,
    clock: Arc<dyn Clock>,
    next_client_id: Arc<AtomicU64>,
    shutdown_notify: Arc<tokio::sync::Notify>,
    tls_identity: Option<Arc<TlsIdentity>>,
    tls_acceptor: Option<TlsAcceptor>,
    device_registry: Arc<tokio::sync::RwLock<DeviceRegistry>>,
    pairing_manager: PairingManager,
    config_dir: PathBuf,
    status_tracker: Arc<StatusTracker>,
}

impl Server {
    /// Bind the listener and prepare the server.
    pub async fn bind(
        config: ServerConfig,
        core_sender: mpsc::Sender<CoreCommand>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, io::Error> {
        if let Err(e) = config.validate() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, e));
        }

        let config_dir = config.config_dir.clone().unwrap_or_else(default_config_dir);
        let device_registry = Arc::new(tokio::sync::RwLock::new(
            DeviceRegistry::load_or_create(&config_dir).map_err(|e| {
                io::Error::other(format!("Failed loading/creating device registry: {e}"))
            })?,
        ));
        let pairing_manager = PairingManager::with_dir(config_dir.clone());

        let (tls_identity, tls_acceptor) = if config.insecure_dev {
            eprintln!(
                "===================================================================\n\
                 * WARNING: --insecure-dev IS ENABLED!                             *\n\
                 * WebSocket transport is unencrypted plain ws://.                 *\n\
                 * Strictly bound to 127.0.0.1 loopback only.                      *\n\
                 * Intended only for local loopback development testing.           *\n\
                 ==================================================================="
            );
            (None, None)
        } else {
            let identity = TlsIdentity::load_or_generate(&config_dir).map_err(|e| {
                io::Error::other(format!("Failed loading/generating TLS identity: {e}"))
            })?;
            let acceptor = identity
                .build_tls_acceptor()
                .map_err(|e| io::Error::other(format!("Failed building TLS acceptor: {e}")))?;
            (Some(Arc::new(identity)), Some(acceptor))
        };

        let bind_target = format!("{}:{}", config.bind_addr, config.port);
        let listener = TcpListener::bind(&bind_target).await?;
        let local_addr = listener.local_addr()?;

        let scheme = if config.insecure_dev { "ws" } else { "wss" };
        logging::info(format!(
            "AgentDesk server listening on {}://{} ({:?})",
            scheme,
            local_addr,
            config.transport_mode()
        ));

        let status_tracker = Arc::new(StatusTracker::new(
            config_dir.clone(),
            config.bind_addr.clone(),
            config.port,
        ));

        Ok(Self {
            listener,
            local_addr,
            config,
            core_sender,
            clock,
            next_client_id: Arc::new(AtomicU64::new(1)),
            shutdown_notify: Arc::new(tokio::sync::Notify::new()),
            tls_identity,
            tls_acceptor,
            device_registry,
            pairing_manager,
            config_dir,
            status_tracker,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn status_tracker(&self) -> Arc<StatusTracker> {
        self.status_tracker.clone()
    }

    pub fn tls_identity(&self) -> Option<&TlsIdentity> {
        self.tls_identity.as_deref()
    }

    pub fn fingerprint(&self) -> Option<&str> {
        self.tls_identity.as_ref().map(|id| id.fingerprint.as_str())
    }

    pub fn device_registry(&self) -> Arc<tokio::sync::RwLock<DeviceRegistry>> {
        self.device_registry.clone()
    }

    pub fn pairing_manager(&self) -> &PairingManager {
        &self.pairing_manager
    }

    pub fn config_dir(&self) -> &std::path::Path {
        &self.config_dir
    }

    pub fn shutdown(&self) {
        self.shutdown_notify.notify_waiters();
    }

    /// Run the server accept loop until shutdown is signaled.
    pub async fn run(&self) -> io::Result<()> {
        let expected_token = Arc::new(self.config.token.clone());
        let transport_mode = self.config.transport_mode();
        if let Some(ref relay_base) = self.config.relay_url {
            let relay_base = relay_base.clone();
            let instance_id = {
                let reg = self.device_registry.read().await;
                reg.instance_id.clone()
            };
            let expected_token_relay = expected_token.clone();
            let transport_mode_relay = TransportMode::Relay;
            let clock_relay = self.clock.clone();
            let core_sender_relay = self.core_sender.clone();
            let device_registry_relay = self.device_registry.clone();
            let pairing_manager_relay = self.pairing_manager.clone();
            let status_tracker_relay = self.status_tracker.clone();
            let config_dir_relay = self.config_dir.clone();
            let shutdown_notify_relay = self.shutdown_notify.clone();
            let next_client_id_relay = self.next_client_id.clone();
            let outbound_cap = self.config.outbound_capacity;
            let debug_log = self.config.debug_logging;
            let pipeline_mode = self.config.pipeline_mode;

            tokio::spawn(async move {
                let mut backoff = std::time::Duration::from_millis(50);
                loop {
                    let relay_target = format!(
                        "{}/ws?role=host&inst={}&token={}",
                        relay_base.trim_end_matches('/'),
                        urlencoding::encode(&instance_id),
                        urlencoding::encode(&expected_token_relay)
                    );

                    tokio::select! {
                        _ = shutdown_notify_relay.notified() => break,
                        connect_res = tokio_tungstenite::connect_async(&relay_target) => {
                            match connect_res {
                                Ok((ws_stream, _)) => {
                                    logging::info(format!("Connected outbound to relay {}", relay_base));
                                    backoff = std::time::Duration::from_millis(50);
                                    let client_id = next_client_id_relay.fetch_add(1, Ordering::SeqCst);
                                    let params = ConnectionParams {
                                        client_id,
                                        expected_token: expected_token_relay.clone(),
                                        pipeline_mode,
                                        transport_mode: transport_mode_relay,
                                        clock: clock_relay.clone(),
                                        core_sender: core_sender_relay.clone(),
                                        outbound_capacity: outbound_cap,
                                        debug_logging: debug_log,
                                        device_registry: device_registry_relay.clone(),
                                        pairing_manager: pairing_manager_relay.clone(),
                                        config_dir: config_dir_relay.clone(),
                                        status_tracker: status_tracker_relay.clone(),
                                    };
                                    handle_relay_trunk_connection(
                                        ws_stream,
                                        format!("relay:{}", relay_base),
                                        params,
                                        instance_id.clone(),
                                    )
                                    .await;
                                    logging::info("Relay session ended, reconnecting...");
                                }
                                Err(e) => {
                                    logging::info(format!("Relay connection failed to {}: {}", relay_base, e));
                                    backoff = std::cmp::min(backoff * 2, std::time::Duration::from_secs(30));
                                    tokio::select! {
                                        _ = shutdown_notify_relay.notified() => break,
                                        _ = tokio::time::sleep(backoff) => {}
                                    }
                                }
                            }
                        }
                    }
                }
            });
        }

        loop {
            tokio::select! {
                _ = self.shutdown_notify.notified() => {
                    logging::info("Server shutdown requested, closing listener");
                    break;
                }
                accept_result = self.listener.accept() => {
                    match accept_result {
                        Ok((stream, peer_addr)) => {
                            let peer_str = peer_addr.to_string();
                            let client_id = self.next_client_id.fetch_add(1, Ordering::SeqCst);
                            let params = ConnectionParams {
                                client_id,
                                expected_token: expected_token.clone(),
                                pipeline_mode: self.config.pipeline_mode,
                                transport_mode,
                                clock: self.clock.clone(),
                                core_sender: self.core_sender.clone(),
                                outbound_capacity: self.config.outbound_capacity,
                                debug_logging: self.config.debug_logging,
                                device_registry: self.device_registry.clone(),
                                pairing_manager: self.pairing_manager.clone(),
                                config_dir: self.config_dir.clone(),
                                status_tracker: self.status_tracker.clone(),
                            };
                            if let Some(acceptor) = &self.tls_acceptor {
                                let acceptor = acceptor.clone();
                                tokio::spawn(async move {
                                    match acceptor.accept(stream).await {
                                        Ok(tls_stream) => {
                                            handle_connection(tls_stream, peer_str, params).await;
                                        }
                                        Err(e) => {
                                            logging::info(format!("TLS handshake failed from {}: {}", peer_str, e));
                                        }
                                    }
                                });
                            } else {
                                tokio::spawn(handle_connection(stream, peer_str, params));
                            }
                        }
                        Err(e) => {
                            logging::info(format!("Error accepting socket connection: {}", e));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
