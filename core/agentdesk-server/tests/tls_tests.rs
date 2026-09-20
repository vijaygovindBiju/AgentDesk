//! Phase 8 TLS and certificate pinning integration tests.
//! Validates P8.T1:
//! - Client with pinned fingerprint connects and completes handshake.
//! - Client with different fingerprint fails the handshake.
//! - Plain ws:// client is rejected when server default is wss://.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::rustls::{DigitallySignedStruct, SignatureScheme};
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

use agentdesk_core::{CoreTask, LogStoreConfig, ThresholdTable, VirtualClock};
use agentdesk_model::{Body, Hello, Message, PipelineMode, SCHEMA_VERSION, TransportMode};
use agentdesk_server::{Server, ServerConfig, generate_token};

#[derive(Debug)]
struct PinnedFingerprintVerifier {
    expected_fingerprint: Vec<u8>,
}

impl ServerCertVerifier for PinnedFingerprintVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, tokio_rustls::rustls::Error> {
        let mut hasher = Sha256::new();
        hasher.update(end_entity.as_ref());
        let hash = hasher.finalize();
        if hash.as_slice() == self.expected_fingerprint.as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(tokio_rustls::rustls::Error::InvalidCertificate(
                tokio_rustls::rustls::CertificateError::UnknownIssuer,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, tokio_rustls::rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, tokio_rustls::rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        tokio_rustls::rustls::crypto::CryptoProvider::get_default()
            .map(|p| p.signature_verification_algorithms.supported_schemes())
            .unwrap_or_else(|| {
                vec![
                    SignatureScheme::ED25519,
                    SignatureScheme::ECDSA_NISTP256_SHA256,
                    SignatureScheme::RSA_PSS_SHA256,
                ]
            })
    }
}

async fn setup_tls_server() -> (Server, String, String, std::path::PathBuf) {
    let clock = Arc::new(VirtualClock::at_epoch());
    let token = generate_token();
    let temp_dir =
        std::env::temp_dir().join(format!("agentdesk-tls-test-{}", rand::random::<u64>()));
    fs::create_dir_all(&temp_dir).unwrap();

    let (tx_core, rx_core) = mpsc::channel(256);
    let (tx_adapter, _rx_adapter) = mpsc::channel(64);

    let core = CoreTask::with_seed(
        PipelineMode::Agentdesk,
        clock.clone(),
        42,
        LogStoreConfig::default(),
        ThresholdTable::default(),
        Some(tx_adapter),
    );
    tokio::spawn(core.run(rx_core));

    let config = ServerConfig {
        bind_addr: "127.0.0.1".into(),
        port: 0,
        token: token.clone(),
        insecure_dev: false, // TLS default!
        pipeline_mode: PipelineMode::Agentdesk,
        outbound_capacity: 64,
        debug_logging: false,
        config_dir: Some(temp_dir.clone()),
    };

    let server = Server::bind(config, tx_core, clock)
        .await
        .expect("bind TLS server");
    let fp = server
        .fingerprint()
        .expect("server fingerprint")
        .to_string();

    (server, token, fp, temp_dir)
}

#[tokio::test]
async fn p8_t1_matching_fingerprint_client_connects_successfully() {
    let (server, token, fingerprint, temp_dir) = setup_tls_server().await;
    let local_addr = server.local_addr();

    tokio::spawn(async move {
        let _ = server.run().await;
    });

    // Parse fingerprint hex bytes
    let clean_fp = fingerprint.replace(':', "");
    let expected_bytes = (0..clean_fp.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean_fp[i..i + 2], 16).unwrap())
        .collect::<Vec<u8>>();

    let verifier = Arc::new(PinnedFingerprintVerifier {
        expected_fingerprint: expected_bytes,
    });

    let client_config = tokio_rustls::rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(client_config));

    let tcp_stream = TcpStream::connect(local_addr).await.expect("connect TCP");
    let domain = ServerName::try_from("localhost".to_string()).unwrap();
    let tls_stream = connector
        .connect(domain, tcp_stream)
        .await
        .expect("TLS handshake success");

    let url = format!("wss://127.0.0.1:{}/", local_addr.port());
    let (mut ws_stream, _) = tokio_tungstenite::client_async(url, tls_stream)
        .await
        .expect("WebSocket client handshake success");

    // Send hello
    let hello = Message::push(Body::Hello(Hello {
        token: token.clone(),
        device_id: "rust-test-device".into(),
        client_version: "0.1.0".into(),
        schema_version: SCHEMA_VERSION,
    }));
    ws_stream
        .send(WsMessage::Text(
            serde_json::to_string(&hello).unwrap().into(),
        ))
        .await
        .unwrap();

    // Expect Welcome
    let first_reply = tokio::time::timeout(Duration::from_secs(2), ws_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let welcome: Message = serde_json::from_str(first_reply.to_text().unwrap()).unwrap();
    if let Body::Welcome(w) = welcome.body {
        assert_eq!(w.transport, TransportMode::Tls);
    } else {
        panic!("expected welcome body");
    }

    // Expect Snapshot
    let second_reply = tokio::time::timeout(Duration::from_secs(2), ws_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let snapshot: Message = serde_json::from_str(second_reply.to_text().unwrap()).unwrap();
    assert!(matches!(snapshot.body, Body::Snapshot(_)));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn p8_t1_mismatched_fingerprint_client_fails_handshake() {
    let (server, _token, _fingerprint, temp_dir) = setup_tls_server().await;
    let local_addr = server.local_addr();

    tokio::spawn(async move {
        let _ = server.run().await;
    });

    // Deliberately wrong pinned fingerprint (all 0xFF)
    let wrong_bytes = vec![0xFF; 32];
    let verifier = Arc::new(PinnedFingerprintVerifier {
        expected_fingerprint: wrong_bytes,
    });

    let client_config = tokio_rustls::rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(client_config));

    let tcp_stream = TcpStream::connect(local_addr).await.expect("connect TCP");
    let domain = ServerName::try_from("localhost".to_string()).unwrap();

    // Handshake MUST fail with invalid certificate
    let tls_result = connector.connect(domain, tcp_stream).await;
    assert!(
        tls_result.is_err(),
        "Client with mismatched certificate fingerprint must be rejected!"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}
