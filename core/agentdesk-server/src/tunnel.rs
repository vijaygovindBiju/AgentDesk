//! Automatic Cloudflare Tunnel and local relay manager.
//! Enables seamless remote-first connectivity on restrictive networks
//! (e.g. college/hostel Wi-Fi behind NAT) without port forwarding or manual VPS setup.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelState {
    pub relay_pid: Option<u32>,
    pub tunnel_pid: Option<u32>,
    pub relay_url: String,
    pub created_at: u64,
}

pub fn tunnel_state_path(config_dir: &Path) -> PathBuf {
    config_dir.join("tunnel.json")
}

pub fn is_pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{}", pid)).exists()
}

pub fn is_cloudflared_installed() -> bool {
    Command::new("cloudflared")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn load_active_tunnel(config_dir: &Path) -> Option<TunnelState> {
    let path = tunnel_state_path(config_dir);
    if !path.exists() {
        return None;
    }
    let data = fs::read_to_string(&path).ok()?;
    let state: TunnelState = serde_json::from_str(&data).ok()?;

    if let Some(t_pid) = state.tunnel_pid {
        if !is_pid_alive(t_pid) {
            return None;
        }
    } else {
        return None;
    }

    if std::net::TcpStream::connect("127.0.0.1:8766").is_err() {
        return None;
    }

    Some(state)
}

pub fn stop_tunnel(config_dir: &Path) {
    let path = tunnel_state_path(config_dir);
    if let Ok(data) = fs::read_to_string(&path) {
        if let Ok(state) = serde_json::from_str::<TunnelState>(&data) {
            if let Some(t_pid) = state.tunnel_pid {
                let _ = Command::new("kill").arg("-TERM").arg(t_pid.to_string()).status();
            }
            if let Some(r_pid) = state.relay_pid {
                let _ = Command::new("kill").arg("-TERM").arg(r_pid.to_string()).status();
            }
        }
    }
    let _ = fs::remove_file(path);
}

pub fn ensure_tunnel(
    config_dir: &Path,
    relay_port: u16,
) -> Result<String, Box<dyn std::error::Error>> {
    // 1. Check if already active and alive
    if let Some(existing) = load_active_tunnel(config_dir) {
        return Ok(existing.relay_url);
    }

    if !is_cloudflared_installed() {
        return Err("cloudflared is not installed or not in PATH".into());
    }

    // Clean up any previous dead tunnel or orphaned process
    stop_tunnel(config_dir);

    crate::logging::info("Ensuring public relay tunnel via Cloudflare...");

    // 2. Ensure agentdesk-relay is running on relay_port
    let is_listening = std::net::TcpStream::connect(format!("127.0.0.1:{}", relay_port)).is_ok();
    let relay_pid = if !is_listening {
        crate::logging::info(format!("Starting local relay server on port {}...", relay_port));
        let relay_child = Command::new("agentdesk-relay")
            .arg("--port")
            .arg(relay_port.to_string())
            .arg("--insecure-dev")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let pid = relay_child.id();
        std::mem::forget(relay_child);
        std::thread::sleep(Duration::from_millis(500));
        Some(pid)
    } else {
        None
    };

    // 3. Start cloudflared tunnel in background
    crate::logging::info("Requesting public tunnel from Cloudflare...");
    let mut tunnel_child = Command::new("cloudflared")
        .arg("tunnel")
        .arg("--edge-ip-version")
        .arg("4")
        .arg("--url")
        .arg(format!("http://127.0.0.1:{}", relay_port))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stderr = tunnel_child
        .stderr
        .take()
        .ok_or("Failed to capture cloudflared stderr")?;
    let reader = BufReader::new(stderr);

    let start = Instant::now();
    let mut resolved_url: Option<String> = None;

    for line_res in reader.lines() {
        if let Ok(line) = line_res {
            if let Some(idx) = line.find("https://") {
                let slice = &line[idx..];
                if let Some(end_idx) = slice.find(".trycloudflare.com") {
                    let domain = &slice[..end_idx + 18];
                    let wss_url = domain.replace("https://", "wss://");
                    resolved_url = Some(wss_url);
                    break;
                }
            }
        }
        if start.elapsed() > Duration::from_secs(15) {
            break;
        }
    }

    let url = resolved_url.ok_or("Timed out waiting for Cloudflare tunnel URL")?;
    let tunnel_pid = tunnel_child.id();

    // Let cloudflared keep running in background
    std::mem::forget(tunnel_child);

    crate::logging::info(format!("Cloudflare public tunnel active: {}", url));

    // 4. Save tunnel state
    let state = TunnelState {
        relay_pid,
        tunnel_pid: Some(tunnel_pid),
        relay_url: url.clone(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    let _ = fs::create_dir_all(config_dir);
    let _ = fs::write(
        tunnel_state_path(config_dir),
        serde_json::to_string_pretty(&state)?,
    );

    Ok(url)
}
