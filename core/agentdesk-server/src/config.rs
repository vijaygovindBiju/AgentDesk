//! CLI argument parsing and configuration options.
//! See docs/TODO.md P6.5, P6.6, P6.7.

use agentdesk_model::PipelineMode;
use std::fs;
use std::io;
use std::net::TcpListener;
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Interactive,
    Pair(PairOptions),
    DevicesList(DeviceOptions),
    DeviceRevoke(DeviceRevokeOptions),
    DeviceRemove(DeviceRevokeOptions),
    Run(RunOptions),
    TokenShow(TokenOptions),
    TokenRotate(TokenOptions),
    RelayShow(DeviceOptions),
    RelaySet { url: String, config_dir: Option<PathBuf> },
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PairOptions {
    pub config_dir: Option<PathBuf>,
    pub relay: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceOptions {
    pub config_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRevokeOptions {
    pub device_id: String,
    pub config_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenOptions {
    pub config_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    pub scenario: Option<PathBuf>,
    pub seed: u64,
    pub mode: PipelineMode,
    pub insecure_dev: bool,
    pub port: u16,
    pub token: Option<String>,
    pub config_dir: Option<PathBuf>,
    pub debug: bool,
    pub agent: Option<String>,
    pub prompt: Option<String>,
    pub antigravity: bool,
    pub relay: Option<String>,
    pub workspace: Option<PathBuf>,
    pub dangerously_skip_permissions: bool,
}

pub const DEFAULT_RELAY_URL: &str = "wss://relay.agentdesk.io";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    pub bind: String,
    pub port: u16,
    pub agent: Option<String>,
    pub antigravity: bool,
    pub relay_url: Option<String>,
    /// Optional access token required by the relay operator (`agentdesk-relay --token`).
    /// This is NOT the laptop master token, which never leaves the laptop.
    pub relay_token: Option<String>,
}

/// The local daemon socket is loopback-only; it serves the on-laptop TUI and
/// is never reachable by phones. Phones connect exclusively via the relay.
pub const LOCAL_DAEMON_BIND: &str = "127.0.0.1";

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            bind: LOCAL_DAEMON_BIND.into(),
            port: 8765,
            agent: None,
            antigravity: false,
            relay_url: None,
            relay_token: None,
        }
    }
}

fn env_nonempty(keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|k| std::env::var(k).ok())
        .map(|v| v.trim().to_string())
        .find(|v| !v.is_empty())
}

/// Validate a relay endpoint. Production relays must be `wss://` on a public
/// host. Plain `ws://` is accepted only for a loopback development relay.
/// Private network addresses are rejected: remote connections must traverse the public relay.
pub fn validate_relay_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| format!("relay URL '{url}' must start with wss://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = if let Some(stripped) = authority.strip_prefix('[') {
        stripped.split(']').next().unwrap_or_default()
    } else {
        authority.rsplit_once(':').map(|(h, _)| h).unwrap_or(authority)
    };
    if host.is_empty() {
        return Err(format!("relay URL '{url}' has no host"));
    }
    let loopback = crate::token::is_loopback_addr(host);
    match scheme {
        "wss" => {}
        "ws" if loopback => return Ok(()),
        "ws" => return Err("plain ws:// relays are allowed only on loopback for development; use wss://".into()),
        other => return Err(format!("unsupported relay scheme '{other}'; use wss://")),
    }
    if loopback {
        return Ok(());
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let private = match ip {
            std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_unspecified(),
            std::net::IpAddr::V6(v6) => {
                v6.is_unspecified() || (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80
            }
        };
        if private {
            return Err(format!(
                "relay host '{host}' is a private network address; phones on mobile data cannot reach it. Use a public relay."
            ));
        }
    }
    if host.ends_with(".local") || host.ends_with(".internal") || host.ends_with(".lan") {
        return Err(format!("relay host '{host}' is an internal network name; use a public relay."));
    }
    Ok(())
}

pub fn service_env_path(config_dir: &std::path::Path) -> PathBuf {
    config_dir.join("service.env")
}

pub fn load_service_config(path: &std::path::Path) -> io::Result<ServiceConfig> {
    if !path.exists() {
        let mut config = ServiceConfig::default();
        config.relay_url = env_nonempty(&["AGENTDESK_RELAY_URL", "AGENTDESK_RELAY"]);
        config.relay_token = env_nonempty(&["AGENTDESK_RELAY_TOKEN"]);
        return Ok(config);
    }
    let content = fs::read_to_string(path)?;
    let mut config = ServiceConfig::default();
    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "AGENTDESK_PORT" => {
                config.port = value.parse().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "AGENTDESK_PORT is not a valid port",
                    )
                })?;
            }
            "AGENTDESK_AGENT" if !value.is_empty() => config.agent = Some(value.to_string()),
            "AGENTDESK_ANTIGRAVITY" => config.antigravity = value == "1",
            "AGENTDESK_RELAY" | "AGENTDESK_RELAY_URL" if !value.is_empty() => {
                config.relay_url = Some(value.to_string())
            }
            "AGENTDESK_RELAY_TOKEN" if !value.is_empty() => {
                config.relay_token = Some(value.to_string())
            }
            _ => {}
        }
    }
    if config.relay_url.is_none() {
        config.relay_url = env_nonempty(&["AGENTDESK_RELAY_URL", "AGENTDESK_RELAY"]);
    }
    if config.relay_token.is_none() {
        config.relay_token = env_nonempty(&["AGENTDESK_RELAY_TOKEN"]);
    }
    validate_port(config.port).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(config)
}

pub fn update_service_config(path: &std::path::Path, config: &ServiceConfig) -> io::Result<()> {
    validate_port(config.port).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    if let Some(ref url) = config.relay_url {
        if url.contains(['\n', '\r']) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid relay URL"));
        }
    }
    let original = fs::read_to_string(path).unwrap_or_default();
    let replacements = [
        ("AGENTDESK_PORT", config.port.to_string()),
        (
            "AGENTDESK_ANTIGRAVITY",
            if config.antigravity {
                "1".into()
            } else {
                "0".into()
            },
        ),
        (
            "AGENTDESK_RELAY",
            config.relay_url.clone().unwrap_or_default(),
        ),
    ];
    let mut seen = [false; 3];
    let mut output = Vec::new();
    for line in original.lines() {
        // Obsolete: the daemon socket is loopback-only; drop legacy bind overrides.
        if line.starts_with("AGENTDESK_BIND=") {
            continue;
        }
        let mut replaced = false;
        for (index, (key, value)) in replacements.iter().enumerate() {
            if line.starts_with(&format!("{key}=")) {
                output.push(format!("{key}={value}"));
                seen[index] = true;
                replaced = true;
                break;
            }
        }
        if !replaced {
            output.push(line.to_string());
        }
    }
    for (index, (key, value)) in replacements.iter().enumerate() {
        if !seen[index] {
            output.push(format!("{key}={value}"));
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("env.tmp");
    fs::write(&tmp, format!("{}\n", output.join("\n")))?;
    fs::rename(tmp, path)?;
    Ok(())
}

pub fn validate_port(port: u16) -> Result<(), String> {
    if port == 0 {
        Err("port must be between 1 and 65535".into())
    } else {
        Ok(())
    }
}

pub fn port_available(bind: &str, port: u16) -> bool {
    let target = if bind.contains(':') && !bind.starts_with('[') {
        format!("[{bind}]:{port}")
    } else {
        format!("{bind}:{port}")
    };
    target
        .to_socket_addrs()
        .ok()
        .map(|addresses| {
            addresses
                .into_iter()
                .any(|addr| TcpListener::bind(addr).is_ok())
        })
        .unwrap_or(false)
}

pub fn service_action(action: &str) -> io::Result<bool> {
    let status = Command::new("systemctl")
        .args(["--user", action, "agentdesk.service"])
        .status()?;
    Ok(status.success())
}

pub fn service_is_active() -> io::Result<bool> {
    let status = Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "agentdesk.service"])
        .status()?;
    Ok(status.success())
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            scenario: None,
            seed: 42,
            mode: PipelineMode::Agentdesk,
            insecure_dev: false,
            port: 8765,
            token: None,
            config_dir: None,
            debug: false,
            agent: None,
            prompt: None,
            antigravity: false,
            relay: None,
            workspace: None,
            dangerously_skip_permissions: false,
        }
    }
}

pub fn parse_args<I, T>(args: I) -> Result<CliCommand, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    if args.len() <= 1 {
        return Ok(CliCommand::Interactive);
    }

    let first = &args[1];
    if first == "-h" || first == "--help" || first == "help" {
        return Ok(CliCommand::Help);
    }

    if first == "token" {
        if args.len() < 3 {
            return Err("Usage: agentdesk token <show|rotate> [--config-dir <path>]".into());
        }
        let sub = &args[2];
        let mut config_dir = None;
        let mut idx = 3;
        while idx < args.len() {
            if args[idx] == "--config-dir" && idx + 1 < args.len() {
                config_dir = Some(PathBuf::from(&args[idx + 1]));
                idx += 2;
            } else {
                return Err(format!("Unknown option for token: '{}'", args[idx]));
            }
        }

        let opts = TokenOptions { config_dir };
        match sub.as_str() {
            "show" => return Ok(CliCommand::TokenShow(opts)),
            "rotate" => return Ok(CliCommand::TokenRotate(opts)),
            other => {
                return Err(format!(
                    "Unknown token command: '{}'. Expected 'show' or 'rotate'.",
                    other
                ));
            }
        }
    }

    if first == "pair" {
        let mut opts = PairOptions::default();
        let mut idx = 2;
        while idx < args.len() {
            match args[idx].as_str() {
                "--config-dir" => {
                    if idx + 1 >= args.len() {
                        return Err("--config-dir requires a path".into());
                    }
                    opts.config_dir = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                }
                "--relay" => {
                    if idx + 1 >= args.len() {
                        return Err("--relay requires a URL".into());
                    }
                    opts.relay = Some(args[idx + 1].clone());
                    idx += 2;
                }
                "-h" | "--help" => return Ok(CliCommand::Help),
                other => return Err(format!("Unknown option for pair: '{}'", other)),
            }
        }
        return Ok(CliCommand::Pair(opts));
    }

    if first == "devices" {
        if args.len() < 3 {
            return Err("Usage: agentdesk devices <list|revoke <device_id>|remove <device_id>> [--config-dir <path>]".into());
        }
        let sub = &args[2];
        match sub.as_str() {
            "list" => {
                let mut config_dir = None;
                let mut idx = 3;
                while idx < args.len() {
                    if args[idx] == "--config-dir" && idx + 1 < args.len() {
                        config_dir = Some(PathBuf::from(&args[idx + 1]));
                        idx += 2;
                    } else {
                        return Err(format!("Unknown option for devices list: '{}'", args[idx]));
                    }
                }
                return Ok(CliCommand::DevicesList(DeviceOptions { config_dir }));
            }
            "revoke" => {
                if args.len() < 4 {
                    return Err("Usage: agentdesk devices revoke <device_id> [--config-dir <path>]".into());
                }
                let device_id = args[3].clone();
                let mut config_dir = None;
                let mut idx = 4;
                while idx < args.len() {
                    if args[idx] == "--config-dir" && idx + 1 < args.len() {
                        config_dir = Some(PathBuf::from(&args[idx + 1]));
                        idx += 2;
                    } else {
                        return Err(format!("Unknown option for devices revoke: '{}'", args[idx]));
                    }
                }
                return Ok(CliCommand::DeviceRevoke(DeviceRevokeOptions {
                    device_id,
                    config_dir,
                }));
            }
            "remove" => {
                if args.len() < 4 {
                    return Err("Usage: agentdesk devices remove <device_id> [--config-dir <path>]".into());
                }
                let device_id = args[3].clone();
                let mut config_dir = None;
                let mut idx = 4;
                while idx < args.len() {
                    if args[idx] == "--config-dir" && idx + 1 < args.len() {
                        config_dir = Some(PathBuf::from(&args[idx + 1]));
                        idx += 2;
                    } else {
                        return Err(format!("Unknown option for devices remove: '{}'", args[idx]));
                    }
                }
                return Ok(CliCommand::DeviceRemove(DeviceRevokeOptions {
                    device_id,
                    config_dir,
                }));
            }
            other => {
                return Err(format!(
                    "Unknown devices command: '{}'. Expected 'list', 'revoke', or 'remove'.",
                    other
                ));
            }
        }
    }

    if first == "relay" {
        let usage = "Usage: agentdesk relay <show|set <wss://relay.example.com>> [--config-dir <path>]";
        let sub = args.get(2).ok_or(usage)?;
        let (url, rest_start) = match sub.as_str() {
            "show" => (None, 3),
            "set" => (Some(args.get(3).ok_or(usage)?.clone()), 4),
            _ => return Err(usage.into()),
        };
        let mut config_dir = None;
        let mut idx = rest_start;
        while idx < args.len() {
            if args[idx] == "--config-dir" && idx + 1 < args.len() {
                config_dir = Some(PathBuf::from(&args[idx + 1]));
                idx += 2;
            } else {
                return Err(format!("Unknown option for relay: '{}'", args[idx]));
            }
        }
        return Ok(match url {
            None => CliCommand::RelayShow(DeviceOptions { config_dir }),
            Some(url) => {
                validate_relay_url(&url)?;
                CliCommand::RelaySet { url: url.trim().to_string(), config_dir }
            }
        });
    }

    if first == "run" {
        let mut opts = RunOptions::default();
        let mut idx = 2;
        while idx < args.len() {
            match args[idx].as_str() {
                "--scenario" => {
                    if idx + 1 >= args.len() {
                        return Err("--scenario requires a file path".into());
                    }
                    opts.scenario = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                }
                "--seed" => {
                    if idx + 1 >= args.len() {
                        return Err("--seed requires a numeric value".into());
                    }
                    opts.seed = args[idx + 1]
                        .parse()
                        .map_err(|_| format!("Invalid seed: '{}'", args[idx + 1]))?;
                    idx += 2;
                }
                "--mode" => {
                    if idx + 1 >= args.len() {
                        return Err("--mode requires raw_lines, raw_events, or agentdesk".into());
                    }
                    opts.mode = match args[idx + 1].as_str() {
                        "raw_lines" => PipelineMode::RawLines,
                        "raw_events" => PipelineMode::RawEvents,
                        "agentdesk" => PipelineMode::Agentdesk,
                        other => return Err(format!("Unknown mode '{}'", other)),
                    };
                    idx += 2;
                }
                "--insecure-dev" => {
                    opts.insecure_dev = true;
                    idx += 1;
                }
                "--bind" | "--no-lan" | "--lan" | "--usb" => {
                    return Err(format!(
                        "'{}' was removed: AgentDesk is relay-only. The laptop only makes an outbound WSS connection to the configured relay (see `agentdesk relay`).",
                        args[idx]
                    ));
                }
                "--port" => {
                    if idx + 1 >= args.len() {
                        return Err("--port requires a port number".into());
                    }
                    opts.port = args[idx + 1]
                        .parse()
                        .map_err(|_| format!("Invalid port: '{}'", args[idx + 1]))?;
                    idx += 2;
                }
                "--token" => {
                    if idx + 1 >= args.len() {
                        return Err("--token requires a token string".into());
                    }
                    opts.token = Some(args[idx + 1].clone());
                    idx += 2;
                }
                "--config-dir" => {
                    if idx + 1 >= args.len() {
                        return Err("--config-dir requires a path".into());
                    }
                    opts.config_dir = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                }
                "--debug" => {
                    opts.debug = true;
                    idx += 1;
                }
                "--agent" => {
                    if idx + 1 >= args.len() {
                        return Err("--agent requires a command string".into());
                    }
                    opts.agent = Some(args[idx + 1].clone());
                    idx += 2;
                }
                "--prompt" => {
                    if idx + 1 >= args.len() {
                        return Err("--prompt requires a prompt string".into());
                    }
                    opts.prompt = Some(args[idx + 1].clone());
                    idx += 2;
                }
                "--antigravity" => {
                    opts.antigravity = true;
                    idx += 1;
                }
                "--relay" => {
                    if idx + 1 >= args.len() {
                        return Err("--relay requires a URL".into());
                    }
                    opts.relay = Some(args[idx + 1].clone());
                    idx += 2;
                }
                "--dir" | "--workspace" | "--cwd" => {
                    if idx + 1 >= args.len() {
                        return Err("--dir/--workspace requires a path".into());
                    }
                    opts.workspace = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                }
                "--dangerously-skip-permissions"
                | "--dangerskip-permission"
                | "--dangerskip-permissions"
                | "--danger-skip-permissions" => {
                    opts.dangerously_skip_permissions = true;
                    idx += 1;
                }
                "-h" | "--help" => return Ok(CliCommand::Help),
                other => return Err(format!("Unknown argument: '{}'", other)),
            }
        }
        return Ok(CliCommand::Run(opts));
    }

    Err(format!(
        "Unknown command: '{}'. Expected 'pair', 'devices', 'relay', 'run', or 'token'.",
        first
    ))
}

pub fn print_help() {
    eprintln!(
        "AgentDesk Daemon & CLI\n\n\
         USAGE:\n\
             agentdesk                              Interactive control & pairing\n\
             agentdesk pair [OPTIONS]               Generate pairing QR code for phone\n\
             agentdesk devices list [OPTIONS]       List paired devices\n\
             agentdesk devices revoke <id> [OPTS]   Revoke a paired device\n\
             agentdesk devices remove <id> [OPTS]   Remove a paired device from registry\n\
             agentdesk relay show [OPTS]            Show the configured relay endpoint\n\
             agentdesk relay set <wss-url> [OPTS]   Set the relay endpoint (relay_url)\n\
             agentdesk run [OPTIONS]                Run daemon directly (headless)\n\
             agentdesk token <show|rotate> [OPTS]   Show or rotate master token\n\n\
         CONNECTIVITY:\n\
             Relay-only. The laptop and phone each open an outbound WSS connection to\n\
             the configured relay; they never connect to each other directly and may\n\
             be on completely different networks (e.g. coffee shop Wi-Fi and 4G/5G).\n\
             No direct peer-to-peer connection, USB/ADB, port forwarding or inbound firewall rule is needed.\n\
             Configure with `agentdesk relay set wss://relay.example.com` or the\n\
             AGENTDESK_RELAY_URL environment variable.\n\n\
         PAIR OPTIONS:\n\
             --config-dir <path>      Override configuration directory\n\
             --relay <url>            Override relay URL\n\n\
         RUN OPTIONS:\n\
             --scenario <path>        Path to simulator scenario JSON (default: standard scenario)\n\
             --seed <u64>             Deterministic random seed (default: 42)\n\
             --mode <mode>            Pipeline mode: raw_lines | raw_events | agentdesk (default: agentdesk)\n\
             --insecure-dev           Plain ws:// on the loopback-only local socket (dev only)\n\
             --port <u16>             Local loopback TUI port (default: 8765; never exposed)\n\
             --relay <url>            Override relay URL for this run\n\
             --token <str>            Override authentication token\n\
             --config-dir <path>      Override configuration directory for token storage\n\
             --debug                  Enable payload debug logging (opt-in)\n\
             --agent <cmd>            Run real agent via ACP over stdio (e.g. \"gemini --skip-trust --acp\")\n\
             --antigravity            Run real Antigravity via PTY adapter\n\
             --prompt <text>          Prompt to send to the real agent\n\
             -h, --help               Print help information\n"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_help_command() {
        assert_eq!(
            parse_args(["agentdesk", "--help"]).unwrap(),
            CliCommand::Help
        );
        assert_eq!(parse_args(["agentdesk"]).unwrap(), CliCommand::Interactive);
    }

    #[test]
    fn parse_token_commands() {
        assert_eq!(
            parse_args(["agentdesk", "token", "show"]).unwrap(),
            CliCommand::TokenShow(TokenOptions { config_dir: None })
        );
        assert_eq!(
            parse_args(["agentdesk", "token", "rotate", "--config-dir", "/tmp/cfg"]).unwrap(),
            CliCommand::TokenRotate(TokenOptions {
                config_dir: Some(PathBuf::from("/tmp/cfg"))
            })
        );
    }

    #[test]
    fn parse_pair_commands() {
        assert_eq!(
            parse_args(["agentdesk", "pair"]).unwrap(),
            CliCommand::Pair(PairOptions {
                config_dir: None,
                relay: None,
            })
        );
        assert_eq!(
            parse_args([
                "agentdesk",
                "pair",
                "--config-dir",
                "/tmp/cfg",
                "--relay",
                "wss://relay.example.com",
            ])
            .unwrap(),
            CliCommand::Pair(PairOptions {
                config_dir: Some(PathBuf::from("/tmp/cfg")),
                relay: Some("wss://relay.example.com".into()),
            })
        );
    }

    #[test]
    fn parse_device_commands() {
        assert_eq!(
            parse_args(["agentdesk", "devices", "list"]).unwrap(),
            CliCommand::DevicesList(DeviceOptions { config_dir: None })
        );
        assert_eq!(
            parse_args([
                "agentdesk",
                "devices",
                "revoke",
                "dev-123",
                "--config-dir",
                "/tmp/cfg",
            ])
            .unwrap(),
            CliCommand::DeviceRevoke(DeviceRevokeOptions {
                device_id: "dev-123".into(),
                config_dir: Some(PathBuf::from("/tmp/cfg")),
            })
        );
        assert_eq!(
            parse_args([
                "agentdesk",
                "devices",
                "remove",
                "dev-123",
                "--config-dir",
                "/tmp/cfg",
            ])
            .unwrap(),
            CliCommand::DeviceRemove(DeviceRevokeOptions {
                device_id: "dev-123".into(),
                config_dir: Some(PathBuf::from("/tmp/cfg")),
            })
        );
    }

    #[test]
    fn parse_run_command_flags() {
        let cmd = parse_args([
            "agentdesk",
            "run",
            "--seed",
            "123",
            "--mode",
            "raw_events",
            "--insecure-dev",
            "--port",
            "9001",
            "--debug",
        ])
        .unwrap();

        match cmd {
            CliCommand::Run(opts) => {
                assert_eq!(opts.seed, 123);
                assert_eq!(opts.mode, PipelineMode::RawEvents);
                assert!(opts.insecure_dev);
                assert_eq!(opts.port, 9001);
                assert!(opts.debug);
            }
            _ => panic!("expected run command"),
        }
    }

    #[test]
    fn parse_run_with_agent() {
        let cmd = parse_args([
            "agentdesk",
            "run",
            "--agent",
            "gemini --skip-trust --acp",
            "--prompt",
            "hello world",
        ])
        .unwrap();

        match cmd {
            CliCommand::Run(opts) => {
                assert_eq!(opts.agent.as_deref(), Some("gemini --skip-trust --acp"));
                assert_eq!(opts.prompt.as_deref(), Some("hello world"));
            }
            _ => panic!("expected run command"),
        }
    }

    #[test]
    fn service_config_update_preserves_unrelated_values() {
        let path =
            std::env::temp_dir().join(format!("agentdesk_service_{}.env", std::process::id()));
        fs::write(
            &path,
            "# keep this\nAGENTDESK_BIND=127.0.0.1\nAGENTDESK_PORT=8765\nOTHER=value\n",
        )
        .unwrap();
        let mut config = load_service_config(&path).unwrap();
        config.port = 8080;
        update_service_config(&path, &config).unwrap();
        let saved = fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# keep this"));
        assert!(saved.contains("OTHER=value"));
        assert!(saved.contains("AGENTDESK_PORT=8080"));
        assert!(!saved.contains("AGENTDESK_BIND"), "legacy bind override must be dropped");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn direct_connectivity_flags_are_rejected() {
        for flag in ["--bind", "--no-lan", "--lan", "--usb"] {
            let err = parse_args(["agentdesk", "run", flag, "0.0.0.0"]).unwrap_err();
            assert!(err.contains("relay-only"), "{flag}: {err}");
        }
    }

    #[test]
    fn relay_url_validation() {
        assert!(validate_relay_url("wss://relay.example.com").is_ok());
        assert!(validate_relay_url("wss://relay.example.com:443/path").is_ok());
        assert!(validate_relay_url("wss://203.0.113.7:8766").is_ok());
        // Local development relay on loopback only
        assert!(validate_relay_url("ws://127.0.0.1:8766").is_ok());
        assert!(validate_relay_url("ws://localhost:8766").is_ok());
        // Rejected: plaintext public, LAN/private addresses, bad schemes
        assert!(validate_relay_url("ws://relay.example.com").is_err());
        assert!(validate_relay_url("wss://192.168.1.10:8766").is_err());
        assert!(validate_relay_url("wss://10.0.0.4").is_err());
        assert!(validate_relay_url("wss://172.16.3.2").is_err());
        assert!(validate_relay_url("wss://laptop.local").is_err());
        assert!(validate_relay_url("https://relay.example.com").is_err());
        assert!(validate_relay_url("relay.example.com").is_err());
    }

    #[test]
    fn parse_relay_commands() {
        assert_eq!(
            parse_args(["agentdesk", "relay", "show"]).unwrap(),
            CliCommand::RelayShow(DeviceOptions { config_dir: None })
        );
        assert_eq!(
            parse_args(["agentdesk", "relay", "set", "wss://relay.example.com"]).unwrap(),
            CliCommand::RelaySet { url: "wss://relay.example.com".into(), config_dir: None }
        );
        assert!(parse_args(["agentdesk", "relay", "set", "wss://192.168.1.2:8766"]).is_err());
    }

    #[test]
    fn invalid_ports_are_rejected() {
        assert!(validate_port(0).is_err());
        assert!(validate_port(1).is_ok());
        assert!(validate_port(u16::MAX).is_ok());
    }

    #[test]
    fn occupied_ports_are_not_reported_available() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!port_available("127.0.0.1", port));
    }
}
