//! AgentDesk transport server crate.
//! Exposes CoreTask over WebSocket with token authentication, request/reply dispatch,
//! slow-client protection, and insecure-dev mode.
//! See docs/TODO.md Phase 6 and docs/COMMUNICATION.md.

pub mod config;
pub mod connection;
pub mod logging;
pub mod server;
pub mod tls;
pub mod token;

pub use config::{CliCommand, RunOptions, TokenOptions, parse_args, print_help};
pub use connection::{
    ConnectionParams, DEFAULT_OUTBOUND_CAPACITY, ServerConnectionSink, handle_connection,
};
pub use logging::{
    LogCapture, LogLevel, current_log_level, debug, info, set_log_capture, set_log_level,
};
pub use server::{Server, ServerConfig};
pub use tls::{
    TlsIdentity, cert_path, compute_sha256_fingerprint, format_fingerprint_hex, key_path,
};
pub use token::{
    default_config_dir, generate_token, is_loopback_addr, load_or_generate_token, rotate_token,
    save_token, token_path, validate_bind_security, verify_token,
};
