// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Serving the Runner API over HTTPS (SRS-IF-010).
//!
//! With a `tls:` section in its configuration the Runner serves HTTPS only,
//! with the configured certificate chain and key, using rustls and the ring
//! crypto provider (no OpenSSL). The certificate and key are loaded and
//! checked at start-up, so a bad file stops the Runner before it binds.

use std::{net::SocketAddr, path::Path, sync::Arc};

use anyhow::{Context, Result};
use axum::Router;
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto,
    service::TowerToHyperService,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use crate::config::TlsConfig;

/// Load the certificate chain and private key of `tls` into a rustls server
/// configuration.
///
/// # Errors
///
/// Returns an error naming the file if either cannot be read or holds no
/// certificate or key, and an error if the key does not belong to the leaf
/// certificate.
pub fn load_server_config(tls: &TlsConfig) -> Result<Arc<rustls::ServerConfig>> {
    let certs = read_certs(&tls.cert)?;
    let key = PrivateKeyDer::from_pem_file(&tls.key).with_context(|| {
        format!("tls.key: no PEM private key could be read from {}", tls.key.display())
    })?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .context("tls: the ring provider supports no default protocol version")?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("tls: the private key does not match the certificate, or is unsupported")?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

fn read_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let certs = CertificateDer::pem_file_iter(path)
        .with_context(|| format!("tls.cert: cannot read {}", path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("tls.cert: {} is not valid PEM", path.display()))?;
    anyhow::ensure!(!certs.is_empty(), "tls.cert: {} holds no certificate", path.display());
    Ok(certs)
}

/// Serve `app` over HTTPS on `listener` until the listener fails.
///
/// Each connection is handled on its own task; a client that fails the TLS
/// handshake (for example one speaking plain HTTP) only loses its connection.
///
/// # Errors
///
/// Returns an error if accepting a connection fails.
pub async fn serve(
    listener: TcpListener,
    app: Router,
    tls: Arc<rustls::ServerConfig>,
) -> Result<()> {
    let acceptor = TlsAcceptor::from(tls);
    loop {
        let (stream, peer) = listener.accept().await.context("tls: accepting a connection")?;
        let acceptor = acceptor.clone();
        let app = app.clone();
        tokio::spawn(async move { handle(acceptor, stream, peer, app).await });
    }
}

async fn handle(
    acceptor: TlsAcceptor,
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    app: Router,
) {
    let stream = match acceptor.accept(stream).await {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(%peer, error = %e, "TLS handshake failed");
            return;
        }
    };
    let service = TowerToHyperService::new(app);
    if let Err(e) = auto::Builder::new(TokioExecutor::new())
        .serve_connection(TokioIo::new(stream), service)
        .await
    {
        tracing::debug!(%peer, error = %e, "HTTPS connection ended with an error");
    }
}

#[cfg(test)]
mod tests {
    use axum::routing::get;

    use super::*;

    /// A self-signed certificate for `localhost`, written to `dir` as PEM.
    fn self_signed(dir: &Path) -> (TlsConfig, String) {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let cert_pem = cert.cert.pem();
        let tls = TlsConfig { cert: dir.join("cert.pem"), key: dir.join("key.pem") };
        std::fs::write(&tls.cert, &cert_pem).unwrap();
        std::fs::write(&tls.key, cert.signing_key.serialize_pem()).unwrap();
        (tls, cert_pem)
    }

    /// Serve a health route over TLS on an ephemeral port; returns the port.
    async fn serve_health(tls: &TlsConfig) -> u16 {
        let config = load_server_config(tls).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new().route("/api/v1/health", get(|| async { "ok" }));
        tokio::spawn(serve(listener, app, config));
        port
    }

    /// Verifies: SRS-IF-010, SRS-INST-001
    #[tokio::test]
    async fn serves_https_to_a_client_that_trusts_the_certificate() {
        let dir = tempfile::tempdir().unwrap();
        let (tls, cert_pem) = self_signed(dir.path());
        let port = serve_health(&tls).await;

        let client = reqwest::Client::builder()
            .add_root_certificate(reqwest::Certificate::from_pem(cert_pem.as_bytes()).unwrap())
            .resolve("localhost", ([127, 0, 0, 1], port).into())
            .build()
            .unwrap();
        let resp =
            client.get(format!("https://localhost:{port}/api/v1/health")).send().await.unwrap();
        assert!(resp.status().is_success());
        assert_eq!(resp.text().await.unwrap(), "ok");
    }

    /// Verifies: SRS-IF-010
    #[tokio::test]
    async fn refuses_plain_http_and_untrusted_clients() {
        let dir = tempfile::tempdir().unwrap();
        let (tls, _) = self_signed(dir.path());
        let port = serve_health(&tls).await;

        // Plain HTTP on the TLS port gets no HTTP answer.
        let plain = reqwest::get(format!("http://127.0.0.1:{port}/api/v1/health")).await;
        assert!(plain.is_err(), "plain HTTP must not be served: {plain:?}");

        // A client that does not trust the certificate fails verification.
        let untrusted = reqwest::Client::builder()
            .resolve("localhost", ([127, 0, 0, 1], port).into())
            .build()
            .unwrap()
            .get(format!("https://localhost:{port}/api/v1/health"))
            .send()
            .await;
        assert!(untrusted.is_err(), "an unverified certificate must be refused");
    }

    /// Verifies: SRS-IF-010
    #[test]
    fn missing_or_malformed_files_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let (good, _) = self_signed(dir.path());

        let missing = TlsConfig { cert: dir.path().join("nope.pem"), key: good.key.clone() };
        let err = format!("{:#}", load_server_config(&missing).unwrap_err());
        assert!(err.contains("tls.cert") && err.contains("nope.pem"), "{err}");

        let garbage = dir.path().join("garbage.pem");
        std::fs::write(&garbage, "not a certificate").unwrap();
        let no_cert = TlsConfig { cert: garbage.clone(), key: good.key.clone() };
        assert!(format!("{:#}", load_server_config(&no_cert).unwrap_err()).contains("tls.cert"));

        let no_key = TlsConfig { cert: good.cert.clone(), key: garbage };
        assert!(format!("{:#}", load_server_config(&no_key).unwrap_err()).contains("tls.key"));
    }

    /// Verifies: SRS-IF-010
    #[test]
    fn a_key_of_another_certificate_is_rejected() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let (tls_a, _) = self_signed(a.path());
        let (tls_b, _) = self_signed(b.path());
        let mixed = TlsConfig { cert: tls_a.cert, key: tls_b.key };
        assert!(load_server_config(&mixed).is_err());
    }
}
