# SQT — Implementation State

**Status**: Working end-to-end (reporter + reference runner). First release
`v0.1.0` (SQT-SW-0) tagged and published on 2026-11-19; SQT-SW-1 (`v0.2.0`) in
progress on `dev`.
**Date**: 2026-11-20 (`dev` at `f9c5639`, after #47), checked against SRS I1R2
and the ADD
**Tests**: 363 tests, every one naming the SRS requirement it verifies; all CI
gates green (rustfmt, clippy, supply-chain, test, e2e)

---

## Scope of This Document

This document describes **what is actually implemented today**, crate by crate,
and how it maps to the target design in `ARCHITECTURE.md`. Where the
implementation diverges from the architecture or the SRS, the divergence is listed
in the "Known Gaps & Deviations" section below and tracked in detail in
`REVIEW_FINDINGS.md`.

> **History note**: earlier revisions of this file were a forward-looking *plan*
> for a single-crate `sqt-reporter` binary (phases T-001…T-076, a "local dev
> server" `sqt-server`, a reporter-side `MetricCollector` extensibility model).
> That plan is essentially complete, but the shape changed: the project is now a
> three-crate workspace, and measurement moved into a real `sqt-runner` server
> rather than the planned `sqt-server` stub. This document has been rewritten to
> describe the resulting system; the old task-by-task plan is preserved in git
> history if needed.

---

## Workspace Layout

Three crates (see `ARCHITECTURE.md` §11.3 for the full tree). How to build,
test, branch, tag tests and release is in `CONTRIBUTING.md`.

| Crate | Role |
|-------|------|
| **`sqt-types`** | Shared DTOs — the `job-api.openapi.yaml` contract expressed in Rust (`JobRequest`, `JobStatusResponse`, `JobMetrics`, `JobOutput`, `HardwareInfo`, `VersionResponse`, `DocsResponse`, `HealthResponse`, `ErrorResponse`). Response DTOs use `#[serde(deny_unknown_fields)]` (SRS-IF-007). |
| **`sqt-reporter`** | The client CLI (the SRS "Orchestrator"): loads config + specs, runs the session protocol, submits/polls jobs, evaluates constraints, computes accuracy/precision client-side, writes reports + traceability + QDP. |
| **`sqt-runner`** | The reference SQT Runner: an `axum` server that executes the binary-under-test in Docker tool containers and measures it. Also ships `bin/test_binary.rs`, a configurable fixture. |

---

## sqt-reporter — Implemented

- **Config** (`config/`): `SqtConfig` (`project`, `test_specs.files`, `runner`,
  `report`); validates before any network call; resolves relative paths; rejects
  empty required fields, bad URL scheme, empty spec list, and a set-but-empty
  `token_env` (SRS-FUNC-002, SRS-IF-006), and a token with a plain `http://`
  Runner URL to any host but the local one (SRS-IF-010). Optional
  `runner.ca_cert`: a PEM CA certificate trusted, in addition to the public
  roots, for an `https://` Runner whose certificate comes from a private CA;
  checked at load. Unknown keys ignored.
- **Spec loading** (`specs/loader.rs`): refuses unknown keys, so a misspelt
  `use_case` is an error rather than a fan-out; reads only the files in
  `test_specs.files` — no directory scan (SRS-MAINT-002). Parses `use_case`,
  `input`, optional `repetitions`/`warmup_repetitions`/`mode`/`timeout_secs`
  (sent with the job, SRS-IF-002; specs that differ in them run as separate
  jobs), `constraints`, and
  `precision_reference`/`precision_tolerance` + `reference_outputs`. Unit-suffixed
  constraint values (`100ms`, `2MB`, `512 KB`) are normalised at load time
  (partial SRS-FUNC-004 — see gaps). A spec without `use_case` is expanded into
  one spec per workload type of the ML Space Profile, so one job runs per type
  (SRS-FUNC-019).
- **Constraint evaluation** (`constraint/`): typed `Constraint`/`Op`/`Value`;
  closed metric vocabulary via `Constraint::from_name`. Missing metric → failing
  outcome (SRS-REL-002); zero-constraint spec → FAIL (SRS-REL-001).
- **API client** (`api/client.rs`): `submit_job`, `get_job_status`,
  `wait_for_job`, `get_version`, `get_docs`; polling cadence from the
  `poll_interval_*` config (SRS-PERF-003); bounded transient-error retry
  distinct from polling backoff, never resending a submission that may have
  reached the Runner, with a per-request timeout (SRS-FUNC-020); bearer token
  wrapped in a `Redacted`
  newtype (SRS-OPS-005). A job the Runner reports `failed` is returned to the
  caller and recorded as `RUNNER_JOB_FAILED` with the Runner's error text
  (SRS-IF-003, SRS-REL-003). Redirects are not followed: the client contacts
  only the configured Runner URL (SRS-RES-002). At the most verbose log level
  every request and response payload is logged, with the token redacted
  (SRS-OPS-002). Every Runner response is recorded as raw bytes for the
  evidence store.
- **Orchestrator** (`orchestrator/mod.rs`): version-compatibility gate at session
  start (SRS-IF-008); groups specs by (workload type, input, measurement
  settings) into jobs; submits and polls the
  jobs concurrently, holding a `max_concurrent_jobs` permit from submit to
  completion (SRS-PERF-001/002), then evaluates; INFRA_ERROR outcome category,
  including a failed submit, that does not abort the run (SRS-OPS-006); on a first SIGTERM/Ctrl-C it stops
  submitting and awaits the jobs already running, which are evaluated; a second
  signal stops waiting; the partial run is persisted and the exit status is 2
  (SRS-OPS-008). A run that stops before evaluation (incompatible contract
  version, failed discovery) still writes its report and prints the summary
  with every specification BLOCKED (SRS-OPS-003); per-step progress logging (a start on SRS-FUNC-012, operator guidance).
- **Accuracy/precision** (`accuracy/`): client-side accuracy metrics and
  element-wise precision comparison against spec-declared references (SRS-FUNC-016,
  SRS-FUNC-017), fed by the output the runner now returns.
- **Baseline** (`baseline/`): parses the requirement documents the Runner
  declares (`GET /api/v1/docs`), classifies them by verification method and
  keeps the documents and attachments verbatim as evidence. A failed fetch
  stops the run before any job is submitted; an empty set proceeds with a
  diagnostic (SRS-FUNC-018).
- **Evidence / report / traceability** (`evidence/`, `report.rs`,
  `traceability/`, `outputs.rs`): one directory per run (`runs/<run_id>/`), raw
  Runner responses and per-spec outcomes as evidence with a SHA-256 manifest;
  `Report` (schema 1.3) with run id, manifest, per-spec evidence ids and
  workload type, specification and requirement counts, and
  provenance (SRS-DATA-001/003); JSON + PDF (`printpdf`) + terminal outputs;
  dual-audience traceability matrix; `qdp.zip` of the run directory.
- **CLI** (`cli.rs`, `main.rs`): `sqt run` and `sqt preflight` (also
  `run --preflight`); exit codes 0/1/2, stated in `sqt run --help` (SRS-HF-001).
  `-v` and `-vv` raise the log level on top of `RUST_LOG`, `-q` logs only
  warnings (SRS-OPS-002). The end-of-run summary lists every specification,
  the specification, requirement and constraint counts and the run directory
  (SRS-OPS-003).

## sqt-runner — Implemented

- **Server** (`main.rs`, `handlers.rs`): axum routes `POST /api/v1/jobs`,
  `GET /api/v1/jobs/{id}`, `GET /api/v1/docs`, `GET /api/v1/version`,
  `GET /api/v1/health`. Bearer-token middleware on every route except `/health`
  when `auth.token_env` is configured, 401 otherwise (SRS-IF-006); without a
  token, auth is off with a startup warning. In-memory job store; one job at a
  time (`Semaphore::new(1)`), with each waiting job's real `queue_position`;
  blocking work runs on `spawn_blocking`. A job's working directory is removed
  when it finishes, and only the newest `max_finished_jobs` finished jobs are
  kept (default 1000).
- **Config** (`config.rs`): `backend:` discriminant + `docker:` section (tools +
  optional build step), `binary`, `workspace`, `hardware`, `runner_hardware`,
  optional `docs`. All paths resolved to absolute at load.
- **Execution backend** (`executor/`): object-safe `ExecutionBackend` +
  ergonomic `LifecycleBackend` (prepare/run/collect/teardown) with a blanket impl;
  `DockerExecutor` is the shipped backend; `build_executor` factory keyed on
  `backend:`.
- **Tools** (`tools/`): `run_binary` (time/RSS/CPU via `/usr/bin/time -v`;
  `time.avg_ms` is the binary's own elapsed time, the Docker overhead goes to
  `harness.docker_overhead_ms`; arguments and stdin target are shell-quoted),
  `repetition` (the binary runs `warmup_repetitions` times unmeasured, then
  `repetitions` times, all within the job timeout; mean, p50/p95/p99 latency,
  throughput in throughput mode, highest peak RSS),
  `output_capture` (stdout of the first measured run as the job output, with
  checksum and `result_consistent` = every run wrote the same output),
  `valgrind`, `coverage`, `static_analysis` (cppcheck), `sonarqube`,
  `binary_size` — each run in its configured Docker image; unconfigured tools are
  skipped and their metrics reported `null`. A tool that did not complete
  (non-zero exit, no finished valgrind run, no source for cppcheck) also
  reports `null`, never `0`. `cpu.avg_normalized_pct` is GNU time's figure
  divided by the CPU count of the measuring host, so 100 means every CPU was
  busy. A finished job's working directory is removed on a blocking thread;
  root-owned tool output is deleted through a short-lived container of the run
  image.
- **TLS** (`tls.rs`): with a `tls:` section the Runner serves HTTPS only
  (rustls, ring provider, no OpenSSL), with the configured certificate chain
  and key, checked at start-up (SRS-IF-010). Without it, plain HTTP, with a
  warning when bound to a non-loopback address.
- **Docker helpers** (`docker.rs`): container run with timeout enforcement and
  kill-on-timeout; reader threads avoid pipe-buffer deadlock.

## sqt-types — Implemented

Single `lib.rs` with all shared DTOs and their tests pinning wire names (e.g. the
six `WorkloadType` values, sent in the `use_case` field). The DTOs are the single
source of truth shared by both sides; there is intentionally no per-crate
duplication. `tests/openapi_contract.rs` fails the build if `job-api.openapi.yaml`
and the DTOs disagree on field names.

---

## Known Gaps & Deviations

Full detail, severity and evidence live in **`REVIEW_FINDINGS.md`**, including
the SQT-SW-1 plan (steps P-1 to P-5), the release follow-ups R-1 to R-10 and the
conformance check against the SRS and the ADD (C-1 to C-5, 2026-11-20). The SRS
and the ADD in the documentation repository are authoritative. Summary of what
is *not* yet aligned with them:

**Deviations from the SRS and the ADD:** none known. The five found by the
check (C-1 to C-5) are fixed in #48.

**SQT-SW-1 requirements without a test yet** (the fulfilment matrix lists them on
every push): FUNC-011, FUNC-012, FUNC-013, FUNC-014, FUNC-021, FUNC-022,
FUNC-023, RES-003, HF-002. Among them:
- API-profile qualification (SRS-FUNC-021 to SRS-FUNC-023): declared API profile,
  generic workload set, coverage report; target design in `ARCHITECTURE.md` §4.7.
- Triggering verification activities through the Runner API (SRS-FUNC-014/015):
  needs a contract change.
- Closed metric/constraint vocabulary versus SRS-FUNC-006(a): decided to open the
  namespace, issue #17 (`ARCHITECTURE.md` §10.2).

**Known limitations of `v0.1.0`**, recorded in the SRelD and carried into SW-1:
- Runner responses are validated by type, required fields and enumerations, not
  against the ranges and patterns of the OpenAPI schema (IF-007).

**Contract / behaviour:**
- `input`: the OpenAPI `JobInput` union versus the free-form value the code
  accepts is still a pending contract decision (rest of F-H2).
- SRS-IF-010 is implemented on the Orchestrator side (a bearer token travels
  only over `https://`, or over plain `http://` to the local host and never
  through a proxy) but is in no release of the SQT-TN-AP plan yet (R-10).
- Robustness item F-M8 (traceability provenance).

**Dependencies:** cargo-audit warns that `chrono` and `unsafe-libyaml` are
unmaintained and `rustix` (test-only) is unsound; allowed by policy (R-7).

---

## Timing metric: execution vs. Docker overhead (review item C3, resolved)

`/usr/bin/time -v` runs **inside** the container and reports the binary's own
elapsed time ("Elapsed (wall clock) time"). `run_binary` parses that line for
**`time.avg_ms`** and reports the outer `docker run` wall-clock minus the inner
elapsed time — container create/start, image setup, shell spawn, teardown — as the
additive diagnostic **`harness.docker_overhead_ms`**, which can also be declared as
a constraint. Without GNU time in the image, `time.avg_ms` falls back to the outer
wall-clock.

---

## Getting Started

```bash
# Build the whole workspace
cargo build --workspace

# Run the reference runner (Docker required for real tool execution)
cargo run -p sqt-runner -- -c examples/sqt-runner.example.yaml --port 3000

# In another shell: preflight, then a full qualification run
cargo run -p sqt-reporter -- preflight -c examples/sqt-reporter.example.yaml
cargo run -p sqt-reporter -- run       -c examples/sqt-reporter.example.yaml

# Tests, format, lints
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# End-to-end: the Orchestrator against the real sqt-runner in Docker
# (SRS-QUAL-003, SRS-VAL-001); the `e2e` CI job runs the same
cargo test -p sqt-runner --test e2e_test -- --ignored

# Fulfilment matrix: which SRS requirements the tests cover
python3 ci/fulfilment-matrix.py --requirements ci/requirements.csv
```

The CI gates, the release pipeline and the conventions every change follows are
described in `CONTRIBUTING.md`.
