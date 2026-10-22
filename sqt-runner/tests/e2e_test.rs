// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! End-to-end: the Orchestrator against the real reference Runner, which runs
//! the binary under test in Docker (SRS-QUAL-003, SRS-VAL-001, SRS-INST-001).
//!
//! Needs Docker, so it is ignored by default:
//!
//! ```text
//! cargo test -p sqt-runner --test e2e_test -- --ignored
//! ```
//!
//! The Runner bind-mounts the binary and its workspace at the same path inside
//! the tool container, so every file the test creates must be visible to the
//! Docker daemon under the same path. Locally that holds for a temp dir; in CI
//! (Docker-in-Docker) set `SQT_E2E_WORKDIR` to a directory under the shared
//! job checkout.

use std::{
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use sqt_reporter::{
    api::ApiClient,
    config, evidence, orchestrator, outputs,
    report::{OutcomeKind, Report},
    specs,
};
use tokio_util::sync::CancellationToken;

/// The tool image: only `/bin/sh` is needed, so a pinned busybox will do.
const RUN_IMAGE: &str =
    "busybox:1.37@sha256:bdf57e528e45e4433820e045b29b4597825a1c9e38353532d90a01445013f82e";

/// The binary under test. A shell script, so it runs in any image regardless
/// of the host's C library. With input `slow` it outlives its job timeout.
const BINARY: &str = "#!/bin/sh\nread -r mode || true\nif [ \"$mode\" = slow ]; then sleep 30; fi\necho '[0.1, 0.9]'\n";

const BASELINE_SDOC: &str = "[DOCUMENT]\nTITLE: E2E requirements\n\n\
[REQUIREMENT]\nUID: REQ-E2E-PASS\nTITLE: Exits cleanly\nVERIFICATION: Test\n\n\
[REQUIREMENT]\nUID: REQ-E2E-FAIL\nTITLE: Exits with 1\nVERIFICATION: Test\n\n\
[REQUIREMENT]\nUID: REQ-E2E-SLOW\nTITLE: Finishes in time\nVERIFICATION: Test\n\n\
[REQUIREMENT]\nUID: REQ-E2E-PROCESS\nTITLE: Developed per ECSS\nVERIFICATION: Inspection\n";

/// Kills the Runner when the test ends, however it ends.
struct RunnerProcess(Child);

impl Drop for RunnerProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn workdir() -> (Option<tempfile::TempDir>, PathBuf) {
    if let Ok(dir) = std::env::var("SQT_E2E_WORKDIR") {
        let path = PathBuf::from(dir).join(format!("e2e-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        (None, path)
    } else {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().to_path_buf();
        (Some(tmp), path)
    }
}

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// Write the Runner's configuration, docs and binary into `dir`, start it and
/// wait until it answers.
fn start_runner(dir: &Path) -> (RunnerProcess, String) {
    let status = Command::new("docker").args(["pull", "-q", RUN_IMAGE]).status().unwrap();
    assert!(status.success(), "cannot pull {RUN_IMAGE}");

    let binary = dir.join("bin/fake-ml.sh");
    write(&binary, BINARY);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    write(&dir.join("docs/e2e.sdoc"), BASELINE_SDOC);
    write(&dir.join("docs/attestation.txt"), "signed off\n");
    let config = dir.join("sqt-runner.yaml");
    write(
        &config,
        &format!(
            "backend: docker\nbinary:\n  path: {binary}\n  input_mode: stdin\n\
             workspace: {workspace}\ndocker:\n  tools:\n    run:\n      image: {RUN_IMAGE}\n\
             docs:\n  directory: {docs}\nhardware:\n  type: x86_64\n  os: busybox\n\
             runner_hardware:\n  target_id: e2e\n  toolchain_versions: {{}}\n",
            binary = binary.display(),
            workspace = dir.join("runner-workspace").display(),
            docs = dir.join("docs").display(),
        ),
    );

    let port = free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_sqt-runner"))
        .arg("-c")
        .arg(&config)
        .args(["--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let runner = RunnerProcess(child);
    let url = format!("http://127.0.0.1:{port}");

    let deadline = Instant::now() + Duration::from_mins(1);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "sqt-runner did not start");
        std::thread::sleep(Duration::from_millis(200));
    }
    (runner, url)
}

/// Write the reporter's config listing `specs`, run a session against `url`
/// and write all outputs, as `sqt-reporter run` does.
async fn run_session(
    url: &str,
    dir: &Path,
    specs: &[&Path],
) -> anyhow::Result<(Report, outputs::RunOutputs)> {
    let files = specs.iter().fold(String::new(), |mut out, s| {
        use std::fmt::Write as _;
        let _ = writeln!(out, "    - {}", s.display());
        out
    });
    let config_path = dir.join("sqt-reporter.yaml");
    write(
        &config_path,
        &format!(
            "project:\n  name: e2e\n  version: \"1.0.0\"\ntest_specs:\n  files:\n{files}\
             runner:\n  url: {url}\n  retry_max: 1\n  poll_interval_max_secs: 2\n\
             report:\n  output_dir: {out}\n",
            out = dir.join("reports").display(),
        ),
    );
    let cfg = config::load_config(&config_path).unwrap();
    let loaded =
        specs::loader::load_specs(&cfg.test_specs.files, cfg.runner.job_timeout_secs).unwrap();
    let client = ApiClient::new(&cfg.runner.url, None, (&cfg.runner).into()).unwrap();
    let mut store =
        evidence::EvidenceStore::new(evidence::RunDir::create(&cfg.report.output_dir).unwrap())
            .unwrap();
    let (mut result, provenance) =
        orchestrator::run(&loaded, &client, &cfg, CancellationToken::new(), &mut store).await?;
    orchestrator::persist_runner_responses(&client, &mut store);
    outputs::write_run_outputs(&mut result, provenance, &mut store, None)
}

fn spec(dir: &Path, name: &str, uid: &str, extra: &str, return_code: i32) -> PathBuf {
    let path = dir.join("specs").join(name);
    write(
        &path,
        &format!(
            "requirement_uid: {uid}\nuse_case: regression\n{extra}constraints:\n  \
             - metric: return_code\n    operator: eq\n    value: {return_code}\n  \
             - metric: time.avg_ms\n    operator: lte\n    value: \"60s\"\n"
        ),
    );
    path
}

fn outcome_of<'a>(report: &'a Report, uid: &str) -> &'a OutcomeKind {
    &report.results.iter().find(|r| r.requirement_uid == uid).unwrap().outcome_kind
}

/// Verifies: SRS-QUAL-003, SRS-VAL-001, SRS-INST-001, SRS-FUNC-007, SRS-FUNC-018, SRS-OPS-001, SRS-OPS-006
#[tokio::test]
#[ignore = "needs Docker; run with --ignored"]
async fn orchestrator_against_the_reference_runner() {
    let (_tmp, dir) = workdir();
    let (_runner, url) = start_runner(&dir);

    let slow_input = dir.join("inputs/slow.txt");
    write(&slow_input, "slow\n");
    let slow_extra = format!("input: {}\ntimeout_secs: 3\n", slow_input.display());
    let pass = spec(&dir, "pass.sqt.yaml", "REQ-E2E-PASS", "", 0);
    let fail = spec(&dir, "fail.sqt.yaml", "REQ-E2E-FAIL", "", 1);
    let slow = spec(&dir, "slow.sqt.yaml", "REQ-E2E-SLOW", &slow_extra, 0);

    // Pass, fail and infrastructure error in one session.
    let (report, written) = run_session(&url, &dir, &[&pass, &fail, &slow]).await.unwrap();
    assert_eq!(*outcome_of(&report, "REQ-E2E-PASS"), OutcomeKind::Pass);
    assert_eq!(*outcome_of(&report, "REQ-E2E-FAIL"), OutcomeKind::Fail);
    assert_eq!(*outcome_of(&report, "REQ-E2E-SLOW"), OutcomeKind::InfraError);
    assert!(report.summary.is_gate_failure(), "a failing spec must fail the gate");

    // The Runner's declared requirements were discovered and preserved.
    let discovery = report.discovery.as_ref().unwrap();
    assert_eq!(discovery.status, "obtained");
    assert_eq!((discovery.testable, discovery.compliant_by_declaration), (3, 1));
    let sdoc = written.run_dir.join(discovery.documents[0].evidence_id.as_ref().unwrap());
    assert_eq!(std::fs::read_to_string(sdoc).unwrap(), BASELINE_SDOC);
    assert_eq!(discovery.attachments[0].filename, "attestation.txt");

    // The real Runner's responses are the run's evidence.
    let report_json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&written.report_json).unwrap()).unwrap();
    assert_eq!(report_json["provenance"]["runner_target_id"], "e2e");
    let pass_ids = report.results.iter().find(|r| r.requirement_uid == "REQ-E2E-PASS").unwrap();
    let raw = written.run_dir.join(&pass_ids.evidence_ids[0]);
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(raw).unwrap()).unwrap();
    assert_eq!(raw["status"], "complete");
    assert_eq!(raw["metrics"]["return_code"], 0);

    // A session that leaves Runner-declared testable requirements without a
    // spec is a failed qualification before any job runs.
    let Err(err) = run_session(&url, &dir, &[&pass]).await else {
        panic!("a session leaving declared requirements uncovered must fail");
    };
    assert!(format!("{err:#}").contains("UNCOVERED"), "{err:#}");

    // All-pass session covering every declared requirement: the gate opens.
    let fail_ok = spec(&dir, "fail-ok.sqt.yaml", "REQ-E2E-FAIL", "", 0);
    let slow_ok = spec(&dir, "slow-ok.sqt.yaml", "REQ-E2E-SLOW", "", 0);
    let (report, _) = run_session(&url, &dir, &[&pass, &fail_ok, &slow_ok]).await.unwrap();
    assert!(report.results.iter().all(|r| r.outcome_kind == OutcomeKind::Pass));
    assert!(!report.summary.is_gate_failure(), "an all-pass run must pass the gate");
}
