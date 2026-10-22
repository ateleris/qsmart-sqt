use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Router,
    response::Json as AJson,
    routing::{get, post},
};
use sqt_reporter::{api, config, evidence, orchestrator, specs};
use sqt_types::{
    JobAccepted, JobMetrics, JobStatus, JobStatusResponse, MeasurementMode, VersionResponse,
    WorkloadType,
};
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// Mock runner helpers
// ---------------------------------------------------------------------------

/// Spawn a mock Runner on an ephemeral port. Returns the base URL.
async fn spawn_mock_runner() -> String {
    spawn_mock_runner_with("1.0", Arc::new(AtomicUsize::new(0))).await
}

/// Like [`spawn_mock_runner`], but reporting `api_version` and counting job
/// submissions in `submitted`.
async fn spawn_mock_runner_with(api_version: &'static str, submitted: Arc<AtomicUsize>) -> String {
    let job_response = Arc::new(JobStatusResponse {
        job_id: "test-job-1".into(),
        use_case: WorkloadType::Regression,
        mode: MeasurementMode::Latency,
        repetitions: 1,
        status: JobStatus::Complete,
        queue_position: None,
        created_at: "2026-01-01T00:00:00Z".into(),
        started_at: Some("2026-01-01T00:00:01Z".into()),
        completed_at: Some("2026-01-01T00:00:02Z".into()),
        error: None,
        tags: None,
        hardware: None,
        metrics: JobMetrics {
            return_code: Some(0),
            time_avg_ms: Some(50.0),
            memory_peak_rss_kb: Some(1024.0),
            ..Default::default()
        },
        output: None,
    });

    let job_response_clone = job_response.clone();

    let app = Router::new()
        .route(
            "/api/v1/version",
            get(move || async move {
                AJson(VersionResponse {
                    api_version: api_version.into(),
                    runner_version: "0.1.0-test".into(),
                    target_id: "test-x86".into(),
                    os: "linux".into(),
                    toolchain_versions: std::collections::HashMap::new(),
                })
            }),
        )
        .route(
            "/api/v1/jobs",
            post(move || async move {
                submitted.fetch_add(1, Ordering::SeqCst);
                AJson(JobAccepted {
                    job_id: "test-job-1".into(),
                    status: "accepted".into(),
                    queue_position: 0,
                    created_at: "2026-01-01T00:00:00Z".into(),
                })
            }),
        )
        .route(
            "/api/v1/jobs/{job_id}",
            get(move || {
                let resp = job_response_clone.clone();
                async move { AJson((*resp).clone()) }
            }),
        )
        .route(
            "/api/v1/docs",
            get(|| async {
                AJson(sqt_types::DocsResponse { documents: vec![], attachments: vec![] })
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn make_test_config(
    url: &str,
    spec_file: &std::path::Path,
    output_dir: &std::path::Path,
) -> config::SqtConfig {
    use std::io::Write as _;

    let yaml = format!(
        r#"
project:
  name: "Integration Test"
  version: "1.0.0"
test_specs:
  files:
    - "{spec}"
runner:
  url: "{url}"
  retry_max: 0
  retry_backoff_initial_secs: 0
report:
  output_dir: "{out}"
"#,
        spec = spec_file.display(),
        url = url,
        out = output_dir.display()
    );

    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "{yaml}").unwrap();
    config::load_config(tmp.path()).unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Verifies: SRS-FUNC-007, SRS-OPS-001, SRS-QUAL-003, SRS-VAL-001
#[tokio::test]
async fn full_run_pass_path() {
    let url = spawn_mock_runner().await;
    let dir = tempfile::tempdir().unwrap();

    // Write a minimal spec file
    let spec_path = dir.path().join("test.spec.yaml");
    std::fs::write(
        &spec_path,
        r#"
requirement_uid: REQ-INTEGRATION-001
use_case: regression
constraints:
  - metric: return_code
    operator: eq
    value: 0
  - metric: time.avg_ms
    operator: lte
    value: "200ms"
"#,
    )
    .unwrap();

    let cfg = make_test_config(&url, &spec_path, dir.path());

    let loaded_specs =
        specs::loader::load_specs(&cfg.test_specs.files, cfg.runner.job_timeout_secs).unwrap();
    assert_eq!(loaded_specs.len(), 1);

    let client = api::ApiClient::new(
        &cfg.runner.url,
        None,
        cfg.runner.retry_max,
        cfg.runner.retry_backoff_initial_secs,
        cfg.runner.poll_interval_initial_secs,
        cfg.runner.poll_interval_max_secs,
    )
    .unwrap();
    let mut ev_store = evidence::EvidenceStore::new(dir.path().join("evidence")).unwrap();
    let shutdown = CancellationToken::new();

    let (result, _provenance) =
        orchestrator::run(&loaded_specs, &client, &cfg, shutdown, &mut ev_store).await.unwrap();

    assert!(!result.interrupted);
    assert_eq!(result.results.len(), 1);
    assert_eq!(
        result.results[0].outcome,
        specs::OutcomeKind::Pass,
        "Expected PASS but got {:?}: {:?}",
        result.results[0].outcome,
        result.results[0].outcomes
    );
}

/// Verifies: SRS-FUNC-007, SRS-OPS-001, SRS-QUAL-003, SRS-VAL-001
#[tokio::test]
async fn run_fails_when_constraint_violated() {
    let url = spawn_mock_runner().await;
    let dir = tempfile::tempdir().unwrap();

    let spec_path = dir.path().join("fail.spec.yaml");
    std::fs::write(
        &spec_path,
        r#"
requirement_uid: REQ-INTEGRATION-002
use_case: regression
constraints:
  - metric: time.avg_ms
    operator: lte
    value: "10ms"
"#,
    )
    .unwrap();

    let cfg = make_test_config(&url, &spec_path, dir.path());
    let loaded_specs =
        specs::loader::load_specs(&cfg.test_specs.files, cfg.runner.job_timeout_secs).unwrap();
    let client = api::ApiClient::new(
        &cfg.runner.url,
        None,
        cfg.runner.retry_max,
        cfg.runner.retry_backoff_initial_secs,
        cfg.runner.poll_interval_initial_secs,
        cfg.runner.poll_interval_max_secs,
    )
    .unwrap();
    let mut ev_store = evidence::EvidenceStore::new(dir.path().join("evidence")).unwrap();
    let shutdown = CancellationToken::new();

    let (result, _) =
        orchestrator::run(&loaded_specs, &client, &cfg, shutdown, &mut ev_store).await.unwrap();

    assert_eq!(result.results[0].outcome, specs::OutcomeKind::Fail);
}

/// Verifies: SRS-IF-008
#[tokio::test]
async fn run_rejects_incompatible_runner_api_version_before_submitting() {
    let submitted = Arc::new(AtomicUsize::new(0));
    let url = spawn_mock_runner_with("2.0", submitted.clone()).await;
    let dir = tempfile::tempdir().unwrap();

    let spec_path = dir.path().join("version.spec.yaml");
    std::fs::write(
        &spec_path,
        r"
requirement_uid: REQ-INTEGRATION-003
use_case: regression
constraints:
  - metric: return_code
    operator: eq
    value: 0
",
    )
    .unwrap();

    let cfg = make_test_config(&url, &spec_path, dir.path());
    let loaded_specs =
        specs::loader::load_specs(&cfg.test_specs.files, cfg.runner.job_timeout_secs).unwrap();
    let client = api::ApiClient::new(
        &cfg.runner.url,
        None,
        cfg.runner.retry_max,
        cfg.runner.retry_backoff_initial_secs,
        cfg.runner.poll_interval_initial_secs,
        cfg.runner.poll_interval_max_secs,
    )
    .unwrap();
    let mut ev_store = evidence::EvidenceStore::new(dir.path().join("evidence")).unwrap();

    let err =
        orchestrator::run(&loaded_specs, &client, &cfg, CancellationToken::new(), &mut ev_store)
            .await
            .expect_err("a Runner with API major version 2 must be rejected");

    assert!(err.to_string().contains("incompatible"), "unexpected error: {err}");
    assert_eq!(submitted.load(Ordering::SeqCst), 0, "no job may be submitted");
}
