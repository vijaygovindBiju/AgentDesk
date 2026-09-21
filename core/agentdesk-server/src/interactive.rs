use crate::config::{
    ServiceConfig, load_service_config, port_available, service_action, service_env_path,
    service_is_active, update_service_config, validate_port,
};
use crate::token::default_config_dir;
use std::io::{self, BufRead, Write};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = default_config_dir();
    let env_path = service_env_path(&config_dir);
    let mut config = load_service_config(&env_path)?;

    loop {
        print_header(&config);
        println!("\nWhat do you want to do?");
        println!("  1) Start AgentDesk");
        println!("  2) Configure AgentDesk");
        println!("  3) Status");
        println!("  4) Restart");
        println!("  5) Stop");
        println!("  6) Exit");
        match prompt("Select an option: ")?.as_str() {
            "1" => {
                config = start_flow(&env_path, config)?;
            }
            "2" => {
                configure_flow(&env_path, &mut config)?;
            }
            "3" => show_status()?,
            "4" => {
                update_service_config(&env_path, &config)?;
                if !service_action("restart")? {
                    eprintln!("Failed to restart AgentDesk.");
                } else {
                    println!("AgentDesk restarted on port {}.", config.port);
                }
            }
            "5" => {
                if !service_action("stop")? {
                    eprintln!("Failed to stop AgentDesk.");
                } else {
                    println!("AgentDesk stopped.");
                }
            }
            "6" => return Ok(()),
            _ => println!("Please choose a menu number."),
        }
        println!();
    }
}

fn print_header(config: &ServiceConfig) {
    println!("\n╭──────────────────────────────────────────────╮");
    println!("│                 AgentDesk                    │");
    println!("╰──────────────────────────────────────────────╯");
    println!("\nCurrent configuration:");
    println!("  Address: {}", config.bind);
    println!("  Port: {}", config.port);
    println!("  Protocol: WSS");
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
    println!("  2) Bind address");
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
            let bind = prompt("Enter bind address: ")?;
            if bind.trim().is_empty() || bind.contains(['\n', '\r', '=']) {
                return Err("invalid bind address".into());
            }
            config.bind = bind;
            update_service_config(env_path, config)?;
            println!("Bind address saved. Restart AgentDesk to apply it.");
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
