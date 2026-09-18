//! Token lifecycle and constant-time authentication.
//! See docs/SECURITY.md and P6.5.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use rand::Rng;
use subtle::ConstantTimeEq;

/// Verify a provided token against the expected token in constant time.
pub fn verify_token(provided: &str, expected: &str) -> bool {
    let p = provided.as_bytes();
    let e = expected.as_bytes();
    if p.len() != e.len() {
        return false;
    }
    p.ct_eq(e).into()
}

/// Generate a cryptographically random 256-bit token formatted as a 64-character hex string.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Resolve the default configuration directory.
/// Priority:
/// 1. `AGENTDESK_CONFIG_DIR` environment variable
/// 2. `XDG_CONFIG_HOME/agentdesk`
/// 3. `HOME/.config/agentdesk`
/// 4. Current working directory `.agentdesk`
pub fn default_config_dir() -> PathBuf {
    if let Some(dir) = std::env::var("AGENTDESK_CONFIG_DIR").ok().filter(|d| !d.trim().is_empty()) {
        return PathBuf::from(dir);
    }

    if let Some(xdg) = std::env::var("XDG_CONFIG_HOME").ok().filter(|d| !d.trim().is_empty()) {
        return PathBuf::from(xdg).join("agentdesk");
    }

    if let Some(home) = std::env::var("HOME").ok().filter(|d| !d.trim().is_empty()) {
        return PathBuf::from(home).join(".config").join("agentdesk");
    }

    PathBuf::from(".agentdesk")
}

/// Return the default token file path within the given config directory.
pub fn token_path(config_dir: &Path) -> PathBuf {
    config_dir.join("token")
}

/// Save a token with strict 0600 permissions (and 0700 on parent dir).
pub fn save_token(path: &Path, token: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
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
        file.write_all(token.trim().as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()?;
    }

    #[cfg(not(unix))]
    {
        fs::write(path, format!("{}\n", token.trim()))?;
    }

    Ok(())
}

/// Load an existing token from path, or generate and save a new 256-bit token with 0600 permissions.
pub fn load_or_generate_token(path: &Path) -> io::Result<String> {
    if path.exists() {
        let content = fs::read_to_string(path)?;
        let token = content.trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }

    let token = generate_token();
    save_token(path, &token)?;
    Ok(token)
}

/// Rotate token by generating a new 256-bit token and overwriting the token file with 0600 permissions.
pub fn rotate_token(path: &Path) -> io::Result<String> {
    let token = generate_token();
    save_token(path, &token)?;
    Ok(token)
}

/// Check whether an address is a loopback address.
pub fn is_loopback_addr(addr: &str) -> bool {
    let clean = addr.trim().trim_matches('[').trim_matches(']');
    clean == "127.0.0.1" || clean == "::1" || clean == "localhost"
}

/// Validate network bind security parameters according to SECURITY.md and P6.5 / P6.6.
pub fn validate_bind_security(
    bind_addr: &str,
    token: &str,
    insecure_dev: bool,
) -> Result<(), String> {
    let clean = bind_addr.trim().trim_matches('[').trim_matches(']');

    // P6.6: Insecure dev mode is strictly restricted to 127.0.0.1.
    // Rejected in combination with any other bind address.
    if insecure_dev && clean != "127.0.0.1" {
        return Err(format!(
            "--insecure-dev mode is strictly restricted to 127.0.0.1 loopback address, got '{bind_addr}'"
        ));
    }

    // P6.5: Refuse non-loopback bind without a token.
    if !is_loopback_addr(clean) && token.trim().is_empty() {
        return Err(format!(
            "refusing to bind to non-loopback address '{bind_addr}' without a configured token"
        ));
    }

    if token.trim().is_empty() {
        return Err("token cannot be empty".into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_token_constant_time() {
        let token = generate_token();
        assert_eq!(token.len(), 64);
        assert!(verify_token(&token, &token));
        assert!(!verify_token(&token, "wrong-token"));
        assert!(!verify_token(&token, ""));
    }

    #[test]
    fn token_file_save_and_load() {
        let temp_dir = std::env::temp_dir().join(format!("agentdesk_test_{}", generate_token()));
        let path = token_path(&temp_dir);

        let tok1 = load_or_generate_token(&path).unwrap();
        assert_eq!(tok1.len(), 64);
        assert!(path.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&path).unwrap();
            let mode = metadata.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        let tok2 = load_or_generate_token(&path).unwrap();
        assert_eq!(tok1, tok2);

        let tok3 = rotate_token(&path).unwrap();
        assert_eq!(tok3.len(), 64);
        assert_ne!(tok1, tok3);

        let tok4 = load_or_generate_token(&path).unwrap();
        assert_eq!(tok3, tok4);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn insecure_dev_rejects_non_loopback() {
        assert!(validate_bind_security("127.0.0.1", "valid_token", true).is_ok());
        assert!(validate_bind_security("0.0.0.0", "valid_token", true).is_err());
        assert!(validate_bind_security("192.168.1.10", "valid_token", true).is_err());
        assert!(validate_bind_security("localhost", "valid_token", true).is_err());
        assert!(validate_bind_security("::1", "valid_token", true).is_err());
    }

    #[test]
    fn non_loopback_refuses_empty_token() {
        assert!(validate_bind_security("0.0.0.0", "", false).is_err());
        assert!(validate_bind_security("192.168.1.10", "   ", false).is_err());
        assert!(validate_bind_security("0.0.0.0", "some-token", false).is_ok());
    }
}
