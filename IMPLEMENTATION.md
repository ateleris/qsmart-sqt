# SQT Generic – Implementation Overview

**Status**: Initial planning
**Date**: 2026-02-24

---

## Scope of This Document

This document describes the concrete implementation plan for the first working version of
SQT Generic. It deliberately narrows the scope compared to ARCHITECTURE.md:

- **Requirements parsing is deferred.** StrictDoc files will be read in a later phase to
  auto-generate Rust implementations. For now, all test specifications are written
  directly in Rust.
- **The plugin system is compile-time only.** There is no dynamic loading, no external
  process protocol, and no Python SDK. Users add a Rust file to the correct module,
  implement one trait, add one `mod` declaration, and recompile. The registry is
  populated automatically via the `inventory` crate — no central registration file.
- **The focus is on getting a runnable qualification loop.** The binary should be able to
  run a set of metric collectors against a target binary, evaluate constraints, and
  produce a basic report.

---

## Design: Compile-Time Metric System

Instead of a dynamic plugin protocol, metrics are plain Rust types that implement a
shared trait. The registry is built automatically at link time using the
[`inventory`](https://crates.io/crates/inventory) crate — each collector file
self-registers with a single macro call. Adding a new metric means:

1. Create `src/metrics/builtin/my_metric.rs` (or `src/metrics/custom/my_metric.rs`)
2. Implement `MetricCollector` for your type
3. Call `inventory::submit!` at the bottom of the file
4. Add `mod my_metric;` to the parent `mod.rs`
5. Run `cargo build`

No central registration file is ever edited. The architecture can evolve toward dynamic
plugins (Section 10 of ARCHITECTURE.md) once the core loop is proven.

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
    /// Unique metric name, e.g. "memory.peak_rss" or "custom.model_drift"
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

// This is all that's needed — no registry.rs edit required
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
            Constraint::new("time.execution", Op::Lte, Value::ms(100.0)),
            Constraint::new("memory.peak_rss", Op::Lte, Value::kb(512.0)),
            Constraint::new("return_code", Op::Eq, Value::int(0)),
        ],
    }
}
```

All specs in `src/specs/` are collected at compile time via a central `all_specs()`
function — no discovery magic needed.

---

## Project Structure

```
ats-sqt/
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
    │   └── schema.rs           ← SqtConfig struct + YAML parsing
    │
    ├── metrics/
    │   ├── mod.rs              ← MetricCollector trait, Target, MetricValue, CollectorFactory
    │   ├── registry.rs         ← build_registry() via inventory::iter — never edit this
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
    │   ├── mod.rs              ← all_specs() — returns Vec<TestSpec>
    │   └── (user writes one file per requirement here)
    │
    ├── constraint/
    │   ├── mod.rs              ← Constraint, Op, Value types
    │   └── evaluator.rs        ← evaluate(constraint, metric_value) → bool
    │
    ├── orchestrator/
    │   └── mod.rs              ← run all specs, collect results
    │
    ├── api/
    │   ├── mod.rs              ← REST client for ML-L Specific (Klepsydra)
    │   ├── client.rs
    │   └── types.rs
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

---

### Phase 0 — Project Scaffold

- [x] **T-001** Initialize Cargo workspace
  - Add dependencies: `clap`, `serde`, `serde_yaml`, `anyhow`, `thiserror`, `tokio`,
    `reqwest` (with `json` and `rustls-tls` features), `tracing`, `tracing-subscriber`,
    `inventory`
  - Set up `rustfmt.toml` and `clippy` lint configuration (`pedantic` at warn)

- [x] **T-002** Implement `SqtConfig` struct and YAML parser
  - Fields: `project.name`, `project.version`, `build_dir`, `api.url`,
    `api.token_env` (optional), `report.output_dir`
  - Validate on load: required fields non-empty, API URL starts with http/https,
    relative paths resolved against config file directory
  - Unit tests with valid and invalid config fixtures (7 tests, all passing)

- [x] **T-003** Implement CLI skeleton
  - Single subcommand: `sqt run --config <path>` (env: `SQT_CONFIG`)
  - Load and validate config, print summary via `tracing::info!`, exit 0
  - `#[tokio::main]` from day one; `tracing_subscriber` with `EnvFilter` (default: info)

---

### Phase 1 — Core Type System

- [x] **T-010** Define `Target`, `Platform`, `MetricValue`, `MetricCollector` trait
  - `Platform`: `enum Platform { Local, Remote(String) }`
  - `Target`: binary path, `Platform`, optional input, working dir, `timeout: Duration`
    — implement `Default` manually with `timeout: Duration::from_secs(30)` so that
    `..Default::default()` in test specs doesn't produce a zero timeout
  - `MetricValue`: `f64` value, unit string, evidence map
  - `MetricCollector`: `name() -> &str`, `collect(target, config) -> Result<MetricValue>`
  - No implementations yet — types and trait only

- [x] **T-011** Define `Constraint`, `Op`, `Value` types
  - `Op`: `Lte | Gte | Lt | Gt | Eq | Ne`
  - `Value`: typed wrapper (milliseconds, kilobytes, percent, integer, boolean)
    supporting unit-aware comparisons (e.g., `512KB` vs `524288` bytes)
  - `Constraint`: metric name + op + value + `config: serde_yaml::Value` (defaults to
    `serde_yaml::Value::Null` if not set — this is what the orchestrator passes to
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

### Phase 2 — Metric Registry and Built-in Collectors

- [ ] **T-020** Implement `MetricRegistry` and `build_registry()`
  - ~~Declare `CollectorFactory` struct and `inventory::collect!(CollectorFactory)`~~ **Done in
    T-010** — `CollectorFactory` and `inventory::collect!` already live in `src/metrics/mod.rs`
  - Remaining scope: `MetricRegistry` and `build_registry()` only
  - `MetricRegistry`: `HashMap<String, Box<dyn MetricCollector>>` with `register_boxed()`,
    `get(name)`, and panic-on-duplicate-name in debug builds
  - `build_registry()` in `src/metrics/registry.rs`: iterates `inventory::iter::<CollectorFactory>`
    and registers each one — this function never needs to be touched again
  - **Note**: in `build_registry()`, add `debug_assert_eq!((factory.create)().name(),
    factory.name)` for each factory to enforce that the instance `name()` and the registration
    key match

Each collector below follows the same two-step pattern:
a) implement `MetricCollector`, b) call `inventory::submit! { CollectorFactory { ... } }`
at the bottom of the file. No other file needs to change.

- [ ] **T-021** Implement `ReturnCodeCollector`  (`return_code`)
  - Spawns the target binary, waits for exit, returns exit code as `f64`
  - Evidence: captured stdout/stderr saved to working dir
  - This is the simplest collector and validates the whole pipeline end-to-end

- [ ] **T-022** Implement `ExecutionTimeCollector`  (`time.execution`)
  - Wraps target execution with `std::time::Instant`
  - Returns wall-clock time in milliseconds
  - Evidence: start time, end time, duration

- [ ] **T-023** Implement `MemoryPeakRssCollector`  (`memory.peak_rss`)
  - On Linux: spawn binary via `/usr/bin/time -v`, parse "Maximum resident set size"
  - Returns value in kilobytes
  - Evidence: raw `/usr/bin/time` output saved to working dir

- [ ] **T-024** Implement `MemoryLeaksCollector`  (`memory.leaks`)
  - Spawn binary under `valgrind --tool=memcheck --leak-check=full`
  - Parse Valgrind XML output (`--xml=yes --xml-file=...`)
  - Returns total leaked bytes; evidence: Valgrind XML report

- [ ] **T-025** Implement `CoverageLineCollector`  (`coverage.line_percent`)
  - Assumes test binary was compiled with `--coverage` (gcov instrumentation)
  - Runs `lcov --capture` then `lcov --summary` and parses line coverage percentage
  - Evidence: lcov `.info` file path and summary text

- [ ] **T-026** Implement `SonarCriticalIssuesCollector`  (`sonar.critical_issues`)
  - Invokes `sonar-scanner` with project config
  - Reads issues from SonarQube REST API (`/api/issues/search?severities=CRITICAL`)
  - Returns count of critical issues as `f64`
  - Evidence: JSON response saved to working dir
  - **Note**: uses `reqwest::blocking` (HTTP from a sync fn). The orchestrator must call
    this collector inside `tokio::task::spawn_blocking` to avoid blocking the async
    executor. Same rule applies to `RemoteCollector` (T-043).

---

### Phase 3 — Test Specs and Orchestrator

- [ ] **T-030** Define `all_specs()` convention
  - `src/specs/mod.rs` exports `pub fn all_specs() -> Vec<TestSpec>`
  - Each spec file in `src/specs/` adds its specs to the vec
  - **Note**: unlike metrics, specs are NOT auto-discovered via `inventory`. Adding a
    new spec requires two manual steps: `mod my_spec;` in `specs/mod.rs` and a call to
    `my_spec::spec()` inside `all_specs()`. This is intentional — specs are project-
    specific configuration, not reusable plugins.
  - Document the pattern so users know exactly what to add and where

- [ ] **T-031** Write first example spec (`src/specs/example.rs`)
  - Uses `ReturnCodeCollector` and `ExecutionTimeCollector` against a local binary
  - Serves as the template users copy when adding their own specs
  - Compile and run this as the first end-to-end smoke test

- [ ] **T-032** Implement `Orchestrator`
  - `run(specs: &[TestSpec], registry: &MetricRegistry) -> RunResult`
  - For each spec: for each constraint: look up collector by `constraint.metric_name` →
    call `collector.collect(spec.target, &constraint.config)` → evaluate outcome
  - Collect all outcomes; mark spec passed iff all constraints pass
  - Handle collector errors gracefully: record as failed constraint with error message,
    continue with remaining specs

- [ ] **T-033** Wire orchestrator into CLI
  - `sqt run`: load config → build registry → load all specs → run orchestrator →
    print summary → exit with 0 (all pass) or 1 (any fail)

---

### Phase 4 — ML-L Specific REST API Client

- [ ] **T-040** Define API types (`src/api/types.rs`)
  - `JobSubmitRequest`, `JobSubmitResponse`, `JobStatus`, `JobResults`
  - Derived from `ml-l-api.minimal.openapi.yaml`
  - All fields `serde`-deserializable

- [ ] **T-041** Implement `MllApiClient`
  - `submit_job(req) -> Result<String>` (returns job ID)
  - `get_status(job_id) -> Result<JobStatus>`
  - `get_results(job_id) -> Result<JobResults>`
  - Bearer token read from env var specified in config

- [ ] **T-042** Implement polling loop with exponential backoff
  - `wait_for_completion(job_id) -> Result<JobResults>`
  - Initial delay: 1 s; max delay: 30 s; max total wait: configurable
  - Returns `Err` on `failed` or `cancelled` status

- [ ] **T-043** Implement `RemoteCollector` wrapper
  - A `MetricCollector` that delegates to the ML-L API for a single metric
  - Submits a job, polls to completion, extracts the relevant metric from `JobResults`
  - Registered in the registry like any built-in
  - This makes remote hardware metrics transparent to the orchestrator

- [ ] **T-044** Write mock API server for offline testing
  - Simple `axum` or `warp` server that returns canned responses
  - Used in integration tests so CI does not require a live Klepsydra endpoint

---

### Phase 5 — Evidence Collection and Reporting

- [ ] **T-050** Implement `EvidenceStore`
  - Writes all evidence files to a timestamped output directory
  - `store(run_id, label, content)` → writes file, returns path
  - `manifest() -> Vec<EvidenceEntry>` for inclusion in reports

- [ ] **T-051** Implement JSON report (`src/report/json.rs`)
  - Serializes full `RunResult` to `report.json` in output dir
  - Machine-readable; consumed by CI systems

- [ ] **T-052** Implement terminal summary (`src/report/text.rs`)
  - Prints a table to stdout: requirement UID | constraints | pass/fail | values
  - Uses ANSI colours for pass (green) / fail (red) when `NO_COLOR` is not set
  - Printed at end of every `sqt run`

- [ ] **T-053** Implement traceability matrix (plain text + JSON)
  - Maps each requirement UID → verification status + evidence file paths
  - Written to `traceability.json` and `traceability.txt`
  - Basis for the future PDF QDP report

---

### Phase 6 — Polish and CI Integration

- [ ] **T-060** Structured logging
  - Replace `println!` with `tracing` spans and events throughout
  - `sqt run --log-level debug` shows full collector invocation detail
  - `sqt run --log-level warn` (default) shows only failures and summary

- [ ] **T-061** Integration test suite
  - Tests in `tests/` using the mock API server from T-044
  - One test per built-in collector using real local binaries (`/bin/true`, `/bin/false`)
  - One full orchestrator test covering pass and fail paths

- [ ] **T-062** CI workflow (GitHub Actions)
  - `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`
  - Runs on every push to `main` and every pull request

---

## Dependency Graph

```
T-001 ──► T-002 ──► T-003
                         │
T-010 ──► T-011          │
       └► T-012          │
       └► T-013          │
                         ▼
T-020 ◄── T-010     T-030 ──► T-031
T-021..T-026 ◄── T-020, T-010
  (each self-registers via inventory::submit!)
          │                    │
          ▼                    ▼
         T-032 ◄── T-013, T-020, T-030
          │
          ▼
         T-033 ◄── T-002, T-003
          │
          ▼
T-040..T-044 (parallel with T-021..T-026)
          │
          ▼
T-050 ──► T-051 ──► T-052 ──► T-053
          │
          ▼
T-060 ──► T-061 ──► T-062
```

---

## What Is Explicitly Out of Scope (For Now)

| Feature | When |
|---|---|
| StrictDoc parsing | Phase 2 of the project — will generate Rust specs automatically |
| Dynamic plugin loading (external process) | After compile-time approach is validated |
| PDF QDP report generation | After traceability matrix is stable |
| Full SonarQube quality-gate integration | After basic collectors are working |
| GR740 / GR765 hardware targets | After Klepsydra API contract is finalised |
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
`serde-yaml-ng` (the community-maintained fork) via Cargo alias — all source code unchanged:

```toml
serde_yaml = { package = "serde_yaml_ng", version = "0.9" }
```

---

### [OPEN] Unit comparison is by convention, not enforcement *(Medium — Phase 2)*

`evaluate()` compares raw `f64` values. `MetricValue.unit` is display-only metadata — there
is no runtime check that the collector's unit matches the constraint's unit. A collector
returning bytes while the constraint uses `Value::kb(...)` gives a silently wrong result.

**Resolution**: When implementing `build_registry()` (T-020) or the first real collector
(T-021), add to `evaluate()`:

```rust
debug_assert_eq!(
    actual.unit,
    constraint.value.unit(),
    "unit mismatch: collector returned '{}' but constraint expects '{}'",
    actual.unit, constraint.value.unit()
);
```

---

### [OPEN] `MetricCollector::name()` vs `CollectorFactory::name` — unenforceable contract *(Medium — T-020)*

The registry keys off `factory.name`. The instance method `fn name(&self) -> &str` is
separate and can diverge silently. Address in T-020: add a `debug_assert_eq!` in
`build_registry()` (noted in the T-020 task description above).

---

### [OPEN] `Target::default().binary` is an empty path *(Medium — T-032)*

`Target::default()` sets `binary: PathBuf::new()`. Any spec that uses `..Default::default()`
without overriding `binary` will fail at runtime with a cryptic OS error when the orchestrator
tries to spawn it. The orchestrator (T-032) must validate `target.binary` is non-empty and
return a clear error before attempting execution.

---

### [OPEN] No `Serialize` on report types *(Low — T-051)*

`RunResult`, `SpecResult`, `ConstraintOutcome`, and `TestSpec` derive `Debug` and `Clone`
but not `Serialize`. T-051 needs to serialize the full `RunResult` to JSON. Adding
`#[derive(serde::Serialize)]` now would be zero-cost and would surface any schema surprises
early (e.g., how `serde_yaml::Value` round-trips through `serde_json`).

---

### [OPEN] `timestamp: String` in `RunResult` — no datetime crate *(Low — T-051)*

Generating an ISO 8601 timestamp from `std::time::SystemTime` without a crate requires
manual date arithmetic (leap years, month lengths). Add `jiff` or `chrono` to `Cargo.toml`
in Phase 5 before implementing T-051. Decision deferred intentionally; do not implement a
hand-rolled formatter.

---

### [OPEN] `serde_yaml::Value` embedded in the `MetricCollector` trait *(Low — Phase 5)*

`fn collect(&self, target: &Target, config: &serde_yaml::Value)` couples the core trait
interface to the YAML library. If the config serialization layer changes, the trait signature
must change too. Acceptable for now; revisit if a config abstraction layer is added before
Phase 5.

---

### [OPEN] `#![allow(dead_code)]` on Phase 1 modules *(Low — Phase 3)*

`metrics/mod.rs`, `constraint/mod.rs`, and `constraint/evaluator.rs` all carry this
suppression. Remove the attributes at the start of Phase 3, when the orchestrator begins
calling into these types, and fix whatever the compiler surfaces. Do not defer past T-032.

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