//! Main executable for the AgentDesk daemon.
//! See docs/TODO.md Phase 6 and docs/COMMUNICATION.md.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use agentdesk_core::{
    AcpAdapter, AcpConfig, Adapter, AdapterCommand, AntigravityConfig, AntigravityPtyAdapter,
    Clock, CoreCommand, CoreTask, LogStoreConfig, SystemClock, ThresholdTable,
};
use agentdesk_server::{
    CliCommand, LogLevel, RunOptions, Server, ServerConfig, TokenOptions, default_config_dir,
    load_or_generate_token, parse_args, print_help, rotate_token, set_log_level, token_path,
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

    let server_config = ServerConfig {
        bind_addr: opts.bind.clone(),
        port: opts.port,
        token: token.clone(),
        insecure_dev: opts.insecure_dev,
        pipeline_mode: opts.mode,
        outbound_capacity: agentdesk_server::DEFAULT_OUTBOUND_CAPACITY,
        debug_logging: opts.debug,
        config_dir: opts.config_dir.clone(),
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

    // P6.7 / P8.1: Startup prints address, fingerprint, and token
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
    println!(" Token:   {}", token);
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
                    if let Some(AdapterCommand::Respond { task_id, decision, request_seq, response, now }) = adapter_cmd {
                        if let Some(ref resp) = response {
                            let _ = adapter.respond_with_response(&task_id, request_seq, resp, now);
                        } else {
                            let _ = adapter.respond(&task_id, decision, now);
                        }
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
