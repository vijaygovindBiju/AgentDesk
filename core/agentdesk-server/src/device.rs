//! Device identity, registration, and persistent device registry.
//! See docs/SECURITY.md and Sections 19-21, 23.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use crate::token::generate_token;

/// Persistent record of a paired mobile device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedDevice {
    pub device_id: String,
    pub device_name: String,
    pub token: String,
    pub paired_at: DateTime<Utc>,
    pub revoked: bool,
}

/// Device registry managing instance identity and authorized paired devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceRegistry {
    pub instance_id: String,
    pub devices: HashMap<String, PairedDevice>,
}

pub fn instance_id_path(config_dir: &Path) -> PathBuf {
    config_dir.join("instance_id")
}

pub fn devices_path(config_dir: &Path) -> PathBuf {
    config_dir.join("devices.json")
}

pub fn generate_instance_id() -> String {
    format!("agentdesk-inst-{}", &generate_token()[..16])
}

impl DeviceRegistry {
    pub fn new(instance_id: String) -> Self {
        Self {
            instance_id,
            devices: HashMap::new(),
        }
    }

    /// Load or create the instance identity and device registry from the config directory.
    pub fn load_or_create(config_dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(config_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(config_dir, fs::Permissions::from_mode(0o700));
        }

        // 1. Resolve or generate instance ID
        let id_file = instance_id_path(config_dir);
        let instance_id = if id_file.exists() {
            let s = fs::read_to_string(&id_file)?.trim().to_string();
            if s.is_empty() {
                let id = generate_instance_id();
                save_secure_file(&id_file, id.as_bytes())?;
                id
            } else {
                s
            }
        } else {
            let id = generate_instance_id();
            save_secure_file(&id_file, id.as_bytes())?;
            id
        };

        // 2. Load or create devices.json
        let dev_file = devices_path(config_dir);
        let mut registry = if dev_file.exists() {
            let content = fs::read_to_string(&dev_file)?;
            match serde_json::from_str::<DeviceRegistry>(&content) {
                Ok(mut reg) => {
                    reg.instance_id = instance_id.clone();
                    reg
                }
                Err(e) => {
                    eprintln!("Warning: failed parsing devices.json ({e}), creating empty registry");
                    DeviceRegistry::new(instance_id.clone())
                }
            }
        } else {
            DeviceRegistry::new(instance_id.clone())
        };

        registry.instance_id = instance_id;
        Ok(registry)
    }

    /// Persist the device registry to disk with 0600 permissions.
    pub fn save(&self, config_dir: &Path) -> io::Result<()> {
        let dev_file = devices_path(config_dir);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        save_secure_file(&dev_file, json.as_bytes())
    }

    /// Verify whether a device is authorized to connect.
    /// Returns false if device does not exist, is revoked, or token mismatch.
    /// Token comparison is performed in constant time.
    pub fn is_authorized(&self, device_id: &str, token: &str) -> bool {
        if let Some(dev) = self.devices.get(device_id) {
            if dev.revoked {
                return false;
            }
            let p = token.as_bytes();
            let e = dev.token.as_bytes();
            if p.len() != e.len() {
                return false;
            }
            p.ct_eq(e).into()
        } else {
            false
        }
    }

    /// Register a newly paired device, issuing a fresh cryptographically random 256-bit token.
    /// Returns the persistent device token.
    pub fn register_device(&mut self, device_id: String, device_name: String) -> String {
        let token = generate_token();
        let paired = PairedDevice {
            device_id: device_id.clone(),
            device_name,
            token: token.clone(),
            paired_at: Utc::now(),
            revoked: false,
        };
        self.devices.insert(device_id, paired);
        token
    }

    /// Revoke a device by device_id.
    /// Returns true if device was found and revoked.
    pub fn revoke_device(&mut self, device_id: &str) -> bool {
        if let Some(dev) = self.devices.get_mut(device_id) {
            dev.revoked = true;
            true
        } else {
            false
        }
    }

    /// Completely remove a device from the registry.
    pub fn remove_device(&mut self, device_id: &str) -> bool {
        self.devices.remove(device_id).is_some()
    }

    /// List all paired devices.
    pub fn list_devices(&self) -> Vec<&PairedDevice> {
        let mut list: Vec<&PairedDevice> = self.devices.values().collect();
        list.sort_by(|a, b| a.paired_at.cmp(&b.paired_at));
        list
    }
}

fn save_secure_file(path: &Path, content: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(content)?;
        file.write_all(b"\n")?;
        file.flush()?;
    }

    #[cfg(not(unix))]
    {
        fs::write(path, content)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lifecycle_and_authorization() {
        let temp_dir = std::env::temp_dir().join(format!("agentdesk_dev_{}", generate_token()));
        let mut registry = DeviceRegistry::load_or_create(&temp_dir).unwrap();
        assert!(!registry.instance_id.is_empty());

        let dev_id = "phone-samsung-1";
        let dev_token = registry.register_device(dev_id.into(), "Galaxy S25".into());
        assert_eq!(dev_token.len(), 64);
        assert!(registry.is_authorized(dev_id, &dev_token));
        assert!(!registry.is_authorized(dev_id, "wrong-token"));
        assert!(!registry.is_authorized("unknown-device", &dev_token));

        registry.save(&temp_dir).unwrap();

        // Reload and verify persistence
        let reloaded = DeviceRegistry::load_or_create(&temp_dir).unwrap();
        assert_eq!(reloaded.instance_id, registry.instance_id);
        assert!(reloaded.is_authorized(dev_id, &dev_token));

        // Revocation
        let mut reg = reloaded;
        assert!(reg.revoke_device(dev_id));
        assert!(!reg.is_authorized(dev_id, &dev_token));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
