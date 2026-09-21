use std::env;
use agentdesk_relay::{DEFAULT_RELAY_PORT, RelayConfig, RelayServer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = env::args().collect();
    let mut config = RelayConfig::default();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--bind" => {
                if i + 1 < args.len() {
                    config.bind_addr = args[i + 1].clone();
                    i += 2;
                } else {
                    eprintln!("Error: --bind requires an IP address");
                    std::process::exit(1);
                }
            }
            "--port" => {
                if i + 1 < args.len() {
                    config.port = args[i + 1].parse().unwrap_or(DEFAULT_RELAY_PORT);
                    i += 2;
                } else {
                    eprintln!("Error: --port requires a port number");
                    std::process::exit(1);
                }
            }
            "--token" => {
                if i + 1 < args.len() {
                    config.auth_token = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("Error: --token requires a token string");
                    std::process::exit(1);
                }
            }
            "-h" | "--help" => {
                println!(
                    "AgentDesk Relay Server\n\n\
                     USAGE:\n\
                         agentdesk-relay [OPTIONS]\n\n\
                     OPTIONS:\n\
                         --bind <addr>    Bind address (default: 0.0.0.0)\n\
                         --port <port>    Bind port (default: 8766)\n\
                         --token <token>  Optional host authentication token\n\
                         -h, --help       Show help\n"
                );
                return Ok(());
            }
            other => {
                eprintln!("Unknown argument: {}", other);
                std::process::exit(1);
            }
        }
    }

    let server = RelayServer::new(config);
    server.run().await
}
