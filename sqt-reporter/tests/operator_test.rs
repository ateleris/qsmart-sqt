// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Operator-facing behaviour against a mock Runner: the pre-flight mode
//! (SRS-OPS-007), payload logging at the most verbose level (SRS-OPS-002) and
//! no network access beyond the configured Runner (SRS-RES-002).

use std::{
    io::Write as _,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    Router,
    extract::Request,
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Json, Redirect},
    routing::{get, post},
};
use sqt_reporter::{api, config, preflight};
use sqt_types::{DocsResponse, JobAccepted, JobRequest, VersionResponse};

const TOKEN: &str = "s3cret-token";

/// Requests the mock Runner received, by kind.
#[derive(Default)]
struct Hits {
    total: AtomicUsize,
    submitted: AtomicUsize,
}

fn version() -> VersionResponse {
    VersionResponse {
        api_version: "1.0".into(),
        runner_version: "0.1.0-test".into(),
        target_id: "test-x86".into(),
        os: "linux".into(),
        toolchain_versions: std::collections::HashMap::new(),
    }
}

/// A mock Runner that requires `Authorization: Bearer TOKEN` on every route.
/// Its version response echoes the `Authorization` header in the `os` field,
/// so a payload log that does not redact the token would show it.
async fn spawn_runner(hits: Arc<Hits>) -> String {
    let submitted = hits.clone();
    let app = Router::new()
        .route(
            "/api/v1/version",
            get(|headers: HeaderMap| async move {
                let echoed = headers
                    .get(header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_owned();
                Json(VersionResponse { os: echoed, ..version() })
            }),
        )
        .route(
            "/api/v1/docs",
            get(|| async { Json(DocsResponse { documents: vec![], attachments: vec![] }) }),
        )
        .route(
            "/api/v1/jobs",
            post(move || async move {
                submitted.submitted.fetch_add(1, Ordering::SeqCst);
                Json(JobAccepted {
                    job_id: "job-1".into(),
                    status: "accepted".into(),
                    queue_position: 0,
                    created_at: "2026-01-01T00:00:00Z".into(),
                })
            }),
        )
        .layer(middleware::from_fn(move |req: Request, next: Next| {
            let hits = hits.clone();
            async move {
                hits.total.fetch_add(1, Ordering::SeqCst);
                let ok = req
                    .headers()
                    .get(header::AUTHORIZATION)
                    .is_some_and(|v| v.as_bytes() == format!("Bearer {TOKEN}").as_bytes());
                if ok { next.run(req).await } else { StatusCode::UNAUTHORIZED.into_response() }
            }
        }));
    serve(app).await
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// A configuration with one spec file, pointing at `url`.
fn config_for(url: &str, dir: &std::path::Path) -> config::SqtConfig {
    let spec = dir.join("a.spec.yaml");
    std::fs::write(
        &spec,
        "requirement_uid: REQ-PF-001\nuse_case: regression\nconstraints:\n  - metric: return_code\n    operator: eq\n    value: 0\n",
    )
    .unwrap();
    let cfg = dir.join("sqt-reporter.yaml");
    let mut f = std::fs::File::create(&cfg).unwrap();
    write!(
        f,
        "project:\n  name: PF\n  version: \"1.0\"\ntest_specs:\n  files:\n    - \"{}\"\nrunner:\n  url: \"{url}\"\n  retry_max: 0\n  retry_backoff_initial_secs: 0\nreport:\n  output_dir: \"{}\"\n",
        spec.display(),
        dir.join("out").display()
    )
    .unwrap();
    config::load_config(&cfg).unwrap()
}

fn client(url: &str, token: Option<&str>) -> api::ApiClient {
    let options =
        api::client::ClientOptions { retry_max: 0, ..api::client::ClientOptions::default() };
    api::ApiClient::new(url, token, options).unwrap()
}

/// Verifies: SRS-OPS-007, SRS-IF-006, SRS-FUNC-018
#[tokio::test]
async fn preflight_checks_reachability_auth_and_requirements_without_submitting() {
    let hits = Arc::new(Hits::default());
    let url = spawn_runner(hits.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_for(&url, dir.path());

    preflight::preflight(&cfg, &client(&url, Some(TOKEN))).await.expect("pre-flight passes");

    // Version and requirement discovery were both fetched, no job submitted.
    assert_eq!(hits.total.load(Ordering::SeqCst), 2);
    assert_eq!(hits.submitted.load(Ordering::SeqCst), 0);
}

/// Verifies: SRS-OPS-007, SRS-IF-006
#[tokio::test]
async fn preflight_fails_on_a_rejected_token_without_submitting() {
    let hits = Arc::new(Hits::default());
    let url = spawn_runner(hits.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_for(&url, dir.path());

    let result = preflight::preflight(&cfg, &client(&url, Some("wrong"))).await;

    assert!(result.is_err(), "pre-flight must fail when the Runner rejects the token");
    assert_eq!(hits.submitted.load(Ordering::SeqCst), 0);
}

/// Verifies: SRS-OPS-007
#[tokio::test]
async fn preflight_fails_when_the_runner_is_unreachable() {
    // Bind and drop a listener, so the port is closed.
    let url = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", l.local_addr().unwrap())
    };
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_for(&url, dir.path());

    assert!(preflight::preflight(&cfg, &client(&url, Some(TOKEN))).await.is_err());
}

/// Verifies: SRS-RES-002
#[tokio::test]
async fn a_redirect_to_another_host_is_not_followed() {
    let elsewhere = Arc::new(AtomicUsize::new(0));
    let counter = elsewhere.clone();
    let other = serve(Router::new().fallback(move || {
        let counter = counter.clone();
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            Json(version())
        }
    }))
    .await;
    let target = format!("{other}/api/v1/version");
    let runner = serve(
        Router::new()
            .route("/api/v1/version", get(move || async move { Redirect::temporary(&target) })),
    )
    .await;

    let result = client(&runner, None).get_version().await;

    let err = format!("{:#}", result.expect_err("a redirect must fail the request"));
    assert!(err.contains(&format!("{other}/api/v1/version")), "target not named: {err}");
    assert!(err.contains("runner.url"), "no hint to fix the configuration: {err}");
    assert_eq!(elsewhere.load(Ordering::SeqCst), 0, "the redirect target was contacted");
}

/// A `MakeWriter` collecting everything the subscriber writes.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Submit one job and fetch the version with logging at `filter`; return the log.
async fn log_of_operations(filter: &str) -> String {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(captured.clone())
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let url = spawn_runner(Arc::new(Hits::default())).await;
    let client = client(&url, Some(TOKEN));
    client.get_version().await.unwrap();
    let request: JobRequest = serde_json::from_value(serde_json::json!({
        "use_case": "regression",
        "input": {"dataset": "payload-marker-42"},
    }))
    .unwrap();
    client.submit_job(&request).await.unwrap();

    String::from_utf8(captured.0.lock().unwrap().clone()).unwrap()
}

/// Verifies: SRS-OPS-002, SRS-OPS-005
#[tokio::test]
async fn most_verbose_level_logs_request_and_response_payloads() {
    let log = log_of_operations("info,sqt_reporter=trace").await;

    assert!(log.contains("payload-marker-42"), "job request payload missing:\n{log}");
    assert!(log.contains("0.1.0-test"), "version response payload missing:\n{log}");
    assert!(log.contains("job-1"), "job-accepted response payload missing:\n{log}");
    assert!(!log.contains(TOKEN), "the bearer token leaked into the log:\n{log}");
}

/// Verifies: SRS-OPS-002
#[tokio::test]
async fn default_level_logs_no_payloads() {
    let log = log_of_operations("info").await;

    assert!(!log.contains("payload-marker-42"), "payload logged at info:\n{log}");
    assert!(!log.contains("0.1.0-test"), "payload logged at info:\n{log}");
}
