//! WebSocket server listener and connection acceptance.
//! See docs/SECURITY.md and P6.1, P6.6.

use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::mpsc;

use agentdesk_core::{Clock, CoreCommand};
use agentdesk_model::{PipelineMode, TransportMode};

use crate::connection::{handle_connection, ConnectionParams, DEFAULT_OUTBOUND_CAPACITY};
use crate::logging;
use crate::token::validate_bind_security;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_addr: String,
    pub port: u16,
    pub token: String,
    pub insecure_dev: bool,
    pub pipeline_mode: PipelineMode,
    pub outbound_capacity: usize,
    pub debug_logging: bool,
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
    next_client_id: AtomicU64,
    shutdown_notify: Arc<tokio::sync::Notify>,
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

        if config.insecure_dev {
            eprintln!(
                "===================================================================\n\
                 * WARNING: --insecure-dev IS ENABLED!                             *\n\
                 * WebSocket transport is unencrypted plain ws://.                 *\n\
                 * Strictly bound to 127.0.0.1 loopback only.                      *\n\
                 * Intended only for emulator / adb reverse development testing.   *\n\
                 ==================================================================="
            );
        }

        let bind_target = format!("{}:{}", config.bind_addr, config.port);
        let listener = TcpListener::bind(&bind_target).await?;
        let local_addr = listener.local_addr()?;

        logging::info(format!(
            "AgentDesk server listening on ws://{} ({:?})",
            local_addr,
            config.transport_mode()
        ));

        Ok(Self {
            listener,
            local_addr,
            config,
            core_sender,
            clock,
            next_client_id: AtomicU64::new(1),
            shutdown_notify: Arc::new(tokio::sync::Notify::new()),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn shutdown(&self) {
        self.shutdown_notify.notify_waiters();
    }

    /// Run the server accept loop until shutdown is signaled.
    pub async fn run(&self) -> io::Result<()> {
        let expected_token = Arc::new(self.config.token.clone());
        let transport_mode = self.config.transport_mode();

        loop {
            tokio::select! {
                _ = self.shutdown_notify.notified() => {
                    logging::info("Server shutdown requested, closing listener");
                    break;
                }
                accept_result = self.listener.accept() => {
                    match accept_result {
                        Ok((stream, _peer_addr)) => {
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
                            };
                            tokio::spawn(handle_connection(stream, params));
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
