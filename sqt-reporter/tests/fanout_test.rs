// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! The default fan-out over workload types (SRS-FUNC-019) against a mock
//! Runner: a specification without `use_case` submits one job per workload
//! type of the ML Space Profile, and passes only if every one of them passes.

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::Path,
    routing::{get, post},
};
use sqt_reporter::{api, config, evidence, orchestrator, report, specs, traceability};
use sqt_types::{
    DocsResponse, JobAccepted, JobMetrics, JobRequest, JobStatus, JobStatusResponse,
    MeasurementMode, VersionResponse, WorkloadType,
};
use tokio_util::sync::CancellationToken;

/// Workload types the mock Runner received, in submission order.
type Submitted = Arc<Mutex<Vec<WorkloadType>>>;

/// A mock Runner. Each job id is the workload type's wire name; the job of
/// `failing` (if any) exits with return code 1, every other with 0.
async fn spawn_runner(submitted: Submitted, failing: Option<WorkloadType>) -> String {
    let app = Router::new()
        .route(
            "/api/v1/version",
            get(|| async {
                Json(VersionResponse {
                    api_version: "1.0".into(),
                    runner_version: "0.1.0-test".into(),
                    target_id: "test-x86".into(),
                    os: "linux".into(),
                    toolchain_versions: std::collections::HashMap::new(),
                })
            }),
        )
        .route(
            "/api/v1/docs",
            get(|| async { Json(DocsResponse { documents: vec![], attachments: vec![] }) }),
        )
        .route(
            "/api/v1/jobs",
            post(move |Json(request): Json<JobRequest>| {
                let submitted = submitted.clone();
                async move {
                    let id = report::workload_name(&request.use_case);
                    submitted.lock().unwrap().push(request.use_case);
                    Json(JobAccepted {
                        job_id: id,
                        status: "accepted".into(),
                        queue_position: 0,
                        created_at: "2026-01-01T00:00:00Z".into(),
                    })
                }
            }),
        )
        .route(
            "/api/v1/jobs/{job_id}",
            get(move |Path(job_id): Path<String>| {
                let failing = failing.clone();
                async move {
                    let use_case = specs::loader::ML_SPACE_PROFILE
                        .iter()
                        .find(|w| report::workload_name(w) == job_id)
                        .cloned()
                        .unwrap_or_default();
                    let return_code = i64::from(failing.as_ref() == Some(&use_case));
                    Json(JobStatusResponse {
                        job_id,
                        use_case,
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
                            return_code: Some(return_code),
                            ..Default::default()
                        },
                        output: None,
                    })
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// Load `spec_yaml` with a config pointing at `url`, run the session and
/// return the run result.
async fn run(url: &str, spec_yaml: &str) -> specs::RunResult {
    let dir = tempfile::tempdir().unwrap();
    let spec = dir.path().join("spec.sqt.yaml");
    std::fs::write(&spec, spec_yaml).unwrap();
    let cfg_path = dir.path().join("sqt-reporter.yaml");
    std::fs::write(
        &cfg_path,
        format!(
            "project:\n  name: FanOut\n  version: \"1.0\"\ntest_specs:\n  files:\n    - \"{}\"\nrunner:\n  url: \"{url}\"\n  retry_max: 0\n  retry_backoff_initial_secs: 0\nreport:\n  output_dir: \"{}\"\n",
            spec.display(),
            dir.path().join("out").display()
        ),
    )
    .unwrap();
    let cfg = config::load_config(&cfg_path).unwrap();
    let loaded =
        specs::loader::load_specs(&cfg.test_specs.files, cfg.runner.job_timeout_secs).unwrap();
    let client = api::ApiClient::new(&cfg.runner.url, None, (&cfg.runner).into()).unwrap();
    let mut store =
        evidence::EvidenceStore::new(evidence::RunDir::create(dir.path()).unwrap()).unwrap();
    let (result, _) =
        orchestrator::run(&loaded, &client, &cfg, CancellationToken::new(), &mut store)
            .await
            .unwrap();
    result
}

const NO_USE_CASE: &str = "requirement_uid: REQ-FAN\nconstraints:\n  - metric: return_code\n    operator: eq\n    value: 0\n";

/// Verifies: SRS-FUNC-019
#[tokio::test]
async fn spec_without_use_case_submits_one_job_per_workload_type() {
    let submitted = Submitted::default();
    let url = spawn_runner(submitted.clone(), None).await;

    let result = run(&url, NO_USE_CASE).await;

    let mut got = submitted.lock().unwrap().clone();
    got.sort_by_key(report::workload_name);
    let mut want = specs::loader::ML_SPACE_PROFILE.to_vec();
    want.sort_by_key(report::workload_name);
    assert_eq!(got, want, "one job per workload type of the ML Space Profile");

    assert_eq!(result.results.len(), 6);
    for r in &result.results {
        assert_eq!(r.spec.requirement_uid, "REQ-FAN");
        assert!(r.spec.fan_out);
        assert_eq!(r.outcome, specs::OutcomeKind::Pass, "{:?}", r.spec.workload_type);
    }
    let refs: Vec<_> = result.results.iter().collect();
    assert_eq!(traceability::combined_outcome(&refs), Some("PASS"));
}

/// Verifies: SRS-FUNC-019
#[tokio::test]
async fn one_failing_workload_type_fails_the_requirement() {
    let url = spawn_runner(Submitted::default(), Some(WorkloadType::PixelSegmentation)).await;

    let result = run(&url, NO_USE_CASE).await;

    let failed: Vec<_> = result
        .results
        .iter()
        .filter(|r| r.outcome == specs::OutcomeKind::Fail)
        .map(|r| r.spec.workload_type.clone())
        .collect();
    assert_eq!(failed, vec![WorkloadType::PixelSegmentation]);
    let refs: Vec<_> = result.results.iter().collect();
    assert_eq!(traceability::combined_outcome(&refs), Some("FAIL"));

    // The report names the workload type of every result, and marks the fan-out.
    let rep = report::Report::from_run_result(
        &result,
        report::RunProvenance {
            sqt_version: "test".into(),
            runner_version: "test".into(),
            runner_target_id: "test".into(),
            runner_os: "linux".into(),
            runner_toolchain_versions: std::collections::HashMap::new(),
            config_summary: report::ConfigSummary {
                runner_url: url.clone(),
                token_env: None,
                max_concurrent_jobs: 1,
            },
        },
    );
    let json = serde_json::to_value(&rep).unwrap();
    let failing = json["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["outcome_kind"] == "FAIL")
        .expect("one failing result");
    assert_eq!(failing["target"]["workload_type"], "pixel_segmentation");
    assert_eq!(failing["target"]["workload_fan_out"], true);
    assert!(rep.summary.is_gate_failure(), "a failing workload type must fail the run");
}

/// Verifies: SRS-FUNC-019
#[tokio::test]
async fn spec_with_use_case_submits_exactly_one_job() {
    let submitted = Submitted::default();
    let url = spawn_runner(submitted.clone(), None).await;

    let result = run(&url, &format!("use_case: regression\n{NO_USE_CASE}")).await;

    assert_eq!(*submitted.lock().unwrap(), vec![WorkloadType::Regression]);
    assert_eq!(result.results.len(), 1);
    assert!(!result.results[0].spec.fan_out);
    let rep = serde_json::to_value(report::Report::from_run_result(
        &result,
        report::RunProvenance {
            sqt_version: "test".into(),
            runner_version: "test".into(),
            runner_target_id: "test".into(),
            runner_os: "linux".into(),
            runner_toolchain_versions: std::collections::HashMap::new(),
            config_summary: report::ConfigSummary {
                runner_url: url,
                token_env: None,
                max_concurrent_jobs: 1,
            },
        },
    ))
    .unwrap();
    let target = &rep["results"][0]["target"];
    assert_eq!(target["workload_type"], "regression");
    assert!(target.get("workload_fan_out").is_none(), "omitted when false: {target}");
}
