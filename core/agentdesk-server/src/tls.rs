//! TLS certificate generation, loading, and fingerprint computation.
//! See docs/SECURITY.md and P8.1, P8.2.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rcgen::generate_simple_self_signed;
use rustls::ServerConfig as RustlsServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};
use tokio_rustls::TlsAcceptor;

pub fn cert_path(config_dir: &Path) -> PathBuf {
    config_dir.join("cert.pem")
}

pub fn key_path(config_dir: &Path) -> PathBuf {
    config_dir.join("key.pem")
}

pub fn compute_sha256_fingerprint(der: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(der);
    let hash = hasher.finalize();
    hash.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(":")
}

pub fn format_fingerprint_hex(fingerprint: &str) -> String {
    fingerprint.replace(':', "").to_lowercase()
}

pub struct TlsIdentity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
    pub fingerprint: String,
}

impl TlsIdentity {
    pub fn generate() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let subject_alt_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
        let cert = generate_simple_self_signed(subject_alt_names)?;
        let cert_der = cert.cert.der().to_vec();
        let key_der = cert.signing_key.serialize_der();
        let fingerprint = compute_sha256_fingerprint(&cert_der);
        Ok(Self {
            cert_der,
            key_der,
            fingerprint,
        })
    }

    pub fn save(&self, config_dir: &Path) -> io::Result<()> {
        fs::create_dir_all(config_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(config_dir, fs::Permissions::from_mode(0o700));
        }

        let cert_file = cert_path(config_dir);
        let key_file = key_path(config_dir);

        // Write DER bytes or PEM
        fs::write(&cert_file, &self.cert_der)?;
        fs::write(&key_file, &self.key_der)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&cert_file, fs::Permissions::from_mode(0o600));
            let _ = fs::set_permissions(&key_file, fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }

    pub fn load(config_dir: &Path) -> io::Result<Self> {
        let cert_file = cert_path(config_dir);
        let key_file = key_path(config_dir);

        let cert_der = fs::read(&cert_file)?;
        let key_der = fs::read(&key_file)?;
        let fingerprint = compute_sha256_fingerprint(&cert_der);

        Ok(Self {
            cert_der,
            key_der,
            fingerprint,
        })
    }

    pub fn load_or_generate(
        config_dir: &Path,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let cert_file = cert_path(config_dir);
        let key_file = key_path(config_dir);

        if cert_file.exists() && key_file.exists() {
            match Self::load(config_dir) {
                Ok(id) => return Ok(id),
                Err(e) => {
                    eprintln!(
                        "Warning: failed loading existing TLS credentials ({e}), regenerating..."
                    );
                }
            }
        }

        let identity = Self::generate()?;
        identity.save(config_dir)?;
        Ok(identity)
    }

    pub fn build_tls_acceptor(
        &self,
    ) -> Result<TlsAcceptor, Box<dyn std::error::Error + Send + Sync>> {
        let cert = CertificateDer::from(self.cert_der.clone());
        let key = PrivateKeyDer::try_from(self.key_der.clone())
            .map_err(|e| format!("Invalid private key: {:?}", e))?;

        let server_config = RustlsServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)?;

        Ok(TlsAcceptor::from(Arc::new(server_config)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_and_fingerprint() {
        let id = TlsIdentity::generate().unwrap();
        assert!(!id.cert_der.is_empty());
        assert!(!id.key_der.is_empty());
        assert_eq!(id.fingerprint.matches(':').count(), 31);
        assert_eq!(format_fingerprint_hex(&id.fingerprint).len(), 64);
    }

    #[test]
    fn build_tls_acceptor_succeeds() {
        let id = TlsIdentity::generate().unwrap();
        let acceptor = id.build_tls_acceptor();
        assert!(acceptor.is_ok());
    }
}
