use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Router,
    response::Json as AJson,
    routing::{get, post},
};
use sqt_reporter::{api, config, evidence, orchestrator, outputs, specs};
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
    make_test_config_files(url, &[spec_file], output_dir)
}

fn make_test_config_files(
    url: &str,
    spec_files: &[&std::path::Path],
    output_dir: &std::path::Path,
) -> config::SqtConfig {
    use std::{fmt::Write as _, io::Write as _};

    let specs = spec_files.iter().fold(String::new(), |mut out, f| {
        let _ = writeln!(out, "    - \"{}\"", f.display());
        out
    });
    let yaml = format!(
        r#"
project:
  name: "Integration Test"
  version: "1.0.0"
test_specs:
  files:
{specs}runner:
  url: "{url}"
  retry_max: 0
  retry_backoff_initial_secs: 0
report:
  output_dir: "{out}"
"#,
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
    let mut ev_store =
        evidence::EvidenceStore::new(evidence::RunDir::create(dir.path()).unwrap()).unwrap();
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
    let mut ev_store =
        evidence::EvidenceStore::new(evidence::RunDir::create(dir.path()).unwrap()).unwrap();
    let shutdown = CancellationToken::new();

    let (result, _) =
        orchestrator::run(&loaded_specs, &client, &cfg, shutdown, &mut ev_store).await.unwrap();

    assert_eq!(result.results[0].outcome, specs::OutcomeKind::Fail);
}

/// Verifies: SRS-IF-008, SRS-FUNC-008
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
    let mut ev_store =
        evidence::EvidenceStore::new(evidence::RunDir::create(dir.path()).unwrap()).unwrap();

    let err =
        orchestrator::run(&loaded_specs, &client, &cfg, CancellationToken::new(), &mut ev_store)
            .await
            .expect_err("a Runner with API major version 2 must be rejected");

    assert!(err.to_string().contains("incompatible"), "unexpected error: {err}");
    assert_eq!(submitted.load(Ordering::SeqCst), 0, "no job may be submitted");

    // The version response that stopped the run is still kept as evidence.
    orchestrator::persist_runner_responses(&client, &mut ev_store);
    let ids: Vec<&str> = ev_store.manifest().iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["evidence/runner/0001-get_version.json"]);
}

/// Verifies: SRS-FUNC-004, SRS-OPS-006
#[tokio::test]
async fn unreconcilable_unit_fails_only_its_constraint() {
    let url = spawn_mock_runner().await;
    let dir = tempfile::tempdir().unwrap();

    let good = dir.path().join("good.spec.yaml");
    std::fs::write(
        &good,
        "requirement_uid: REQ-UNIT-OK\nuse_case: regression\nconstraints:\n  \
         - metric: time.avg_ms\n    operator: lte\n    value: \"1s\"\n",
    )
    .unwrap();
    // The mock reports 1024 KB peak RSS; "5ms" cannot be a memory size.
    let bad = dir.path().join("bad.spec.yaml");
    std::fs::write(
        &bad,
        "requirement_uid: REQ-UNIT-BAD\nuse_case: classification\nconstraints:\n  \
         - metric: return_code\n    operator: eq\n    value: 0\n  \
         - metric: memory.peak_rss_kb\n    operator: lte\n    value: \"5ms\"\n",
    )
    .unwrap();

    let cfg = make_test_config_files(&url, &[good.as_path(), bad.as_path()], dir.path());
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
    let mut ev_store =
        evidence::EvidenceStore::new(evidence::RunDir::create(dir.path()).unwrap()).unwrap();

    let (result, _) =
        orchestrator::run(&loaded_specs, &client, &cfg, CancellationToken::new(), &mut ev_store)
            .await
            .unwrap();

    let by_uid = |uid: &str| result.results.iter().find(|r| r.spec.requirement_uid == uid).unwrap();
    assert_eq!(by_uid("REQ-UNIT-OK").outcome, specs::OutcomeKind::Pass);

    let bad = by_uid("REQ-UNIT-BAD");
    assert_eq!(bad.outcome, specs::OutcomeKind::Fail);
    assert_eq!(bad.outcomes.len(), 2, "both constraints are reported");
    assert!(bad.outcomes[0].passed, "return_code still evaluates: {:?}", bad.outcomes[0]);
    let unit = &bad.outcomes[1];
    assert!(!unit.passed);
    assert!(unit.message.contains("memory.peak_rss_kb"), "{}", unit.message);
    assert!(unit.message.contains("does not reduce"), "{}", unit.message);
}

/// Run one spec against the mock Runner and write all run outputs into
/// `out`. Returns the report and where it was written.
async fn run_and_write(
    url: &str,
    spec_path: &std::path::Path,
    out: &std::path::Path,
    report_copy: Option<&std::path::Path>,
) -> (sqt_reporter::report::Report, outputs::RunOutputs) {
    let cfg = make_test_config(url, spec_path, out);
    let loaded_specs =
        specs::loader::load_specs(&cfg.test_specs.files, cfg.runner.job_timeout_secs).unwrap();
    let client = api::ApiClient::new(&cfg.runner.url, None, 0, 0, 1, 1).unwrap();
    let run_dir = evidence::RunDir::create(&cfg.report.output_dir).unwrap();
    let mut ev_store = evidence::EvidenceStore::new(run_dir).unwrap();
    let (mut result, provenance) =
        orchestrator::run(&loaded_specs, &client, &cfg, CancellationToken::new(), &mut ev_store)
            .await
            .unwrap();
    orchestrator::persist_runner_responses(&client, &mut ev_store);
    outputs::write_run_outputs(&mut result, provenance, &mut ev_store, report_copy).unwrap()
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    sha2::Sha256::digest(data).iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// Verifies: SRS-FUNC-008, SRS-FUNC-009, SRS-OPS-004
#[tokio::test]
async fn run_writes_report_and_evidence_into_its_own_directory() {
    let url = spawn_mock_runner().await;
    let dir = tempfile::tempdir().unwrap();
    let spec_path = dir.path().join("pass.spec.yaml");
    std::fs::write(
        &spec_path,
        "requirement_uid: REQ-EVIDENCE-001\nuse_case: regression\nconstraints:\n  \
         - metric: return_code\n    operator: eq\n    value: 0\n",
    )
    .unwrap();
    let out = dir.path().join("out");
    let copy = dir.path().join("copy/report.json");

    let (report, written) = run_and_write(&url, &spec_path, &out, Some(&copy)).await;
    let run_id = report.run_id.clone().expect("report names its run");
    assert_eq!(written.run_dir, out.join("runs").join(&run_id));
    assert_eq!(written.report_json, written.run_dir.join("report.json"));

    // The raw Runner responses, in arrival order, byte for byte.
    let runner_dir = written.run_dir.join("evidence/runner");
    let mut raw: Vec<String> = std::fs::read_dir(&runner_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    raw.sort();
    assert_eq!(
        raw,
        [
            "0001-get_version.json",
            "0002-get_docs.json",
            "0003-submit_job-test-job-1.json",
            "0004-get_job_status-test-job-1.json",
        ]
    );
    let served = reqwest::get(format!("{url}/api/v1/jobs/test-job-1")).await.unwrap();
    let served = served.bytes().await.unwrap();
    assert_eq!(std::fs::read(runner_dir.join(&raw[3])).unwrap(), served.as_ref());

    // The manifest lists every evidence file with its SHA-256.
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(written.run_dir.join("evidence/manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["run_id"], run_id.as_str());
    let entries = manifest["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 5, "four responses and one outcomes file");
    for entry in entries {
        let id = entry["id"].as_str().unwrap();
        let data = std::fs::read(written.run_dir.join(id)).unwrap();
        assert_eq!(entry["sha256"], sha256_hex(&data).as_str(), "{id}");
    }

    // The report references its evidence by those ids.
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&written.report_json).unwrap()).unwrap();
    assert_eq!(json["schema_version"], "1.1");
    assert_eq!(json["run_id"], run_id.as_str());
    assert_eq!(json["evidence_manifest"], "evidence/manifest.json");
    let ids: Vec<&str> = json["results"][0]["evidence_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "evidence/runner/0004-get_job_status-test-job-1.json",
            "evidence/outcomes/001-REQ-EVIDENCE-001.json"
        ]
    );
    for id in &ids {
        assert!(entries.iter().any(|e| e["id"] == *id), "{id} must be in the manifest");
    }
    let outcomes: serde_json::Value =
        serde_json::from_slice(&std::fs::read(written.run_dir.join(ids[1])).unwrap()).unwrap();
    assert_eq!(outcomes["outcomes"][0]["metric"], "return_code");
    assert_eq!(outcomes["outcomes"][0]["passed"], true);

    // The copy is identical, and the QDP holds the whole run directory.
    assert_eq!(std::fs::read(&copy).unwrap(), std::fs::read(&written.report_json).unwrap());
    let mut qdp = zip::ZipArchive::new(std::fs::File::open(&written.qdp).unwrap()).unwrap();
    for name in ["report.json", "report.pdf", "evidence/manifest.json", ids[0], ids[1]] {
        assert!(qdp.by_name(name).is_ok(), "qdp.zip must contain {name}");
    }

    // A second run gets its own, later directory; the first is untouched.
    let (second, _) = run_and_write(&url, &spec_path, &out, None).await;
    let second_id = second.run_id.unwrap();
    assert!(second_id > run_id, "{second_id} must sort after {run_id}");
    assert!(out.join("runs").join(&run_id).join("report.json").is_file());
}
