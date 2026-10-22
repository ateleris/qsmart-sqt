# SQT — Implementation State

**Status**: Working end-to-end (reporter + reference runner)
**Date**: 2026-07-03
**Tests**: 279 passing (`cargo test --workspace`); `cargo clippy --workspace` clean

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
  `wait_for_job`, `get_version`, `get_docs`; bounded transient-error retry
  distinct from polling backoff (SRS-IF-009); bearer token wrapped in a `Redacted`
  newtype (SRS-OPS-005).
- **Orchestrator** (`orchestrator/mod.rs`): version-compatibility gate at session
  start (SRS-IF-008); groups specs by `(use_case, input)`; submits, polls, and
  evaluates; INFRA_ERROR outcome category that does not abort the run
  (SRS-OPS-006); SIGTERM/Ctrl-C handler that drains, persists a partial run, and
  exits 2 (SRS-OPS-008); per-step progress logging (SRS-FUNC-012).
- **Accuracy/precision** (`accuracy/`): client-side accuracy metrics and
  element-wise precision comparison against spec-declared references (SRS-FUNC-016,
  SRS-FUNC-017) — see gaps re: end-to-end reachability.
- **Baseline** (`baseline/`): parses baseline requirement docs, classifies by
  verification method, coverage gate.
- **Evidence / report / traceability** (`evidence/`, `report.rs`,
  `traceability/`, `outputs.rs`): one directory per run (`runs/<run_id>/`), raw
  Runner responses and per-spec outcomes as evidence with a SHA-256 manifest;
  `Report` (schema 1.1) with run id, manifest and per-spec evidence ids, and
  provenance (SRS-DATA-001/003); JSON + PDF (`printpdf`) + terminal outputs;
  dual-audience traceability matrix; `qdp.zip` of the run directory.
- **CLI** (`main.rs`): `sqt run` and `sqt preflight` (also `run --preflight`);
  exit codes 0/1/2.

## sqt-runner — Implemented

- **Server** (`main.rs`, `handlers.rs`): axum routes `POST /api/v1/jobs`,
  `GET /api/v1/jobs/{id}`, `GET /api/v1/docs`, `GET /api/v1/version`,
  `GET /api/v1/health`. In-memory job store; one job at a time
  (`Semaphore::new(1)`); blocking work runs on `spawn_blocking`.
- **Config** (`config.rs`): `backend:` discriminant + `docker:` section (tools +
  optional build step), `binary`, `workspace`, `hardware`, `runner_hardware`,
  optional `docs`. All paths resolved to absolute at load.
- **Execution backend** (`executor/`): object-safe `ExecutionBackend` +
  ergonomic `LifecycleBackend` (prepare/run/collect/teardown) with a blanket impl;
  `DockerExecutor` is the shipped backend; `build_executor` factory keyed on
  `backend:`.
- **Tools** (`tools/`): `run_binary` (time/RSS/CPU via `/usr/bin/time -v`),
  `valgrind`, `coverage`, `static_analysis` (cppcheck), `sonarqube`,
  `binary_size` — each run in its configured Docker image; unconfigured tools are
  skipped and their metrics reported `null`.
- **Docker helpers** (`docker.rs`): container run with timeout enforcement and
  kill-on-timeout; reader threads avoid pipe-buffer deadlock.

## sqt-types — Implemented

Single `lib.rs` with all shared DTOs and their tests pinning wire names (e.g. the
six APCO use-case values). The DTOs are the single source of truth shared by both
sides; there is intentionally no per-crate duplication.

---

## Known Gaps & Deviations

Full detail, severity, and file:line evidence live in **`REVIEW_FINDINGS.md`**
(2026-07-03 review). Summary of what is *not* yet aligned with the architecture /
SRS:

**Security / correctness (must-fix):**
- Command injection via the client-supplied `input` field in the Docker backend
  (`tools/run_binary.rs`); the runner enforces no bearer auth (`handlers.rs` TODO)
  despite the contract requiring it (SRS-IF-006).
- `time.avg_ms` reports the whole `docker run` wall-clock, not the binary's own
  elapsed time (Docker overhead inflates the primary latency metric). See the
  "Timing" note below for the fix.
- A run in which every spec is INFRA_ERROR exits 0 (`main.rs` checks only
  `summary.failed`, which excludes `infra_errors`) — violates SRS-OPS-001/006.

**Contract / behaviour:**
- `job-api.openapi.yaml` still says `model`; the code uses `use_case` everywhere.
  The OpenAPI file must be regenerated (add a contract test).
- The runner never returns `output`, so client-side accuracy (SRS-FUNC-017) and
  precision (SRS-FUNC-016) and `result_consistent` (SRS-REL-004) are dead
  end-to-end against the shipped runner.
- Concurrency (SRS-PERF-001/002/003) is not realised: submit and poll are
  sequential and the `max_concurrent_jobs` semaphore permit is dropped immediately
  after each submit (no-op). `poll_interval_*` config keys are parsed but unwired.
- Closed metric/constraint vocabulary conflicts with SRS-FUNC-006 (open design
  decision — see `REVIEW_FINDINGS.md` §Decisions and `ARCHITECTURE.md` §10.2).

**Housekeeping:**
- No `LICENSE` / license metadata (SRS-DES-002).
- rustfmt CI gate is advisory (`continue-on-error: true`) — SRS-QUAL-002/CDR-003.

---

## Timing metric: execution vs. Docker overhead (resolution of review item C3)

`/usr/bin/time -v` runs **inside** the container and already reports the binary's
true elapsed time ("Elapsed (wall clock) time"), but `run_binary.rs` currently
discards that line and reports the outer `docker run` duration instead. We can and
should distinguish, and report, both:

- **`time.avg_ms`** → the binary's own elapsed time, parsed from the `time -v`
  "Elapsed" line (the value constraints care about).
- **harness overhead** → outer `docker run` wall-clock minus the inner elapsed
  time (container create/start, image setup, shell spawn, teardown), reported as a
  separate diagnostic field.

Reporting the overhead separately is worth it for transparency. It requires an
additive field on `sqt-types::JobMetrics` (which has `deny_unknown_fields`, so
this is a deliberate schema addition and a matching constraint name on the reporter
side). Tracked in `REVIEW_FINDINGS.md`.

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
