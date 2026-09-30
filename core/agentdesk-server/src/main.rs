//! Main executable for the AgentDesk daemon.
//! See docs/TODO.md Phase 6 and docs/COMMUNICATION.md.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use agentdesk_core::{
    AcpAdapter, AcpConfig, Adapter, AdapterCommand, AdapterOutcome, AntigravityConfig,
    AntigravityPtyAdapter, Clock, CoreCommand, CoreTask, LogStoreConfig, RespondError, SystemClock,
    ThresholdTable,
};
use agentdesk_server::{
    AgentDiscovery, CliCommand, DeviceOptions, DeviceRegistry, DeviceRevokeOptions, LogLevel,
    PairOptions, RunOptions, Server, ServerConfig, TokenOptions, default_config_dir,
    load_or_generate_token, load_service_config, parse_args, print_help, rotate_token,
    service_env_path, set_log_level, token_path,
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
        CliCommand::RelayShow(opts) => {
            let config_dir = opts.config_dir.unwrap_or_else(default_config_dir);
            let config = load_service_config(&service_env_path(&config_dir))?;
            match config.relay_url {
                Some(url) => println!("relay_url = {url}"),
                None => println!(
                    "relay_url is not set (run `agentdesk relay set wss://relay.example.com`)"
                ),
            }
            println!("Transport: outbound WSS to relay only (no LAN, no USB, no inbound ports)");
            Ok(())
        }
        CliCommand::RelaySet { url, config_dir } => {
            let config_dir = config_dir.unwrap_or_else(default_config_dir);
            let env_path = service_env_path(&config_dir);
            let mut config = load_service_config(&env_path)?;
            config.relay_url = Some(url.clone());
            agentdesk_server::update_service_config(&env_path, &config)?;
            println!("relay_url = {url}");
            println!(
                "Saved to {}. Restart AgentDesk to apply.",
                env_path.display()
            );
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
    let token = match opts.token.as_deref() {
        Some(t) => t.to_string(),
        None => load_or_generate_token(&token_file)?,
    };

    let relay_url = agentdesk_server::tunnel::resolve_or_ensure_relay_url(
        &config_dir,
        opts.relay
            .clone()
            .or_else(|| {
                let env_path = service_env_path(&config_dir);
                load_service_config(&env_path)
                    .ok()
                    .and_then(|c| c.relay_url)
            })
            .or_else(|| std::env::var("AGENTDESK_RELAY_URL").ok())
            .or_else(|| std::env::var("AGENTDESK_RELAY").ok()),
    );

    if let Err(e) = agentdesk_server::validate_relay_url(&relay_url) {
        eprintln!("Relay configuration error: {e}");
        std::process::exit(1);
    }

    let server_config = ServerConfig {
        bind_addr: agentdesk_server::LOCAL_DAEMON_BIND.into(),
        port: opts.port,
        token: token.clone(),
        insecure_dev: opts.insecure_dev,
        pipeline_mode: opts.mode,
        outbound_capacity: agentdesk_server::DEFAULT_OUTBOUND_CAPACITY,
        debug_logging: opts.debug,
        config_dir: opts.config_dir.clone(),
        relay_url: Some(relay_url),
    };

    if let Err(e) = server_config.validate() {
        eprintln!("Configuration error: {}", e);
        std::process::exit(1);
    }

    let clock = Arc::new(SystemClock);
    let start_time = clock.now();

    let mut opts = opts;
    if !opts.antigravity && opts.agent.is_none() && opts.scenario.is_none() {
        if AgentDiscovery::discover()
            .iter()
            .any(|a| a.id == "antigravity" && a.is_ready())
        {
            opts.antigravity = true;
            agentdesk_server::info(
                "Auto-detected Antigravity (agy) on PATH; using real Antigravity PTY adapter",
            );
        }
    }

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
        // A phone prompt is typed into the agent only when its live screen is at the idle
        // chat prompt. Typing it while a dialog (workspace trust, command approval, question)
        // is on screen would answer that dialog without an explicit phone response.
        let mut pending_prompt: Option<String> = None;
        // Opt-in local diagnostic (`AGENTDESK_SCREEN_DUMP=<file>`): the parsed VT screen and the
        // detected semantic state, written only when they change. Never sent over the network.
        let screen_dump = std::env::var_os("AGENTDESK_SCREEN_DUMP").map(std::path::PathBuf::from);
        let mut last_dump = String::new();
        loop {
            let now = chrono::Utc::now();
            let outputs = adapter.poll(now);
            if let (Some(path), Some(screen)) = (screen_dump.as_ref(), adapter.screen()) {
                let snap = screen.snapshot();
                let dump = format!(
                    "detected: {:?}\ncursor: row={} col={} visible={} alt_screen={}\nreversed_rows: {:?}\n{}\n",
                    agentdesk_core::detect_state(&snap),
                    snap.cursor_row,
                    snap.cursor_col,
                    snap.cursor_visible,
                    snap.in_alt_screen,
                    snap.reversed_lines
                        .iter()
                        .map(|(r, _)| *r)
                        .collect::<Vec<_>>(),
                    snap.lines
                        .iter()
                        .enumerate()
                        .map(|(i, l)| format!("{i:02}|{l}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
                if dump != last_dump {
                    let _ = fs::write(path, &dump);
                    last_dump = dump;
                }
            }
            if let Some(prompt) = pending_prompt.take() {
                let live = adapter
                    .screen()
                    .map(|s| agentdesk_core::detect_state(&s.snapshot()));
                if live.as_ref().is_none_or(|s| s.accepts_chat_input()) {
                    let payload = if prompt.contains('\n') {
                        format!("\x1b[200~{prompt}\x1b[201~\r")
                    } else {
                        format!("{prompt}\r")
                    };
                    match adapter.send_raw_input(payload.as_bytes()) {
                        Ok(()) => {
                            agentdesk_server::info("Delivered phone prompt to agent at idle prompt")
                        }
                        Err(e) => agentdesk_server::logging::warn(format!(
                            "Failed sending prompt to adapter: {}",
                            e
                        )),
                    }
                } else {
                    pending_prompt = Some(prompt);
                }
            }
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
                    match adapter_cmd {
                        Some(AdapterCommand::Respond { event_id, task_id, decision, request_seq, response, now }) => {
                            let result = if let Some(ref resp) = response {
                                adapter.respond_with_response(&task_id, request_seq, resp, now)
                            } else {
                                adapter.respond(&task_id, decision, now)
                            };
                            match result {
                                Ok(()) => {
                                    let _ = adapter_sender
                                        .send(CoreCommand::AdapterResponse(
                                            AdapterOutcome::Delivered { event_id, decision },
                                        ))
                                        .await;
                                }
                                Err(RespondError::NoSuchTask) => {
                                    // Task is not managed by the daemon's local adapter (e.g. running in an attached TUI session).
                                    // Attached clients deliver it to their PTY and report AdapterOutcome back.
                                }
                                Err(e) => {
                                    let _ = adapter_sender
                                        .send(CoreCommand::AdapterResponse(
                                            AdapterOutcome::DeliveryFailed {
                                                event_id,
                                                reason: e.to_string(),
                                            },
                                        ))
                                        .await;
                                }
                            }
                        }
                        Some(AdapterCommand::StartTask { prompt, workspace, .. }) => {
                            agentdesk_server::info(format!("Received prompt from phone via relay (len: {})", prompt.len()));
                            if adapter.is_finished() && opts.antigravity {
                                let new_config = AntigravityConfig {
                                    command: "agy".into(),
                                    args: Vec::new(),
                                    cwd: workspace.map(std::path::PathBuf::from),
                                    agent_id: "antigravity".into(),
                                    agent_name: "Antigravity".into(),
                                    project: "AgentDesk".into(),
                                    initial_prompt: Some(prompt.clone()),
                                    cols: 120,
                                    rows: 40,
                                };
                                match AntigravityPtyAdapter::spawn(new_config) {
                                    Ok(new_adapter) => {
                                        adapter = Box::new(new_adapter);
                                        agentdesk_server::info("Spawned new Antigravity process with initial prompt");
                                    }
                                    Err(e) => {
                                        agentdesk_server::logging::warn(format!("Failed to respawn Antigravity: {}", e));
                                    }
                                }
                            } else {
                                let live = adapter.screen().map(|s| agentdesk_core::detect_state(&s.snapshot()));
                                if !live.as_ref().is_none_or(|s| s.accepts_chat_input()) {
                                    agentdesk_server::info(format!(
                                        "Agent is not at its idle prompt ({:?}); holding phone prompt until it is",
                                        live
                                    ));
                                }
                                pending_prompt = Some(prompt);
                            }
                        }
                        None => {}
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
