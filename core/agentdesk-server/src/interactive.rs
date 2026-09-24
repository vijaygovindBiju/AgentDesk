use crate::config::{
    ServiceConfig, load_service_config, port_available, service_action, service_env_path,
    service_is_active, update_service_config, validate_port,
};
use crate::device::DeviceRegistry;
use crate::pairing::{
    PAIRING_DEFAULT_DURATION_SECS, PairingManager, load_session_file,
    render_qr_terminal,
};
use crate::tls::TlsIdentity;
use crate::token::default_config_dir;
use std::io::{self, BufRead, Write};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = default_config_dir();
    let env_path = service_env_path(&config_dir);
    let mut config = load_service_config(&env_path)?;

    loop {
        print_header(&config);
        println!("\nWhat do you want to do?");
        println!("  1) Pair Phone");
        println!("  2) Start AgentDesk");
        println!("  3) Configure AgentDesk");
        println!("  4) Status");
        println!("  5) Restart");
        println!("  6) Stop");
        println!("  7) Paired Devices");
        println!("  8) Exit");
        match prompt("Select an option: ")?.as_str() {
            "1" => {
                pair_flow(&config_dir, &config, None)?;
            }
            "2" => {
                config = start_flow(&env_path, config)?;
            }
            "3" => {
                configure_flow(&env_path, &mut config)?;
            }
            "4" => show_status()?,
            "5" => {
                update_service_config(&env_path, &config)?;
                if !service_action("restart")? {
                    eprintln!("Failed to restart AgentDesk.");
                } else {
                    println!("AgentDesk restarted on port {}.", config.port);
                }
            }
            "6" => {
                if !service_action("stop")? {
                    eprintln!("Failed to stop AgentDesk.");
                } else {
                    println!("AgentDesk stopped.");
                }
            }
            "7" => {
                devices_flow(&config_dir)?;
            }
            "8" => return Ok(()),
            _ => println!("Please choose a menu number."),
        }
        println!();
    }
}

pub fn pair_flow(
    config_dir: &std::path::Path,
    config: &ServiceConfig,
    relay_url: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let active = service_is_active().unwrap_or(false);
    let is_listening = !port_available(&config.bind, config.port);
    if !active && !is_listening {
        println!("AgentDesk service is not currently running. Starting it now...");
        let _ = service_action("start");
        std::thread::sleep(std::time::Duration::from_millis(1500));
        if !service_is_active().unwrap_or(false) && port_available(&config.bind, config.port) {
            println!("Note: If running standalone, ensure 'agentdesk run' is active in another terminal.");
        }
    }

    let reg = DeviceRegistry::load_or_create(config_dir)?;
    let instance_id = reg.instance_id.clone();
    let tls_id = TlsIdentity::load_or_generate(config_dir).map_err(|e| io::Error::other(e.to_string()))?;
    let fingerprint = tls_id.fingerprint.clone();

    let resolved_relay = relay_url
        .or_else(|| config.relay_url.clone())
        .or_else(|| std::env::var("AGENTDESK_RELAY_URL").ok())
        .or_else(|| std::env::var("AGENTDESK_RELAY").ok())
        .unwrap_or_else(|| crate::config::DEFAULT_RELAY_URL.to_string());

    let pairing_manager = PairingManager::with_dir(config_dir.to_path_buf());
    let (session, _payload, uri) = pairing_manager.create_session(
        instance_id.clone(),
        fingerprint.clone(),
        Some(resolved_relay.clone()),
        Some(PAIRING_DEFAULT_DURATION_SECS),
        chrono::Utc::now(),
    );

    let qr = render_qr_terminal(&uri).map_err(|e| io::Error::other(e.to_string()))?;
    println!("\n╭──────────────────────────────────────────────────────────────╮");
    println!("│                    Pair AgentDesk Phone                      │");
    println!("╰──────────────────────────────────────────────────────────────╯\n");
    println!("Scan this QR code with the AgentDesk Android app:\n");
    println!("{}", qr);
    println!("  Instance:    {}", instance_id);
    println!("  Relay:       {}", resolved_relay);
    println!("  Expires in:  3 minutes");
    println!("\nWaiting for phone to scan QR code...\n");

    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(PAIRING_DEFAULT_DURATION_SECS as u64);

    while start.elapsed() < timeout {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Some(s) = load_session_file(config_dir) {
            if s.session_id == session.session_id && s.used {
                let name = s.paired_device_name.as_deref().unwrap_or("Android phone");
                let id = s.paired_device_id.as_deref().unwrap_or("unknown");
                println!("✓ Phone successfully paired!");
                println!("  Device Name: {}", name);
                println!("  Device ID:   {}", id);
                println!("  Connection:  Remote Relay ({})", resolved_relay);
                return Ok(());
            }
        }
    }

    println!("✗ Pairing session expired after 3 minutes. Please try again.");
    pairing_manager.clear();
    Ok(())
}

pub fn devices_flow(config_dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut reg = DeviceRegistry::load_or_create(config_dir)?;
    loop {
        println!("\n╭──────────────────────────────────────────────╮");
        println!("│                Paired Devices                │");
        println!("╰──────────────────────────────────────────────╯\n");
        let devices = reg.list_devices();
        if devices.is_empty() {
            println!("  No paired devices found.");
            println!("  Use 'Pair Phone' to pair your mobile device.\n");
            println!("  1) Back");
            let _ = prompt("Select an option: ")?;
            return Ok(());
        }

        for (idx, dev) in devices.iter().enumerate() {
            let status = if dev.revoked { "REVOKED" } else { "ACTIVE" };
            println!(
                "  {}) {} [{}] (ID: {}, Paired: {})",
                idx + 1,
                dev.device_name,
                status,
                dev.device_id,
                dev.paired_at.format("%Y-%m-%d %H:%M:%S UTC")
            );
        }

        println!("\nOptions:");
        println!("  1) Revoke a device");
        println!("  2) Back");
        match prompt("Select an option: ")?.as_str() {
            "1" => {
                let num_str = prompt("Enter device number to revoke: ")?;
                if let Ok(num) = num_str.parse::<usize>() {
                    if num >= 1 && num <= devices.len() {
                        let dev_id = devices[num - 1].device_id.clone();
                        let dev_name = devices[num - 1].device_name.clone();
                        reg.revoke_device(&dev_id);
                        reg.save(config_dir)?;
                        println!("✓ Revoked device: {}", dev_name);
                    } else {
                        println!("Invalid device number.");
                    }
                } else {
                    println!("Invalid input.");
                }
            }
            "2" => return Ok(()),
            _ => println!("Please choose a menu number."),
        }
    }
}

fn print_header(config: &ServiceConfig) {
    println!("\n╭──────────────────────────────────────────────╮");
    println!("│                 AgentDesk                    │");
    println!("╰──────────────────────────────────────────────╯");
    println!("\nCurrent configuration:");
    println!(
        "  Relay: {}",
        config.relay_url.as_deref().unwrap_or(crate::config::DEFAULT_RELAY_URL)
    );
    println!("  Local Port: {}", config.port);
    println!("  Transport: Outbound WSS / Relay Only");
    println!(
        "  Agent: {}",
        if config.antigravity {
            config.agent.as_deref().unwrap_or("Antigravity")
        } else {
            config.agent.as_deref().unwrap_or("Simulator")
        }
    );
}

fn start_flow(
    env_path: &std::path::Path,
    mut config: ServiceConfig,
) -> Result<ServiceConfig, Box<dyn std::error::Error>> {
    loop {
        println!("\nWhich port should AgentDesk use?");
        println!("  1) Use current port ({})", config.port);
        println!("  2) 8080");
        println!("  3) 3000");
        println!("  4) Custom port");
        println!("  5) Cancel");
        let selection = prompt("Select a port: ")?;
        let selected = match selection.as_str() {
            "1" => config.port,
            "2" => 8080,
            "3" => 3000,
            "4" => match prompt("Enter port: ")?.parse::<u16>() {
                Ok(port) if validate_port(port).is_ok() => port,
                _ => {
                    println!("Invalid port. Enter a number between 1 and 65535.");
                    continue;
                }
            },
            "5" => return Ok(config),
            _ => {
                println!("Please choose a menu number.");
                continue;
            }
        };

        let already_configured = selected == config.port;
        let active = service_is_active()?;
        if (!already_configured || !active) && !port_available(&config.bind, selected) {
            println!("✗ Port {} is already in use.", selected);
            println!("  1) Choose another port");
            println!("  2) Cancel");
            if prompt("Select an option: ")? == "2" {
                return Ok(config);
            }
            continue;
        }

        config.port = selected;
        update_service_config(env_path, &config)?;
        let action = if active { "restart" } else { "start" };
        if !service_action(action)? {
            eprintln!("Failed to {} AgentDesk service.", action);
        } else {
            println!("✓ AgentDesk is running on port {}.", selected);
        }
        return Ok(config);
    }
}

fn configure_flow(
    env_path: &std::path::Path,
    config: &mut ServiceConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nWhat do you want to configure?");
    println!("  1) Port");
    println!("  2) Relay URL");
    println!("  3) Back");
    match prompt("Select an option: ")?.as_str() {
        "1" => {
            let value = prompt("Enter port: ")?;
            let port = value.parse::<u16>().map_err(|_| "invalid port")?;
            validate_port(port).map_err(|e| e.to_string())?;
            config.port = port;
            update_service_config(env_path, config)?;
            println!("Port saved. Restart AgentDesk to apply it.");
        }
        "2" => {
            let relay = prompt("Enter relay URL (e.g. wss://relay.agentdesk.io): ")?;
            if relay.trim().is_empty() || relay.contains(['\n', '\r']) {
                return Err("invalid relay URL".into());
            }
            config.relay_url = Some(relay.trim().to_string());
            update_service_config(env_path, config)?;
            println!("Relay URL saved. Restart AgentDesk to apply it.");
        }
        "3" => {}
        _ => println!("Please choose a menu number."),
    }
    Ok(())
}

fn show_status() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new("systemctl")
        .args(["--user", "status", "agentdesk.service", "--no-pager"])
        .output()?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    Ok(())
}

fn prompt(message: &str) -> io::Result<String> {
    print!("{message}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    Ok(input.trim().to_string())
}
