// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Serving the Runner API over HTTPS (SRS-IF-010).
//!
//! With a `tls:` section in its configuration the Runner serves HTTPS only,
//! with the configured certificate chain and key, using rustls and the ring
//! crypto provider (no OpenSSL). The certificate and key are loaded and
//! checked at start-up, so a bad file stops the Runner before it binds.

use std::{net::SocketAddr, path::Path, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::Router;
use hyper_util::{
    rt::{TokioExecutor, TokioIo, TokioTimer},
    server::conn::auto,
    service::TowerToHyperService,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use tokio::{net::TcpListener, sync::Semaphore};
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

/// Limits that keep slow or idle clients from tying up the Runner.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Most connections served at once; further clients wait in the listen
    /// backlog until one closes.
    pub max_connections: usize,
    /// Time a client has to complete the TLS handshake.
    pub handshake_timeout: Duration,
    /// Time a client has to send the headers of an HTTP/1 request.
    pub header_read_timeout: Duration,
}

impl Default for Limits {
    /// 256 connections, 10 s for the handshake, 30 s for request headers.
    fn default() -> Self {
        Self {
            max_connections: 256,
            handshake_timeout: Duration::from_secs(10),
            header_read_timeout: Duration::from_secs(30),
        }
    }
}

/// Serve `app` over HTTPS on `listener` with the default [`Limits`], for as
/// long as the process runs.
pub async fn serve(listener: TcpListener, app: Router, tls: Arc<rustls::ServerConfig>) {
    serve_with(listener, app, tls, Limits::default()).await;
}

/// Serve `app` over HTTPS on `listener` within `limits`.
///
/// Each connection is handled on its own task; a client that fails the TLS
/// handshake (for example one speaking plain HTTP), takes longer than the
/// handshake timeout for it, or sends no request headers within the header
/// timeout only loses its connection. A failed `accept` (for example when the
/// process is out of file descriptors) is logged and the loop goes on, as
/// `axum::serve` does on the plain-HTTP path, so one bad moment does not stop
/// the Runner and its queued jobs.
pub async fn serve_with(
    listener: TcpListener,
    app: Router,
    tls: Arc<rustls::ServerConfig>,
    limits: Limits,
) {
    let acceptor = TlsAcceptor::from(tls);
    let slots = Arc::new(Semaphore::new(limits.max_connections));
    loop {
        let Ok(slot) = slots.clone().acquire_owned().await else {
            return; // the semaphore is never closed
        };
        let (stream, peer) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                tracing::warn!(error = %e, "tls: accepting a connection failed");
                tokio::time::sleep(accept_backoff(&e)).await;
                continue;
            }
        };
        let acceptor = acceptor.clone();
        let app = app.clone();
        tokio::spawn(async move {
            handle(acceptor, stream, peer, app, limits).await;
            drop(slot);
        });
    }
}

/// How long to wait after a failed `accept`: a second when the process or
/// system is out of file descriptors, so connections can close first;
/// otherwise (an aborted or reset connection) not noticeably.
fn accept_backoff(e: &std::io::Error) -> Duration {
    // EMFILE (24) and ENFILE (23) on Linux.
    if matches!(e.raw_os_error(), Some(23 | 24)) {
        Duration::from_secs(1)
    } else {
        Duration::from_millis(10)
    }
}

async fn handle(
    acceptor: TlsAcceptor,
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    app: Router,
    limits: Limits,
) {
    let handshake = tokio::time::timeout(limits.handshake_timeout, acceptor.accept(stream));
    let stream = match handshake.await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            tracing::debug!(%peer, error = %e, "TLS handshake failed");
            return;
        }
        Err(_) => {
            tracing::debug!(%peer, "TLS handshake timed out");
            return;
        }
    };
    let service = TowerToHyperService::new(app);
    let mut builder = auto::Builder::new(TokioExecutor::new());
    builder.http1().timer(TokioTimer::new()).header_read_timeout(limits.header_read_timeout);
    if let Err(e) = builder.serve_connection(TokioIo::new(stream), service).await {
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

    /// Verifies: SRS-INST-001
    #[test]
    fn running_out_of_file_descriptors_backs_off_longer() {
        let emfile = std::io::Error::from_raw_os_error(24);
        let aborted = std::io::Error::from(std::io::ErrorKind::ConnectionAborted);
        assert_eq!(accept_backoff(&emfile), Duration::from_secs(1));
        assert!(accept_backoff(&aborted) < Duration::from_millis(100));
    }

    /// Verifies: SRS-INST-001
    #[tokio::test]
    async fn a_client_that_never_completes_the_handshake_is_dropped() {
        use tokio::io::AsyncReadExt as _;
        let dir = tempfile::tempdir().unwrap();
        let (tls, _) = self_signed(dir.path());
        let config = load_server_config(&tls).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let limits = Limits { handshake_timeout: Duration::from_millis(200), ..Limits::default() };
        tokio::spawn(serve_with(listener, Router::new(), config, limits));

        let mut idle = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let mut buf = [0u8; 16];
        let read = tokio::time::timeout(Duration::from_secs(3), idle.read(&mut buf))
            .await
            .expect("the server closes an idle connection after the handshake timeout");
        assert_eq!(read.unwrap_or(0), 0, "the connection is closed, not answered");
    }

    /// Verifies: SRS-INST-001
    #[tokio::test]
    async fn connections_beyond_the_limit_wait_for_a_free_slot() {
        let dir = tempfile::tempdir().unwrap();
        let (tls, cert_pem) = self_signed(dir.path());
        let config = load_server_config(&tls).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let limits = Limits {
            max_connections: 1,
            handshake_timeout: Duration::from_millis(500),
            ..Limits::default()
        };
        let app = Router::new().route("/api/v1/health", get(|| async { "ok" }));
        tokio::spawn(serve_with(listener, app, config, limits));

        // An idle client takes the only slot until its handshake times out;
        // a real client is served once the slot is free.
        let _idle = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let client = reqwest::Client::builder()
            .add_root_certificate(reqwest::Certificate::from_pem(cert_pem.as_bytes()).unwrap())
            .resolve("localhost", ([127, 0, 0, 1], port).into())
            .build()
            .unwrap();
        let started = std::time::Instant::now();
        let resp = client.get(format!("https://localhost:{port}/api/v1/health")).send().await;
        assert!(resp.unwrap().status().is_success());
        assert!(
            started.elapsed() >= Duration::from_millis(300),
            "the second connection had to wait for the first slot"
        );
    }

    /// Verifies: SRS-IF-010, SRS-INST-001
    #[tokio::test]
    async fn an_idle_connection_does_not_block_other_clients() {
        let dir = tempfile::tempdir().unwrap();
        let (tls, cert_pem) = self_signed(dir.path());
        let port = serve_health(&tls).await;

        // A client that connects and never starts the handshake.
        let _idle = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();

        let client = reqwest::Client::builder()
            .add_root_certificate(reqwest::Certificate::from_pem(cert_pem.as_bytes()).unwrap())
            .resolve("localhost", ([127, 0, 0, 1], port).into())
            .build()
            .unwrap();
        let resp = tokio::time::timeout(
            Duration::from_secs(5),
            client.get(format!("https://localhost:{port}/api/v1/health")).send(),
        )
        .await
        .expect("served while another connection idles")
        .unwrap();
        assert!(resp.status().is_success());
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
