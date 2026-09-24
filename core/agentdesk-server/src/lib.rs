//! AgentDesk transport server crate.
//! Exposes CoreTask over WebSocket with token authentication, request/reply dispatch,
//! slow-client protection, and insecure-dev mode.
//! See docs/TODO.md Phase 6 and docs/COMMUNICATION.md.

pub mod config;
pub mod connection;
pub mod device;
pub mod discovery;
pub mod interactive;
pub mod logging;
pub mod pairing;
pub mod server;
pub mod status;
pub mod tls;
pub mod token;
pub mod tui;
pub mod tunnel;

pub use config::{
    CliCommand, DeviceOptions, DeviceRevokeOptions, PairOptions, RunOptions, ServiceConfig,
    TokenOptions, DEFAULT_RELAY_URL, load_service_config, parse_args, print_help, service_env_path,
    update_service_config,
};
pub use connection::{
    ConnectionParams, DEFAULT_OUTBOUND_CAPACITY, ServerConnectionSink, handle_connection,
    handle_ws_connection,
};
pub use device::{DeviceRegistry, PairedDevice, devices_path, generate_instance_id, instance_id_path};
pub use discovery::{AdapterType, AgentAvailability, AgentDefinition, AgentDiscovery};
pub use logging::{
    LogCapture, LogLevel, current_log_level, debug, info, set_log_capture, set_log_level,
};
pub use pairing::{
    PairingError, PairingManager, PairingPayload, PairingSession,
    PAIRING_DEFAULT_DURATION_SECS, PAIRING_PROTOCOL_VERSION,
    load_session_file, pairing_session_path, remove_session_file,
    render_qr_terminal, save_session_file,
};
pub use server::{Server, ServerConfig};
pub use status::{
    RuntimeStatus, load_runtime_status, remove_runtime_status, runtime_status_path,
    save_runtime_status,
};
pub use tls::{
    TlsIdentity, cert_path, compute_sha256_fingerprint, format_fingerprint_hex, key_path,
};
pub use token::{
    default_config_dir, generate_token, is_loopback_addr, load_or_generate_token, rotate_token,
    save_token, token_path, validate_bind_security, verify_token,
};
pub use tui::run_tui;
