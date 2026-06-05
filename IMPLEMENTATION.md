# SQT Reporter – Implementation Overview

**Status**: In progress
**Date**: 2026-02-24

---

## Scope of This Document

This document describes the concrete implementation plan for the first working version of
SQT Reporter. It deliberately narrows the scope compared to ARCHITECTURE.md:

- **Requirements parsing is deferred.** StrictDoc files fetched from `GET /api/v1/docs`
  will drive spec generation in a later phase. For now, all test specifications are written
  directly in Rust.
- **The metric extensibility system is compile-time only (Runner-side).** There is no dynamic
  loading, no external process protocol, and no Python SDK. The metric collectors described
  in Phase 2 are Runner-side reference implementations that will be wired into the local dev
  server (T-063). The Reporter itself never calls collectors directly - it submits jobs to the
  Runner API and reads the flat `JobMetrics` response.
- **The focus is on getting a runnable qualification loop.** The Reporter binary should be able
  to fetch docs, submit jobs to the SQT Runner, evaluate metric constraints, and produce a
  basic report.

---

## Design: Compile-Time Metric System

Instead of a dynamic plugin protocol, metrics are plain Rust types that implement a
shared trait. The registry is built automatically at link time using the
[`inventory`](https://crates.io/crates/inventory) crate - each collector file
self-registers with a single macro call. Adding a new metric means:

1. Create `src/metrics/builtin/my_metric.rs` (or `src/metrics/custom/my_metric.rs`)
2. Implement `MetricCollector` for your type
3. Call `inventory::submit!` at the bottom of the file
4. Add `mod my_metric;` to the parent `mod.rs`
5. Run `cargo build`

No central registration file is ever edited. This extensibility model lives in the SQT Runner;
the Reporter only reads the resulting `JobMetrics` fields over the REST API.

### Central Trait

```rust
// src/metrics/mod.rs

pub struct Target {
    pub binary:      PathBuf,
    pub platform:    Platform,   // Local | Remote(String)
    pub input:       Option<PathBuf>,
    pub working_dir: PathBuf,
    pub timeout:     Duration,
}

pub struct MetricValue {
    pub value:    f64,
    pub unit:     String,
    pub evidence: HashMap<String, String>,  // label → file path or inline text
}

pub trait MetricCollector: Send + Sync {
    /// Unique metric name, e.g. "memory.peak_rss_kb" or "custom.model_drift"
    fn name(&self) -> &str;

    /// Execute the measurement and return the value + evidence
    fn collect(&self, target: &Target, config: &serde_yaml::Value) -> anyhow::Result<MetricValue>;
}

/// Registration handle submitted by each collector via inventory::submit!
pub struct CollectorFactory {
    pub name:   &'static str,
    pub create: fn() -> Box<dyn MetricCollector>,
}

inventory::collect!(CollectorFactory);
```

### Self-Registration (one call per collector file)

```rust
// src/metrics/builtin/return_code.rs

pub struct ReturnCodeCollector;

impl MetricCollector for ReturnCodeCollector {
    fn name(&self) -> &str { "return_code" }
    fn collect(&self, target: &Target, _config: &serde_yaml::Value) -> anyhow::Result<MetricValue> {
        // ... implementation ...
    }
}

// This is all that's needed - no registry.rs edit required
inventory::submit! {
    CollectorFactory { name: "return_code", create: || Box::new(ReturnCodeCollector) }
}
```

### Registry (never needs to be edited)

```rust
// src/metrics/registry.rs

pub fn build_registry() -> MetricRegistry {
    let mut r = MetricRegistry::new();
    for factory in inventory::iter::<CollectorFactory> {
        r.register_boxed(factory.name, (factory.create)());
    }
    r
}
```

### Test Specification (Rust, not YAML)

Each test spec is a Rust struct that the user writes directly:

```rust
// src/specs/ml_l_req_003.rs

pub fn spec() -> TestSpec {
    TestSpec {
        requirement_uid: "REQ-ML-L-003",
        target: Target {
            binary:   "build/ml_l_inference".into(),
            platform: Platform::Local,
            input:    Some("datasets/reference_mnist.bin".into()),
            ..Default::default()
        },
        constraints: vec![
            Constraint::new("time.avg_ms", Op::Lte, Value::ms(100.0)),
            Constraint::new("memory.peak_rss_kb", Op::Lte, Value::kb(512.0)),
            Constraint::new("return_code", Op::Eq, Value::int(0)),
        ],
    }
}
```

All specs in `src/specs/` are collected at compile time via a central `all_specs()`
function - no discovery magic needed.

---

## Project Structure

```
sqt-reporter/
├── Cargo.toml
├── Cargo.lock
├── ARCHITECTURE.md
├── IMPLEMENTATION.md           ← this document
├── requirements/               ← StrictDoc SRS (reference only for now)
│
└── src/
    ├── main.rs                 ← CLI entry point
    │
    ├── config/
    │   ├── mod.rs
    │   └── schema.rs           ← ReporterConfig struct + sqt-reporter.yaml parsing
    │
    ├── metrics/                ← Runner-side reference collectors (used by local dev server T-063)
    │   ├── mod.rs              ← MetricCollector trait, Target, MetricValue, CollectorFactory
    │   ├── registry.rs         ← build_registry() via inventory::iter - never edit this
    │   ├── builtin/
    │   │   ├── mod.rs
    │   │   ├── return_code.rs
    │   │   ├── execution_time.rs
    │   │   ├── memory_rss.rs
    │   │   ├── memory_leaks.rs
    │   │   ├── coverage_line.rs
    │   │   └── sonar_issues.rs
    │   └── custom/
    │       └── mod.rs          ← user-defined collectors go here
    │
    ├── specs/
    │   ├── mod.rs              ← all_specs() - returns Vec<TestSpec>
    │   └── (user writes one file per requirement here)
    │
    ├── constraint/
    │   ├── mod.rs              ← Constraint, Op, Value types
    │   └── evaluator.rs        ← evaluate(constraint, metric_value) → bool
    │
    ├── orchestrator/
    │   └── mod.rs              ← run all specs, collect results
    │
    ├── runner_api/             ← REST client for SQT Runner (job-api.openapi.yaml)
    │   ├── mod.rs
    │   ├── client.rs           ← JobApiClient
    │   └── types.rs            ← JobRequest, JobStatusResponse, JobMetrics, …
    │
    ├── evidence/
    │   └── mod.rs              ← RunResult, RequirementResult aggregation
    │
    └── report/
        ├── mod.rs
        ├── json.rs             ← machine-readable output
        └── text.rs             ← human-readable terminal summary
```

---

## Implementation Tasks

Tasks are ordered by dependency. Complete each phase before starting the next.

> **Architecture alignment note (2026-06-05)**: After a full ARCHITECTURE.md ↔ SRS.sdoc
> alignment pass, a code vs. architecture gap analysis identified eight independent work
> streams (Phase 6, T-070–T-076 + T-044 correction). These are non-negotiable for SRS
> conformance; implementation of Phase 4 open items must incorporate them. See Phase 6
> for task details and the dependency graph at the bottom of this document.

---

### Phase 0 - Project Scaffold

- [x] **T-001** Initialize Cargo workspace
  - Add dependencies: `clap`, `serde`, `serde_yaml`, `anyhow`, `thiserror`, `tokio`,
    `reqwest` (with `json` and `rustls-tls` features), `tracing`, `tracing-subscriber`,
    `inventory`
  - Set up `rustfmt.toml` and `clippy` lint configuration (`pedantic` at warn)

- [x] **T-002** Implement `ReporterConfig` struct and YAML parser (`sqt-reporter.yaml`)
  - Full field set (matches ARCHITECTURE.md Section 3.1):
    - `project`: `name`, `version`, `ecss_level`, `target_trl`
    - `runner`: `url`, `auth.type`, `auth.token_env` (optional),
      `poll_interval_initial_secs`, `poll_interval_max_secs`, `job_timeout_secs`
    - `test_specs.directory`
    - `reports`: `output_dir`, `timestamp_format`,
      `formats.pdf.enabled`, `formats.pdf.engine` (typst | pdflatex | xelatex),
      `formats.html.enabled`, `formats.json.enabled`
    - `ci_integration`: `provider` (github_actions | gitlab_ci | jenkins | none),
      `artifacts.upload`, `artifacts.retention_days`, `artifacts.paths`
    - `logging`: `level`, `output` (stdout | file | both), `file_path`, `format` (json | plain)
  - No `model_zip_path` - the model is configured on the Runner at startup, not transmitted per job
  - Validate on load: required fields non-empty, runner URL starts with http/https,
    relative paths resolved against config file directory
  - `ci_integration` section can be stubbed with an `Unknown` provider variant initially
  - Unit tests with valid and invalid config fixtures (7 tests, all passing)

- [x] **T-003** Implement CLI skeleton
  - Single subcommand: `sqt run --config <path>` (env: `SQT_CONFIG`)
  - No `--model-zip` flag - the model is Runner-configured at startup
  - Load and validate config, print summary via `tracing::info!`, exit 0
  - `#[tokio::main]` from day one; `tracing_subscriber` with `EnvFilter` (default: info)

---

### Phase 1 - Core Type System

- [x] **T-010** Define `Target`, `Platform`, `MetricValue`, `MetricCollector` trait
  - `Platform`: `enum Platform { Local, Remote(String) }`
  - `Target`: binary path, `Platform`, optional input, working dir, `timeout: Duration`
    - implement `Default` manually with `timeout: Duration::from_secs(30)` so that
    `..Default::default()` in test specs doesn't produce a zero timeout
  - `MetricValue`: `f64` value, unit string, evidence map
  - `MetricCollector`: `name() -> &str`, `collect(target, config) -> Result<MetricValue>`
  - No implementations yet - types and trait only

- [x] **T-011** Define `Constraint`, `Op`, `Value` types
  - `Op`: `Lte | Gte | Lt | Gt | Eq | Ne`
  - `Value`: typed wrapper (milliseconds, kilobytes, percent, integer, boolean)
    supporting unit-aware comparisons (e.g., `512KB` vs `524288` bytes)
  - `Constraint`: metric name + op + value + `config: serde_yaml::Value` (defaults to
    `serde_yaml::Value::Null` if not set - this is what the orchestrator passes to
    `collect()` for that collector)

- [x] **T-012** Implement `ConstraintEvaluator`
  - `evaluate(actual: &MetricValue, constraint: &Constraint) -> ConstraintOutcome`
  - `ConstraintOutcome`: `{ passed: bool, actual: f64, expected: f64, message: String }`
  - Unit tests covering all six operators and unit conversions

- [x] **T-013** Define `TestSpec`, `SpecResult`, `RunResult`
  - `TestSpec`: requirement UID, target, vec of constraints
  - `SpecResult`: spec + vec of `ConstraintOutcome` + overall pass/fail
  - `RunResult`: vec of `SpecResult` + metadata (timestamp, config version)

---

### Phase 2 - Metric Registry and Built-in Collectors

> **Architecture note**: These collectors execute tools locally (valgrind, lcov, etc.).
> In the primary (API-first) architecture, they run **server-side** in the SQT Runner.
> The code remains in this crate as reference implementations and will be wired into the
> local dev server binary (T-063). The Reporter orchestrator (Phase 3) never calls
> collectors directly - it submits jobs to the Runner API and reads `JobMetrics`.

- [x] **T-020** Implement `MetricRegistry` and `build_registry()`
  - ~~Declare `CollectorFactory` struct and `inventory::collect!(CollectorFactory)`~~ **Done in
    T-010** - `CollectorFactory` and `inventory::collect!` already live in `src/metrics/mod.rs`
  - Remaining scope: `MetricRegistry` and `build_registry()` only
  - `MetricRegistry`: `HashMap<String, Box<dyn MetricCollector>>` with `register_boxed()`,
    `get(name)`, and panic-on-duplicate-name in debug builds
  - `build_registry()` in `src/metrics/registry.rs`: iterates `inventory::iter::<CollectorFactory>`
    and registers each one - this function never needs to be touched again
  - **Note**: in `build_registry()`, add `debug_assert_eq!((factory.create)().name(),
    factory.name)` for each factory to enforce that the instance `name()` and the registration
    key match

Each collector below follows the same two-step pattern:
a) implement `MetricCollector`, b) call `inventory::submit! { CollectorFactory { ... } }`
at the bottom of the file. No other file needs to change.

- [x] **T-021** Implement `ReturnCodeCollector`  (`return_code`)
  - Spawns the target binary, waits for exit, returns exit code as `f64`
  - Evidence: captured stdout/stderr saved to working dir
  - This is the simplest collector and validates the whole pipeline end-to-end

- [x] **T-022** Implement `ExecutionTimeCollector`  (`time.avg_ms`)
  - Wraps target execution with `std::time::Instant`
  - Returns average wall-clock time per repetition in milliseconds
  - Evidence: start time, end time, duration

- [x] **T-023** Implement `MemoryPeakRssCollector`  (`memory.peak_rss_kb`)
  - On Linux: spawn binary via `/usr/bin/time -v`, parse "Maximum resident set size"
  - Returns value in kilobytes
  - Evidence: raw `/usr/bin/time` output saved to working dir

- [x] **T-024** Implement `MemoryLeaksCollector`  (`memory.leaks_bytes`)
  - Spawn binary under `valgrind --tool=memcheck --leak-check=full`
  - Parse Valgrind XML output (`--xml=yes --xml-file=...`)
  - Returns total leaked bytes; evidence: Valgrind XML report

- [x] **T-025** Implement `CoverageLineCollector`  (`coverage.line_pct`)
  - Assumes test binary was compiled with `--coverage` (gcov instrumentation)
  - Runs `lcov --capture` then `lcov --summary` and parses line coverage percentage
  - Evidence: lcov `.info` file path and summary text

- [x] **T-026** Implement `SonarCriticalIssuesCollector`  (`static.critical_issues`)
  - Invokes `sonar-scanner` with project config
  - Reads issues from SonarQube REST API (`/api/issues/search?severities=CRITICAL`)
  - Returns count of critical issues as `f64`
  - Evidence: JSON response saved to working dir
  - **Note**: uses `reqwest::blocking` (HTTP from a sync fn). The orchestrator must call
    this collector inside `tokio::task::spawn_blocking` to avoid blocking the async
    executor. The same constraint applies to the local dev server (T-063) when it invokes
    this collector.

---

### Phase 3 - API Client, Specs, and Orchestrator

> **Note**: T-030 through T-035 were originally implemented against an old single-job
> design, then updated in Phase 4 for a two-endpoint (`/runs` + `/analysis`) design.
> **Phase 4 is being reverted** - see below. The Phase 3 code provides a working
> skeleton that Phase 4 will rebuild on top of.

- [x] **T-030** Define API types (`src/api/types.rs`)
  - ~~`JobRequest`, `TaskSpec`, `JobAccepted`, `JobStatusResponse`, `TaskResult`~~
    **→ Phase 4 will replace with** `JobRequest`, `JobAccepted`, `JobStatusResponse`,
    `JobMetrics`, `HardwareInfo`, `JobOutput`
  - `JobStatus`, `ErrorResponse`, `ApiError` - remain valid
  - All fields `Deserialize` + `Serialize`; `Option` fields use
    `#[serde(skip_serializing_if = "Option::is_none")]`

- [x] **T-031** Implement `JobApiClient` (`src/api/client.rs`)
  - `new(base_url, token)` - creates `reqwest` async client with optional Bearer header
  - ~~`submit_job`, `get_status`, `wait_for_completion`~~ **→ Phase 4 will replace with**
    `submit_job(&JobRequest) -> Result<String>` (job_id),
    `get_job_status(job_id) -> Result<JobStatusResponse>`,
    `wait_for_job(job_id) -> Result<JobStatusResponse>`
  - Exponential backoff: 1 s initial, x2 per poll, 30 s cap - unchanged

- [x] **T-032** Define `all_specs()` convention (`src/specs/mod.rs`)
  - `src/specs/mod.rs` exports `pub fn all_specs() -> Vec<TestSpec>`
  - Each spec file in `src/specs/` adds its specs to the vec
  - **Note**: unlike metrics, specs are NOT auto-discovered via `inventory`. Adding a
    new spec requires two manual steps: `mod my_spec;` in `specs/mod.rs` and a call to
    `my_spec::spec()` inside `all_specs()`. This is intentional - specs are project-
    specific configuration, not reusable plugins.
  - Each `TestSpec` carries its own workload type + input - the orchestrator uses
    `(workload, input)` as the deduplication key when grouping specs into jobs

- [x] **T-033** Write first example spec (`src/specs/example.rs`)
  - `REQ-EXAMPLE-001`: `return_code == 0`, `time.avg_ms <= 100`, `memory.peak_rss_kb <= 512`
  - Serves as the template users copy when adding their own specs

- [x] **T-034** Implement `Orchestrator` (`src/orchestrator/mod.rs`)
  - `run(specs, client, config) -> RunResult`
  - Group specs by `(workload, input)` → deduplicated `Vec<JobRequest>` (one per unique pair)
  - Submit all jobs concurrently (`tokio::join!` or `futures::join_all`); wait concurrently
  - Map `constraint.metric_name` → nullable field on `JobMetrics`; null → failing `ConstraintOutcome`
  - Assemble `RunResult` with one `SpecResult` per spec

- [x] **T-035** Wire orchestrator into CLI
  - `sqt run`: load config → `all_specs()` → `JobApiClient::new` → `orchestrator::run`
    → log `{passed}/{total}` summary → `exit(1)` on any failure

---

### Phase 4 - API Redesign (Single-Endpoint), Orchestrator Rewrite, Evidence and Reporting

> **Context**: The two-endpoint design implemented in the original Phase 4
> (`/api/v1/runs` + `/api/v1/analysis`) is being replaced by the single-endpoint
> design (`/api/v1/jobs`) documented in `job-api.openapi.yaml` v2.0.0. The key
> problems with the two-endpoint design were:
> - Implicit metric-routing by name prefix (`sonar.*` → analysis, else → run)
>   was fragile and silently wrong on typos.
> - Static analysis was a separate lifecycle that complicated the orchestrator.
> - No support for workload types, measurement modes, instrumentation, or output transfer.
>
> The new design folds everything into one job. The orchestrator becomes simpler:
> one job per unique `(workload, input)` pair; all metrics in one flat response.

#### API and orchestrator (redesign)

- [x] **T-040** Redesign API types for single-endpoint model (`sqt-types/src/lib.rs`)
  - **Note**: types were placed in the shared `sqt-types` workspace crate rather than
    `src/runner_api/types.rs` to avoid duplication between Reporter and Runner.
    All Reporter source imports them as `sqt_types::*`.
  - Replace `RunRequest`/`RunAccepted`/`RunStatusResponse`/`RunMetrics`/
    `AnalysisRequest`/`AnalysisAccepted`/`AnalysisStatusResponse`/`AnalysisMetrics`
    with the new unified types:
    - `WorkloadType` - enum: `ObjectDetection | ImageClassification | PixelSegmentation | Classification | Regression | TimeSeriesForecasting`
    - `Instrumentation` - enum: `None | Asan | Valgrind | Gprof`
    - `MeasurementMode` - enum: `Latency | Throughput`
    - `JobInput` - enum over input variants: `Images(Vec<Bytes>)`, `DatasetRef(String)`, `Tabular(Vec<Vec<f64>>)`, `TimeSeries(Vec<Vec<f64>>)`
    - `JobRequest` - `model`, `input`, `repetitions`, `instrumentation`, `mode`,
      `warmup_repetitions`, `batch_size`, `tags`, `timeout_secs`
      - **No `model_zip`** - the model is configured on the Runner at startup
    - `JobAccepted` - required fields: `job_id: String`, `status: String` (always `"accepted"`),
      `queue_position: u32` (**required, non-optional** - always present in the 202 response),
      `created_at: String`
    - `JobStatusResponse` - `job_id: String`, `model: WorkloadType`, `mode: MeasurementMode`,
      `repetitions: u32`, `status: JobStatusEnum`,
      `queue_position: Option<u32>` (non-null only when `status == "accepted"`),
      `created_at: String`, `started_at: Option<String>`, `completed_at: Option<String>`,
      `error: Option<String>`, `tags: Option<HashMap<String,String>>`,
      `hardware: Option<HardwareInfo>`, `metrics: Option<JobMetrics>`, `output: Option<JobOutput>`
    - `HardwareInfo` - `type: String` (required), `model: Option<String>`,
      `cores: Option<i64>`, `memory_total_kb: Option<i64>`, `accelerator_model: Option<String>`,
      `clock_mhz: Option<f64>` (note: float, not integer - e.g. GR740 @ 250.0 MHz),
      `os: Option<String>`
    - `JobMetrics` - all nullable fields matching `job-api.openapi.yaml §JobMetrics`:
      `return_code: Option<i64>`,
      `time.avg_ms: Option<f64>`, `time.latency_p50_ms: Option<f64>`,
      `time.latency_p95_ms: Option<f64>`, `time.latency_p99_ms: Option<f64>`,
      `time.throughput_fps: Option<f64>`, `time.startup_ms: Option<f64>`,
      `time.first_inference_ms: Option<f64>`,
      `memory.avg_rss_kb: Option<f64>`, `memory.peak_rss_kb: Option<f64>`,
      `memory.leaks_bytes: Option<i64>`, `memory.stack_kb: Option<f64>`,
      `memory.heap_allocations: Option<i64>`,
      `cpu.avg_normalized_pct: Option<f64>`,
      `coverage.line_pct: Option<f64>`, `coverage.branch_pct: Option<f64>`,
      `coverage.function_pct: Option<f64>`, `coverage.lines_covered: Option<i64>`,
      `coverage.lines_total: Option<i64>`,
      `static.warnings: Option<i64>`, `static.critical_issues: Option<i64>`,
      `static.major_issues: Option<i64>`,
      `binary.size_kb: Option<f64>`,
      `power.avg_watts: Option<f64>`, `power.peak_watts: Option<f64>`,
      `hardware.temperature_celsius: Option<f64>`
      - Field names contain dots: use `#[serde(rename = "time.avg_ms")]` on every field
      - `JobMetrics` is a flat struct, not a nested map
    - `JobOutput` - `type`, `result_consistent`, `outputs: Option<Vec<serde_json::Value>>`,
      `outputs_checksum: Option<String>`
  - All fields `Deserialize` + `Serialize`; `Option` fields use
    `#[serde(skip_serializing_if = "Option::is_none")]`

- [x] **T-041** Update `JobApiClient` for single-endpoint model (`src/api/client.rs`)
  - `submit_job`, `get_job_status`, `wait_for_job` — **done**
  - `get_docs()`, `get_version()`, transient retry, credential redaction — **moved to T-073**
    (Phase 6, Stream E; requires `VersionResponse` from T-071)

- [ ] **T-043** Implement baseline checker (`src/baseline/mod.rs`)
  - Called at session start before any jobs are submitted
  - Call `client.get_docs()` → `DocsResponse`
  - Parse each `StrictdocDocument.content` string: extract `[REQUIREMENT]` blocks,
    `UID`, `STATUS`, `TITLE`, `VERIFICATION` field
  - Classify each requirement:
    - `VERIFICATION` contains `Test` → testable; requires a matching `.test.yaml`
    - `VERIFICATION` contains only non-Test methods → compliant by declaration; no job needed
  - ECSS compliance gate: if required baseline documents are missing or empty, halt with
    a non-zero exit and a clear error message before any jobs are submitted
  - Returns `RequirementSet { testable: Vec<Requirement>, compliant_by_declaration: Vec<Requirement> }`
  - **Note**: full Strictdoc parsing (nested sections, relations, etc.) is deferred; only
    the fields needed for the compliance gate and traceability matrix are extracted here
  - Prerequisite of T-042 and T-044

- [ ] **T-044** *(corrected — see Phase 6 Stream B for full spec)*
  Implement YAML spec loader (`src/specs/loader.rs`)
  - **Reads only the files listed in `config.test_specs.files`** — does NOT scan a
    directory (SRS-MAINT-002 prohibits auto-discovery)
  - See T-044 in Phase 6 (Stream B) for the complete task description including
    unit-aware value parsing, `use_case` field name, and per-spec `timeout_secs`
  - Prerequisite of T-042 and T-043

- [x] **T-042** *(partial)* Rewrite `Orchestrator` for workload-based grouping (`src/orchestrator/mod.rs`)
  - Group specs by `(workload_type, input_key)` → deduplicated `Vec<JobRequest>`
  - No `model_zip` - omit entirely from `JobRequest`
  - No metric routing by prefix - all constraints reference fields of `JobMetrics` directly
  - Submit all jobs upfront; Runner executes sequentially (queue); Reporter polls each
    job ID via `wait_for_job` with exponential backoff
  - Map nullable `JobMetrics` field → `MetricValue` (null → `ConstraintOutcome { passed: false }`)
  - After job completes: if `output.outputs` is non-null, pass to accuracy evaluator
    (client-side computation of accuracy/F1/IoU/L2 against reference labels from spec)
  - Assemble `RunResult` with one `SpecResult` per spec

#### Evidence and reporting

- [ ] **T-050** Implement `EvidenceStore`
  - Writes all evidence files to a timestamped output directory
  - `store(job_id, label, content)` → writes file, returns path
  - Evidence content sourced from `JobOutput` and `JobMetrics` in job response
  - `manifest() -> Vec<EvidenceEntry>` for inclusion in reports

- [ ] **T-051** Implement JSON report (`src/report/json.rs`)
  - Serializes full `RunResult` to `report.json` in output dir
  - Machine-readable; consumed by CI systems

- [ ] **T-052** Implement terminal summary (`src/report/text.rs`)
  - Prints a table to stdout: requirement UID | constraints | pass/fail | values
  - Uses ANSI colours for pass (green) / fail (red) when `NO_COLOR` is not set
  - Printed at end of every `sqt run`

- [ ] **T-053** Implement traceability matrix (plain text + JSON)
  - Requires input from T-043 (requirement set) and T-044 (spec matching results)
  - Must cover all three ECSS-required categories for every requirement:
    - **Tested**: covered by a `.test.yaml` spec + job result (pass/fail + evidence paths)
    - **Compliant by declaration**: non-Test requirements accepted on server's authority;
      recorded with `evidence_source: server_declaration` and the declared verification methods
    - **Uncovered**: testable requirements with no matching `.test.yaml` spec (always fail)
  - Written to `traceability.json` and `traceability.txt`
  - Basis for the future PDF QDP report (T-055)

- [ ] **T-054** Implement client-side accuracy evaluator (`src/accuracy/mod.rs`)
  - Called after a completed job when `output.outputs` is non-null
  - Compares outputs against reference labels attached to the `TestSpec`
  - Computes configurable metrics per workload type:
    - `image_classification`, `classification`: accuracy, F1, precision, recall
    - `object_detection`: mAP, IoU@50, IoU@75
    - `regression`: MSE, MAE, R²
    - `time_series_forecasting`: MSE, MAE, MAPE
  - Returns a `Vec<MetricValue>` that feeds into `ConstraintEvaluator` like any other metric
  - Reference labels are attached to `TestSpec` as an optional `reference: Vec<serde_json::Value>`
    field; if absent, accuracy constraints for that spec are skipped with a warning

- [ ] **T-055** QDP packaging (`src/report/qdp.rs`) *(deferred - after T-053 is stable)*
  - Bundle all report outputs into `qdp.zip` with a manifest file
  - Minimum contents: `system-test-report.pdf` (or `.json` until PDF is implemented),
    `traceability.json`, `traceability.txt`, evidence files from `EvidenceStore`
  - The QDP is a required ECSS output artefact (ARCHITECTURE.md Section 1, 6.2, 7 step 9)
  - Scope: packaging only; report generation is T-051/T-053; PDF generation is post-T-055

---

### Phase 5 - Polish and CI Integration

- [ ] **T-060** Structured logging
  - Replace `println!` with `tracing` spans and events throughout
  - `sqt run --log-level debug` shows full API request/response detail
  - `sqt run --log-level warn` (default) shows only failures and summary

- [ ] **T-061** Integration test suite
  - Tests in `tests/` using a mock job API server (axum or warp)
  - Mock server returns canned `JobStatusResponse` for offline CI
  - One full orchestrator test covering pass and fail paths

- [ ] **T-062** CI workflow (GitHub Actions)
  - `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`
  - Runs on every push to `main` and every pull request

- [ ] **T-063** Local dev API server binary (`sqt-server`)
  - Second binary in the Cargo workspace: `sqt-server`
  - Uses the Phase 2 metric collectors as task implementations
  - Implements the `job-api.openapi.yaml` contract (SQT Runner interface)
  - Allows end-to-end testing without a live SQT Runner deployment

---

## Dependency Graph

```
T-001 ──► T-002 ──► T-003
T-010 ──► T-011
       └► T-012
       └► T-013

Phase 2 (done): T-020..T-026
  T-020 ◄── T-010
  T-021..T-026 ◄── T-020, T-010
  (each self-registers via inventory::submit!)

Phase 3 (new):
  T-030 (API types) ──► T-031 (API client)
  T-032 (specs)     ──► T-033 (example spec)
  T-031 + T-033     ──► T-034 (orchestrator) ──► T-035 (CLI wiring)
  T-035 ◄── T-002, T-003, T-013

Phase 4 (API redesign - single endpoint):
  T-040 (types) ──► T-041 (client) ──► T-043 (baseline checker)
  T-043 ──► T-044 (spec loader)
  T-041 + T-043 + T-044 ──► T-042 (orchestrator rewrite)
  T-042 ◄── T-035

Phase 4 (reporting + accuracy):
  T-050 ──► T-051 ──► T-052
  T-043 + T-044 + T-050 ──► T-053 (traceability matrix)
  T-053 ──► T-055 (QDP packaging)
  T-054 (accuracy evaluator) ◄── T-042

Phase 5: T-060 ──► T-061 ──► T-062 ──► T-063
  T-061 ◄── T-034
```

---

## What Is Explicitly Out of Scope (For Now)

| Feature | When |
|---|---|
| StrictDoc parsing from `GET /api/v1/docs` | Phase 2 - Reporter fetches docs at session start and auto-generates specs |
| Dynamic plugin loading (external process) | Explicitly rejected (AD-012); compile-time only |
| PDF QDP report generation | After traceability matrix is stable |
| Full SonarQube quality-gate integration | After basic collectors are working in Runner |
| GR740 / GR765 hardware targets | After TAL design is confirmed with Klepsydra |
| HTML interactive report | Nice-to-have, after JSON report is working |

---

## Known Issues and Forward Risks

Items identified during implementation review. Each entry has a severity, the phase where
it matters, and a resolution note.

---

### [RESOLVED] Empty constraints → vacuous PASS *(was High)*

`SpecResult::new` with zero outcomes previously returned `passed = true` (vacuous truth).
Fixed in Phase 1: `!outcomes.is_empty() && outcomes.iter().all(...)`. A spec with no
constraints now fails, as expected in a qualification context.

---

### [RESOLVED] `serde_yaml 0.9` deprecated *(was Medium)*

The original `serde_yaml` crate was abandoned by its author in 2024. Migrated to
`serde-yaml-ng` (the community-maintained fork) via Cargo alias - all source code unchanged:

```toml
serde_yaml = { package = "serde_yaml_ng", version = "0.9" }
```

---

### [RESOLVED] Unit comparison is by convention, not enforcement *(was Medium)*

Added `debug_assert_eq!(actual.unit, constraint.value.unit(), ...)` at the top of
`evaluate()` in Phase 2. Fires in debug builds if a collector returns a different unit
than the constraint expects (e.g., collector returns `"bytes"` but constraint uses
`Value::kb(...)`).

---

### [RESOLVED] `MetricCollector::name()` vs `CollectorFactory::name` - unenforceable contract *(was Medium)*

`build_registry()` now calls `debug_assert_eq!(instance.name(), factory.name, ...)` for
every factory. Diverging names panic immediately in debug builds. The trait return type
was also narrowed to `&'static str` to make it clear that names must be compile-time
constants.

---

### [OPEN] `Target::default().binary` is an empty path *(Medium - T-032)*

`Target::default()` sets `binary: PathBuf::new()`. Any spec that uses `..Default::default()`
without overriding `binary` will fail at runtime with a cryptic OS error when the orchestrator
tries to spawn it. The orchestrator (T-032) must validate `target.binary` is non-empty and
return a clear error before attempting execution.

---

### [OPEN] No `Serialize` on report types *(Low - T-051)*

`RunResult`, `SpecResult`, `ConstraintOutcome`, and `TestSpec` derive `Debug` and `Clone`
but not `Serialize`. T-051 needs to serialize the full `RunResult` to JSON. Adding
`#[derive(serde::Serialize)]` now would be zero-cost and would surface any schema surprises
early (e.g., how `serde_yaml::Value` round-trips through `serde_json`).

---

### [OPEN] `timestamp: String` in `RunResult` - no datetime crate *(Low - T-051)*

Generating an ISO 8601 timestamp from `std::time::SystemTime` without a crate requires
manual date arithmetic (leap years, month lengths). Add `jiff` or `chrono` to `Cargo.toml`
in Phase 5 before implementing T-051. Decision deferred intentionally; do not implement a
hand-rolled formatter.

---

### [OPEN] `serde_yaml::Value` embedded in the `MetricCollector` trait *(Low - Phase 5)*

`fn collect(&self, target: &Target, config: &serde_yaml::Value)` couples the core trait
interface to the YAML library. If the config serialization layer changes, the trait signature
must change too. Acceptable for now; revisit if a config abstraction layer is added before
Phase 5.

---

### [OPEN] `MetricCollector` trait signature couples to local execution *(Medium - T-063)*

`MetricCollector::collect(&self, target: &Target, config: &serde_yaml::Value)` assumes
the collector has local access to spawn processes. In the API-first architecture, the
orchestrator never calls collectors directly - it submits jobs and maps results. The
trait remains valid for the local dev API server (T-063). Revisit the interface when
building the server binary.

---

### [RESOLVED] Metric routing uses name-prefix convention - implicit coupling *(was Medium)*

The two-endpoint design required routing `constraint.metric_name` to either
`RunMetrics` or `AnalysisMetrics` based on a name-prefix convention. This was
fragile and implicitly coupled.

Resolved in Phase 4 redesign (AD-013): the single-endpoint design puts all metrics
in one flat `JobMetrics` struct. Constraints reference fields directly by name;
there is no routing logic. Metric name mismatches surface as `null` (failed
constraint) with a clear warning in the terminal summary.

---

### [OPEN] `Platform` enum no longer used by orchestrator *(Low - T-034)*

`Platform::Local/Remote` was designed for direct collector dispatch. The orchestrator
now always talks to an API URL (from `config.api.url`) - no platform selection needed.
`Platform` remains used by local collectors internally for the `bail!` guard. Consider
removing `Platform` from `Target` when the orchestrator is wired up in T-034.

---

### [RESOLVED] `#![allow(dead_code)]` on Phase 1 modules *(was Low)*

The broad `#![allow(dead_code)]` was removed from `metrics/mod.rs`. Replaced with
targeted `#[allow(dead_code)]` attributes on specific public API items and impl blocks
that are not yet wired to `main()` (will be connected in Phase 3 by the orchestrator).
`constraint/mod.rs` and `constraint/evaluator.rs` retain their broad suppression until
Phase 3 (T-032) when the orchestrator starts calling into them.

---

---

### Phase 6 — Architecture Alignment (SRS Conformance)

> These tasks close the gaps identified in the 2026-06-05 architecture ↔ code gap
> analysis. They are organized into **eight independent streams** that can be assigned
> and reviewed in parallel. Stream dependencies are noted per task. The architecture
> document (ARCHITECTURE.md) is the source of truth; the SRS supersedes where the two
> are ambiguous.
>
> Cross-cutting constraint: every task in this phase must keep `cargo test` green and
> `cargo clippy -- -D warnings` clean on completion.

---

#### Stream A — Config Schema Overhaul (SRS-MAINT-002, SRS-PERF-002/003/004, SRS-IF-006/009)

**Files changed**: `sqt-reporter/src/config/schema.rs`, `sqt-reporter/src/config/mod.rs`,
any fixture YAML files used in tests.

- [ ] **T-070** Overhaul `SqtConfig` to match ARCHITECTURE.md §3.2
  - **Remove** the `build_dir` field entirely (legacy from auto-discovery design)
  - **Add** `test_specs.files: Vec<PathBuf>` — explicit list of spec YAML paths; required
    field, empty list is a validation error (SRS-MAINT-002)
  - **Add** under `runner:` (extend the existing `ApiConfig` or a new sub-section):
    - `max_concurrent_jobs: u32` — default 8; controls Reporter-side semaphore
      (SRS-PERF-002)
    - `job_timeout_secs: u64` — default 300; project-level timeout, overridable per spec
      (SRS-PERF-004)
    - `retry_max: u32` — default 3; transient error retry ceiling (SRS-IF-009)
    - `retry_backoff_initial_secs: u64` — default 1; first retry delay (SRS-IF-009)
    - `poll_interval_initial_secs: u64` — default 1; first polling interval (SRS-PERF-003)
    - `poll_interval_max_secs: u64` — default 30; polling cap (SRS-PERF-003)
  - **Add** validation rule: if `runner.auth.token_env` is set (not None), the named env
    var must be present and non-empty at startup; if it is absent or empty, exit non-zero
    with: `"bearer token env var '$VAR' is set but empty or unset — check your environment"`
    (SRS-IF-006)
  - Update T-002 note in this document to reflect the corrected schema
  - Tests: update existing config tests; add test for empty token-env validation

---

#### Stream B — YAML Spec Loader Correction (SRS-MAINT-002, SRS-PERF-004, SRS-FUNC-004)

**Files changed**: `sqt-reporter/src/specs/loader.rs` (create), `sqt-reporter/src/specs/mod.rs`.
**Depends on**: Stream A (needs `test_specs.files` from config).

- [ ] **T-044** *(corrected)* Implement YAML spec loader (`src/specs/loader.rs`)
  - **Correction from original T-044**: do NOT scan a directory. Read only the files listed
    in `config.test_specs.files`. Auto-discovery from the filesystem is prohibited
    (SRS-MAINT-002)
  - Each file deserialises into `TestSpec`:
    ```yaml
    requirement_uid: REQ-ML-001
    use_case: image_classification          # APCO Use Case name (see WorkloadType)
    input: datasets/reference.bin           # optional
    repetitions: 5                          # optional, default 1
    timeout_secs: 120                       # optional; overrides runner.job_timeout_secs
    constraints:
      - metric: time.avg_ms
        operator: lte
        value: 100ms                        # unit-suffixed OR bare number
      - metric: memory.peak_rss_kb
        operator: lte
        value: 512 KB
      - metric: return_code
        operator: eq
        value: 0
      - metric: output.result_consistent
        operator: eq
        value: true
    ```
  - **Unit-aware value parsing** (SRS-FUNC-004): `value` field accepts `<number><unit>`
    strings (e.g., `100ms`, `2MB`, `512 KB`, `1.5s`). Normalise to the canonical unit
    of the target metric before storing (ms→ms, s→ms×1000; KB→KB, MB→KB×1024;
    bare number → metric's canonical unit). Parse error → non-zero exit.
  - **Terminology**: use `use_case` field (not `workload_type`) in the YAML schema
  - **Per-spec timeout**: deserialise optional `timeout_secs`; fall back to
    `config.runner.job_timeout_secs` when absent
  - **UNCOVERED requirements gate**: after matching specs to requirement UIDs from the
    baseline (T-043), any testable requirement with no matching spec is reported and exits
    non-zero (this gate runs in T-043, but the loader must return enough information for it)
  - Unit tests: parse minimal spec, parse full spec, parse unit-suffixed values, bad-unit
    parse error, missing required fields error, `output.result_consistent` constraint

---

#### Stream C — Runner: GET /api/v1/version Endpoint (SRS-IF-008)

**Files changed**: `sqt-types/src/lib.rs`, `sqt-runner/src/handlers.rs`,
`sqt-runner/src/main.rs`, `sqt-runner/src/config.rs`.
**Independent** — no Reporter dependency.

- [ ] **T-071** Add `VersionResponse` to `sqt-types` and wire the Runner endpoint
  - **`sqt-types`**: add `VersionResponse` struct:
    ```rust
    pub struct VersionResponse {
        pub api_version: String,       // e.g. "1.0"
        pub runner_version: String,    // from build-time env!("CARGO_PKG_VERSION")
        pub target_id: String,         // hardware descriptor key, e.g. "gr740-leon4"
        pub os: String,                // uname or cfg! info
        pub toolchain_versions: HashMap<String, String>,  // e.g. {"onnxruntime": "1.17"}
    }
    ```
  - **Runner config**: add `hardware.target_id: String` and
    `hardware.toolchain_versions: HashMap<String,String>` to the runner config struct so
    the handler can read them at startup (not hardcoded)
  - **Handler** (`sqt-runner/src/handlers.rs`): add `get_version(State<AppState>) -> Json<VersionResponse>`
    - `runner_version`: `env!("CARGO_PKG_VERSION")` baked in at compile time
    - `api_version`: `"1.0"` (match `job-api.openapi.yaml` major version)
    - `target_id`, `os`, `toolchain_versions`: read from `AppState` (populated at startup
      from config + runtime `uname`)
  - **Router** (`sqt-runner/src/main.rs`): add `GET /api/v1/version` → `get_version`
  - Tests: `GET /api/v1/version` returns 200 with all required fields; version string
    matches the Cargo.toml version

---

#### Stream D — Use Case Terminology Fix (ARCHITECTURE.md §5.5.1, §5.8)

**Files changed**: `sqt-types/src/lib.rs`, `sqt-reporter/src/orchestrator/mod.rs`,
`sqt-runner/src/handlers.rs` (any references to the field name in serialisation).
**Note**: this is a **breaking wire-format change** — Reporter and Runner must be updated
together; do not merge partial.

- [ ] **T-072** Rename `model` → `use_case` in the REST wire format
  - In `sqt-types`: rename `JobRequest.model: WorkloadType` → `use_case: WorkloadType`
    (the Rust field name changes; `serde(rename)` was not in use so the JSON key changes
    from `"model"` to `"use_case"`)
  - In `sqt-types`: rename `JobStatusResponse.model: WorkloadType` → `use_case`
  - In `sqt-reporter/src/orchestrator/mod.rs`: update `build_job_request` to set
    `req.use_case` instead of `req.model`; update `GroupKey` type if `WorkloadType` is
    still the right key (it is — just the struct field name changes)
  - In `sqt-runner`: update any handler code that reads/writes `model` from
    `JobRequest`/`JobStatusResponse`
  - Update all test fixtures that embed `"model":` JSON strings to `"use_case":`
  - Clippy + full test suite must pass on both crates after this change

---

#### Stream E — API Client Hardening (SRS-IF-008/009, SRS-OPS-005, SRS-IF-004)

**Files changed**: `sqt-reporter/src/api/client.rs`, `sqt-types/src/lib.rs` (VersionResponse
already added in C), `sqt-reporter/Cargo.toml` (no new deps expected).
**Depends on**: Stream C (VersionResponse type).

- [ ] **T-073** Harden `ApiClient` with version check, docs fetch, transient retry, and credential redaction
  - **Add `get_version() -> Result<VersionResponse>`**: `GET /api/v1/version`, deserialise
    into `VersionResponse`. Used by the orchestrator at session start
  - **Complete `get_docs() -> Result<DocsResponse>`**: this was listed in original T-041
    but the current `client.rs` lacks it. Fetch `GET /api/v1/docs` and return
    `DocsResponse`. Emit a `tracing::warn!` when `documents` is empty (SRS-IF-004
    empty-discovery diagnostic)
  - **Transient error retry** (SRS-IF-009): for `submit_job`, `get_version`, `get_docs`,
    and individual `get_job_status` calls, wrap each in a retry loop:
    - Retry on: `reqwest::Error::is_connect()`, `reqwest::Error::is_timeout()`,
      HTTP 500–504
    - Use config values `retry_max` and `retry_backoff_initial_secs` (exponential, same
      backoff formula as polling but separate counter)
    - After `retry_max` consecutive transient failures, return an `Err(...)` with the
      last error — the orchestrator maps this to an `INFRA_ERROR` outcome (T-074)
    - Do **not** retry on 401, 403, 422 (non-transient errors)
  - **Credential redaction** (SRS-OPS-005): the Bearer token must never appear in any
    `tracing` event, log line, `Display` output, or persisted report. Apply at the
    construction site:
    - `ApiClient` must NOT store the raw token in any field that derives `Debug`
      (wrap it in a `Redacted(String)` newtype with `Debug` impl that prints `"[REDACTED]"`)
    - Ensure no `format!("{:?}", client)` or equivalent leaks the token
  - Tests: mock server with `axum` (or `httpmock`); verify empty-docs warning; verify
    retry fires on 503 and stops after `retry_max`; verify token does not appear in
    `format!("{client:?}")`

---

#### Stream F — Orchestrator Session Protocol + INFRA_ERROR + SIGTERM (SRS-IF-008, SRS-PERF-002, SRS-OPS-006/008, SRS-FUNC-012, SRS-REL-003)

**Files changed**: `sqt-reporter/src/orchestrator/mod.rs`, `sqt-reporter/src/specs/mod.rs`,
`sqt-reporter/src/main.rs`, `Cargo.toml` (add `tokio-util` or use `tokio::signal`).
**Depends on**: Streams C (VersionResponse) and E (get_version, INFRA_ERROR retry).

- [ ] **T-074** Orchestrator: session start, concurrency cap, INFRA_ERROR, SIGTERM, progress
  - **Session start protocol** (SRS-IF-008): before submitting any job, call
    `client.get_version()`. Parse the returned `api_version`. If the major version does not
    match the Reporter's compiled `SUPPORTED_API_VERSION` constant, exit non-zero with:
    `"Runner API version {runner} incompatible with Reporter's supported {supported}"`
    Emit version details in the terminal at INFO level.
  - **Concurrency semaphore** (SRS-PERF-002): wrap job submission inside a
    `tokio::sync::Semaphore::new(config.runner.max_concurrent_jobs)` guard. Acquire before
    `submit_job`, release after the submission `await` returns. This limits how many
    in-flight submissions the Reporter holds at once. Polling continues concurrently for
    all submitted jobs regardless.
  - **INFRA_ERROR outcome** (SRS-OPS-006): when `client.wait_for_job` returns `Err(_)` OR
    when the `JobStatusResponse.status == JobStatus::Failed`, do NOT `bail!`. Instead,
    insert a synthetic `SpecResult` with `outcome = OutcomeKind::InfraError` and
    `message` containing the `error.code` + `error.message` from the `ErrorResponse`
    (or the transport error text). Continue evaluating remaining specs. The final exit
    code is non-zero if any outcome is non-PASS (including INFRA_ERROR).
  - **Error surfacing** (SRS-REL-003): on INFRA_ERROR, insert into
    `ConstraintOutcome::evidence` the keys: `error_code`, `error_message`,
    `error_details` from the Runner's `ErrorResponse`. These appear in both terminal
    summary and JSON report.
  - **Per-step progress signals** (SRS-FUNC-012): emit a `tracing::info!` at each of:
    1. Session start (after version check): `"SQT Reporter {sqt_ver} — Runner {runner_ver} at {url}"`
    2. Document fetch start/result
    3. Spec load: `"Loaded {n} specs from {m} files"`
    4. Each job submission: `"[{i}/{total}] Submitting use_case={uc} input={input} → job_id={id}"`
    5. Each job completion: `"[job_id={id}] {status} in {elapsed}s"`
    6. Constraint evaluation: `"Evaluating {n} constraints for {uid}"`
    7. Report write: `"Report written to {path}"`
    Signals must be sufficient for an operator to determine current step, prior outcome,
    and any intervention required — without consulting an external runbook (SRS-FUNC-012).
  - **SIGTERM graceful shutdown** (SRS-OPS-008): install a `tokio::signal::ctrl_c` /
    `tokio::signal::unix::signal(SIGTERM)` handler at session start. On signal:
    1. Stop accepting new submission slots (drop the semaphore permits to 0)
    2. Await completion of all already-submitted jobs (up to `job_timeout_secs` drain timeout)
    3. Persist the partial `RunResult` (jobs that completed) with `interrupted: true` in
       the report metadata
    4. Exit with code **2** (distinct from 0=all-pass, 1=any-fail)
  - Tests: unit-test that INFRA_ERROR does not propagate as `Err(…)` to the caller;
    test that zero-constraint spec → FAIL (vacuous pass prevention, already in SpecResult::new
    but verify the orchestrator path); test concurrency semaphore does not deadlock on
    `max_concurrent_jobs: 1`

---

#### Stream G — Report Hardening (SRS-DATA-001/003, SRS-OPS-006, SRS-REL-003/004)

**Files changed**: `sqt-reporter/src/report.rs` (and/or `src/report/`),
`sqt-reporter/src/specs/mod.rs`.
**Depends on**: Stream F (INFRA_ERROR type, interrupted flag, provenance from version check).

- [ ] **T-075** Harden report DTOs: schema_version, provenance, INFRA_ERROR, result_consistent
  - **`schema_version`** (SRS-DATA-001): add `pub schema_version: &'static str` = `"1.0"` to
    the top-level `Report` struct; always serialise it as the first JSON field. Emit a
    deprecation warning if the consumer sends back a report with a different `schema_version`
    (not needed now but reserve the field)
  - **Runner provenance** (SRS-DATA-003): add `pub provenance: RunProvenance` to `Report`:
    ```rust
    pub struct RunProvenance {
        pub sqt_version: String,          // env!("CARGO_PKG_VERSION")
        pub runner_version: String,        // from VersionResponse
        pub runner_target_id: String,
        pub runner_os: String,
        pub runner_toolchain_versions: HashMap<String, String>,
        pub config_summary: ConfigSummary, // sanitised: token_env shown as name only, no value
    }
    ```
    Populate from the `VersionResponse` received at session start (pass it through from
    orchestrator → report builder)
  - **`INFRA_ERROR` outcome** (SRS-OPS-006): add `InfraError` variant to the outcome
    category enum (wherever `PASS`/`FAIL` are defined). JSON key: `"INFRA_ERROR"`.
    Terminal summary: print in yellow, distinct from red (FAIL) and green (PASS)
  - **Error surfacing** (SRS-REL-003): `OutcomeReport` must carry the forwarded
    `error_code`, `error_message`, `error_details` from the Runner when the outcome is
    INFRA_ERROR. Shown in terminal summary one line below the outcome row
  - **`result_consistent` recording** (SRS-REL-004): for every completed job, record
    `output.result_consistent` (true/false/null) in the per-job evidence — regardless of
    whether the spec declares a reproducibility constraint. A `null` or absent field is
    recorded as `"result_consistent": null`. This field appears in both JSON report and
    terminal evidence section
  - **`interrupted` flag**: add `pub interrupted: bool` to `Report` metadata; set `true`
    when the run was terminated by SIGTERM (SRS-OPS-008)
  - Tests: JSON roundtrip includes `schema_version`; INFRA_ERROR serialises correctly;
    `result_consistent` null/true/false all appear in evidence

---

#### Stream H — Pre-flight CLI Mode (SRS-OPS-007)

**Files changed**: `sqt-reporter/src/main.rs`, `sqt-reporter/src/orchestrator/mod.rs`
(or a new `src/preflight.rs`).
**Depends on**: Streams D+E (get_version, get_docs).

- [ ] **T-076** Add `--preflight` flag and preflight logic
  - Add `--preflight` boolean flag to `Commands::Run` in the CLI (or as a separate
    subcommand `sqt preflight --config <path>` — either is acceptable; `--preflight` on
    `run` is simpler)
  - **Preflight steps** (SRS-OPS-007):
    1. Parse and validate config (same as `run`)
    2. Call `get_version()` — check compatibility; print result
    3. Call `get_docs()` — print document count and titles; warn if empty
    4. Verify bearer auth: if `token_env` is configured, confirm the env var is non-empty
       and that the version call succeeded (auth failures surface as 401 here)
    5. Load spec files from `test_specs.files`, report count and any parse errors
    6. Exit 0 if all steps pass; exit non-zero with a summary of which steps failed
  - **No jobs are submitted** during preflight under any circumstance
  - Terminal output must be sufficient for an operator to confirm the setup is correct
    without a full qualification run (SRS-OPS-007)
  - Exit code 2 for preflight interrupted by SIGTERM (same handler as full run)
  - Tests: preflight with valid config exits 0; preflight with unreachable URL exits
    non-zero with a clear message; preflight does NOT call `submit_job`

---

#### Updated Dependency Graph (Phase 6)

```
Stream A (T-070: config schema)
  └─► Stream B (T-044: YAML spec loader)

Stream C (T-071: Runner version endpoint)    [independent]

Stream D (T-072: use_case rename)            [independent, breaking wire change]

Stream E (T-073: API client)
  └─ needs Stream C for VersionResponse type

Stream F (T-074: orchestrator)
  └─ needs Stream C (VersionResponse)
  └─ needs Stream E (get_version, transient retry)

Stream G (T-075: report hardening)
  └─ needs Stream F (provenance, INFRA_ERROR, result_consistent)

Stream H (T-076: preflight)
  └─ needs Stream E (get_version, get_docs)
  └─ needs Stream A (config, test_specs.files)
```

**Parallel tracks**: Streams A+B, C, and D can start simultaneously.
Streams E and F can start as soon as C is merged. Streams G and H follow E+F.

---

## Getting Started (Once T-001 is Done)

```bash
# Build
cargo build

# Run against a local binary with an example spec
cargo run -- run --config examples/basic.sqt.yaml

# Run tests
cargo test

# Check formatting and lints
cargo fmt --check && cargo clippy -- -D warnings
```