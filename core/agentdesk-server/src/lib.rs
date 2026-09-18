//! AgentDesk transport server crate.
//! Exposes CoreTask over WebSocket with token authentication, request/reply dispatch,
//! slow-client protection, and insecure-dev mode.
//! See docs/TODO.md Phase 6 and docs/COMMUNICATION.md.

pub mod config;
pub mod connection;
pub mod logging;
pub mod server;
pub mod token;

pub use config::{parse_args, print_help, CliCommand, RunOptions, TokenOptions};
pub use connection::{
    handle_connection, ConnectionParams, ServerConnectionSink, DEFAULT_OUTBOUND_CAPACITY,
};
pub use logging::{current_log_level, debug, info, set_log_capture, set_log_level, LogCapture, LogLevel};
pub use server::{Server, ServerConfig};
pub use token::{
    default_config_dir, generate_token, is_loopback_addr, load_or_generate_token, rotate_token,
    save_token, token_path, validate_bind_security, verify_token,
};
