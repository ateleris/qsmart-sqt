// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Run-lifecycle behaviour the SRS and the ADD require, against a mock Runner:
//! pre-flight checks the contract version (SRS-OPS-007, ADD LLR-017), a run
//! that stops before evaluation still reports every specification (SRS-OPS-003,
//! ADD LLR-031), and a termination signal awaits the jobs already running
//! (SRS-OPS-008).

use std::{
    collections::HashMap,
    fmt::Write as _,
    io::Write as _,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    extract::Path,
    routing::{get, post},
};
use sqt_reporter::{api, config, evidence, orchestrator, outputs, preflight, report, specs};
use sqt_types::{
    DocsResponse, JobAccepted, JobMetrics, JobStatus, JobStatusResponse, MeasurementMode,
    VersionResponse, WorkloadType,
};
use tokio_util::sync::CancellationToken;

/// What the mock Runner saw and how it behaves.
struct Mock {
    api_version: &'static str,
    /// Polls of a job before it reports `complete`; `usize::MAX` never does.
    complete_after: usize,
    submitted: AtomicUsize,
    polls: Mutex<HashMap<String, usize>>,
}

fn status(job_id: &str, status: JobStatus) -> JobStatusResponse {
    let done = status == JobStatus::Complete;
    JobStatusResponse {
        job_id: job_id.into(),
        use_case: WorkloadType::Regression,
        mode: MeasurementMode::Latency,
        repetitions: 1,
        status,
        queue_position: None,
        created_at: "2026-01-01T00:00:00Z".into(),
        started_at: Some("2026-01-01T00:00:01Z".into()),
        completed_at: done.then(|| "2026-01-01T00:00:02Z".into()),
        error: None,
        tags: None,
        hardware: None,
        metrics: if done {
            JobMetrics { return_code: Some(0), ..Default::default() }
        } else {
            JobMetrics::default()
        },
        output: None,
    }
}

async fn spawn(mock: Arc<Mock>) -> String {
    let (m1, m2, m3) = (mock.clone(), mock.clone(), mock.clone());
    let app = Router::new()
        .route(
            "/api/v1/version",
            get(move || async move {
                Json(VersionResponse {
                    api_version: m1.api_version.into(),
                    runner_version: "0.1.0-test".into(),
                    target_id: "test-x86".into(),
                    os: "linux".into(),
                    toolchain_versions: HashMap::new(),
                })
            }),
        )
        .route(
            "/api/v1/docs",
            get(|| async { Json(DocsResponse { documents: vec![], attachments: vec![] }) }),
        )
        .route(
            "/api/v1/jobs",
            post(move || async move {
                let n = m2.submitted.fetch_add(1, Ordering::SeqCst) + 1;
                Json(JobAccepted {
                    job_id: format!("job-{n}"),
                    status: "accepted".into(),
                    queue_position: 0,
                    created_at: "2026-01-01T00:00:00Z".into(),
                })
            }),
        )
        .route(
            "/api/v1/jobs/{job_id}",
            get(move |Path(job_id): Path<String>| async move {
                let polls = {
                    let mut p = m3.polls.lock().unwrap();
                    let n = p.entry(job_id.clone()).or_default();
                    *n += 1;
                    *n
                };
                let s = if polls > m3.complete_after {
                    JobStatus::Complete
                } else {
                    JobStatus::Running
                };
                Json(status(&job_id, s))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn mock(api_version: &'static str, complete_after: usize) -> Arc<Mock> {
    Arc::new(Mock {
        api_version,
        complete_after,
        submitted: AtomicUsize::new(0),
        polls: Mutex::new(HashMap::new()),
    })
}

/// A configuration with one spec per workload type in `workloads`, each with
/// two constraints; one job in flight at a time, polled every second.
fn config_for(url: &str, dir: &std::path::Path, workloads: &[&str]) -> config::SqtConfig {
    let mut files = String::new();
    for (i, w) in workloads.iter().enumerate() {
        let spec = dir.join(format!("s{i}.spec.yaml"));
        std::fs::write(
            &spec,
            format!(
                "requirement_uid: REQ-C-{i}\nuse_case: {w}\nconstraints:\n  - metric: return_code\n    operator: eq\n    value: 0\n  - metric: return_code\n    operator: lte\n    value: 0\n"
            ),
        )
        .unwrap();
        let _ = writeln!(files, "    - \"{}\"", spec.display());
    }
    let cfg = dir.join("sqt-reporter.yaml");
    let mut f = std::fs::File::create(&cfg).unwrap();
    write!(
        f,
        "project:\n  name: C\n  version: \"1.0\"\ntest_specs:\n  files:\n{files}runner:\n  url: \"{url}\"\n  max_concurrent_jobs: 1\n  retry_max: 0\n  retry_backoff_initial_secs: 0\n  poll_interval_initial_secs: 1\n  poll_interval_max_secs: 1\nreport:\n  output_dir: \"{}\"\n",
        dir.join("out").display()
    )
    .unwrap();
    config::load_config(&cfg).unwrap()
}

fn client_for(cfg: &config::SqtConfig) -> api::ApiClient {
    api::ApiClient::new(&cfg.runner.url, None, (&cfg.runner).into()).unwrap()
}

fn load(cfg: &config::SqtConfig) -> Vec<specs::TestSpec> {
    specs::loader::load_specs(&cfg.test_specs.files, cfg.runner.job_timeout_secs).unwrap()
}

/// Verifies: SRS-OPS-007, SRS-IF-008
#[tokio::test]
async fn preflight_refuses_an_incompatible_contract_version() {
    let m = mock("2.0", 0);
    let url = spawn(m.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_for(&url, dir.path(), &["regression"]);

    let result = preflight::preflight(&cfg, &client_for(&cfg)).await;

    assert!(result.is_err(), "pre-flight must fail on contract major version 2");
    assert_eq!(m.submitted.load(Ordering::SeqCst), 0);
}

/// Verifies: SRS-OPS-003, SRS-FUNC-009
#[tokio::test]
async fn a_run_that_stops_before_evaluation_still_reports_every_spec() {
    let m = mock("2.0", 0);
    let url = spawn(m.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_for(&url, dir.path(), &["regression", "classification"]);
    let specs = load(&cfg);
    let client = client_for(&cfg);
    let mut store =
        evidence::EvidenceStore::new(evidence::RunDir::create(&cfg.report.output_dir).unwrap())
            .unwrap();

    let err = orchestrator::run(&specs, &client, &cfg, CancellationToken::new(), &mut store)
        .await
        .expect_err("an incompatible contract version stops the run");
    let reason = format!("{err:#}");
    let (mut result, provenance) = orchestrator::blocked_run(&specs, &cfg, &reason);
    let (rep, written) =
        outputs::write_run_outputs(&mut result, provenance, &mut store, None).unwrap();
    let text = report::render_terminal(&rep, &written.run_dir);

    assert_eq!(m.submitted.load(Ordering::SeqCst), 0);
    assert!(written.report_json.exists(), "the report is written for the aborted run");
    for uid in ["REQ-C-0", "REQ-C-1"] {
        let line =
            text.lines().find(|l| l.starts_with(uid)).unwrap_or_else(|| panic!("{uid}: {text}"));
        assert!(line.ends_with("BLOCKED"), "{line}");
    }
    assert!(text.contains("0/2 specs passed"), "{text}");
    assert!(
        text.contains("0/4 constraints passed"),
        "every declared constraint is counted: {text}"
    );
    assert!(text.contains(&written.run_dir.display().to_string()), "evidence location: {text}");
    assert!(text.contains("incompatible"), "the reason is shown: {text}");
}

/// Wait until the mock has accepted `n` submissions, then cancel `token`.
fn cancel_after_submissions(m: &Arc<Mock>, n: usize, token: &CancellationToken) {
    let (m, token) = (m.clone(), token.clone());
    tokio::spawn(async move {
        while m.submitted.load(Ordering::SeqCst) < n {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        token.cancel();
    });
}

/// Verifies: SRS-OPS-008, SRS-PERF-002
#[tokio::test]
async fn a_termination_signal_awaits_the_running_jobs_and_submits_no_more() {
    // The job completes on its third poll, so it is still running when the
    // signal arrives right after its submission.
    let m = mock("1.0", 2);
    let url = spawn(m.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_for(&url, dir.path(), &["regression", "classification"]);
    let specs = load(&cfg);
    let client = client_for(&cfg);
    let mut store =
        evidence::EvidenceStore::new(evidence::RunDir::create(&cfg.report.output_dir).unwrap())
            .unwrap();
    let shutdown = CancellationToken::new();
    cancel_after_submissions(&m, 1, &shutdown);

    let (result, _) = orchestrator::run(&specs, &client, &cfg, shutdown, &mut store).await.unwrap();

    assert!(result.interrupted, "the run is marked interrupted");
    assert_eq!(m.submitted.load(Ordering::SeqCst), 1, "no job is submitted after the signal");
    let passed: Vec<_> = result.results.iter().filter(|r| r.passed).collect();
    assert_eq!(passed.len(), 1, "the running job was awaited and evaluated");
    let other = result.results.iter().find(|r| !r.passed).unwrap();
    assert_eq!(other.outcome, specs::OutcomeKind::InfraError);
    assert!(other.outcomes[0].message.contains("before submit"), "{}", other.outcomes[0].message);
}

/// Verifies: SRS-OPS-008
#[tokio::test]
async fn a_second_signal_stops_waiting_for_running_jobs() {
    let m = mock("1.0", usize::MAX);
    let url = spawn(m.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_for(&url, dir.path(), &["regression"]);
    let specs = load(&cfg);
    let client = client_for(&cfg);
    let mut store =
        evidence::EvidenceStore::new(evidence::RunDir::create(&cfg.report.output_dir).unwrap())
            .unwrap();
    let (shutdown, abort) = (CancellationToken::new(), CancellationToken::new());
    cancel_after_submissions(&m, 1, &shutdown);
    cancel_after_submissions(&m, 1, &abort);

    let run = orchestrator::run_with_abort(&specs, &client, &cfg, shutdown, abort, &mut store);
    let (result, _) = tokio::time::timeout(Duration::from_secs(20), run)
        .await
        .expect("the second signal ends the wait for a job that never completes")
        .unwrap();

    assert!(result.interrupted);
    let r = &result.results[0];
    assert_eq!(r.outcome, specs::OutcomeKind::InfraError);
    assert!(r.outcomes[0].message.contains("second shutdown signal"), "{}", r.outcomes[0].message);
}
