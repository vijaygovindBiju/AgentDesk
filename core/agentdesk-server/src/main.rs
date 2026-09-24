//! Main executable for the AgentDesk daemon.
//! See docs/TODO.md Phase 6 and docs/COMMUNICATION.md.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use agentdesk_core::{
    AcpAdapter, AcpConfig, Adapter, AdapterCommand, AdapterOutcome, AntigravityConfig,
    AntigravityPtyAdapter, Clock, CoreCommand, CoreTask, LogStoreConfig, SystemClock,
    ThresholdTable,
};
use agentdesk_server::{
    CliCommand, DeviceOptions, DeviceRegistry, DeviceRevokeOptions, LogLevel, PairOptions,
    RunOptions, Server, ServerConfig, TokenOptions, default_config_dir, load_or_generate_token,
    load_service_config, parse_args, print_help, rotate_token, service_env_path, set_log_level,
    token_path,
};
use agentdesk_sim::{Scenario, Simulator};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let command = match parse_args(args) {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("Error: {}", e);
            eprintln!();
            print_help();
            std::process::exit(1);
        }
    };

    match command {
        CliCommand::Interactive => {
            agentdesk_server::run_tui(None).await?;
            Ok(())
        }
        CliCommand::Pair(opts) => {
            handle_pair(opts)?;
            Ok(())
        }
        CliCommand::DevicesList(opts) => {
            handle_devices_list(opts)?;
            Ok(())
        }
        CliCommand::DeviceRevoke(opts) => {
            handle_device_revoke(opts)?;
            Ok(())
        }
        CliCommand::DeviceRemove(opts) => {
            handle_device_remove(opts)?;
            Ok(())
        }
        CliCommand::Help => {
            print_help();
            Ok(())
        }
        CliCommand::TokenShow(opts) => {
            handle_token_show(opts)?;
            Ok(())
        }
        CliCommand::TokenRotate(opts) => {
            handle_token_rotate(opts)?;
            Ok(())
        }
        CliCommand::Run(opts) => {
            handle_run(opts).await?;
            Ok(())
        }
    }
}

fn handle_pair(opts: PairOptions) -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = opts.config_dir.unwrap_or_else(default_config_dir);
    let env_path = service_env_path(&config_dir);
    let config = load_service_config(&env_path)?;
    let relay = opts.relay.or(config.relay_url.clone());
    agentdesk_server::interactive::pair_flow(&config_dir, &config, relay)?;
    Ok(())
}

fn handle_devices_list(opts: DeviceOptions) -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = opts.config_dir.unwrap_or_else(default_config_dir);
    let reg = DeviceRegistry::load_or_create(&config_dir)?;
    let devices = reg.list_devices();
    if devices.is_empty() {
        println!("No paired devices found.");
        return Ok(());
    }
    println!("{:<20} {:<36} {:<10} {:<20}", "NAME", "DEVICE ID", "STATUS", "PAIRED AT");
    println!("{}", "-".repeat(88));
    for dev in devices {
        let status = if dev.revoked { "REVOKED" } else { "ACTIVE" };
        println!(
            "{:<20} {:<36} {:<10} {:<20}",
            dev.device_name,
            dev.device_id,
            status,
            dev.paired_at.format("%Y-%m-%d %H:%M:%S")
        );
    }
    Ok(())
}

fn handle_device_revoke(opts: DeviceRevokeOptions) -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = opts.config_dir.unwrap_or_else(default_config_dir);
    let mut reg = DeviceRegistry::load_or_create(&config_dir)?;
    if reg.revoke_device(&opts.device_id) {
        reg.save(&config_dir)?;
        println!("✓ Device '{}' revoked successfully.", opts.device_id);
    } else {
        println!("Device '{}' not found or already revoked.", opts.device_id);
    }
    Ok(())
}

fn handle_device_remove(opts: DeviceRevokeOptions) -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = opts.config_dir.unwrap_or_else(default_config_dir);
    let mut reg = DeviceRegistry::load_or_create(&config_dir)?;
    if reg.remove_device(&opts.device_id) {
        reg.save(&config_dir)?;
        println!("✓ Device '{}' removed successfully.", opts.device_id);
    } else {
        println!("Device '{}' not found in registry.", opts.device_id);
    }
    Ok(())
}

fn handle_token_show(opts: TokenOptions) -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = opts.config_dir.unwrap_or_else(default_config_dir);
    let path = token_path(&config_dir);
    let token = load_or_generate_token(&path)?;
    println!("{}", token);
    Ok(())
}

fn handle_token_rotate(opts: TokenOptions) -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = opts.config_dir.unwrap_or_else(default_config_dir);
    let path = token_path(&config_dir);
    let token = rotate_token(&path)?;
    println!("{}", token);
    Ok(())
}

async fn handle_run(opts: RunOptions) -> Result<(), Box<dyn std::error::Error>> {
    if opts.debug {
        set_log_level(LogLevel::Debug);
    }

    let config_dir = opts.config_dir.clone().unwrap_or_else(default_config_dir);
    let token_file = token_path(&config_dir);

    // Resolve or generate token
    let token = match opts.token {
        Some(t) => t,
        None => load_or_generate_token(&token_file)?,
    };

    let mut relay_url = opts
        .relay
        .clone()
        .or_else(|| {
            let env_path = service_env_path(&config_dir);
            load_service_config(&env_path).ok().and_then(|c| c.relay_url)
        })
        .or_else(|| std::env::var("AGENTDESK_RELAY_URL").ok())
        .or_else(|| std::env::var("AGENTDESK_RELAY").ok())
        .or_else(|| Some(agentdesk_server::DEFAULT_RELAY_URL.to_string()));

    if relay_url.as_deref() == Some(agentdesk_server::DEFAULT_RELAY_URL) {
        if agentdesk_server::tunnel::is_cloudflared_installed() {
            if let Ok(tunnel_url) = agentdesk_server::tunnel::ensure_tunnel(&config_dir, 8766) {
                relay_url = Some(tunnel_url);
            }
        }
    }

    let server_config = ServerConfig {
        bind_addr: opts.bind.clone(),
        port: opts.port,
        token: token.clone(),
        insecure_dev: opts.insecure_dev,
        pipeline_mode: opts.mode,
        outbound_capacity: agentdesk_server::DEFAULT_OUTBOUND_CAPACITY,
        debug_logging: opts.debug,
        config_dir: opts.config_dir.clone(),
        relay_url,
    };

    if let Err(e) = server_config.validate() {
        eprintln!("Configuration error: {}", e);
        std::process::exit(1);
    }

    let clock = Arc::new(SystemClock);
    let start_time = clock.now();

    let mut adapter: Box<dyn Adapter> = if opts.antigravity {
        let agent_cmd = opts.agent.as_deref().unwrap_or("agy");
        let parts: Vec<String> = agent_cmd
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        let command = parts.first().cloned().unwrap_or_else(|| "agy".into());
        let args = if parts.len() > 1 {
            parts[1..].to_vec()
        } else {
            Vec::new()
        };
        let prompt = opts.prompt.clone();

        let agy_config = AntigravityConfig {
            command,
            args,
            cwd: None,
            agent_id: "antigravity".into(),
            agent_name: "Antigravity".into(),
            project: "AgentDesk".into(),
            initial_prompt: prompt,
            cols: 120,
            rows: 40,
        };
        match AntigravityPtyAdapter::spawn(agy_config) {
            Ok(a) => Box::new(a),
            Err(e) => {
                eprintln!("Failed to spawn real Antigravity PTY agent: {}", e);
                std::process::exit(1);
            }
        }
    } else if let Some(agent_cmd) = &opts.agent {
        let parts: Vec<String> = agent_cmd
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        if parts.is_empty() {
            eprintln!("Configuration error: --agent command cannot be empty");
            std::process::exit(1);
        }
        let command = parts[0].clone();
        let args = parts[1..].to_vec();
        let prompt = opts
            .prompt
            .clone()
            .unwrap_or_else(|| "Explain the purpose of this project and report git status.".into());

        let acp_config = AcpConfig {
            command,
            args,
            cwd: None,
            agent_id: "gemini".into(),
            agent_name: "Gemini CLI".into(),
            project: "AgentDesk".into(),
            initial_prompt: prompt,
        };
        match AcpAdapter::spawn(acp_config) {
            Ok(a) => Box::new(a),
            Err(e) => {
                eprintln!("Failed to spawn real ACP agent: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        // Load scenario
        let scenario = if let Some(path) = &opts.scenario {
            let content = fs::read_to_string(path)?;
            Scenario::from_json(&content)?
        } else {
            Scenario::default_scenario()
        };
        Box::new(Simulator::new(scenario, opts.seed, start_time))
    };

    let (tx_adapter, mut rx_adapter) = tokio::sync::mpsc::channel(64);
    let (tx_core, rx_core) = tokio::sync::mpsc::channel(256);

    let mut core = CoreTask::with_seed(
        opts.mode,
        clock.clone(),
        opts.seed,
        LogStoreConfig::default(),
        ThresholdTable::default(),
        Some(tx_adapter),
    );
    core.register_agents(adapter.agents());

    // Spawn core task
    let core_sender = tx_core.clone();
    tokio::spawn(core.run(rx_core));

    // Bind server listener
    let server = Server::bind(server_config, core_sender.clone(), clock.clone()).await?;
    let local_addr = server.local_addr();

    // Startup diagnostics expose the endpoint and certificate identity only.
    // Authentication remains available through the protected token file.
    let scheme = if opts.insecure_dev { "ws" } else { "wss" };
    println!("============================================================");
    println!(" AgentDesk Daemon started");
    println!(" Address: {}://{}", scheme, local_addr);
    println!(" Mode:    {:?}", opts.mode);
    if opts.antigravity {
        println!(
            " Agent:   Real Antigravity PTY ({})",
            opts.agent.as_deref().unwrap_or("agy")
        );
    } else if let Some(agent_cmd) = &opts.agent {
        println!(" Agent:   Real ACP ({})", agent_cmd);
    } else {
        println!(" Agent:   Simulator");
    }
    if let Some(fp) = server.fingerprint() {
        println!(" Fingerprint (SHA-256): {}", fp);
    }
    if opts.insecure_dev {
        println!(" Note:    --insecure-dev mode active (loopback plain ws://)");
    }
    println!("============================================================");

    // Spawn server accept loop
    let server_handle = tokio::spawn(async move {
        if let Err(e) = server.run().await {
            eprintln!("Server error: {}", e);
        }
    });

    // Spawn tick generator (every 10 seconds)
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
                if adapter_sender
                    .send(CoreCommand::Adapter(out))
                    .await
                    .is_err()
                {
                    return;
                }
            }

            // Check if any adapter responses arrived
            tokio::select! {
                adapter_cmd = rx_adapter.recv() => {
                    if let Some(AdapterCommand::Respond { event_id, task_id, decision, request_seq, response, now }) = adapter_cmd {
                        let result = if let Some(ref resp) = response {
                            adapter.respond_with_response(&task_id, request_seq, resp, now)
                        } else {
                            adapter.respond(&task_id, decision, now)
                        };
                        let outcome = match result {
                            Ok(()) => AdapterOutcome::Delivered { event_id, decision },
                            Err(e) => AdapterOutcome::DeliveryFailed {
                                event_id,
                                reason: e.to_string(),
                            },
                        };
                        // Report outcome back to core. If core is gone, we're shutting down.
                        let _ = adapter_sender.send(CoreCommand::AdapterResponse(outcome)).await;
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {}
            }
        }
    });

    // Wait for server or shutdown signal
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("\nReceived Ctrl-C, shutting down AgentDesk...");
            let _ = core_sender.send(CoreCommand::Shutdown).await;
        }
        _ = server_handle => {}
    }

    Ok(())
}
