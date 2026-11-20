// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! HTTPS to a Runner whose certificate comes from a private CA (SRS-IF-010):
//! the client verifies the certificate, trusting the configured CA certificate
//! in addition to the public roots.
//!
//! The fixtures under `tests/fixtures/tls/` are a test-only CA certificate and
//! a server certificate and key for `localhost` and `127.0.0.1`, valid for
//! 100 years. The CA key was discarded after signing.

use std::{path::PathBuf, sync::Arc};

use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use sqt_reporter::api;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tls").join(name)
}

/// A one-request-at-a-time HTTPS server answering every request with a valid
/// `VersionResponse`. Returns its base URL.
async fn spawn_tls_runner() -> String {
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(fixture("server.pem"))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_file(fixture("server.key")).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(certs, key)
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else { return };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                // A client that does not trust the certificate fails the handshake.
                let Ok(mut tls) = acceptor.accept(tcp).await else { return };
                let mut buf = vec![0u8; 4096];
                let mut req = Vec::new();
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    match tls.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => req.extend_from_slice(&buf[..n]),
                    }
                }
                let body = r#"{"api_version":"1.0","runner_version":"0.1.0-tls","target_id":"test","os":"linux","toolchain_versions":{}}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tls.write_all(resp.as_bytes()).await;
                let _ = tls.shutdown().await;
            });
        }
    });
    format!("https://localhost:{port}")
}

fn options() -> api::client::ClientOptions {
    api::client::ClientOptions { retry_max: 0, ..api::client::ClientOptions::default() }
}

/// Verifies: SRS-IF-010
#[tokio::test]
async fn a_runner_certificate_from_the_configured_ca_is_trusted() {
    let url = spawn_tls_runner().await;
    let ca = std::fs::read(fixture("ca.pem")).unwrap();

    let client = api::ApiClient::with_ca_cert(&url, Some("tok"), options(), Some(&ca)).unwrap();
    let version = client.get_version().await.expect("the CA-signed certificate verifies");

    assert_eq!(version.runner_version, "0.1.0-tls");
}

/// Verifies: SRS-IF-010
#[tokio::test]
async fn a_runner_certificate_from_an_unknown_ca_is_refused() {
    let url = spawn_tls_runner().await;

    let client = api::ApiClient::new(&url, Some("tok"), options()).unwrap();
    let err = client.get_version().await.expect_err("an untrusted certificate must be refused");

    let text = format!("{err:#}").to_lowercase();
    assert!(text.contains("certificate") || text.contains("tls"), "{text}");
}
