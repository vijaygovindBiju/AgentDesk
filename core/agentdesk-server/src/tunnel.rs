//! Automatic Cloudflare Tunnel and local relay manager.
//! Enables seamless remote-first connectivity on restrictive networks
//! (e.g. college/hostel Wi-Fi behind NAT) without port forwarding or manual VPS setup.

use std::fs;
use std::net::ToSocketAddrs;
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

pub fn tunnel_log_path(config_dir: &Path) -> PathBuf {
    config_dir.join("tunnel.log")
}

pub fn is_pid_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        let status_path = format!("/proc/{}/status", pid);
        if let Ok(content) = fs::read_to_string(&status_path) {
            for line in content.lines() {
                if line.starts_with("State:") {
                    let state = line.trim_start_matches("State:").trim();
                    // 'Z' (zombie) or 'X' (dead) processes have terminated
                    if state.starts_with('Z') || state.starts_with('X') {
                        return false;
                    }
                    return true;
                }
            }
            return true;
        }
        return false;
    }

    #[cfg(not(target_os = "linux"))]
    {
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

pub fn extract_hostname(url: &str) -> Option<String> {
    let without_scheme = url
        .strip_prefix("wss://")
        .or_else(|| url.strip_prefix("ws://"))
        .or_else(|| url.strip_prefix("https://"))
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    let host = without_scheme.split(&['/', ':', '?'][..]).next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

pub fn check_dns_resolves(host: &str) -> bool {
    (host, 443).to_socket_addrs().is_ok() || (host, 80).to_socket_addrs().is_ok()
}

pub fn parse_quick_tunnel_url(line: &str) -> Option<String> {
    let idx = line.find("https://")?;
    let slice = &line[idx..];
    let end_idx = slice.find(".trycloudflare.com")?;
    let domain = &slice[..end_idx + 18];
    let host = domain.strip_prefix("https://").unwrap_or(domain).trim();
    // Exclude root domain and API endpoints
    if host == "trycloudflare.com" || host == "api.trycloudflare.com" || host.starts_with("api.") {
        return None;
    }
    if !host.ends_with(".trycloudflare.com") || host.len() <= ".trycloudflare.com".len() {
        return None;
    }
    Some(format!("wss://{}", host))
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

pub fn is_tunnel_healthy(config_dir: &Path, state: &TunnelState) -> bool {
    if let Some(t_pid) = state.tunnel_pid {
        if !is_pid_alive(t_pid) {
            return false;
        }
    } else {
        return false;
    }

    if let Some(r_pid) = state.relay_pid {
        if !is_pid_alive(r_pid) {
            return false;
        }
    }

    if std::net::TcpStream::connect("127.0.0.1:8766").is_err() {
        return false;
    }

    // Inspect recent lines in tunnel.log for unrecoverable tunnel errors
    let log_path = tunnel_log_path(config_dir);
    if let Ok(content) = fs::read_to_string(&log_path) {
        let recent = if content.len() > 4096 {
            &content[content.len() - 4096..]
        } else {
            &content[..]
        };
        if recent.contains("Unauthorized: Tunnel not found")
            || recent.contains("Tunnel has been deleted")
        {
            crate::logging::warn("Tunnel log indicates session is unauthorized or deleted by Cloudflare");
            return false;
        }
    }

    // Verify hostname resolves via DNS if using trycloudflare, after a grace period.
    // Newly created tunnels (e.g. < 180 seconds old) may still be propagating through local DNS resolvers.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let age = now.saturating_sub(state.created_at);

    if age >= 180 {
        if let Some(host) = extract_hostname(&state.relay_url) {
            if host.ends_with(".trycloudflare.com") {
                let mut resolved = false;
                for _ in 0..5 {
                    if check_dns_resolves(&host) {
                        resolved = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
                if !resolved {
                    crate::logging::warn(format!(
                        "Tunnel hostname '{}' (age: {}s) does not resolve via DNS; marking tunnel as stale.",
                        host, age
                    ));
                    return false;
                }
            }
        }
    }

    true
}

pub fn load_active_tunnel(config_dir: &Path) -> Option<TunnelState> {
    let path = tunnel_state_path(config_dir);
    if !path.exists() {
        return None;
    }
    let data = fs::read_to_string(&path).ok()?;
    let state: TunnelState = serde_json::from_str(&data).ok()?;

    if !is_tunnel_healthy(config_dir, &state) {
        stop_tunnel(config_dir);
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
                std::thread::sleep(Duration::from_millis(100));
                if is_pid_alive(t_pid) {
                    let _ = Command::new("kill").arg("-KILL").arg(t_pid.to_string()).status();
                }
            }
            if let Some(r_pid) = state.relay_pid {
                let _ = Command::new("kill").arg("-TERM").arg(r_pid.to_string()).status();
                std::thread::sleep(Duration::from_millis(100));
                if is_pid_alive(r_pid) {
                    let _ = Command::new("kill").arg("-KILL").arg(r_pid.to_string()).status();
                }
            }
        }
    }
    let _ = fs::remove_file(path);

    #[cfg(unix)]
    {
        let _ = Command::new("pkill")
            .args(["-9", "-f", "cloudflared tunnel.*127.0.0.1:8766"])
            .status();
    }
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
        let mut relay_cmd = Command::new("agentdesk-relay");
        relay_cmd
            .arg("--bind")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(relay_port.to_string())
            .arg("--insecure-dev")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(target_os = "linux")]
        unsafe {
            use std::os::unix::process::CommandExt;
            relay_cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let mut relay_child = relay_cmd.spawn()?;
        let pid = relay_child.id();
        // Spawn reaper thread so terminated relay child does not become a zombie
        std::thread::spawn(move || {
            let _ = relay_child.wait();
        });
        std::thread::sleep(Duration::from_millis(500));
        Some(pid)
    } else {
        None
    };

    // 3. Start cloudflared tunnel in background writing to log file
    crate::logging::info("Requesting public tunnel from Cloudflare...");
    let _ = fs::create_dir_all(config_dir);
    let log_path = tunnel_log_path(config_dir);
    let log_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)?;
    let log_file_err = log_file.try_clone()?;

    let mut cmd = Command::new("cloudflared");
    cmd.arg("tunnel")
        .arg("--edge-ip-version")
        .arg("4")
        .arg("--url")
        .arg(format!("http://127.0.0.1:{}", relay_port))
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        #[cfg(target_os = "linux")]
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }

    let mut tunnel_child = cmd.spawn()?;
    let tunnel_pid = tunnel_child.id();

    // Spawn reaper thread so terminated cloudflared does not become a zombie
    std::thread::spawn(move || {
        let _ = tunnel_child.wait();
    });

    let start = Instant::now();
    let mut resolved_url: Option<String> = None;
    let timeout = Duration::from_secs(25);

    while start.elapsed() < timeout {
        if !is_pid_alive(tunnel_pid) {
            let log_snippet = fs::read_to_string(&log_path).unwrap_or_default();
            return Err(format!("cloudflared exited unexpectedly: {}", log_snippet).into());
        }

        if let Ok(content) = fs::read_to_string(&log_path) {
            for line in content.lines() {
                if let Some(url) = parse_quick_tunnel_url(line) {
                    resolved_url = Some(url);
                    break;
                }
            }
        }

        if resolved_url.is_some() {
            break;
        }

        std::thread::sleep(Duration::from_millis(250));
    }

    let url = resolved_url.ok_or_else(|| {
        let log_snippet = fs::read_to_string(&log_path).unwrap_or_default();
        format!("Timed out waiting for Cloudflare tunnel URL. Tunnel logs:\n{}", log_snippet)
    })?;

    // Wait for DNS propagation so phone pairing doesn't fail on immediate scan
    if let Some(host) = extract_hostname(&url) {
        crate::logging::info(format!("Waiting for DNS propagation for {}...", host));
        let dns_start = Instant::now();
        let mut dns_ok = false;
        while dns_start.elapsed() < Duration::from_secs(15) {
            if check_dns_resolves(&host) {
                dns_ok = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if dns_ok {
            crate::logging::info(format!("DNS verified for {}", host));
        } else {
            crate::logging::warn(format!(
                "DNS resolution for {} taking longer than expected; proceeding...",
                host
            ));
        }
    }

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
    let _ = fs::write(
        tunnel_state_path(config_dir),
        serde_json::to_string_pretty(&state)?,
    );

    Ok(url)
}

/// Synchronizes the given relay URL to the local service environment file,
/// restarting the background systemd service if running.
pub fn sync_relay_to_service_env(config_dir: &Path, relay_url: &str) {
    let env_path = crate::config::service_env_path(config_dir);
    if let Ok(mut cfg) = crate::config::load_service_config(&env_path) {
        if cfg.relay_url.as_deref() != Some(relay_url) {
            cfg.relay_url = Some(relay_url.to_string());
            let _ = crate::config::update_service_config(&env_path, &cfg);
            let is_daemon = std::env::var("INVOCATION_ID").is_ok()
                || std::env::args().any(|a| a == "run");
            if !is_daemon && crate::config::service_is_active().unwrap_or(false) {
                let _ = crate::config::service_action("restart");
            }
        }
    }
}

/// Resolves the authoritative public relay URL to use across the system.
///
/// Priority:
/// 1. If an explicit, non-default, non-trycloudflare relay was supplied (e.g. self-hosted VPS),
///    honor it directly.
/// 2. If a locally managed active tunnel exists and is alive, its URL is the authoritative trycloudflare URL.
/// 3. If the candidate was a trycloudflare URL but no active tunnel was alive, or candidate was None/DEFAULT,
///    and cloudflared is installed: ensure a fresh tunnel.
/// 4. Fallback: If requested_relay is valid and not trycloudflare, use it; otherwise DEFAULT_RELAY_URL.
pub fn resolve_or_ensure_relay_url(
    config_dir: &Path,
    requested_relay: Option<String>,
) -> String {
    // Never use a relay that points at a LAN/private or plaintext public endpoint.
    let requested_relay = requested_relay.filter(|candidate| {
        match crate::config::validate_relay_url(candidate) {
            Ok(()) => true,
            Err(e) => {
                crate::logging::warn(format!("Ignoring invalid relay URL '{}': {}", candidate, e));
                false
            }
        }
    });

    // 1. Explicit custom VPS relay
    if let Some(ref candidate) = requested_relay {
        let trimmed = candidate.trim();
        if !trimmed.is_empty()
            && trimmed != crate::config::DEFAULT_RELAY_URL
            && !trimmed.contains("trycloudflare.com")
        {
            return trimmed.to_string();
        }
    }

    // 2. Locally managed active tunnel
    if let Some(active) = load_active_tunnel(config_dir) {
        sync_relay_to_service_env(config_dir, &active.relay_url);
        return active.relay_url;
    }

    // 3. Auto-provision Cloudflare tunnel on demand
    if is_cloudflared_installed() {
        match ensure_tunnel(config_dir, 8766) {
            Ok(new_url) => {
                sync_relay_to_service_env(config_dir, &new_url);
                return new_url;
            }
            Err(e) => {
                crate::logging::warn(format!("Failed to provision Cloudflare tunnel: {}", e));
            }
        }
    }

    // 4. Fallback
    let fallback = requested_relay
        .filter(|c| !c.contains("trycloudflare.com") && !c.trim().is_empty())
        .unwrap_or_else(|| crate::config::DEFAULT_RELAY_URL.to_string());
    sync_relay_to_service_env(config_dir, &fallback);
    fallback
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_hostname() {
        assert_eq!(
            extract_hostname("wss://something-random.trycloudflare.com"),
            Some("something-random.trycloudflare.com".to_string())
        );
        assert_eq!(
            extract_hostname("https://something-random.trycloudflare.com/ws?foo=bar"),
            Some("something-random.trycloudflare.com".to_string())
        );
        assert_eq!(
            extract_hostname("ws://127.0.0.1:8766/ws"),
            Some("127.0.0.1".to_string())
        );
    }

    #[test]
    fn test_parse_quick_tunnel_url() {
        let banner_line = "|  https://random-words-1234.trycloudflare.com  |";
        assert_eq!(
            parse_quick_tunnel_url(banner_line),
            Some("wss://random-words-1234.trycloudflare.com".to_string())
        );

        let api_line = "failed to request quick Tunnel: Post \"https://api.trycloudflare.com/tunnel\": ...";
        assert_eq!(parse_quick_tunnel_url(api_line), None);

        let root_line = "Requesting new quick Tunnel on trycloudflare.com...";
        assert_eq!(parse_quick_tunnel_url(root_line), None);
    }

    #[test]
    fn test_is_pid_alive_nonexistent() {
        assert!(!is_pid_alive(99999999));
    }

    #[test]
    fn test_resolve_explicit_custom_relay() {
        let temp_dir = tempfile::tempdir().unwrap();
        let custom = "wss://my-custom-relay.example.com";
        let resolved = resolve_or_ensure_relay_url(temp_dir.path(), Some(custom.to_string()));
        assert_eq!(resolved, custom);
    }

    #[test]
    #[ignore = "requires live cloudflared tunnel creation over the internet"]
    fn test_resolve_fallback_when_no_cloudflared() {
        let temp_dir = tempfile::tempdir().unwrap();
        // Passing a stale trycloudflare URL when no active tunnel exists
        let stale = "wss://stale-xyz-123.trycloudflare.com";
        let resolved = resolve_or_ensure_relay_url(temp_dir.path(), Some(stale.to_string()));
        // If cloudflared is installed on the build machine, it may return a tunnel URL or fallback
        // But it MUST NOT return the stale trycloudflare URL!
        assert_ne!(resolved, stale);
    }

    #[test]
    fn test_is_tunnel_healthy_dead_pid() {
        let temp_dir = tempfile::tempdir().unwrap();
        let state = TunnelState {
            relay_pid: None,
            tunnel_pid: Some(99999999),
            relay_url: "wss://some-tunnel.trycloudflare.com".to_string(),
            created_at: 100,
        };
        assert!(!is_tunnel_healthy(temp_dir.path(), &state));
    }

    #[test]
    fn test_is_tunnel_healthy_unauthorized_log() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_file = tunnel_log_path(temp_dir.path());
        let _ = fs::write(&log_file, "ERR Register tunnel error from server side error=\"Unauthorized: Tunnel not found\"\n");
        let state = TunnelState {
            relay_pid: None,
            tunnel_pid: Some(std::process::id()),
            relay_url: "wss://some-tunnel.trycloudflare.com".to_string(),
            created_at: 100,
        };
        assert!(!is_tunnel_healthy(temp_dir.path(), &state));
    }

    #[test]
    fn test_is_tunnel_healthy_unresolvable_dns() {
        let temp_dir = tempfile::tempdir().unwrap();
        let state = TunnelState {
            relay_pid: None,
            tunnel_pid: Some(std::process::id()),
            relay_url: "wss://this-subdomain-definitely-does-not-exist-123456789.trycloudflare.com".to_string(),
            created_at: 100,
        };
        assert!(!is_tunnel_healthy(temp_dir.path(), &state));
    }
}
