//! Runtime status tracking and persistence for AgentDesk daemon and TUI.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub fn runtime_status_path(config_dir: &Path) -> PathBuf {
    config_dir.join("runtime_status.json")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub pid: u32,
    pub port: u16,
    pub bind_addr: String,
    pub phone_connected: bool,
    pub phone_transport: Option<String>,
    pub phone_device_name: Option<String>,
    pub phone_last_seen: Option<DateTime<Utc>>,
    pub active_clients: usize,
    pub updated_at: DateTime<Utc>,
}

impl Default for RuntimeStatus {
    fn default() -> Self {
        Self {
            pid: std::process::id(),
            port: 8765,
            bind_addr: "127.0.0.1".into(),
            phone_connected: false,
            phone_transport: None,
            phone_device_name: None,
            phone_last_seen: None,
            active_clients: 0,
            updated_at: Utc::now(),
        }
    }
}

pub fn save_runtime_status(config_dir: &Path, status: &RuntimeStatus) -> io::Result<()> {
    let path = runtime_status_path(config_dir);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let json = serde_json::to_string(status)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(&path, json)
}

pub fn load_runtime_status(config_dir: &Path) -> Option<RuntimeStatus> {
    let path = runtime_status_path(config_dir);
    if !path.exists() {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn remove_runtime_status(config_dir: &Path) {
    let path = runtime_status_path(config_dir);
    let _ = fs::remove_file(path);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveClient {
    pub client_id: u64,
    pub device_id: String,
    pub device_name: String,
    pub transport: String,
    pub connected_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

pub struct StatusTracker {
    config_dir: PathBuf,
    status: Mutex<RuntimeStatus>,
    clients: Mutex<HashMap<u64, ActiveClient>>,
}

impl StatusTracker {
    pub fn new(config_dir: PathBuf, bind_addr: String, port: u16) -> Self {
        let status = RuntimeStatus {
            pid: std::process::id(),
            port,
            bind_addr,
            phone_connected: false,
            phone_transport: None,
            phone_device_name: None,
            phone_last_seen: None,
            active_clients: 0,
            updated_at: Utc::now(),
        };
        let _ = save_runtime_status(&config_dir, &status);
        Self {
            config_dir,
            status: Mutex::new(status),
            clients: Mutex::new(HashMap::new()),
        }
    }

    pub fn register_client(
        &self,
        client_id: u64,
        device_id: String,
        device_name: String,
        transport: &str,
    ) {
        let now = Utc::now();
        let client = ActiveClient {
            client_id,
            device_id: device_id.clone(),
            device_name: device_name.clone(),
            transport: transport.to_string(),
            connected_at: now,
            last_seen: now,
        };

        {
            let mut clients = self.clients.lock().unwrap();
            clients.insert(client_id, client);
        }

        self.recompute_and_save();
    }

    pub fn unregister_client(&self, client_id: u64) {
        {
            let mut clients = self.clients.lock().unwrap();
            clients.remove(&client_id);
        }
        self.recompute_and_save();
    }

    pub fn update_last_seen(&self, client_id: u64) {
        let mut clients = self.clients.lock().unwrap();
        if let Some(c) = clients.get_mut(&client_id) {
            c.last_seen = Utc::now();
        }
    }

    pub fn list_clients(&self) -> Vec<ActiveClient> {
        let clients = self.clients.lock().unwrap();
        clients.values().cloned().collect()
    }

    pub fn current_status(&self) -> RuntimeStatus {
        let s = self.status.lock().unwrap();
        s.clone()
    }

    fn recompute_and_save(&self) {
        let clients = self.clients.lock().unwrap();
        let mut status = self.status.lock().unwrap();

        status.active_clients = clients.len();
        status.updated_at = Utc::now();

        // Check if any phone is connected (ignore internal TUI or desktop IDs)
        let phone = clients.values().find(|c| {
            !c.device_id.is_empty()
                && c.device_id != "agentdesk-tui"
                && c.device_id != "desktop"
                && c.device_id != "local"
        });

        if let Some(p) = phone {
            status.phone_connected = true;
            status.phone_transport = Some(p.transport.clone());
            status.phone_device_name = Some(p.device_name.clone());
            status.phone_last_seen = Some(p.last_seen);
        } else {
            status.phone_connected = false;
        }

        let _ = save_runtime_status(&self.config_dir, &status);
    }
}

impl Drop for StatusTracker {
    fn drop(&mut self) {
        remove_runtime_status(&self.config_dir);
    }
}
