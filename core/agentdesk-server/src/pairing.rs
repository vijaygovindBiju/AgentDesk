//! Ephemeral QR pairing protocol, session management, and QR terminal rendering.
//! See docs/SECURITY.md and Sections 4-6, 19, 22.

use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use chrono::{DateTime, Duration, Utc};
use qrcode::QrCode;
use qrcode::render::unicode;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::token::generate_token;

pub const PAIRING_DEFAULT_DURATION_SECS: i64 = 180;
pub const PAIRING_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingError {
    NoActiveSession,
    WrongSession,
    Expired,
    Replayed,
    InvalidSecret,
    WrongInstance,
}

impl std::fmt::Display for PairingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PairingError::NoActiveSession => write!(f, "no active pairing session"),
            PairingError::WrongSession => write!(f, "pairing session not found"),
            PairingError::Expired => write!(f, "pairing session expired"),
            PairingError::Replayed => write!(f, "pairing session has already been used"),
            PairingError::InvalidSecret => write!(f, "invalid pairing secret"),
            PairingError::WrongInstance => write!(f, "instance mismatch"),
        }
    }
}

impl std::error::Error for PairingError {}

/// The QR payload structure carried in the terminal QR code.
/// Format: `agentdesk://pair?v=1&inst=...&sess=...&sec=...&fp=...[&lan=...][&relay=...]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingPayload {
    pub version: u32,
    pub instance_id: String,
    pub session_id: String,
    pub pairing_secret: String,
    pub fingerprint: String,
    pub lan_address: Option<String>,
    pub relay_url: Option<String>,
}

impl PairingPayload {
    pub fn to_uri(&self) -> String {
        let mut uri = format!(
            "agentdesk://pair?v={}&inst={}&sess={}&sec={}&fp={}",
            self.version,
            urlencoding::encode(&self.instance_id),
            urlencoding::encode(&self.session_id),
            urlencoding::encode(&self.pairing_secret),
            urlencoding::encode(&self.fingerprint),
        );
        if let Some(ref lan) = self.lan_address {
            uri.push_str(&format!("&lan={}", urlencoding::encode(lan)));
        }
        if let Some(ref relay) = self.relay_url {
            uri.push_str(&format!("&relay={}", urlencoding::encode(relay)));
        }
        uri
    }

    pub fn from_uri(uri_str: &str) -> Result<Self, String> {
        let uri_str = uri_str.trim();
        if !uri_str.starts_with("agentdesk://pair?") && !uri_str.starts_with("agentdesk://pair/?") {
            return Err("invalid scheme or endpoint; expected agentdesk://pair?...".into());
        }

        let query = match uri_str.split_once('?') {
            Some((_, q)) => q,
            None => return Err("missing query string in pairing URI".into()),
        };

        let mut version = None;
        let mut instance_id = None;
        let mut session_id = None;
        let mut pairing_secret = None;
        let mut fingerprint = None;
        let mut lan_address = None;
        let mut relay_url = None;

        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let val = urlencoding::decode(v).map_err(|e| format!("URL decode error: {e}"))?;
            match k {
                "v" => {
                    let ver = val
                        .parse::<u32>()
                        .map_err(|_| "invalid version number in pairing payload")?;
                    if ver != PAIRING_PROTOCOL_VERSION {
                        return Err(format!(
                            "unsupported pairing protocol version: {ver} (expected {PAIRING_PROTOCOL_VERSION})"
                        ));
                    }
                    version = Some(ver);
                }
                "inst" => {
                    if !val.trim().is_empty() {
                        instance_id = Some(val.into_owned());
                    }
                }
                "sess" => {
                    if !val.trim().is_empty() {
                        session_id = Some(val.into_owned());
                    }
                }
                "sec" => {
                    let sec = val.trim();
                    if sec.len() != 64 || !sec.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Err("invalid pairing secret; must be 64-char hex".into());
                    }
                    pairing_secret = Some(sec.to_string());
                }
                "fp" => {
                    if !val.trim().is_empty() {
                        fingerprint = Some(val.into_owned());
                    }
                }
                "lan" => {
                    if !val.trim().is_empty() {
                        lan_address = Some(val.into_owned());
                    }
                }
                "relay" => {
                    if !val.trim().is_empty() {
                        relay_url = Some(val.into_owned());
                    }
                }
                _ => {} // Ignore forward-compatible unknown query params
            }
        }

        let version = version.ok_or_else(|| "missing version in pairing payload".to_string())?;
        let instance_id =
            instance_id.ok_or_else(|| "missing instance_id in pairing payload".to_string())?;
        let session_id =
            session_id.ok_or_else(|| "missing session_id in pairing payload".to_string())?;
        let pairing_secret =
            pairing_secret.ok_or_else(|| "missing pairing_secret in pairing payload".to_string())?;
        let fingerprint =
            fingerprint.ok_or_else(|| "missing fingerprint in pairing payload".to_string())?;

        Ok(Self {
            version,
            instance_id,
            session_id,
            pairing_secret,
            fingerprint,
            lan_address,
            relay_url,
        })
    }
}

pub fn pairing_session_path(config_dir: &Path) -> PathBuf {
    config_dir.join("pairing_session.json")
}

pub fn save_session_file(config_dir: &Path, session: &PairingSession) -> std::io::Result<()> {
    fs::create_dir_all(config_dir)?;
    let target = pairing_session_path(config_dir);
    let tmp = config_dir.join(format!(".pairing_session_{}.tmp", Uuid::new_v4()));
    let json = serde_json::to_string_pretty(session)?;
    fs::write(&tmp, json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&tmp) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = fs::set_permissions(&tmp, perms);
        }
    }
    fs::rename(tmp, target)?;
    Ok(())
}

pub fn load_session_file(config_dir: &Path) -> Option<PairingSession> {
    let target = pairing_session_path(config_dir);
    let content = fs::read_to_string(target).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn remove_session_file(config_dir: &Path) {
    let target = pairing_session_path(config_dir);
    let _ = fs::remove_file(target);
}

/// Active pairing session (in-memory and optional file sync).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingSession {
    pub session_id: String,
    pub pairing_secret: String,
    pub instance_id: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub used: bool,
    #[serde(default)]
    pub paired_device_id: Option<String>,
    #[serde(default)]
    pub paired_device_name: Option<String>,
    #[serde(default)]
    pub lan_address: Option<String>,
    pub fingerprint: String,
    #[serde(default)]
    pub relay_url: Option<String>,
}

/// Manages ephemeral pairing sessions with single-use, expiration, and constant-time verification.
#[derive(Debug, Default, Clone)]
pub struct PairingManager {
    inner: Arc<Mutex<HashMap<String, PairingSession>>>,
    config_dir: Option<PathBuf>,
}

impl PairingManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            config_dir: None,
        }
    }

    pub fn with_dir(config_dir: PathBuf) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            config_dir: Some(config_dir),
        }
    }

    pub fn config_dir(&self) -> Option<&Path> {
        self.config_dir.as_deref()
    }

    /// Create a new short-lived pairing session.
    pub fn create_session(
        &self,
        instance_id: String,
        fingerprint: String,
        lan_address: Option<String>,
        relay_url: Option<String>,
        duration_secs: Option<i64>,
        now: DateTime<Utc>,
    ) -> (PairingSession, PairingPayload, String) {
        let session_id = Uuid::new_v4().to_string();
        let pairing_secret = generate_token(); // 256-bit random hex string
        let duration = duration_secs.unwrap_or(PAIRING_DEFAULT_DURATION_SECS);
        let created_at = now;
        let expires_at = created_at + Duration::seconds(duration);

        let session = PairingSession {
            session_id: session_id.clone(),
            pairing_secret: pairing_secret.clone(),
            instance_id: instance_id.clone(),
            created_at,
            expires_at,
            used: false,
            paired_device_id: None,
            paired_device_name: None,
            lan_address: lan_address.clone(),
            fingerprint: fingerprint.clone(),
            relay_url: relay_url.clone(),
        };

        let payload = PairingPayload {
            version: PAIRING_PROTOCOL_VERSION,
            instance_id,
            session_id: session_id.clone(),
            pairing_secret,
            fingerprint,
            lan_address,
            relay_url,
        };

        let uri = payload.to_uri();

        let mut lock = self.inner.lock().unwrap();
        // Clear any old sessions to ensure single active pairing session
        lock.clear();
        lock.insert(session_id, session.clone());

        if let Some(ref dir) = self.config_dir {
            let _ = save_session_file(dir, &session);
        }

        (session, payload, uri)
    }

    /// Validate and consume a pairing attempt.
    /// Single-use: once verified, the session is permanently marked used and cannot be replayed.
    pub fn validate_and_consume(
        &self,
        session_id: &str,
        provided_secret: &str,
        now: DateTime<Utc>,
    ) -> Result<PairingSession, PairingError> {
        let mut lock = self.inner.lock().unwrap();

        // Fallback: check file if session is not currently in memory
        if !lock.contains_key(session_id) {
            if let Some(ref dir) = self.config_dir {
                if let Some(file_sess) = load_session_file(dir) {
                    if file_sess.session_id == session_id {
                        lock.insert(session_id.to_string(), file_sess);
                    }
                }
            }
        }

        let session = lock.get_mut(session_id).ok_or(PairingError::WrongSession)?;

        if session.used {
            return Err(PairingError::Replayed);
        }

        if now > session.expires_at {
            return Err(PairingError::Expired);
        }

        let p = provided_secret.as_bytes();
        let e = session.pairing_secret.as_bytes();
        if p.len() != e.len() || !bool::from(p.ct_eq(e)) {
            return Err(PairingError::InvalidSecret);
        }

        // Single-use: immediately mark used!
        session.used = true;
        let consumed = session.clone();

        if let Some(ref dir) = self.config_dir {
            let _ = save_session_file(dir, &consumed);
        }

        Ok(consumed)
    }

    /// Record newly paired device information on the session.
    pub fn record_paired_device(
        &self,
        session_id: &str,
        device_id: &str,
        device_name: &str,
    ) {
        let mut lock = self.inner.lock().unwrap();
        if let Some(session) = lock.get_mut(session_id) {
            session.paired_device_id = Some(device_id.to_string());
            session.paired_device_name = Some(device_name.to_string());
            if let Some(ref dir) = self.config_dir {
                let _ = save_session_file(dir, session);
            }
        } else if let Some(ref dir) = self.config_dir {
            if let Some(mut file_sess) = load_session_file(dir) {
                if file_sess.session_id == session_id {
                    file_sess.paired_device_id = Some(device_id.to_string());
                    file_sess.paired_device_name = Some(device_name.to_string());
                    let _ = save_session_file(dir, &file_sess);
                    lock.insert(session_id.to_string(), file_sess);
                }
            }
        }
    }

    /// Invalidate any active pairing sessions.
    pub fn clear(&self) {
        self.inner.lock().unwrap().clear();
        if let Some(ref dir) = self.config_dir {
            remove_session_file(dir);
        }
    }
}

/// Render a terminal-friendly compact QR code string.
pub fn render_qr_terminal(content: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let code = QrCode::new(content.as_bytes())?;
    let rendered = code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Dark)
        .light_color(unicode::Dense1x2::Light)
        .quiet_zone(true)
        .build();
    Ok(rendered)
}

/// Detect the most likely LAN IP address of this machine for pairing.
pub fn detect_lan_ip() -> Option<IpAddr> {
    // Attempt connecting a UDP socket to a public address (no packet is sent)
    // to discover which local interface the kernel routes traffic through.
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let local_addr = socket.local_addr().ok()?;
    let ip = local_addr.ip();
    if !ip.is_loopback() {
        Some(ip)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_uri_round_trip() {
        let payload = PairingPayload {
            version: 1,
            instance_id: "inst-123".into(),
            session_id: "sess-456".into(),
            pairing_secret: generate_token(),
            fingerprint: "AA:BB:CC:DD".into(),
            lan_address: Some("192.168.1.50:8765".into()),
            relay_url: Some("wss://relay.agentdesk.net".into()),
        };

        let uri = payload.to_uri();
        assert!(uri.starts_with("agentdesk://pair?"));

        let parsed = PairingPayload::from_uri(&uri).unwrap();
        assert_eq!(payload, parsed);
    }

    #[test]
    fn payload_rejects_unsupported_version() {
        let bad_ver_uri = "agentdesk://pair?v=99&inst=inst-1&sess=s-1&sec=0000000000000000000000000000000000000000000000000000000000000000&fp=AA:BB";
        let err = PairingPayload::from_uri(bad_ver_uri).unwrap_err();
        assert!(err.contains("unsupported pairing protocol version"));
    }

    #[test]
    fn payload_rejects_malformed_secret() {
        let bad_sec_uri = "agentdesk://pair?v=1&inst=inst-1&sess=s-1&sec=too-short&fp=AA:BB";
        let err = PairingPayload::from_uri(bad_sec_uri).unwrap_err();
        assert!(err.contains("invalid pairing secret"));
    }

    #[test]
    fn payload_rejects_invalid_scheme() {
        let bad_scheme = "http://pair?v=1&inst=inst-1&sess=s-1&sec=0000000000000000000000000000000000000000000000000000000000000000&fp=AA:BB";
        let err = PairingPayload::from_uri(bad_scheme).unwrap_err();
        assert!(err.contains("invalid scheme"));
    }

    #[test]
    fn pairing_manager_lifecycle_and_security() {
        let manager = PairingManager::new();
        let instance_id = "inst-laptop-1";
        let fp = "11:22:33:44";

        let now = Utc::now();

        let (session, payload, _uri) = manager.create_session(
            instance_id.into(),
            fp.into(),
            Some("10.0.0.5:8765".into()),
            None,
            Some(60), // 60 seconds
            now,
        );

        // 1. Invalid secret -> rejected
        let err = manager
            .validate_and_consume(&session.session_id, "wrong-secret-000000000000000000000000000000000000000000000000000000", now)
            .unwrap_err();
        assert_eq!(err, PairingError::InvalidSecret);

        // 2. Valid secret -> success
        let validated = manager
            .validate_and_consume(&session.session_id, &payload.pairing_secret, now)
            .unwrap();
        assert_eq!(validated.session_id, session.session_id);
        assert_eq!(validated.instance_id, instance_id);

        // 3. Replay with same secret -> rejected (single-use)
        let replay_err = manager
            .validate_and_consume(&session.session_id, &payload.pairing_secret, now)
            .unwrap_err();
        assert_eq!(replay_err, PairingError::Replayed);

        // 4. Expiration check
        let (exp_session, exp_payload, _) = manager.create_session(
            instance_id.into(),
            fp.into(),
            None,
            None,
            Some(10),
            now,
        );
        let future_time = now + Duration::seconds(15);
        let exp_err = manager
            .validate_and_consume(&exp_session.session_id, &exp_payload.pairing_secret, future_time)
            .unwrap_err();
        assert_eq!(exp_err, PairingError::Expired);
    }

    #[test]
    fn render_qr_code_succeeds() {
        let uri = "agentdesk://pair?v=1&inst=inst-1&sess=sess-1&sec=0000000000000000000000000000000000000000000000000000000000000000&fp=AA:BB";
        let qr = render_qr_terminal(uri).unwrap();
        assert!(!qr.is_empty());
        assert!(qr.contains('\n'));
    }
}
