# SQT — Implementation State

**Status**: Working end-to-end (reporter + reference runner)
**Date**: 2026-10-15 (`main` at `b4ed836`)
**Tests**: `cargo test --workspace` green in CI on `b4ed836` (279 at the 2026-07-03
review); `cargo clippy --workspace` clean

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

Three crates (see `ARCHITECTURE.md` §11.3 for the full tree):

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
  `token_env` (SRS-FUNC-002, SRS-IF-006). Unknown keys ignored.
- **Spec loading** (`specs/loader.rs`): reads only the files in
  `test_specs.files` — no directory scan (SRS-MAINT-002). Parses `use_case`,
  `input`, optional `repetitions`/`timeout_secs`, `constraints`, and
  `precision_reference`/`precision_tolerance` + `reference_outputs`. Unit-suffixed
  constraint values (`100ms`, `2MB`, `512 KB`) are normalised at load time
  (partial SRS-FUNC-004 — see gaps).
- **Constraint evaluation** (`constraint/`): typed `Constraint`/`Op`/`Value`;
  closed metric vocabulary via `Constraint::from_name`. Missing metric → failing
  outcome (SRS-REL-002); zero-constraint spec → FAIL (SRS-REL-001).
- **API client** (`api/client.rs`): `submit_job`, `get_job_status`,
  `wait_for_job`, `get_version`, `get_docs`; polling cadence from the
  `poll_interval_*` config (SRS-PERF-003); bounded transient-error retry
  distinct from polling backoff (SRS-FUNC-020); bearer token wrapped in a `Redacted`
  newtype (SRS-OPS-005).
- **Orchestrator** (`orchestrator/mod.rs`): version-compatibility gate at session
  start (SRS-IF-008); groups specs by `(use_case, input)`; submits and polls the
  jobs concurrently, holding a `max_concurrent_jobs` permit from submit to
  completion (SRS-PERF-001/002), then evaluates; INFRA_ERROR outcome category,
  including a failed submit, that does not abort the run (SRS-OPS-006); SIGTERM/Ctrl-C handler that drains, persists a partial run, and
  exits 2 (SRS-OPS-008); per-step progress logging (SRS-FUNC-012).
- **Accuracy/precision** (`accuracy/`): client-side accuracy metrics and
  element-wise precision comparison against spec-declared references (SRS-FUNC-016,
  SRS-FUNC-017), fed by the output the runner now returns.
- **Baseline** (`baseline/`): parses baseline requirement docs, classifies by
  verification method, coverage gate.
- **Evidence / report / traceability** (`evidence/`, `report.rs`,
  `traceability/`): per-job evidence store; `Report` with `schema_version` and
  provenance (SRS-DATA-001/003); JSON + PDF (`printpdf`) + terminal outputs;
  dual-audience traceability matrix; `qdp.zip` bundling.
- **CLI** (`main.rs`): `sqt run` and `sqt preflight` (also `run --preflight`);
  exit codes 0/1/2.

## sqt-runner — Implemented

- **Server** (`main.rs`, `handlers.rs`): axum routes `POST /api/v1/jobs`,
  `GET /api/v1/jobs/{id}`, `GET /api/v1/docs`, `GET /api/v1/version`,
  `GET /api/v1/health`. Bearer-token middleware on every route except `/health`
  when `auth.token_env` is configured, 401 otherwise (SRS-IF-006); without a
  token, auth is off with a startup warning. In-memory job store; one job at a
  time (`Semaphore::new(1)`); blocking work runs on `spawn_blocking`.
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
  `output_capture` (stdout as the job output with checksum and
  `result_consistent`),
  `valgrind`, `coverage`, `static_analysis` (cppcheck), `sonarqube`,
  `binary_size` — each run in its configured Docker image; unconfigured tools are
  skipped and their metrics reported `null`.
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

Full detail, severity, and file:line evidence live in **`REVIEW_FINDINGS.md`**
(2026-07-03 review, status updated 2026-10-15). The critical and high findings of
that review are fixed except as listed here. Summary of what is *not* yet aligned
with the architecture / SRS:

**Planned for SQT-SW-1:**
- API-profile qualification (SRS-FUNC-021 to SRS-FUNC-023): declared API profile,
  generic workload set, coverage report — not implemented; target design in
  `ARCHITECTURE.md` §4.7.
- Closed metric/constraint vocabulary versus SRS-FUNC-006(a): decided to open the
  namespace, issue #17 (`ARCHITECTURE.md` §10.2).

**Contract / behaviour:**
- `input`: the OpenAPI `JobInput` union versus the free-form value the code
  accepts is still a pending contract decision (rest of F-H2).
- Runner job failures surface as transport errors, not `RUNNER_JOB_FAILED`
  (F-M3); the runner ignores `repetitions` / `warmup_repetitions` / `mode` (F-M6);
  no default fan-out when a spec declares no workload type (F-M7, SRS-FUNC-019).
- Evidence is not run-scoped and the JSON report needs `--output` (F-M4); other
  robustness items F-M1, F-M5 and F-M8 to F-M11.

**Housekeeping:**
- rustfmt CI gate is advisory (`continue-on-error: true`) — SRS-QUAL-002/CDR-003.
- No release tags yet (SRS-CDR-001); `v0.1.0` is planned for SQT-SW-0 at PDR.
- `sqt-reporter/src/metrics/` is unwired legacy code (deletion candidate).

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
cargo clippy --workspace -- -D warnings
```
