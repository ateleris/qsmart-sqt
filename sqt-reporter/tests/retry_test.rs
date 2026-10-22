// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Bounded retry of transient Runner errors (SRS-FUNC-020), against a scripted
//! HTTP server that can fail in ways a framework mock cannot: resetting the
//! connection mid-response, closing it before answering, or never answering.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use sqt_reporter::api::{ApiClient, client::ClientOptions};
use sqt_types::JobRequest;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const VERSION_BODY: &str = r#"{"api_version":"1.0","runner_version":"0.1.0","target_id":"x","os":"linux","toolchain_versions":{}}"#;
const ACCEPTED_BODY: &str = r#"{"job_id":"job-1","status":"accepted","queue_position":0,"created_at":"2026-01-01T00:00:00Z"}"#;
const ERROR_BODY: &str =
    r#"{"error":{"code":"INTERNAL_ERROR","message":"boom","timestamp":"2026-01-01T00:00:00Z"}}"#;

/// What the server does with one connection, after reading the request.
#[derive(Clone, Copy)]
enum Step {
    Respond(u16, &'static str),
    /// Send the headers and part of the body, then reset the connection.
    ResetMidBody,
    /// Reset the connection without answering.
    ResetBeforeResponse,
    /// Never answer (longer than the client's request timeout).
    Hang,
}

/// Serve `script`, one step per connection; the last step repeats. Returns the
/// base URL and the number of connections served so far.
async fn scripted_server(script: Vec<Step>) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let served = count.clone();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let n = served.fetch_add(1, Ordering::SeqCst);
            let step = script[n.min(script.len() - 1)];
            tokio::spawn(handle(stream, step));
        }
    });
    (url, count)
}

async fn handle(mut stream: TcpStream, step: Step) {
    read_request(&mut stream).await;
    match step {
        Step::Respond(status, body) => {
            let head = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).await.unwrap();
            stream.write_all(body.as_bytes()).await.unwrap();
        }
        Step::ResetMidBody => {
            let head =
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 500\r\n\r\n";
            stream.write_all(head.as_bytes()).await.unwrap();
            stream.write_all(&VERSION_BODY.as_bytes()[..20]).await.unwrap();
            stream.flush().await.unwrap();
            reset(stream);
        }
        Step::ResetBeforeResponse => reset(stream),
        Step::Hang => tokio::time::sleep(Duration::from_secs(5)).await,
    }
}

/// Close with an RST instead of a FIN: a network reset.
fn reset(stream: TcpStream) {
    stream.set_zero_linger().unwrap();
    drop(stream);
}

/// Read the request head and, if it has one, its body.
async fn read_request(stream: &mut TcpStream) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        let n = stream.read(&mut chunk).await.unwrap();
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_ascii_lowercase();
    let length: usize = head
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .map_or(0, |v| v.trim().parse().unwrap());
    while buf.len() < head_end + length {
        let n = stream.read(&mut chunk).await.unwrap();
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn client(url: &str, retry_max: u32) -> ApiClient {
    let options = ClientOptions {
        retry_max,
        retry_backoff_initial_secs: 0,
        request_timeout_secs: 1,
        ..ClientOptions::default()
    };
    ApiClient::new(url, None, options).unwrap()
}

fn job_request() -> JobRequest {
    // The contract's defaults for everything not given.
    serde_json::from_str(r#"{"use_case":"regression","timeout_secs":30}"#).unwrap()
}

/// Verifies: SRS-FUNC-020
#[tokio::test]
async fn read_is_retried_on_unavailable_and_evidence_keeps_each_response() {
    let (url, served) = scripted_server(vec![
        Step::Respond(503, ERROR_BODY),
        Step::Respond(503, ERROR_BODY),
        Step::Respond(200, VERSION_BODY),
    ])
    .await;
    let client = client(&url, 3);

    let version = client.get_version().await.expect("third attempt succeeds");
    assert_eq!(version.api_version, "1.0");
    assert_eq!(served.load(Ordering::SeqCst), 3);
    let statuses: Vec<u16> = client.take_recorded().iter().map(|r| r.status).collect();
    assert_eq!(statuses, [503, 503, 200]);
}

/// Verifies: SRS-FUNC-020
#[tokio::test]
async fn read_is_retried_after_reset_mid_response() {
    let (url, served) =
        scripted_server(vec![Step::ResetMidBody, Step::Respond(200, VERSION_BODY)]).await;
    client(&url, 2).get_version().await.expect("second attempt succeeds");
    assert_eq!(served.load(Ordering::SeqCst), 2);
}

/// Verifies: SRS-FUNC-020
#[tokio::test]
async fn read_is_retried_after_reset_before_response() {
    let (url, served) =
        scripted_server(vec![Step::ResetBeforeResponse, Step::Respond(200, VERSION_BODY)]).await;
    client(&url, 2).get_version().await.expect("second attempt succeeds");
    assert_eq!(served.load(Ordering::SeqCst), 2);
}

/// Verifies: SRS-FUNC-020
#[tokio::test]
async fn read_is_retried_after_timeout() {
    let (url, served) = scripted_server(vec![Step::Hang, Step::Respond(200, VERSION_BODY)]).await;
    client(&url, 2).get_version().await.expect("second attempt succeeds");
    assert_eq!(served.load(Ordering::SeqCst), 2);
}

/// Verifies: SRS-FUNC-020
#[tokio::test]
async fn internal_error_is_not_retried() {
    let (url, served) = scripted_server(vec![Step::Respond(500, ERROR_BODY)]).await;
    let err = client(&url, 3).get_version().await.unwrap_err();
    assert!(format!("{err:#}").contains("INTERNAL_ERROR"), "{err:#}");
    assert_eq!(served.load(Ordering::SeqCst), 1);
}

/// Verifies: SRS-FUNC-020
#[tokio::test]
async fn retries_stop_at_retry_max() {
    let (url, served) = scripted_server(vec![Step::Respond(503, ERROR_BODY)]).await;
    let err = client(&url, 2).get_version().await.unwrap_err();
    assert!(format!("{err:#}").contains("503"), "{err:#}");
    assert_eq!(served.load(Ordering::SeqCst), 3, "one attempt plus two retries");
}

/// Verifies: SRS-FUNC-020
#[tokio::test]
async fn submission_is_resent_when_the_runner_refused_it() {
    let (url, served) =
        scripted_server(vec![Step::Respond(503, ERROR_BODY), Step::Respond(202, ACCEPTED_BODY)])
            .await;
    let job_id = client(&url, 2).submit_job(&job_request()).await.expect("second attempt");
    assert_eq!(job_id, "job-1");
    assert_eq!(served.load(Ordering::SeqCst), 2);
}

/// Verifies: SRS-FUNC-020
#[tokio::test]
async fn submission_is_not_resent_once_it_may_have_reached_the_runner() {
    for step in
        [Step::ResetBeforeResponse, Step::ResetMidBody, Step::Hang, Step::Respond(502, ERROR_BODY)]
    {
        let (url, served) = scripted_server(vec![step, Step::Respond(202, ACCEPTED_BODY)]).await;
        let result = client(&url, 3).submit_job(&job_request()).await;
        assert!(result.is_err(), "a possibly delivered submission must not be retried");
        assert_eq!(served.load(Ordering::SeqCst), 1, "the job could otherwise be duplicated");
    }
}

/// Verifies: SRS-FUNC-020
#[tokio::test]
async fn submission_is_resent_when_the_connection_was_refused() {
    // A port that was just free: connecting to it is refused.
    let url = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    let err = client(&url, 2).submit_job(&job_request()).await.unwrap_err();
    assert!(format!("{err:#}").contains("gave up after 3 attempts"), "{err:#}");
}
