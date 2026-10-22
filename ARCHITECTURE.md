# Q-SMART SQT Architecture Overview

**Project**: Qualified Software Machine Learning Toolkit for Space Hardware
**Components**: SQT Runner + SQT Reporter (both Ateleris responsibility)
**Technology**: Rust applications
**Version**: 1.3
**Date**: 2026-10-15

> **Authority**: the Architectural Design Document (ADD, SQT-DD-0001-ATS) is the
> authoritative design; this document is the developers' working view of it and
> follows it wherever the two differ. §2.4 maps the sections and crate modules
> here to the ADD component identifiers (C-ORC-nn, C-RNR-nn, C-SHR-01). The ADD
> calls the SQT Reporter the **Orchestrator** and the `sqt-runner` crate the
> **reference Runner**; its design is normative for the Orchestrator and the
> Runner API contract, and non-normative for third-party Runners.
>
> **v1.3 revision note**: aligned with the PDR documents (SRS SQT-SP-0001-ATS
> Issue 1 Rev 2, ADD, SQT-TN-AP): component mapping (§2.4), external interfaces
> (§6.4), run lifecycle and exit status (§7.3), target emulation (§8.3), decided
> open questions (§13), and the SQT-SW-0..3 release plan (§14).
>
> **v1.2 revision note**: Sections 3 (configuration), 8 (Runner internals), 9
> (report generation), 10 (metric extensibility), and 11 (technology stack /
> project layout) were rewritten to describe the **target architecture that the
> implemented workspace realises** — the three-crate layout (`sqt-types`,
> `sqt-reporter`, `sqt-runner`), the `backend:`/`docker:` runner config, and the
> `ExecutionBackend`/`LifecycleBackend` abstraction. Behavioural sections (2, 5,
> 7) describe SRS-mandated behaviour and remain the target even where the current
> implementation does not yet fully meet it; those deltas are tracked in
> `REVIEW_FINDINGS.md` and reflected in `IMPLEMENTATION.md`, not softened here.

---

## Table of Contents

- [1. Executive Summary](#1-executive-summary)
- [2. System Architecture](#2-system-architecture)
  - [2.4 Mapping to ADD Components](#24-mapping-to-add-components)
- [3. Configuration Management](#3-configuration-management)
- [4. Requirements Management](#4-requirements-management)
  - [4.5 Declarative Test Specifications](#45-declarative-test-specifications)
  - [4.6 Compliant Requirements (Non-Testable)](#46-compliant-requirements-non-testable)
  - [4.7 API-Profile Qualification](#47-api-profile-qualification)
- [5. REST API Interface (SQT Runner)](#5-rest-api-interface-sqt-runner)
  - [5.2 Workload Types](#52-workload-types)
  - [5.5 API Endpoints](#55-api-endpoints)
  - [5.5.3 Health Check](#553-health-check)
  - [5.10 Requirements Documents Endpoint](#510-requirements-documents-endpoint)
- [6. Scope Definition](#6-scope-definition)
  - [6.4 Interface Contract](#64-interface-contract)
- [7. Qualification Workflow](#7-qualification-workflow)
  - [7.3 Run Lifecycle and Exit Status](#73-run-lifecycle-and-exit-status)
- [8. SQT Runner Target Architecture](#8-sqt-runner-target-architecture)
- [9. Report Generation](#9-report-generation)
- [10. Metric Extensibility](#10-metric-extensibility)
- [11. Technology Stack](#11-technology-stack)
- [12. Key Architectural Decisions](#12-key-architectural-decisions)
- [13. Open Questions](#13-open-questions)
- [14. Next Steps](#14-next-steps)
  - [14.1 Release Plan](#141-release-plan)
- [Appendices](#appendices)

---

## 1. Executive Summary

The Software Qualification Toolkit (SQT) is the Ateleris-developed component of the Q-SMART qualification infrastructure. It qualifies Machine Learning Libraries (ML-L) targeting space-grade hardware and consists of **two distinct Rust applications**:

### SQT Runner

The SQT Runner is a REST API server deployed on **customer-provided hardware**. It is the component that actually executes tests against the AI system under qualification. It:

- **Assembles the AI system** from the ML-L Provider's artifacts (inference engine, trained model, main program) into a fully runnable task
- **Executes workload tests** (representative workloads per workload type: does the ML-L load each one, execute it and return a valid result) and **Performance tests** (timing, memory, throughput) against the assembled system
- **Serves static qualification artifacts** (ECSS documentation baseline, unit test results, validation test results) pre-placed on its filesystem by the ML-L Provider
- **Exposes a REST API** (`job-api.openapi.yaml`) that the SQT Reporter calls to trigger and monitor test jobs
- **Supports multiple execution targets** via a pluggable execution backend (x86\_64/Linux via Docker today; the GR740 and GR765/NOEL-V through emulation behind the Runner in SQT-SW-2, §8.3)

### SQT Reporter

The SQT Reporter is a CLI application that **orchestrates the full qualification session**. It:

- **Fetches the Runner-declared requirements** and their compliance evidence (ECSS documentation baseline, static test results) from the SQT Runner
- **Partitions them by verification method** before any job is submitted: non-Test requirements are recorded as compliant by Runner declaration, Test requirements must be covered by a registered specification (SRS-FUNC-018)
- **Submits all test jobs concurrently** to the SQT Runner via REST API (receives job IDs immediately; a configurable concurrency bound limits in-flight submissions) and polls all outstanding jobs concurrently until completion
- **Evaluates metric constraints** against returned results (declarative test specifications), with unit-aware comparison
- **Reports API-profile coverage**: relates the workload outcomes to the declared API profile and reports which of its elements were exercised and which were not (SRS-FUNC-021 to SRS-FUNC-023, from SQT-SW-1)
- **Generates the qualification record**: per-run evidence, a machine-readable JSON report, a PDF summary, the dual-audience traceability matrix and the QDP (Quality Data Package) bundle (§9)

### Key Principles

1. **Separation of concerns**: The SQT Runner handles test execution and artifact serving; the SQT Reporter handles orchestration, requirements checking, and reporting
2. **Deployment flexibility**: The SQT Runner deploys on the customer's hardware (where access to the target systems exists); the SQT Reporter runs on any commodity x86-64 Linux host (SRS-PORT-001; other hosts are permitted but not a normative property), needs no inbound connectivity, and contacts only the configured Runner (SRS-RES-002)
3. **Contract-first, target-agnostic Reporter**: the Reporter is designed against the versioned Runner API contract (`job-api.openapi.yaml`), not against a Runner; all target-specific behaviour lives on the Runner side (SRS-PORT-002, ADD LLR-032). New execution targets (GR740, GR765) are added behind the Runner's execution backend without changing the Reporter or the contract
4. **Configuration-driven**: All aspects configurable via YAML; test specifications registered by explicit declaration (no auto-discovery)
5. **Qualification capability, not qualified tool**: the SQT enables qualification of the ML-L to ECSS-E-ST-40C / ECSS-Q-ST-80C criticality level C (SRS-GEN-002); the SQT itself is assigned no criticality category (SRS-DES-005). Workload types and the declared API profile follow the ML Space Profile (ML-TN-T1/T2)
6. **Evidence-based**: Comprehensive traceability from requirements to test results
7. **Concurrent orchestration**: The Reporter submits all jobs and polls all outcomes concurrently, bounded by a configurable concurrency limit; the Runner queues and executes jobs sequentially (initial implementation: one at a time)

---

## 2. System Architecture

### 2.1 High-Level Architecture

The overall qualification process follows a three-tier swimlane model:

```
╔══════════════════════════════════════════════════════════════════════════════╗
║              ML-L Provider  (e.g. Klepsydra)  [Out of Scope]                ║
║                                                                              ║
║  Prepares and supplies (pre-placed on Runner filesystem before session):     ║
║    • ECSS Documentation Baseline (Strictdoc)                                ║
║    • Unit Test Results (static)                                              ║
║    • Validation Test Results (static)                                        ║
║    • Inference Engine binary/library                                         ║
║    • Trained Model (e.g. ONNX)                                               ║
║    • Main Program (host application)                                         ║
║    • Input / Output reference data                                           ║
╚═════════════════════════════╤════════════════════════════════════════════════╝
                              │  Artifacts pre-placed on Runner filesystem
                              │  (configurable paths; zero network transfer
                              │   from Provider at qualification time)
                              ▼
╔══════════════════════════════════════════════════════════════════════════════╗
║        SQT Runner  (Ateleris - Rust)  [Customer-provided hardware]          ║
╠══════════════════════════════════════════════════════════════════════════════╣
║                                                                              ║
║  ┌──────────────────────┐    ┌──────────────────────────────────────────┐   ║
║  │  Document Store      │    │  AI System Assembly                      │   ║
║  │  (configurable paths)│    │  • Links inference engine, trained model │   ║
║  │                      │    │    and main program into a runnable task │   ║
║  │  • ECSS baseline     │    │  • Exact mechanism TBD (depends on       │   ║
║  │  • Unit test results │    │    ML-L Provider's delivery format)      │   ║
║  │  • Validation results│    └────────────────┬─────────────────────────┘   ║
║  └─────────┬────────────┘                     │                             ║
║            │ GET /api/v1/docs           ┌─────▼───────────────────────┐     ║
║            │                            │  Test Execution Engine      │     ║
║  ┌─────────▼────────────────────────┐   │                             │     ║
║  │  REST API Server                 │   │  ┌─────────────────────┐    │     ║
║  │  (implements job-api.openapi)    │◄──┤  │ Workload Tests       │    │     ║
║  │                                  │   │  │ (representative,     │    │     ║
║  │  GET  /api/v1/version → compat   │   │  │  workload types)     │    │     ║
║  │  GET  /api/v1/health → liveness  │   │  └─────────────────────┘    │     ║
║  │  POST /api/v1/jobs  → enqueue    │   │  ┌─────────────────────┐    │     ║
║  │  GET  /api/v1/jobs/{id} → poll   │   │  │ Performance Tests    │    │     ║
║  │  GET  /api/v1/docs  → serve docs │   │  │ (timing, memory,     │    │     ║
║  └──────────────────────────────────┘   │  │  throughput)         │    │     ║
║                                         │  └─────────────────────┘    │     ║
║                                         └─────────────┬───────────────┘     ║
║                                                       │                     ║
║  ┌────────────────────────────────────────────────────▼─────────────────┐   ║
║  │                  Execution Backend Abstraction (TAL)                  │   ║
║  │                                                                       │   ║
║  │   ┌──────────────────┐   ┌─────────────────────┐  ┌───────────────┐  │   ║
║  │   │ x86_64 / Linux   │   │ GR740 (LEON4)       │  │ GR765/NOEL-V  │  │   ║
║  │   │ Docker backend   │   │ Emulated: QEMU,     │  │ Emulated      │  │   ║
║  │   │ [shipped]        │   │ Renode, SIS [SW-2]  │  │ [SQT-SW-2]    │  │   ║
║  │   └──────────────────┘   └─────────────────────┘  └───────────────┘  │   ║
║  └───────────────────────────────────────────────────────────────────────┘   ║
╚══════════════════════════════════════════════════════════════════════════════╝
                               │
         REST API  (job-api.openapi.yaml)
         • GET  /api/v1/version   - contract version (session start)
         • GET  /api/v1/health    - liveness probe (not used by the Reporter)
         • POST /api/v1/jobs      - submit test request
         • GET  /api/v1/jobs/{id} - poll for results + metrics
         • GET  /api/v1/docs      - fetch document baseline
                               │
                               ▼
╔══════════════════════════════════════════════════════════════════════════════╗
║        SQT Reporter  (Ateleris - Rust)  [x86-64 Linux host]                 ║
╠══════════════════════════════════════════════════════════════════════════════╣
║                                                                              ║
║  ┌──────────────────┐  ┌──────────────────────┐  ┌────────────────────────┐ ║
║  │ Baseline Checker │  │  Session Control     │  │  Report Generator      │ ║
║  │                  │  │                      │  │                        │ ║
║  │ • GET /api/v1/   │  │ • Submit test jobs   │  │ • System Test Report   │ ║
║  │   docs           │  │   via POST           │  │ • QDP                  │ ║
║  │ • Partition by   │  │ • Poll until done    │  │ • JSON report + PDF    │ ║
║  │   VERIFICATION   │  │ • Evaluate metric    │  │ • Traceability matrix  │ ║
║  │   (FUNC-018)     │  │   constraints        │  │                        │ ║
║  │ • Halt if a Test │  │ • Collect evidence   │  │                        │ ║
║  │   req has no spec│  └──────────────────────┘  └────────────────────────┘ ║
║  └──────────────────┘                                                        ║
║  ┌──────────────────────────────────────────────────────────────────────┐    ║
║  │  Requirements Manager + Traceability                                 │    ║
║  │  • Parses Strictdoc requirements from fetched docs                   │    ║
║  │  • Maps requirements to declarative test specs (.test.yaml)          │    ║
║  │  • Builds traceability matrix (requirements ↔ evidence)              │    ║
║  └──────────────────────────────────────────────────────────────────────┘    ║
╚══════════════════════════════════════════════════════════════════════════════╝
                               │
                               ▼
                      QDP + System Test Report
```

### 2.2 Process Flow

The qualification session follows this sequence:

```
SQT Reporter                            SQT Runner
────────────────────────────────────────────────────────────────────
0. [Pre-flight mode only] Validate config, GET /api/v1/docs, verify
   endpoint reachability and auth. Exit without submitting any job.

1. GET /api/v1/version  ──────────────────────────────────────────►
                            ◄── { api_version, runner_version, ... }
   Verify API version compatibility; exit non-zero if mismatched.

2. GET /api/v1/docs  ─────────────────────────────────────────────►
                            ◄── ECSS baseline + static test results
   If empty → proceed with project-declared specs + emit diagnostic.

3. Parse Strictdoc requirements from fetched docs; partition by
   VERIFICATION (SRS-FUNC-018): non-Test → compliant by Runner
   declaration, no further evaluation; Test → needs a registered spec

4. Match registered specs to Test requirements by UID (SRS-MAINT-002)
   Gate: Test requirement without a spec → UNCOVERED → non-zero exit
   before any job is submitted (see §13.2 item 9)

5. Submit ALL jobs concurrently (up to max_concurrent_jobs in flight):
   POST /api/v1/jobs  ─────────────────────────────────────────────►
                            ◄── 202 Accepted { job_id, queue_position }
   (Repeat for each spec; Runner queues them, executes one at a time)

6. Poll ALL outstanding job IDs concurrently with exponential backoff:
   GET /api/v1/jobs/{id}  ─────────────────────────────────────────►
                            ◄── status: "accepted" (queue_position: N)
                            ◄── status: "running"
                            ◄── status: "complete" | "failed"
                                 + metrics, outputs, hardware descriptor

7. Evaluate metric constraints against results (unit-aware)

8. Persist evidence, then render report + traceability matrix + QDP
   (persisting precedes rendering: ADD LLR-025)
```

The ordering is normative in the ADD ("Static, dynamic and behavioural
architecture"): configuration is validated before any contract operation, the
contract version is negotiated before any job is submitted, requirement discovery
precedes submission, and evidence is persisted before reports are rendered.

### 2.3 Component Responsibilities

#### SQT Runner (Ateleris - new, generic implementation)

| Responsibility | Detail |
|---|---|
| REST API server | Implements `job-api.openapi.yaml`; accepts jobs, returns metrics |
| Document store | Serves pre-placed ML-L Provider artifacts via `GET /api/v1/docs` |
| AI system assembly | Links inference engine + model + main program into runnable task |
| Workload test execution | representative workloads per workload type: load, execute, valid result (SRS-FUNC-022) |
| Performance test execution | Timing, memory, throughput benchmarks |
| Target abstraction | Pluggable execution backend: Docker on x86\_64/Linux (shipped); GR740 and GR765/NOEL-V through emulation (SQT-SW-2, §8.3) |
| Configuration | All artifact paths and target settings configurable via a config file; minimal changes needed to deploy |

#### SQT Reporter (Ateleris - formerly "SQT Generic")

| Responsibility | Detail |
|---|---|
| Configuration management | Parse and validate YAML config |
| Requirement discovery | Fetch the Runner-declared requirements and evidence; partition by verification method (SRS-FUNC-018) |
| Requirements management | Parse Strictdoc from fetched docs; match Test requirements to registered specs |
| Job orchestration | Submit test jobs; poll for results; evaluate metric constraints |
| Evidence collection | Persist raw responses and outcomes per run |
| Report generation | JSON report, PDF summary, traceability matrix, QDP |

#### ML-L Provider (e.g. Klepsydra - out of scope / stubs during development)

The ML-L Provider is responsible for producing the artifacts that are pre-placed on the SQT Runner's filesystem. No active API interaction occurs between the Reporter and the ML-L Provider; the Runner is the single point of contact for the Reporter.

| Artifact | Description |
|---|---|
| ECSS Documentation Baseline | Requirements specification in Strictdoc format |
| Unit Test Results (static) | Pre-existing unit test evidence |
| Validation Test Results (static) | Pre-existing validation evidence |
| Inference Engine | The ML-L binary/library (e.g. Klepsydra AI inference engine) |
| Trained Model | The AI model to qualify (e.g. ONNX file) |
| Main Program | The host application that drives the inference engine |
| Input / Output Data | Reference datasets for test execution (configurable; Ateleris-supplied during development) |

#### Separation of Concerns

```
SQT Reporter                    │  SQT Runner
────────────────────────────────┼────────────────────────────────────────
YAML config parsing             │  REST API server
Requirements parsing            │  Document store (pre-placed artifacts)
Baseline compliance check       │  AI system assembly
Job submission + polling        │  Workload test execution
Metric constraint evaluation    │  Performance test execution
Evidence collection             │  Target abstraction (x64, GR740, GR765)
Report generation (JSON/PDF)    │  Job queue + sequential execution
QDP production                  │  Metric collection and reporting
```

### 2.4 Mapping to ADD Components

The ADD decomposes the Orchestrator (SQT Reporter) into nine components arranged
as a four-stage pipeline (Input → Execution → Assessment → Output), the reference
Runner into five, and the contract data types into one shared unit. The ADD keeps
the component-to-implementation mapping in the source repository rather than in
the ADD (ADD "Component references"); this table is that mapping for the current
workspace.

| ADD component | Stage | Realised by (crate module) | Sections here |
|---|---|---|---|
| C-ORC-01 Configuration | Input | `sqt-reporter/src/config/` (`SqtConfig`, `load_config`, `validate`) | §3 |
| C-ORC-02 Specification Model | Input | `sqt-reporter/src/specs/` (`TestSpec`, `loader.rs`); constraint types in `constraint/mod.rs` | §4.3, §4.5, §4.7 |
| C-ORC-03 Runner Contract Client | Execution | `sqt-reporter/src/api/client.rs` (`ApiClient`, retry, response validation, `Redacted` token) | §5 |
| C-ORC-04 Session Control | Execution | `sqt-reporter/src/orchestrator/mod.rs` (version check, discovery, semaphore, polling, drain); `baseline/` (requirement partition, coverage gate); `preflight.rs` | §2.2, §4.6, §7 |
| C-ORC-05 Measurement Collection | Assessment | No module of its own yet: metrics are looked up by name in `sqt-types::JobMetrics` inside `orchestrator/mod.rs` (open namespace in SQT-SW-1, §10.2) | §4.5.3, §10 |
| C-ORC-06 Constraint Evaluation | Assessment | `sqt-reporter/src/constraint/` (`evaluator.rs`), `accuracy/` (accuracy and precision) | §4.5, §5.6 |
| C-ORC-07 Evidence Store | Output | `sqt-reporter/src/evidence/` (`EvidenceStore`) | §9 |
| C-ORC-08 Reporting & Traceability | Output | `sqt-reporter/src/report.rs`, `traceability/` | §4.4, §9 |
| C-ORC-09 Operator Interface | Input/Output | `sqt-reporter/src/main.rs` (CLI, signal handling, exit status); terminal summary in `report::print_terminal` | §7.3 |
| C-RNR-01 Contract Server | Runner | `sqt-runner/src/main.rs` (router), `handlers.rs`, `auth.rs` | §5 |
| C-RNR-02 Job Store & Queue | Runner | `sqt-runner/src/store.rs`; one-job execution semaphore in `main.rs` | §5.9 |
| C-RNR-03 Execution Backend Abstraction | Runner | `sqt-runner/src/executor/` (`ExecutionBackend`, `LifecycleBackend`, `docker.rs`); `Backend` in `config.rs` | §8.1–§8.3 |
| C-RNR-04 Measurement Tool Pipeline | Runner | `sqt-runner/src/tools/`, `docker.rs` | §8.5, §10 |
| C-RNR-05 Document & Evidence Store | Runner | `get_docs` in `sqt-runner/src/handlers.rs` (scans `docs.directory`) | §5.10 |
| C-SHR-01 Contract Data Types | Shared | `sqt-types/src/lib.rs`; drift test `sqt-types/tests/openapi_contract.rs` | §5, §11.3 |

Where the code departs from the ADD's allocation: the bearer token is read in
`main.rs` and wrapped in `Redacted` by `ApiClient`, not inside C-ORC-01 (ADD
LLR-003 places credential resolution there); and `sqt-reporter/src/metrics/` and
the `inventory`-based `SpecFactory` in `specs/` are unwired legacy code that maps
to no ADD component (§10.1). The ADD's reference-Runner components are
non-normative: a third-party Runner only has to implement the contract.

---

## 3. Configuration Management

### 3.1 Configuration File Format

Primary configuration format: **YAML** (human-readable, widely supported)

Two separate configuration files exist: one for the **SQT Reporter** and one for the **SQT Runner**.

#### SQT Reporter Configuration: `sqt-reporter.yaml`

The schema is deliberately minimal: the four sections below are the complete set
of keys the Reporter reads. Unknown keys are ignored (serde default), so forward
config additions do not break older binaries.

```yaml
# Project identity — stamped into report provenance (SRS-DATA-003)
project:
  name: "AI ML-L Qualification"
  version: "1.0.0"

# Test specifications — explicit list of declarative spec files.
# Registered by explicit declaration; no directory scanning (SRS-MAINT-002).
# Relative paths resolve against this config file's directory.
test_specs:
  files:
    - "./specs/REQ-ML-L-001.sqt.yaml"
    - "./specs/REQ-ML-L-003.sqt.yaml"
    - "./specs/REQ-ML-L-004.sqt.yaml"

# SQT Runner connection + operational tuning
runner:
  url: "http://runner-host:8080"     # http:// or https:// (validated at load)
  token_env: "SQT_RUNNER_TOKEN"      # optional: env var holding the bearer token;
                                     # value is redacted from all output (SRS-OPS-005).
                                     # If set, the var must be present and non-empty
                                     # at startup or the Reporter exits non-zero (SRS-IF-006).

  # Operational tuning (defaults shown):
  max_concurrent_jobs: 8             # in-flight submission bound (SRS-PERF-002)
  job_timeout_secs: 300              # project-default per-job timeout; overridable per spec (SRS-PERF-004)
  retry_max: 3                       # transient-error retry ceiling (SRS-FUNC-020)
  retry_backoff_initial_secs: 1      # initial retry backoff (SRS-FUNC-020)
  poll_interval_initial_secs: 1      # first polling interval (SRS-PERF-003)
  poll_interval_max_secs: 30         # polling backoff cap (SRS-PERF-003)

# Report generation
report:
  output_dir: "./reports"            # relative paths resolve against this config file
```

Every run gets its own directory, `report.output_dir/runs/<run_id>/`, holding the
machine-readable JSON report, a PDF summary, the traceability matrix (`.json` +
`.txt`), the run's evidence under `evidence/`, and the QDP bundle (`qdp.zip`). Report *format selection* (choosing
which of these to emit, alternate PDF engines, HTML) is not a config surface today
— all outputs are always produced. See §9.

> **Diagnostic verbosity** (SRS-OPS-002) is currently driven by the `RUST_LOG`
> environment variable (`tracing` env-filter), not a config key or CLI flag.

#### SQT Runner Configuration: `sqt-runner.yaml`

The runner config is organised around a top-level **`backend:`** discriminant.
Backend-specific settings live in a section named after the variant. Today the
only backend is `docker`, whose settings (per-tool images and an optional build
step) live under `docker:`. Adding a new backend (e.g. on-target hardware) is a
new `Backend` enum variant plus its own config section — see §8.

```yaml
backend: docker                       # execution backend (default: docker)

# Bearer-token authentication (SRS-IF-006). Optional: when present, every route
# except GET /api/v1/health requires `Authorization: Bearer <token>` (401
# UNAUTHORIZED otherwise); startup aborts if the variable is unset or empty.
# When absent, auth is disabled and the runner logs a warning.
auth:
  token_env: SQT_RUNNER_TOKEN         # env var holding the expected token

# Binary under test (pre-built, or compiled per job when [docker.build] is present)
binary:
  path: ./artifacts/ml_l_inference    # relative paths resolved to absolute at load
  args: []                            # optional: fixed args always appended
  env:                                # optional: env vars set for every run
    LD_LIBRARY_PATH: /opt/libs
  input_mode: stdin                   # stdin | arg | env:VAR_NAME  (default: stdin)

# Per-job scratch directory for tool output (valgrind XML, coverage data, etc.)
workspace: /var/sqt/workspace         # default: /tmp/sqt-dev-server

# Docker-backend-specific settings
docker:
  # Per-tool Docker image configuration
  tools:
    run:
      image: sqt-run:latest           # required for run jobs; image used to execute the binary
    valgrind:
      image: sqt-valgrind:latest      # optional; enables memory.leaks_bytes
    coverage:
      image: sqt-coverage:latest      # optional; enables coverage.line_pct
    sonarqube:
      url: "http://sonar.local:9000"  # optional; enables static.critical_issues / major_issues
      token_env: SONAR_TOKEN          # env var holding the auth token (default: SONAR_TOKEN)
    static_analysis:
      image: sqt-static:latest        # optional; enables static.warnings

  # Build step (optional; omit when using a pre-built binary).
  # When present the runner compiles [build.source] against [build.engine_lib]
  # before each job, choosing compiler flags from the job's instrumentation.
  build:
    image: gcc:14-bookworm            # Docker image providing the compiler
    source: ./src/main.cpp            # main program source file
    engine_lib: ./artifacts/libengine.so  # ML-L engine shared library (linked + copied to workspace)
    compiler: g++                     # default: g++
    extra_flags: ["-std=c++17"]
    include_dirs: ["./include"]
    compile_timeout_secs: 300

# Target hardware descriptor (returned verbatim in JobStatusResponse.hardware).
# All sub-fields optional; omit the section to report type: unknown.
hardware:
  type: x86_64                        # serde wire name is `type`
  model: Intel Core i7-12700
  cores: 20
  memory_total_kb: 32768000
  clock_mhz: null
  os: Ubuntu 24.04 LTS

# Runner identity + toolchain reported via GET /api/v1/version (SRS-DATA-003).
runner_hardware:
  target_id: x86_64-linux
  toolchain_versions:
    inference_engine: "2.1.0"
    gcc: "13.2"

# Qualification documents served via GET /api/v1/docs (optional).
# When absent the endpoint returns 404. The runner scans this directory
# (non-recursively) at request time:
#   *.sdoc -> documents[]   (raw UTF-8 text, parsed client-side by the Reporter)
#   *.*    -> attachments[] (base64 blobs)
docs:
  directory: ./docs
```

The binary and artifacts are configured at Runner startup, not per job (AD-018).
To qualify a different version, restart the Runner with an updated config file.

### 3.2 Configuration Validation

Both components validate on startup, before doing any real work.

**Reporter** (`SqtConfig::validate`, `sqt-reporter/src/config/schema.rs`): runs
before the API client is constructed or any network call is made (SRS-FUNC-002).
It rejects an empty `project.name`/`project.version`, a `runner.url` whose scheme
is not `http`/`https` (case-insensitive), and an empty `test_specs.files`; when
`runner.token_env` is set, the named environment variable must be present and
non-empty (SRS-IF-006). Relative `test_specs.files` and `report.output_dir` paths
are resolved against the config file's directory. Each error names the offending
field so the operator can fix it without reading logs. Validation currently stops
at the first invalid field; the ADD (C-ORC-01, LLR-002) asks for every invalid
field to be reported in one diagnostic. Deeper checks (endpoint
reachability, auth, docs) are performed by pre-flight mode (§6.2, SRS-OPS-007),
not by config validation.

**Runner** (`ServerConfig::load` + `run_preflight_checks`, `sqt-runner/src/`):
`load` parses the YAML and resolves `binary.path`, `workspace`, `docs.directory`,
and any `docker.build.*` paths to absolute (Docker bind mounts require absolute
paths). Startup readiness is then delegated to the execution backend's own
`preflight()` (for the Docker backend: daemon up, configured images present, the
binary executable inside the run image) plus a backend-independent check that the
configured `docs.directory`, if any, exists and is readable. A failing check
aborts startup before the server binds.

---

## 4. Requirements Management

### 4.1 Strictdoc Format Support

**Strictdoc** (https://github.com/strictdoc-project/strictdoc) is a requirements management tool that uses a text-based format.

#### Example Strictdoc File

```strictdoc
[DOCUMENT]
TITLE: ML-L Software Requirements Specification
VERSION: 1.0.0

[SECTION]
TITLE: Functional Requirements

[REQUIREMENT]
UID: REQ-ML-L-001
STATUS: Active
TITLE: Multi-core inference support
STATEMENT: >>>
The ML-L shall support inference execution on multi-core processors
with configurable parallelization strategies.
<<<
RATIONALE: >>>
Space processors like GR740 (quad-core Leon4) require efficient
utilization of all available cores to meet performance requirements.
<<<
VERIFICATION: Test
RELATIONS:
- TYPE: Parent
  VALUE: SYS-REQ-001

[REQUIREMENT]
UID: REQ-ML-L-002
STATUS: Active
TITLE: Model format support
STATEMENT: The ML-L shall load and execute models in the model format of its declared API profile.
RATIONALE: The API and the model format are selected in the ML Space Profile, not by the SQT.
VERIFICATION: Test, Inspection
RELATIONS:
- TYPE: Parent
  VALUE: SYS-REQ-002

[REQUIREMENT]
UID: REQ-ML-L-003
STATUS: Active
TITLE: Inference latency requirement
STATEMENT: >>>
The ML-L shall execute inference on a ResNet-50 model with input size 224x224
in less than 100ms on GR740 @ 250MHz.
<<<
VERIFICATION: Test
COMMENT: >>>
This requirement is verified through performance benchmarking on target hardware.
<<<

[REQUIREMENT]
UID: REQ-ML-L-004
STATUS: Active
TITLE: Memory footprint
STATEMENT: The ML-L runtime shall consume no more than 2MB of RAM during inference execution.
VERIFICATION: Test, Analysis
RELATIONS:
- TYPE: Parent
  VALUE: SYS-REQ-005

[SECTION]
TITLE: Non-Functional Requirements

[REQUIREMENT]
UID: REQ-ML-L-NFR-001
STATUS: Active
TITLE: ECSS compliance
STATEMENT: The ML-L shall be developed and qualified according to ECSS-E-ST-40C and ECSS-Q-ST-80C.
VERIFICATION: Inspection, Review

[REQUIREMENT]
UID: REQ-ML-L-NFR-002
STATUS: Active
TITLE: Code coverage
STATEMENT: Unit test coverage shall achieve minimum 80% line coverage and 75% function coverage.
VERIFICATION: Test
```

### 4.2 Requirements Parsing Strategy

The Reporter parses StrictDoc with its own small Rust parser
(`sqt-reporter/src/baseline/mod.rs`); no Python StrictDoc installation is needed
at run time (§13.2, item 5). It is line-based and reads only what the partition
of SRS-FUNC-018 needs:

```rust
// sqt-reporter/src/baseline/mod.rs
pub struct Requirement {
    pub uid: String,
    pub title: String,
    pub status: String,
    pub verification_methods: Vec<String>,   // from the comma-separated VERIFICATION field
}

pub struct RequirementSet {
    pub testable: Vec<Requirement>,                 // VERIFICATION contains "Test"
    pub compliant_by_declaration: Vec<Requirement>, // other methods only
    pub unclassified: Vec<Requirement>,             // no VERIFICATION field
}
```

`[REQUIREMENT]` and `[COMPOSITE_REQUIREMENT]` blocks are recognised; any other
`[...]` block ends the current requirement. Only single-line `UID`, `TITLE`,
`STATUS` and `VERIFICATION` fields are read: multi-line fields (`STATEMENT: >>>`),
relations, and grammars that name the verification field differently are not
interpreted. The raw `.sdoc` text and the attachments are not yet persisted as
evidence (ADD C-ORC-07 requires them verbatim; F-M11 in `REVIEW_FINDINGS.md`).

### 4.3 Requirements-to-Tests Mapping

Qualification specifications are registered by **explicit declaration** in
`sqt-reporter.yaml` (SRS-MAINT-002). Auto-discovery from filesystem state
(directory scanning, naming conventions) is not performed.

Each `.test.yaml` file is listed explicitly in the config:

```yaml
# sqt-reporter.yaml
test_specs:
  files:
    - "./specs/REQ-ML-L-001.test.yaml"
    - "./specs/REQ-ML-L-003.test.yaml"
```

Each spec file names the requirement it covers:

```yaml
# specs/REQ-ML-L-001.test.yaml
requirement_uid: REQ-ML-L-001
use_case: image_classification      # workload type (SRS-FUNC-019)
constraints:
  - metric: return_code
    operator: eq
    value: 0
  - metric: time.avg_ms
    operator: lte
    value: 100ms                    # unit-aware: ms accepted, compared to ms from Runner
```

Testable requirements not covered by any listed spec are reported as UNCOVERED
and cause a non-zero exit.

### 4.4 Traceability Matrix Generation

Generate comprehensive traceability matrix:

```
┌──────────────┬─────────────────────────────┬─────────────┬────────────┐
│ Requirement  │ Test Cases                  │ Status      │ Coverage   │
├──────────────┼─────────────────────────────┼─────────────┼────────────┤
│ REQ-ML-L-001 │ • Job job-12340             │ ✓ Passed    │ N/A        │
│              │   return_code=0, t=74ms     │             │            │
├──────────────┼─────────────────────────────┼─────────────┼────────────┤
│ REQ-ML-L-002 │ • Job job-12341             │ ✓ Passed    │ N/A        │
│              │   return_code=0             │             │            │
├──────────────┼─────────────────────────────┼─────────────┼────────────┤
│ REQ-ML-L-003 │ • Job job-12342             │ ✓ Passed    │ N/A        │
│              │   time.avg_ms=87 ≤ 100ms   │             │            │
├──────────────┼─────────────────────────────┼─────────────┼────────────┤
│ REQ-ML-L-004 │ • Job job-12343             │ ✓ Passed    │ N/A        │
│              │   memory.peak_rss_kb=1843   │             │            │
│              │   ≤ 2 MB                   │             │            │
└──────────────┴─────────────────────────────┴─────────────┴────────────┘
```

### 4.5 Declarative Test Specifications

Rather than mapping requirements to hand-written test code for every case, SQT Reporter supports a **declarative test specification** approach: testable traits (metrics/properties) are defined as built-in or plugin-provided capabilities, and requirements directly specify which constraints must hold - no boilerplate test code required.

#### 4.5.1 Concept

Instead of:
```
Requirement → Manual test code → Execute → Results
```

The declarative approach is:
```
Requirement + Constraint spec → Auto-executed metric collection → Results
```

This is similar to property-based testing and contract testing. Specs are sidecar YAML files today; embedding them in `.sdoc` files through Strictdoc's custom grammar feature (see Notes section) is an idea, not implemented. Regardless of location, each spec file must be explicitly listed in `sqt-reporter.yaml` — no auto-discovery from filesystem state is performed (SRS-MAINT-002, ADD LLR-005).

#### 4.5.2 Spec YAML Format

Test spec files are listed explicitly in the Reporter config (see §3.1):

```yaml
# specs/REQ-ML-L-003.test.yaml
requirement_uid: REQ-ML-L-003
use_case: image_classification        # workload type (SRS-FUNC-019)
input: datasets/reference_mnist.bin
timeout_secs: 120                     # optional; overrides project-default (SRS-PERF-004)

constraints:
  - metric: time.avg_ms
    operator: lte
    value: 100ms                      # unit-aware: "100ms" or bare 100 (unit inferred from metric)

  - metric: memory.peak_rss_kb
    operator: lte
    value: 2 MB                       # reconciled to KB automatically (SRS-FUNC-004)

  - metric: memory.leaks_bytes
    operator: eq
    value: 0

# Optional: numerical output precision comparison (SRS-FUNC-016)
# Compares raw inference outputs against reference values within tolerance.
# Distinct from accuracy metric computation (SRS-FUNC-017).
precision_reference:
  file: datasets/reference_output.bin
  tolerance: 1e-4
```

The `value` field accepts `<number><unit>` strings. The evaluator reconciles
compatible units (ms↔s, KB↔MB↔bytes, etc.) before comparison (SRS-FUNC-004).
Irreconcilable units (e.g. ms vs bytes) cause the constraint to fail with an
explanatory message; the rest of the spec and the run are evaluated as usual.
A spec with zero constraints at evaluation time is recorded as FAIL — vacuous
passes are rejected (SRS-REL-001). The ADD enforces this at both ends (C-ORC-02,
LLR-006): a spec file with an empty `constraints` list is also rejected when it
is loaded.

#### 4.5.3 Core Metric Catalogue

These names correspond directly to keys in the `JobStatusResponse.metrics` object returned by the Runner (see `job-api.openapi.yaml`). The Runner collects and fills them; the Reporter evaluates constraints by looking up each key in the returned map.

**Memory Metrics**

| Metric key | Unit | Description |
|------------|------|-------------|
| `memory.peak_rss_kb` | KB | Peak resident set size |
| `memory.avg_rss_kb` | KB | Average resident set size across repetitions |
| `memory.leaks_bytes` | bytes | Leaked bytes (Valgrind / ASan); 0 = no leaks |
| `memory.stack_kb` | KB | Peak stack usage; critical on RTEMS/baremetal fixed-stack targets |
| `memory.heap_allocations` | count | Total heap allocation calls; 0 required for post-init safety-critical code |

**Timing Metrics**

| Metric key | Unit | Description |
|------------|------|-------------|
| `time.avg_ms` | ms | Average wall-clock execution time per repetition |
| `time.latency_p50_ms` | ms | 50th percentile latency |
| `time.latency_p95_ms` | ms | 95th percentile latency |
| `time.latency_p99_ms` | ms | 99th percentile latency |
| `time.throughput_fps` | fps | Throughput mode only; null in latency mode |
| `time.startup_ms` | ms | Model load + runtime init time |
| `time.first_inference_ms` | ms | Time to first inference result |

**Correctness Metrics**

| Metric key | Unit | Description |
|------------|------|-------------|
| `return_code` | int | Process exit code (0 = success) |

**Code Quality Metrics**

| Metric key | Unit | Description |
|------------|------|-------------|
| `static.critical_issues` | count | CRITICAL-severity SonarQube issues |
| `static.major_issues` | count | MAJOR-severity SonarQube issues |
| `static.warnings` | count | Total static analyser warnings |
| `coverage.line_pct` | % | Line coverage percentage |
| `coverage.branch_pct` | % | Branch coverage percentage |
| `coverage.function_pct` | % | Function coverage percentage |
| `coverage.lines_covered` | count | Absolute covered source lines |
| `coverage.lines_total` | count | Total instrumented source lines |

**CPU / Binary Metrics**

| Metric key | Unit | Description |
|------------|------|-------------|
| `cpu.avg_normalized_pct` | % | CPU utilisation normalised to all available compute |
| `binary.size_kb` | KB | On-disk binary size |
| `harness.docker_overhead_ms` | ms | Docker-backend diagnostic: container wall-clock minus the binary's own elapsed time |

**Power / Thermal Metrics**

| Metric key | Unit | Description |
|------------|------|-------------|
| `power.avg_watts` | W | Average power draw during inference |
| `power.peak_watts` | W | Peak power draw across all repetitions |
| `hardware.temperature_celsius` | °C | Average die/board temperature during the run |

The reference Runner's built-in collectors must cover at least the SRS-FUNC-005 set: execution outcome (`return_code`), execution time (`time.avg_ms`), peak memory (`memory.peak_rss_kb`), memory-leak absence (`memory.leaks_bytes`), line coverage (`coverage.line_pct`), and severity-graded static-analysis findings (`static.critical_issues`, `static.major_issues`). The other keys are populated where a backend can measure them and are otherwise `null`.

#### 4.5.4 Execution Flow

```rust
// Pseudo-code: API-driven orchestration

// Submit one job per testable requirement
let job_id = api_client.submit_job(&JobRequest {
    use_case: spec.use_case,           // workload type (SRS-FUNC-019)
    input: spec.input.clone(),
    repetitions: spec.repetitions,
    instrumentation: spec.instrumentation,
    mode: spec.mode,
    timeout_secs: spec.timeout_secs.unwrap_or(config.runner.job_timeout_secs),
    tags: [("requirement_uid", &spec.requirement_uid)].into(),
    ..Default::default()
}).await?;

// Poll until complete (exponential backoff)
let job = api_client.wait_for_completion(&job_id).await?;

// Evaluate constraints against returned metrics (flat map)
for constraint in &spec.constraints {
    let actual = job.metrics.get(&constraint.metric);  // e.g. "time.avg_ms"
    let outcome = evaluate(actual, constraint);
    results.push(outcome);
}
```

The orchestrator never runs tools directly — all measurement happens server-side
in the Runner (the Docker tool pipeline, §8.5). The orchestrator submits jobs and
reads the flat `JobMetrics` response. See §10 for how metrics are produced and how
the Runner is extended.

### 4.6 Compliant Requirements (Non-Testable)

Not all requirements can be verified by automated testing. Requirements verified
by **inspection**, **review**, **analysis**, or **demonstration** cannot be
expressed as SQT metric constraints - yet they must appear in the qualification
report as verified.

#### 4.6.1 The Problem

An `.sdoc` file may contain:

```strictdoc
[REQUIREMENT]
UID: REQ-ML-L-NFR-001
TITLE: ECSS compliance
STATEMENT: The ML-L shall be developed according to ECSS-E-ST-40C.
VERIFICATION: Inspection, Review
```

There is no automated test that can check "was this standard followed?". The
verification evidence consists of documents, design reviews, and audits -
artefacts pre-placed on the Runner by the ML-L Provider. SQT Reporter has no way to
evaluate these directly.

#### 4.6.2 Solution: Server-Provided Requirements via `GET /api/v1/docs`

The server exposes its StrictDoc requirements documents through
`GET /api/v1/docs`. SQT Reporter fetches these once at session start and applies
the following rule:

| Requirement `VERIFICATION` field | SQT treatment |
|---|---|
| Includes `Test` | Must be covered by a `.test.yaml` spec; evaluated normally |
| Does **not** include `Test` | Marked **compliant** on the server's authority |
| Requirement absent from server docs | Not applicable (not owned by server) |

Requirements in the second row - inspection-only, review-only, or
analysis-only - are recorded in the report with:

- `status: compliant`
- `evidence_source: server_declaration` (as opposed to `sqt_test`)
- The requirement's UID, title, and statement from the parsed `.sdoc`
- The list of declared verification methods

#### 4.6.3 Qualification Report Impact

The report's traceability matrix separates requirements by verification path:

```
Requirements Coverage
─────────────────────────────────────────────────────
Tested (via SQT specs):       REQ-ML-L-001 ✅  REQ-ML-L-003 ✅
Compliant by declaration:     REQ-ML-L-NFR-001 ✅  REQ-ML-L-NFR-002 ✅
Not covered:                  (none)
```

This gives auditors a complete picture without conflating two fundamentally
different verification modes.

---

### 4.7 API-Profile Qualification

*Target design for SQT-SW-1 (SRS-FUNC-021 to SRS-FUNC-023; ADD components C-ORC-02,
C-ORC-04, C-ORC-07, C-ORC-08 and LLR-035). Not implemented in SQT-SW-0.*

The SQT qualifies the ML-L against a **declared API profile**: the API elements
inside the qualification boundary. The SQT does not select the API or its elements
and is not tied to a particular API or model format; the selection is made and
justified in the ML Space Profile and the API and ML-L analysis (ML-TN-T1/T2,
APCO, WP100).

- **Declared API profile** (SRS-FUNC-021): a version-controllable artefact with a
  stable identifier, registered in the Reporter configuration like a test
  specification. The Reporter treats its elements as opaque names and records the
  profile with its identifier in the evidence of every run.
- **Workload set** (SRS-FUNC-022): the SQT is delivered with a generic,
  representative workload set covering the workload types of the ML Space Profile
  (§5.2), as test specifications plus the data they reference, so it runs against
  the reference Runner and any conformant Runner alike. A project narrows or
  extends the set through its configuration; mission use cases (WP420) are added
  the same way. Each workload is one job, and the Reporter records whether the
  ML-L loaded it, executed it and returned a valid result.
- **Declared elements**: each workload specification lists the profile elements it
  exercises.
- **Coverage report** (SRS-FUNC-023): the Reporter relates the workload outcomes to
  the profile and reports every element as *exercised* — with the outcomes of the
  workloads exercising it — or *not exercised*. A coverage gap is reported, not
  failed: the exit code stays governed by the specification outcomes (SRS-OPS-001,
  AD-028).

---

## 5. REST API Interface (SQT Runner)

### 5.1 API Contract (OpenAPI Specification)

The full interface contract is defined in **`job-api.openapi.yaml`** at the root of
the repository. The spec is implemented by the **SQT Runner** (the `sqt-runner`
crate is the reference implementation); the **SQT Reporter** is the API client.

The contract is the ADD's applicable document AD-4 and its interface EI-1: it is
normative for the Reporter and for every Runner, and `sqt-types` (ADD C-SHR-01)
holds its data types once for both sides. `sqt-types/tests/openapi_contract.rs`
fails the build when the OpenAPI field names and the DTOs drift apart.

> **Known contract drift**: `input` is typed as a discriminated union in the
> contract but sent as a bare path string by the code; which of the two becomes
> the contract is a pending decision (rest of F-H2 in `REVIEW_FINDINGS.md`). The
> workload field is `use_case` on both sides.

#### Base URL
```
http://localhost:8080
```

#### Authentication
```http
Authorization: Bearer <token>
Content-Type: application/json
```

Bearer-token authentication on every contract operation (SRS-IF-006; decided,
§13.2 item 6). The Reporter reads the token from the environment variable named
by `runner.token_env` and never from the config file itself (ADD EI-5, LLR-003);
the Runner enforces it when `auth.token_env` is configured (§3.1). Only
`GET /api/v1/health` is unauthenticated.

#### Design Principles

The Runner is configured at startup (via `sqt-runner.yaml` or CLI flags) with the
hardware target, inference engine, trained model, and main program. These are
**not** transmitted per job. The client (SQT Reporter) specifies only the workload type
(§5.2) and measurement parameters; the Runner returns everything it can measure.

A single job type covers all qualification activities: execution runs, instrumented
runs (valgrind/asan/gprof), coverage collection, and static analysis. In the
reference Runner, coverage and static analysis run inside every job for which
their tool is configured (§8.5). The ADD lists *verification-activity invocation*
as a separate EI-1 operation class (SRS-FUNC-014, SRS-FUNC-015, SQT-SW-1): the
Reporter triggers project-declared activities and persists their outputs
verbatim. The contract has no such operation yet; how it is expressed is open
(§13.2 item 8).

**Session start protocol (SRS-IF-008):** Before submitting any job, the Reporter
calls `GET /api/v1/version` to verify the Runner's API version is within the set
declared by the Reporter binary. An incompatible version causes an immediate
non-zero exit with a clear mismatch message.

**Response schema validation (SRS-IF-007):** Every Runner response is validated
against the published contract schema before consumption. A non-conformant response
causes the affected job to be recorded as infrastructure-failed.

**Transient error retry (SRS-FUNC-020):** Network resets, connection timeouts, and
recoverable 5xx responses trigger bounded retries with exponential backoff (up to
`runner.retry_max` attempts). After exhausting retries, the affected job is
recorded as infrastructure-failed. This is distinct from the steady-state polling
backoff in §5.9.

**Credential redaction (SRS-OPS-005):** Bearer tokens are redacted from all
operational output (log lines, terminal summary, persisted evidence, verbose
request/response traces) at every verbosity level without exception. The token is
carried in a `Redacted` newtype whose `Debug`/`Display` print `[REDACTED]`
(`sqt-reporter/src/api/client.rs`), the ADD's "credential carrier" (LLR-003).

**Single network egress (SRS-RES-002, ADD LLR-007):** `ApiClient` is the only
Reporter component that opens connections, and only to `runner.url`; it must not
follow URLs from responses. Current state: the `reqwest` client is built with its
default redirect policy, so an HTTP redirect from the Runner would be followed —
to be disabled.

### 5.2 Workload Types

The workload types are taken from the **ML Space Profile** (RD-5 / ML-TN-T1) and
enumerated once, in `sqt-types::WorkloadType`, so Reporter and Runner reason over
the same closed set; the Runner maps each workload type to the appropriate binary
invocation, input handling, and output interpretation (SRS-FUNC-019). On the wire
the field is named `use_case`, as published in `job-api.openapi.yaml`; the SRS, the
ADD and this document call it the *workload type* (AD-025).

| Workload type | Domain | Typical Input | Typical Output |
|---|---|---|---|
| `object_detection` | Computer vision | Images | Bounding boxes + class labels |
| `image_classification` | Computer vision | Images | Class label + confidence score |
| `pixel_segmentation` | Computer vision | Images | Per-pixel class mask |
| `classification` | Tabular ML | Feature vectors | Class label |
| `regression` | Tabular ML | Feature vectors | Scalar prediction |
| `time_series_forecasting` | Sequential ML | Time series sequences | Forecast sequence |

**Default session**: when a spec does not declare a workload type, the SQT Reporter
submits one job per supported workload type (SRS-FUNC-019). Jobs are queued on the Runner
side and polled concurrently by the Reporter. Not implemented yet: the spec loader
requires `use_case` (F-M7 in `REVIEW_FINDINGS.md`).

### 5.3 Measurement Modes

Each job runs in one of two modes, set by the `mode` field. The workload type is set by the `use_case` field (§5.2):

| Mode | Behaviour | Populated metrics |
|---|---|---|
| `latency` (default) | Each inference measured independently | `time.avg_ms`, `time.latency_p50/p95/p99_ms` |
| `throughput` | Backend maximises pipeline utilisation | `time.throughput_fps` |

### 5.4 Instrumentation Options

| Value | Tool | Effect |
|---|---|---|
| `none` (default) | - | No overhead; use for benchmarks and accuracy runs |
| `asan` | AddressSanitizer | Memory error detection (buffer overflows, UAF) |
| `valgrind` | Valgrind memcheck | Leak detection; populates `memory.leaks_bytes` |
| `gprof` | GNU profiler | Call-graph profiling; does not affect memory metrics |

Only one instrumentation option may be active per job. Timing metrics reflect
instrumented execution when active and should not be compared to bare baselines.

### 5.5 API Endpoints

Three endpoints for qualification work, plus a version/health endpoint called once
at session start.

#### 5.5.0 Version / Health Check

**Endpoint**: `GET /api/v1/version`

Called before any job submission to verify API contract compatibility (SRS-IF-008).

**Response** (200 OK):
```json
{
  "api_version": "1.0",
  "runner_version": "1.2.3",
  "target_id": "x86_64-linux",
  "os": "Ubuntu 22.04",
  "toolchain_versions": {
    "gcc": "12.3.0",
    "valgrind": "3.21.0",
    "lcov": "1.16"
  }
}
```

The Reporter declares its supported `api_version` set at compile time. A mismatch
causes immediate non-zero exit before any job is submitted. The Runner-supplied
fields (`runner_version`, `target_id`, `os`, `toolchain_versions`) are stamped
into every report for provenance (SRS-DATA-003).

#### 5.5.1 Submit Job

**Endpoint**: `POST /api/v1/jobs`

**Request Body**:
```json
{
  "use_case": "image_classification",
  "input": { "type": "images", "images": ["<base64-image>", "..."] },
  "repetitions": 10,
  "warmup_repetitions": 3,
  "instrumentation": "none",
  "mode": "latency",
  "timeout_secs": 60,
  "tags": { "requirement_uid": "REQ-ML-L-003", "git_sha": "a1b2c3d" }
}
```

The Runner uses the model, inference engine, and main program configured at startup.
The Reporter does not transmit the model — it only specifies the workload type
and measurement parameters.

For large image batches (hundreds or thousands of images), the request body will be
proportionally large. The server's HTTP frontend must be configured to accept it
(e.g. `client_max_body_size 2g` in nginx). Alternatively, reference a server-side
pre-loaded dataset via `input.type = "dataset_ref"` with a server-assigned identifier.
The ADD ("Resources allocation") says job submission carries the workload type and
measurement parameters, and that Reporter memory must not scale with payload size
(SRS-RES-003); inline batches sit uneasily with that, which is one input to the
pending `input` decision (§5.1). The code today sends a server-side path string.

**Response** (202 Accepted):
```json
{ "job_id": "job-abc123", "status": "accepted", "created_at": "2026-03-06T10:00:00Z" }
```

#### 5.5.2 Get Job Status

**Endpoint**: `GET /api/v1/jobs/{job_id}`

**Response** (terminal - job complete):
```json
{
  "job_id": "job-abc123",
  "use_case": "image_classification",
  "mode": "latency",
  "repetitions": 10,
  "status": "complete",
  "created_at": "2026-03-06T10:00:00Z",
  "started_at": "2026-03-06T10:00:01Z",
  "completed_at": "2026-03-06T10:00:05Z",
  "hardware": {
    "type": "cpu",
    "model": "GR740 Leon4",
    "cores": 4,
    "memory_total_kb": 131072,
    "accelerator_model": null,
    "clock_mhz": 250.0,
    "os": "RTEMS 6"
  },
  "metrics": {
    "return_code":               0,
    "time.startup_ms":           312.5,
    "time.first_inference_ms":   145.2,
    "time.avg_ms":               87.3,
    "time.latency_p50_ms":       85.1,
    "time.latency_p95_ms":       99.2,
    "time.latency_p99_ms":       103.4,
    "time.throughput_fps":       null,
    "memory.avg_rss_kb":         3200.0,
    "memory.peak_rss_kb":        4096.0,
    "memory.stack_kb":           48.0,
    "memory.heap_allocations":   0,
    "memory.leaks_bytes":        null,
    "cpu.avg_normalized_pct":    24.3,
    "power.avg_watts":           4.2,
    "power.peak_watts":          5.8,
    "hardware.temperature_celsius": 52.3,
    "coverage.line_pct":         87.2,
    "coverage.branch_pct":       72.1,
    "coverage.function_pct":     95.0,
    "coverage.lines_covered":    1234,
    "coverage.lines_total":      1416,
    "static.warnings":           1,
    "static.critical_issues":    0,
    "static.major_issues":       3,
    "binary.size_kb":            412.0
  },
  "output": {
    "type": "classification_labels",
    "result_consistent": true,
    "outputs": [
      { "label": "spacecraft", "confidence": 0.983 },
      { "label": "solar_panel", "confidence": 0.971 }
    ],
    "outputs_checksum": "a3f1c2d4e5b6..."
  }
}
```

**Status values**: `accepted` | `running` | `complete` | `failed`

**Terminal states**: `complete` and `failed`. SQT stops polling when the job reaches a
terminal state. A `complete` job may have `null` metric fields - those indicate
unavailable tools, not infrastructure failure. A `failed` status means the job could not
run at all; the `error` field provides a description.

#### 5.5.3 Health Check

**Endpoint**: `GET /api/v1/health`

A lightweight liveness and readiness probe intended for load balancers, Docker
health checks, Kubernetes probes, and CI scripts that need to confirm the Runner
process is up and accepting requests before submitting jobs.

Unlike `GET /api/v1/version` (which is part of the session-start protocol and
performs API-version compatibility verification), `/health` is a pure operational
probe: it does not carry version semantics and must never be used to gate API
compatibility. Its contract is deliberately minimal so that monitoring
infrastructure can call it without understanding the SQT API.

**What "healthy" means** (all conditions must hold):

| Condition | Check |
|---|---|
| Process is alive and the HTTP server is accepting connections | The endpoint is reachable at all |
| Job queue is operational | In-memory `JobStore` is readable without error |
| Execution semaphore is available | Semaphore has not been permanently closed |
| Workspace directory exists | The configured `workspace` path is present on the filesystem |

If the docs directory is configured, the health check additionally verifies that
it exists and is readable. A missing docs directory is reported as `degraded`
rather than `unhealthy` because it does not prevent job execution.

**Response** (200 OK — healthy):
```json
{
  "status": "healthy",
  "runner_version": "1.2.3",
  "target_id": "x86_64-linux",
  "checks": {
    "job_store": "ok",
    "workspace": "ok",
    "docs_dir": "ok"
  }
}
```

**Response** (200 OK — degraded, docs directory missing or unreadable):
```json
{
  "status": "degraded",
  "runner_version": "1.2.3",
  "target_id": "x86_64-linux",
  "checks": {
    "job_store": "ok",
    "workspace": "ok",
    "docs_dir": "missing"
  }
}
```

**Response** (503 Service Unavailable — unhealthy, workspace missing):
```json
{
  "status": "unhealthy",
  "runner_version": "1.2.3",
  "target_id": "x86_64-linux",
  "checks": {
    "job_store": "ok",
    "workspace": "missing",
    "docs_dir": "ok"
  }
}
```

**Status semantics:**

| `status` | HTTP code | Meaning |
|---|---|---|
| `healthy` | 200 | All checks passed; ready to accept jobs |
| `degraded` | 200 | Core functionality operational; non-critical check failed (e.g. docs directory missing) |
| `unhealthy` | 503 | Critical check failed; do not submit jobs |

**SQT Reporter behaviour:** The Reporter does **not** call `/health` during a
qualification session. The session-start protocol uses `GET /api/v1/version`
(SRS-IF-008). The `/health` endpoint is for infrastructure tooling only.

**Authentication:** The `/health` endpoint does **not** require a Bearer token. It
must be reachable by monitoring probes that have no knowledge of qualification
credentials. Operators who wish to restrict access should do so at the network
layer (e.g. allow-list the monitoring subnet in a firewall or reverse proxy).

### 5.6 Output Handling and Accuracy Metrics

The server returns model outputs directly in the job response. `output.outputs`
always contains **one entry per input item** - never one per repetition.
Repetitions exist for statistical measurement only.

For all workload types (including `pixel_segmentation`), the server executes all
`repetitions` and transfers the output batch once. `result_consistent` records
whether every repetition produced bit-identical results:

```
output.result_consistent = true   → all N repetitions agreed
output.result_consistent = false  → non-determinism detected (possible hardware fault)
```

SQT Reporter computes accuracy metrics (accuracy, F1, IoU, L2, etc.) client-side by
comparing `output.outputs` against reference labels attached to the spec. An
`outputs_checksum` (SHA-256 of the outputs array) enables cross-session
reproducibility verification without re-transferring the data.

**Reproducibility evidence (SRS-REL-004):** The Reporter records `result_consistent` in
the per-run evidence for every job regardless of whether a reproducibility constraint is
declared. A spec may include a reproducibility constraint:

```yaml
- metric: output.result_consistent
  operator: eq
  value: true
```

A `null` or absent `result_consistent` field is treated as a failing constraint. Non-identical
outputs are recorded neutrally — without root-cause attribution — as non-determinism evidence.

### 5.7 CPU Utilisation Normalisation

**Decision**: normalise server-side (see AD-013).

`cpu.avg_normalized_pct` is computed by the server as:

```
cpu.avg_normalized_pct = (average CPU used across all cores) / (core_count * 100) * 100
```

- 100 % means all available compute was fully saturated throughout the run.
- This formula is unambiguous regardless of hardware type (CPU, GPU, FPGA, NPU).
- The `hardware` descriptor in the response provides `cores` and `type` for users
  who need to reconstruct the per-core percentage.

### 5.8 Error Handling

**Standard Error Response**:
```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "unknown workload type: 'depth_estimation'",
    "details": "Supported types: object_detection, image_classification, ...",
    "timestamp": "2026-03-06T10:00:00Z"
  }
}
```

**Error Codes**:
- `VALIDATION_ERROR`: Invalid request parameters (e.g., unknown workload type)
- `NOT_FOUND`: Job not found
- `TIMEOUT`: Operation exceeded timeout
- `UNAUTHORIZED`: Missing or invalid Bearer token
- `INTERNAL_ERROR`: Internal server error

**SQT Reporter behaviour on error responses (SRS-REL-003):** The Reporter surfaces the
Runner-supplied `error.code`, `error.message`, and `error.details` in both the terminal
summary and the persisted run evidence. The three operationally distinct error classes —
endpoint unreachable (connection error), authentication failure (`UNAUTHORIZED`), and
request validation failure (`VALIDATION_ERROR`) — shall be distinguishable to the operator
in the terminal output without requiring inspection of raw logs.

### 5.9 Polling Strategy

SQT Reporter implements exponential backoff for all polling (SRS-PERF-003).
Results are embedded in the GET response — no separate fetch is needed.

```
initial delay: 1 s
backoff factor: x2 per poll
maximum delay: 30 s (configurable: runner.poll_interval_max_secs)
```

**Concurrent polling (SRS-PERF-001):** The Reporter submits all jobs first
(receiving job IDs via 202 Accepted), then polls all outstanding IDs
concurrently — one async task per job. When the total number of in-flight jobs
exceeds `max_concurrent_jobs` (SRS-PERF-002), further submissions queue until
in-flight slots free up. The per-job polling tasks run independently; a job
completing early does not delay evaluation of subsequent specs.

**Runner-side execution:** The Runner currently executes one job at a time
(concurrency limit = 1 on the Runner). This is a valid initial implementation:
the SRS allows any concurrency bound ≥ 1 on the Runner side. Wall-clock at the
Reporter is therefore the sum of all job execution durations in the current
configuration; this will improve as Runner concurrency increases.

### 5.10 Requirements Documents Endpoint

**Endpoint**: `GET /api/v1/docs`

Returns the StrictDoc requirements documents managed by the server. Called
once per qualification session before any jobs are submitted.

**Response** (200 OK):
```json
{
  "documents": [
    {
      "filename": "ml-l-requirements.sdoc",
      "title": "ML-L Software Requirements Specification",
      "content": "[DOCUMENT]\nTITLE: ML-L Software Requirements Specification\n..."
    }
  ]
}
```

- `documents` may be an empty array if the server has no requirements documents configured.
- `content` is the full raw StrictDoc text, UTF-8 encoded with LF line endings.
- `title` is provided as a convenience; SQT Reporter parses `content` authoritatively.

**SQT Reporter behaviour**:

1. Fetch `GET /api/v1/docs`.
2. Parse each `content` string as StrictDoc; extract all `[REQUIREMENT]` blocks.
3. For each requirement, inspect the `VERIFICATION` field:
   - Contains `Test` → schedule for normal constraint evaluation (requires a listed spec in config).
   - No `Test` → record as `compliant` with `evidence_source: server_declaration`.
4. Include all requirements in the report traceability matrix, grouped by verification path.

See **Section 4.6** for the full rationale and report output format.

---

## 6. Scope Definition

### 6.1 SQT Runner Scope (Ateleris)

#### ✅ IN SCOPE

1. **REST API Server**
   - Implement `job-api.openapi.yaml` endpoints (`GET /api/v1/health`, `GET /api/v1/version`, `POST /api/v1/jobs`, `GET /api/v1/jobs/{id}`, `GET /api/v1/docs`)
   - Job queue and sequential job execution (one job at a time)
   - Configuration via YAML (artifact paths, target selection, etc.)

2. **Document Store**
   - Serve pre-placed ML-L Provider artifacts via `GET /api/v1/docs`
   - Configurable paths so users need not modify source code

3. **AI System Assembly**
   - Assemble inference engine + trained model + main program into a runnable task
   - Exact mechanism TBD (depends on ML-L Provider delivery format; see Section 8)

4. **Workload Test Execution**
   - Execute each representative workload against the assembled AI system: load, execute, return a valid result (SRS-FUNC-022)
   - Collect and return results as job metrics

5. **Performance Test Execution**
   - Execute timing, memory, and throughput benchmarks
   - Collect and return metrics in the standard job response schema

6. **Target Abstraction Layer** (execution backend, ADD C-RNR-03)
   - x86\_64 / Linux via the Docker backend **[shipped]**
   - GR740 (LEON4) and GR765/NOEL-V through functional emulation (QEMU, Renode, SIS) behind the Runner **[planned, SQT-SW-2]** — per SQT-TN-AP; no backend for it exists yet
   - On-target runs on Gaisler-hosted boards (SSH, GRMON) **[opportunistic]** — automation TBC in SQT-TN-AP
   - Architecture designed to add new targets without changing the Reporter or API contract

#### ❌ OUT OF SCOPE for SQT Runner

1. **Report Generation** - the Reporter produces all reports and the QDP
2. **Requirements Management** - requirements are parsed and evaluated by the Reporter
3. **ML model conversion / quantization** - the ML-L Provider supplies the model as-is
4. **Cross-compilation of ML-L binaries** - the ML-L Provider supplies pre-built binaries
5. **Procured target hardware** - none: TSIM is not procured and the SB740 board is not provided (SQT-TN-AP); emulated timing and memory figures are observations, not normative for the SQT

### 6.2 SQT Reporter Scope (Ateleris)

The SQT Reporter is what the SRS calls the **Orchestrator**.

#### ✅ IN SCOPE

1. **Configuration Management**
   - Parse YAML configuration files; validate before any job submission (SRS-FUNC-002)
   - Explicitly registered test specs — no auto-discovery from filesystem (SRS-MAINT-002)
   - Exit non-zero with the offending field identified on invalid config

2. **Session Start Protocol**
   - API version compatibility check via `GET /api/v1/version` (SRS-IF-008)
   - Bearer-token validation: configured-but-empty token → non-zero exit before any job (SRS-IF-006)
   - Credential redaction from all output at all verbosity levels (SRS-OPS-005)

3. **Pre-flight Validation Mode (SRS-OPS-007)**
   - `--preflight` flag: validate config, fetch docs, verify reachability and auth
   - Exits without submitting any job; reports outcome in same channels as a full run
   - The ADD (LLR-017) includes contract-version negotiation; today's pre-flight reads `GET /api/v1/version` but does not check compatibility

4. **Requirement Discovery** (ADD C-ORC-04, LLR-018)
   - Fetch the Runner-declared requirements and compliance evidence via `GET /api/v1/docs`
   - If empty document set → proceed with project-declared specs + emit diagnostic (SRS-FUNC-018)
   - Preserve the returned evidence verbatim (not yet: F-M11)
   - No separate "ECSS compliance" judgement of the documents: non-Test requirements are accepted on the Runner's declaration; the only gate is the UNCOVERED check of item 5

5. **Requirements Management**
   - Parse Strictdoc requirements from fetched documents
   - Classify by verification method: Test-verifiable vs compliant-by-Runner-declaration (SRS-FUNC-018)
   - Test requirement without a registered spec → UNCOVERED → non-zero exit before any job (§13.2 item 9)
   - Build requirements database and traceability matrix

6. **REST API Client (to SQT Runner)**
   - Concurrent job submission: submit all jobs, receive job IDs (202 Accepted) (SRS-PERF-001)
   - Bounded concurrency: `max_concurrent_jobs` semaphore (SRS-PERF-002)
   - Concurrent polling of all outstanding IDs with exponential backoff (SRS-PERF-003)
   - Per-job timeout enforcement; project-default applied when spec omits one (SRS-PERF-004)
   - Response schema validation before consuming (SRS-IF-007)
   - Transient error retry with bounded back-off (SRS-FUNC-020)

7. **Metric Constraint Evaluation**
   - Unit-aware evaluation: reconcile compatible units (ms↔s, KB↔bytes) (SRS-FUNC-004)
   - Map null/absent metrics to failed constraints (SRS-REL-002)
   - Reject vacuous passes: zero-constraint specs recorded as FAIL (SRS-REL-001)
   - Numerical output precision comparison against reference values (SRS-FUNC-016)
   - Client-side accuracy metric computation per workload type (SRS-FUNC-017):
     - `image_classification` / `classification`: accuracy, F1, precision, recall
     - `object_detection`: mAP, IoU@50, IoU@75
     - `regression`: MSE, MAE, R²
     - `time_series_forecasting`: MSE, MAE, MAPE

8. **Partial-Failure Semantics (SRS-OPS-006)**
   - Infrastructure-failed jobs do not abort the run
   - Remaining specs continue to be evaluated
   - `INFRA_ERROR` outcome is distinct from `PASS` / `FAIL` in reports and exit code

9. **Graceful Shutdown (SRS-OPS-008)**
   - On SIGTERM: stop submitting new jobs, await in-flight polls, persist partial run result
   - Exit code 2 (interrupted) — distinct from 0 (all-pass) and 1 (any-fail/infra-error)

10. **Evidence Collection**
    - Aggregate all job results and static documents
    - Build requirements traceability matrix (machine-readable + human-readable)
    - Organize qualification artifacts per run under a timestamped, uniquely identified location (SRS-OPS-004, ADD LLR-024; not yet: F-M4)
    - Persist raw Runner responses and outcomes before rendering any report (ADD LLR-025)

11. **Report Generation** (ADD C-ORC-08)
    - JSON machine-readable report and a PDF summary, all rendered from one report data model (ADD LLR-026); HTML deferred (§9.1)
    - Every output carries `schema_version` field; within major version: additive changes only (SRS-DATA-001)
    - Every output stamped with: SQT binary version, Runner-supplied identifiers (version, hardware, OS, toolchain), loaded project configuration — tokens redacted (SRS-DATA-003)
    - Traceability matrix in machine-readable and human-readable representations (SRS-FUNC-010)
    - Produce QDP (Quality Data Package)

12. **Per-step Progress Signals (SRS-FUNC-012)**
    - At each qualification workflow step emit a signal sufficient for the operator to
      determine: the current step, the outcome of the previous step, and any required
      intervention — without consulting an external runbook
    - Signals emitted at: session start (version check), document fetch, baseline gate,
      spec loading, job submission (per job), polling progress (per completion), constraint
      evaluation, report generation

13. **CI/CD Integration**
    - Single CLI entry point; self-discoverable options without external documentation (SRS-HF-001)
    - Exit 0 = all pass; exit 1 = any fail or infra-error; exit 2 = interrupted (SRS-OPS-001)
    - End-of-run terminal summary: per-spec pass/fail/blocked counts, evidence location (SRS-OPS-003)
    - Operator-configurable diagnostic verbosity; full request/response trace at max verbosity (SRS-OPS-002)
    - No implicit network access beyond the configured Runner endpoint (SRS-RES-002)

14. **API-Profile Qualification (SQT-SW-1; §4.7)**
    - Accept the declared API profile and record it with its identifier in the evidence of every run (SRS-FUNC-021)
    - Run the workload set — the generic set delivered with the SQT, narrowed or extended by the project — and record per workload whether the ML-L loaded it, executed it and returned a valid result (SRS-FUNC-022)
    - Report each profile element as exercised (with the outcomes of the workloads exercising it) or not exercised; coverage does not change the exit code (SRS-FUNC-023)

15. **Verification-Activity Triggering (SQT-SW-1)**
    - Trigger the project-declared verification activities (e.g. static analysis, structural coverage, coding-standard conformance) through the contract and persist their outputs verbatim with the test results (SRS-FUNC-014, SRS-FUNC-015); no fixed tool inventory is assumed. Contract operation still to be defined (§13.2 item 8)

#### ❌ OUT OF SCOPE for SQT Reporter

1. **Test execution** — the Runner handles all test execution
2. **Hardware access** — the Runner abstracts all target hardware; no target-, OS-, engine- or model-format-specific behaviour in the Reporter (SRS-PORT-002, ADD LLR-032)
3. **ML-L artifact management** — the Reporter consumes artifacts already served by the Runner
4. **REST API server** — the Reporter is a client only
5. **Ateleris/Klepsydra runtime dependency** — the Reporter binary shall be operable
   against any conformant Runner endpoint without credentials, network access, or
   configuration issued by Ateleris or Klepsydra; no call-home, telemetry, or
   licence-check to either organisation's infrastructure (SRS-HF-002)

### 6.3 ML-L Provider Responsibilities (Out of Scope / Stubs)

The ML-L Provider (e.g. Klepsydra) is responsible for producing the qualification artifacts. From the SQT perspective, this side is out of scope and represented by stubs during development.

#### ✅ ML-L Provider DOES

1. **Prepare ECSS Documentation Baseline** - Strictdoc requirements specification
2. **Provide Static Test Evidence** - unit test results, validation test results
3. **Supply Inference Engine** - compiled binary/library for the target
4. **Supply Trained Model** - e.g. ONNX file
5. **Supply Main Program** - host application driving the inference engine
6. **Pre-place artifacts on the Runner filesystem** - before a qualification session begins

The ADD's context diagram names a second external role, the **Use Case
Provider**, who defines the use-case requirements; mission use cases extend the
workload set through configuration (§4.7). Each SQT release is handed to the ML-L
role, and each new ML-L version is re-run against the current SQT release — a
configuration change and a run, not a code change (SQT-TN-AP release plan).

### 6.4 Interface Contract

```
SQT Reporter                    SQT Runner
────────────────────────────────────────────────────────────────────
"Fetch the document baseline"   → ECSS docs + static test results

"Are documents compliant?"      Gate in Reporter: proceed or halt

"Execute workload: image_classification"
                                → Assemble AI system
                                  Run workload + Performance tests
                                  Return metrics

"What's the status?"            → "accepted" (queue_position) | "running"

"Done - give me results"        → Metrics + hardware descriptor

"Here is the qualification      (Runner does not generate reports)
 report and QDP"
```

The Reporter is always the initiator: the Runner never calls back, so every
state change is discovered by polling (SRS-IF-003). The ADD fixes exactly five
external interfaces for the Orchestrator; there are no others (SRS-RES-002):

| ADD interface | Realisation | ADD owner |
|---|---|---|
| EI-1 Runner API contract | `job-api.openapi.yaml` over REST: version enquiry (`GET /api/v1/version`), requirement discovery (`GET /api/v1/docs`), job submission (`POST /api/v1/jobs`), status polling (`GET /api/v1/jobs/{id}`); verification-activity invocation not yet in the contract (§13.2 item 8). `GET /api/v1/health` is a Runner operations probe, not used by the Reporter | C-ORC-03 |
| EI-2 Configuration and specification input | `sqt-reporter.yaml` and the spec files it lists (§3.1, §4.3) | C-ORC-01, C-ORC-02 |
| EI-3 Evidence and report output | `report.output_dir` (§9) | C-ORC-07, C-ORC-08 |
| EI-4 Operator interface | CLI `run` / `preflight`, `RUST_LOG` verbosity, terminal summary, exit status (§7.3) | C-ORC-09 |
| EI-5 Credential source | Environment variable named by `runner.token_env` | C-ORC-01 |

---

## 7. Qualification Workflow

### 7.1 Complete Workflow

```
┌──────────────────────────────────────────────────────────────────┐
│ [Pre-flight mode: --preflight flag]  [SQT Reporter]              │
│                                                                  │
│    Validate config, GET /api/v1/version, GET /api/v1/docs,       │
│    verify endpoint reachability and bearer-token auth.           │
│    Exit here — no jobs submitted. (SRS-OPS-007)                  │
└──────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────┐
│ 1. Session Start  [SQT Reporter → SQT Runner]                    │
│                                                                  │
│    GET /api/v1/version                                           │
│    ← { api_version, runner_version, target_id, os, toolchain }  │
│    Verify API version compatible; exit non-zero if not.          │
│    (SRS-IF-008)                                                  │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 2. Fetch Document Baseline  [SQT Reporter → SQT Runner]          │
│                                                                  │
│    GET /api/v1/docs                                              │
│    ← ECSS documentation baseline (.sdoc)                        │
│    ← Unit test results (static, .sdoc)                          │
│    ← Validation test results (static, .sdoc)                    │
│    If empty → proceed with project-declared specs + WARN         │
│    (SRS-FUNC-018)                                                │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 3. Parse Requirements  [SQT Reporter]                            │
│                                                                  │
│    No compliance judgement of the documents themselves: the      │
│    Runner's declaration is authoritative for non-Test            │
│    requirements (SRS-FUNC-018, ADD LLR-018).                     │
│                                                                  │
│    Parse Strictdoc from fetched documents.                       │
│    For each [REQUIREMENT]:                                       │
│      • VERIFICATION includes Test  → must have listed .test.yaml │
│      • VERIFICATION is non-Test    → mark compliant by server    │
│        (Inspection / Review / Analysis)        declaration       │
│                                                                  │
│    REQ-ML-L-001  VERIFICATION: Test        → needs spec          │
│    REQ-ML-L-NFR-001  VERIFICATION: Review  → compliant           │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 4. Load Test Specs  [SQT Reporter]                               │
│                                                                  │
│    Load specs listed explicitly in sqt-reporter.yaml.            │
│    Match each spec to a requirement by uid. (SRS-MAINT-002)      │
│    Unmatched testable requirements → UNCOVERED → halt            │
│                                                                  │
│    # specs/REQ-ML-L-001.test.yaml                                │
│    requirement_uid: REQ-ML-L-001                                 │
│    use_case: image_classification                                │
│    constraints:                                                  │
│      - metric: return_code    operator: eq   value: 0            │
│      - metric: time.avg_ms    operator: lte  value: 100ms        │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 5. Submit ALL Jobs Concurrently  [SQT Reporter → SQT Runner]     │
│                                                                  │
│    Submit all specs concurrently, bounded by max_concurrent_jobs │
│    (SRS-PERF-001, SRS-PERF-002). Receive job IDs immediately.   │
│                                                                  │
│    POST /api/v1/jobs  { use_case: "image_classification",        │
│                         repetitions: 10, mode: "latency" }      │
│    ← { job_id: "job-abc123", status: "accepted",                │
│        queue_position: 1 }                                       │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 6. Execute & Poll Concurrently  [Runner + Reporter polling]      │
│                                                                  │
│  Runner (one job at a time in current implementation):           │
│    • Assemble AI system (inference engine + model + main prog)   │
│    • Execute workload tests (one job per workload)               │
│    • Execute Performance tests (timing, memory, throughput)      │
│    • Collect all metrics; update job status → complete           │
│                                                                  │
│  Reporter polls ALL outstanding IDs concurrently:                │
│    GET /api/v1/jobs/{id}  (exponential backoff per job)          │
│    ← status: "accepted" (queue_position: 1)                      │
│    ← status: "running"                                           │
│    ← status: "complete"  { metrics: { ... }, output: { ... } }  │
│    ← status: "failed"    → record as INFRA_ERROR; continue       │
│    (SRS-OPS-006: infrastructure failures do not abort the run)   │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 7. Evaluate Metric Constraints  [SQT Reporter]                   │
│                                                                  │
│    Unit-aware: reconcile compatible units before comparison.     │
│    null metric → FAILED with missing-measurement message.        │
│    Zero-constraint spec → FAILED (vacuous pass rejected).        │
│    (SRS-FUNC-004, SRS-REL-001, SRS-REL-002)                     │
│                                                                  │
│    REQ-ML-L-001:                                                 │
│      return_code = 0   == 0    → ✓ PASS                         │
│      time.avg_ms = 87.3 ≤ 100ms → ✓ PASS                       │
│    REQ-ML-L-002:                                                 │
│      return_code = 1   == 0    → ✗ FAIL                         │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 8. Build Traceability Matrix  [SQT Reporter]                     │
│                                                                  │
│    REQ-ML-L-001  PASS  return_code=0 ✓  time=87ms ✓             │
│    REQ-ML-L-002  FAIL  return_code=1 ✗                          │
│    REQ-ML-L-NFR-001  COMPLIANT (Review - server declaration)     │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 9. Generate System Test Report + QDP  [SQT Reporter]             │
│                                                                  │
│    • Stamp SQT version + Runner provenance in every output       │
│    • Include schema_version in machine-readable output           │
│    • Render JSON report and PDF summary (printpdf)               │
│    • Persist evidence first; then produce QDP (qdp.zip)          │
│    (SRS-DATA-001, SRS-DATA-003)                                  │
│                                                                  │
│    Output: report.json, report.pdf, traceability.*, qdp.zip      │
└──────────────────────────────────────────────────────────────────┘
```

### 7.2 Example: End-to-End Trace

**Requirement** (from fetched ECSS baseline):
```strictdoc
[REQUIREMENT]
UID: REQ-ML-L-003
STATEMENT: Inference latency shall be < 100ms for the configured model on the target.
VERIFICATION: Test
```

**Test Spec** (`specs/REQ-ML-L-003.test.yaml`):
```yaml
requirement_uid: REQ-ML-L-003
use_case: image_classification         # workload type (SRS-FUNC-019)
constraints:
  - metric: time.avg_ms
    operator: lte
    value: 100ms                       # unit-aware
  - metric: return_code
    operator: eq
    value: 0
```

**Execution**:
1. Reporter submits `POST /api/v1/jobs` (with all other jobs concurrently) — Runner queues it, returns `job-12345`
2. Runner executes the model on the configured target; measures latency
3. Reporter polls; receives `complete` with `time.avg_ms: 87.3`, `return_code: 0`
4. Reporter evaluates: `87.3 <= 100` ✓, `0 == 0` ✓ → **PASS**

**Traceability Matrix Entry**:
```
REQ-ML-L-003 | job-12345 | ✓ PASS | time.avg_ms=87.3ms ≤ 100ms
```

**Report Section**:
```
Requirement REQ-ML-L-003: VERIFIED
  Statement: Inference latency shall be < 100ms
  Evidence: job-12345 (x86_64-linux, model: model.onnx)
  Result: PASS (time.avg_ms = 87.3ms, threshold ≤ 100ms)
```

### 7.3 Run Lifecycle and Exit Status

The ADD ("Static, dynamic and behavioural architecture") models a run as a state
machine — Configuring, Handshaking, Discovering, Submitting, Polling, Draining,
Evaluating, Reporting — with one entry and seven terminal states. The exit status
is derived at a single point, in `main.rs` (C-ORC-09, LLR-029):

| ADD terminal state | Reached when | Exit code today |
|---|---|---|
| ConfigRejected | A configuration field is invalid (SRS-FUNC-002) | 1 |
| VersionRejected | Runner API major version unsupported (SRS-IF-008) | 1 |
| AuthRejected | `runner.token_env` set but the variable is absent or empty (SRS-IF-006) | 1 |
| PreflightComplete | Pre-flight finished, no job submitted (SRS-OPS-007) | 0, or 1 if a check failed |
| RunPassed | Every specification passed (SRS-OPS-001) | 0 |
| RunFailed | Any constraint failed or any job infrastructure-failed (SRS-OPS-001, SRS-OPS-006) | 1 |
| RunInterrupted | Run drained on SIGTERM / Ctrl-C (SRS-OPS-008) | 2 |

ConfigRejected, VersionRejected and AuthRejected end before any job is submitted
and before Reporting, so no report is written. The current code adds one path
the ADD does not list: an UNCOVERED Test requirement ends the run with exit 1
before submission (§13.2 item 9). Every path that reaches Reporting persists
evidence and prints the terminal summary, including the drained one (SRS-OPS-003).

---

## 8. SQT Runner Target Architecture

The SQT Runner is built around a pluggable **execution backend**. All
target-specific logic (how a job is provisioned, run, and measured) sits behind a
trait, so a new target — different hardware, a different ML-L framework — can be
added without touching the HTTP layer, the job store, or the SQT Reporter. This is
the concrete realisation of the Target Abstraction Layer (AD-023; ADD C-RNR-03,
LLR-DER-002, LLR-DER-003). Like every reference-Runner component in the ADD, it is
a record of one conformant implementation, not a design a third-party Runner has
to follow.

### 8.1 Execution Backend Abstraction

The HTTP/queue/store layer only ever holds an `Arc<dyn ExecutionBackend>` — an
object-safe trait with three methods (`sqt-runner/src/executor/mod.rs`):

```rust
pub trait ExecutionBackend: Send + Sync {
    /// One-time readiness check, run at startup before the server binds.
    fn preflight(&self) -> anyhow::Result<()>;

    /// Run one job in the caller-owned per-job workspace; return its metrics.
    fn execute(&self, request: &JobRequest, workspace: &Path) -> anyhow::Result<JobMetrics>;

    /// Target hardware, identity, and toolchain versions —
    /// feeds JobStatusResponse.hardware and GET /api/v1/version.
    fn describe(&self) -> BackendDescriptor;
}
```

New backends do **not** implement `ExecutionBackend` directly. They implement the
ergonomic, lifecycle-oriented `LifecycleBackend` trait, and a blanket
`impl<T: LifecycleBackend> ExecutionBackend for T` supplies the `execute`
orchestration for free (prepare → run → collect, with teardown always run
afterwards, even on failure):

```rust
pub trait LifecycleBackend: Send + Sync {
    type Prepared;   // backend-private handle (compiled binary, container spec, board session)
    type RawRun;     // backend-private raw output, mapped to metrics by `collect`

    fn preflight(&self) -> anyhow::Result<()>;
    fn describe(&self) -> BackendDescriptor;

    fn prepare(&self, ctx: &JobContext) -> anyhow::Result<Self::Prepared>;               // provision (compile/flash/stage)
    fn run(&self, ctx: &JobContext, prepared: &Self::Prepared) -> anyhow::Result<Self::RawRun>;  // run under measurement
    fn collect(&self, ctx: &JobContext, raw: Self::RawRun) -> anyhow::Result<JobMetrics>;         // map raw → JobMetrics
    fn teardown(&self, ctx: &JobContext, prepared: Self::Prepared) -> anyhow::Result<()> { Ok(()) }
}
```

`JobContext` bundles the `JobRequest`, the per-job workspace path, and the
resolved timeout, so backends never re-derive timeout policy. `Prepared`/`RawRun`
stay private to the backend; only `JobContext` in and `JobMetrics` /
`BackendDescriptor` out cross the boundary.

**Backend selection.** The active backend is chosen by the top-level `backend:`
config key (§3.1). `main.rs` has a small `build_executor()` factory that maps each
`Backend` enum variant to its constructor. Adding a backend is therefore three
localised edits — a new `Backend` variant, its config section, and a match arm —
with no change to handlers, the store, `AppState`, or the trait boundary.

```yaml
# sqt-runner.yaml (see §3.1 for the full schema)
backend: docker        # the shipped backend; future: an on-target hardware backend
```

The shipped backend is `DockerExecutor` (`sqt-runner/src/executor/docker.rs`),
which compiles the workload when a `docker.build` section is present and executes
it inside per-tool Docker containers.

### 8.2 AI System Assembly

Before test execution the Runner assembles the AI system. The exact mechanism depends on the ML-L Provider's delivery format, which is not yet fully defined. The assembly step may involve:

- Copying artifacts into a working directory
- Setting environment variables (paths to inference engine, model file, etc.)
- Linking or composing components into a single runnable process
- Compiling a thin wrapper that binds the engine and model (if required by the delivery format)

The backend's `prepare()` lifecycle phase (§8.1) encapsulates this step. For the
Docker backend with a `docker.build` section, "assembly" is compiling the
customer-supplied main program against the ML-L engine shared library; the
instrumentation flags come from the job request. When a pre-built binary is
configured instead, `prepare()` simply stages it.

### 8.3 Supported and Planned Backends

| Backend | Status | Execution Method | Notes |
|--------|--------|-----------------|-------|
| `docker` (x86_64/Linux) | **Shipped** (SQT-SW-0) | Compile/execute inside Docker containers on the host | Development and CI baseline; the only shipped `Backend` variant |
| GR740 (LEON4, SPARC V8), emulated | **Planned, SQT-SW-2** | New backend driving functional emulation (QEMU mainline with the LEON3 SMP model, Renode, SIS) | End-to-end ML-L-QDP generation on emulation is in the SQT-SW-2 scope (SQT-TN-AP release plan) |
| GR765/NOEL-V, emulated | **Planned, SQT-SW-2** | Same emulation route | Assumes RTEMS 6 SMP POSIX support for the GR765/NOEL-V in time; the LEON5 configuration was excluded at negotiation (SQT-TN-AP) |
| GR740 / GR765 boards | **Opportunistic** | Gaisler-hosted boards over SSH and GRMON | No board is procured; automation still TBC in SQT-TN-AP |

**Design constraint for GR740/GR765**: Ateleris has no target hardware; TSIM is
not procured and the SB740 board is not provided (SQT-TN-AP). Target execution is
therefore emulated behind the Runner: a new `Backend` variant implementing
`LifecycleBackend` stages the assembled AI system for the emulator in `prepare`,
starts it in `run`, and gathers the results in `collect`. How the backend drives
the emulators is not designed yet (§13.1 item 3). The backend abstraction (§8.1)
ensures this can be added without touching the API, handlers, or store, and the
Reporter is unaffected (SRS-PORT-002).

**Limits of emulation** (SQT-TN-AP, risk R-01): emulation is representative at the
functional and interface level, not in timing. Cycle-level figures do not transfer
from an emulated target to silicon, so timing and memory figures measured under
emulation are recorded as observations and are non-normative for the SQT;
opportunistic hardware-in-the-loop is the route to authoritative on-target figures.

### 8.4 Workload Tests and Performance Tests

Both test types are executed by the Runner's Test Execution Engine for each job:

**Workload tests** (SRS-FUNC-022):
- One job per representative workload, from the generic set delivered with the SQT, narrowed or extended by the project
- Verify that the ML-L loads the workload, executes it and returns a valid result on the target
- Each workload specification declares which elements of the declared API profile it exercises, which is what API-profile coverage is computed from (§4.7)
- Results reported as standard job metrics

**Performance Tests**:
- Timing benchmarks (latency, throughput)
- Memory benchmarks (peak RSS, stack usage, heap allocations)
- Power and temperature metrics (where the target exposes them)
- Results reported as standard job metrics

Both test types use the same job submission flow; the workload type in the job request (`use_case`, §5.2) determines how the Runner invokes the binary and interprets its output.

### 8.5 Docker Containerization

The shipped `docker` backend does not run the SQT Runner *itself* in a container;
it runs each **measurement tool** in its own purpose-built container, so the
toolchain is isolated and reproducible without polluting the host. Images are
configured per tool under `docker.tools` (§3.1); the repository ships Dockerfiles
for them under `docker/` (`Dockerfile.run`, `.valgrind`, `.coverage`, `.static`,
`.sonar`) built by `docker/build-all.sh`.

For each job the backend creates a per-job workspace directory and bind-mounts it
(plus the binary and any input) into the relevant tool container:

```yaml
docker:
  tools:
    run:      { image: sqt-run:latest }        # required — executes the binary, measures time/RSS/CPU
    valgrind: { image: sqt-valgrind:latest }   # optional — memory.leaks_bytes
    coverage: { image: sqt-coverage:latest }   # optional — coverage.line_pct
    static_analysis: { image: sqt-static:latest }  # optional — static.warnings
    sonarqube:                                 # optional — static.critical_issues / major_issues
      url: http://sonar.local:9000
      token_env: SONAR_TOKEN
```

**Benefits**: reproducible tool environment, toolchain isolation per tool
(valgrind, lcov, cppcheck, …), and easy CI/CD integration on x86_64. A tool whose
image is not configured is simply skipped, and its metrics are reported as `null`
rather than failing the job (ADD C-RNR-04, LLR-DER-004); the Reporter then fails
any constraint on that metric (SRS-REL-002).

---

## 9. Report Generation

### 9.1 Report Outputs

Evidence persistence is ADD component C-ORC-07 and rendering is C-ORC-08; the ADD
keeps them apart so that adding a rendering never touches the persistence path,
and evidence is always persisted before any report is rendered (LLR-025). Every
rendering is derived from one report data model (LLR-026), every machine-readable
output declares its schema version (LLR-027), and every report and traceability
matrix is stamped from one provenance record (LLR-028).

Every `sqt run` writes into a directory reserved for that run (SRS-FUNC-008,
SRS-OPS-004, ADD LLR-024). All outputs are produced on every run — there is no
per-format on/off switch. `-o <path>` additionally copies `report.json` elsewhere.

```text
<report.output_dir>/runs/<run_id>/       run_id = UTC timestamp, e.g. 20261022T101530123Z
    report.json                          run report, schema 1.1 (SRS-FUNC-009)
    report.pdf
    traceability.json, traceability.txt  when the Runner serves a baseline
    qdp.zip                              the whole run directory
    evidence/
        manifest.json                    every artefact: id, kind, SHA-256, size, job/spec
        runner/NNNN-<operation>[-<job_id>].json   raw Runner responses, in arrival order
        outcomes/NNN-<requirement_uid>.json       constraint outcomes per spec
```

An evidence artefact's stable identifier is its path relative to the run
directory. The report names the manifest (`evidence_manifest`) and, per spec,
the ids of its evidence (`evidence_ids`): the Runner's final response for the
job and the persisted outcomes. Raw responses are stored byte for byte as
received; request headers, and so the bearer token, are never recorded. When a
run stops early, the responses received so far and the manifest are still
written.

#### JSON Report (Machine-Readable) — the primary artefact
- **Target audience**: CI/CD, automation, downstream tooling
- **Contents**: one entry per spec with each constraint outcome and measured value,
  plus run metadata: `schema_version` (SRS-DATA-001), provenance (SQT version,
  Runner version/target/OS/toolchain, sanitised config — SRS-DATA-003), and the
  `interrupted` flag (SRS-OPS-008)
- **Requirement**: SRS-FUNC-009 mandates a single machine-readable report be
  produced for every run

#### PDF Report (ECSS-oriented summary)
- **Target audience**: ESA, customers, certification
- **Engine**: rendered with `printpdf` (decided, AD-006); any other engine would be
  a substitution behind `write_pdf` and the same report data model
- The ADD does not require a PDF; it is an additional rendering of the same model
- **Sections**: executive summary, requirements traceability, test-results
  summary, per-requirement detail

#### Traceability Matrix (dual-audience)
- Emitted as both `traceability.json` and `traceability.txt` from one data model
  (SRS-FUNC-010, SRS-DATA-002); produced when the Runner serves a baseline

#### Terminal Summary
- Printed at the end of every run: per-spec PASS / FAIL / INFRA_ERROR with counts
  and evidence location (SRS-OPS-003)

#### QDP (Quality Data Package)
- `qdp.zip` bundling the whole run directory (report, PDF, traceability and
  evidence) — the ECSS qualification output artefact

#### API-profile coverage (SQT-SW-1)
- Per declared profile element: exercised, with the outcomes of the workloads
  exercising it, or not exercised (SRS-FUNC-023, ADD LLR-035, §4.7). Not
  implemented yet.

> **HTML report**: not implemented. It is a possible future developer-facing
> output; it is not an SRS requirement and no code path emits it today. Earlier
> drafts of this document listed it as a first-class format (AD-010) — treat that
> as deferred, not current.

### 9.2 Report Structure Principles

The report is **requirement-centric**: the outer loop is requirements, not runs.

A single qualification session may test the binary with multiple inputs
(e.g. `reference.bin`, `edge_case.bin`, `stress.bin`). Each input produces one
execution run. Each requirement's `TestSpec` references the input relevant to it.
The orchestrator groups specs by (workload type, input), submits one job per
group, and fans results back to each spec. The Runner uses the model and inference engine
configured at startup for all jobs in the session.

The report table shows one row per requirement, citing its evidence source:

```
REQ-ML-L-001  PASS  return_code=0 ✓  time=87ms ✓    [ml_l_inference + reference.bin]
REQ-ML-L-002  FAIL  return_code=1 ✗                  [ml_l_inference + edge_case.bin]
REQ-ML-L-003  PASS  peak_rss=3.9MB ✓                 [ml_l_inference + reference.bin]
REQ-STATIC-001 PASS sonar.critical=0 ✓               [ml_l_inference, static analysis]
```

The run is evidence *for* the requirement - not the unit of reporting. Multiple
requirements can share the same run without re-executing the binary.

### 9.3 Report Content Structure

Illustrative target content; the current `printpdf` summary renders a subset of
it (§9.1, §9.4).

```
Validation Report - Klepsydra AI ML-L v1.0.0
schema_version: 1.0
═══════════════════════════════════════════════════════════════

Provenance (SRS-DATA-003):
   • Generated by: SQT v1.0.0 (commit: a1b2c3d)
   • Runner version: 1.2.3 | Target: x86_64-linux | OS: Ubuntu 22.04
   • Toolchain: gcc 12.3.0, valgrind 3.21.0, lcov 1.16
   • Configuration: sqt-reporter.yaml (token: [REDACTED])

1. Executive Summary
   • Project: Klepsydra AI ML-L Qualification
   • ECSS Level: D
   • Target TRL: 6
   • Status: ✓ PASSED
   • Coverage: 87.5% (threshold: 80%)
   • Requirements: 45/45 verified

2. Test Results Overview
   ┌──────────────────────┬────────┬────────┬─────────┐
   │ Test Category        │ Total  │ Passed │ Failed  │
   ├──────────────────────┼────────┼────────┼─────────┤
   │ Unit Tests           │ 150    │ 148    │ 2       │
   │ Integration Tests    │ 45     │ 45     │ 0       │
   │ Hardware Tests       │ 12     │ 12     │ 0       │
   │ Performance Tests    │ 8      │ 8      │ 0       │
   └──────────────────────┴────────┴────────┴─────────┘

3. Requirements Traceability Matrix
   [See Section 3 for full matrix]

4. Static Analysis Results
   • SonarQube Quality Gate: PASSED
   • Bugs: 0
   • Vulnerabilities: 0
   • Code Smells: 12 (acceptable)
   • Technical Debt: 2h 30m

5. Coverage Analysis
   • Line Coverage: 87.5%
   • Function Coverage: 91.2%
   • Branch Coverage: 78.3%

6. Performance Metrics (GR740 @ 250MHz)
   • Inference Latency (ResNet50): 87.3ms (threshold: <100ms) ✓
   • Throughput: 11.5 FPS
   • Memory Usage: 1.8 MB (threshold: <2MB) ✓

7. Artifacts
   • Binary: ml-l-gr740-v1.0.0.elf (2048 KB)
   • Test Reports: test-report.html
   • Coverage Reports: coverage/index.html
   • Source Archive: ml-l-src-v1.0.0.tar.gz

8. Conclusion
   The ML-L v1.0.0 has successfully passed all qualification
   criteria for ECSS Level D with TRL 6.

   Recommendation: APPROVED for space mission integration.

═══════════════════════════════════════════════════════════════
Generated by: SQT v1.0.0
Date: 2026-02-13 14:30:00 UTC
═══════════════════════════════════════════════════════════════
```

### 9.4 Report Generation Implementation

Report generation lives in `sqt-reporter/src/report.rs` and
`sqt-reporter/src/traceability/`. A single `Report` data model is built from the
`RunResult` plus the `VersionResponse` provenance gathered at session start, then
rendered to each output:

- `write_json` — the machine-readable report (`schema_version` first field)
- `write_pdf` — the PDF summary via `printpdf`
- `print_terminal` — the end-of-run terminal summary
- `traceability::write_json` / `write_text` — the dual-audience matrix; both come
  from one matrix model, but that model is built from the `RunResult` and the
  requirement set rather than from `Report` (ADD LLR-026 asks for one model behind
  every output) and does not yet carry provenance (F-M8)
- `write_qdp` — zips the run directory into `qdp.zip`
- `outputs::write_run_outputs` — persists outcomes, writes the manifest, then all of the above

There is no template-engine layer: the PDF is emitted directly by `printpdf`
(AD-006). Substituting another engine would be a change behind `write_pdf` only;
the `Report` data model and every other output are unaffected. The ADD foresees
splitting C-ORC-08 at CDR into report-model construction and per-format rendering,
which is the shape this code already has.

---

## 10. Metric Extensibility

### 10.0 Where Metrics Come From

**All measurement happens in the SQT Runner.** For the shipped Docker backend, each
metric is produced by a tool step in `sqt-runner/src/tools/` (`run_binary`,
`valgrind`, `coverage`, `static_analysis`, `sonarqube`, `binary_size`), run inside
its configured container and mapped into the flat `JobMetrics` DTO
(`sqt-types::JobMetrics`). The **SQT Reporter never runs tools**; it reads
`JobMetrics` off the job response and evaluates constraints against it.

```
SQT Runner                              SQT Reporter
──────────────────────────────────────────────────────────────
tools/*.rs run inside containers        deserialize JobMetrics
map raw tool output → JobMetrics        evaluate Constraint vs. JobMetrics field
describe() → hardware/version           format metric → report row / traceability
```

### 10.1 Two Extension Axes

**(a) A new *target* (hardware or ML-L framework)** → implement a new
`LifecycleBackend` (§8.1). This is the supported, first-class extension seam:
clone the runner, add a `Backend` variant + config section + a `build_executor`
match arm, implement `prepare/run/collect/describe`, recompile. The HTTP/store
layer and the Reporter are untouched.

**(b) A new *metric* from a new tool** on the existing Docker backend → today: add
a step under `sqt-runner/src/tools/`, populate the corresponding `JobMetrics` field
in `collect`, and add a matching constraint name on the Reporter side. The ADD
target ("Portability and lifetime design"; SRS-FUNC-006(b), SRS-MAINT-001) is one
registration entry at a designated collector extension point in the reference
Runner, one file, and — via the open namespace of §10.2 — no Reporter change; that
extension point does not exist yet (SQT-SW-1). There is no
runtime plugin protocol, no dynamic loading, and no external process bridge — the
Runner is a single auditable statically-linked binary, which suits a qualification
tool, and the ADD rules out run-time plug-in discovery in the Orchestrator. (The
reporter-side `MetricCollector` collector tree of the pre-workspace single-crate
era was removed on 2026-10-22; `sqt-reporter/src/metrics/` now holds only the
specification `Target` descriptor. The `inventory`-based `SpecFactory` /
`all_specs()` and the example specs in `sqt-reporter/src/specs/` are still never
called by `main.rs`. The ADD ("Deactivated code") commits the SQT to shipping no
deactivated code, so they have to be removed or wired before a release that
claims it.)

### 10.2 The Closed-Vocabulary Constraint (decided: open namespace in SQT-SW-1)

The current implementation fixes the metric vocabulary on **both** sides:

- `sqt-types::JobMetrics` uses `#[serde(deny_unknown_fields)]` — a Runner that
  returns a metric the DTO does not know fails deserialization by design (this is
  the SRS-IF-007 strictness guarantee).
- The Reporter's `Constraint::from_name` recognises only a hardcoded set of metric
  names; the spec loader rejects unknown metric names.

**Consequence**: axis (b) is *not* purely local today — a genuinely new metric name
requires editing `sqt-types` (add the field) **and** `sqt-reporter`'s constraint
vocabulary, then recompiling both, which falls short of **SRS-FUNC-006(a)** ("no
Orchestrator source file requires modification").

**Decision** (ADD review for PDR): SRS-FUNC-006 stands and the ADD designs the
namespace as open (component C-ORC-05, LLR-019). The implementation follows in
SQT-SW-1 (issue #17): known metrics stay typed, an open extension map carries
Runner-declared metrics, and a generic numeric constraint evaluates names that are
not built in. See AD-029.

### 10.3 The Core Metric Catalogue

The canonical list of metric keys the Runner may populate and the Reporter may
constrain is §4.5.3. Keys map 1:1 to `JobMetrics` fields and to
`JobStatusResponse.metrics` in `job-api.openapi.yaml`.

---

## 11. Technology Stack

### 11.1 Workspace and Dependencies

The project is a **Cargo workspace** (`edition = "2024"`, resolver 2) with three
member crates and a shared `[workspace.dependencies]` table. Key dependencies:

```toml
# Shared
serde, serde_json
serde_yaml            # = serde_yaml_ng (maintained fork; aliased as serde_yaml)
tokio                 # rt-multi-thread, macros, sync, signal
reqwest = "0.12"      # json, rustls-tls, blocking
chrono                # clock (no default features)
tracing, tracing-subscriber (env-filter)
clap = "4"            # derive, env
anyhow

# sqt-reporter (client)
tokio-util, futures   # concurrency + graceful shutdown
zip, walkdir          # QDP bundling
printpdf              # PDF report rendering
inventory             # compile-time registration of the example specs

# sqt-runner (server)
axum = "0.8"          # HTTP server
uuid                  # job IDs (v4)
base64                # docs attachments

# dev
tempfile, axum/tower/http-body-util (handler tests)
```

Notably **not** present (contrary to earlier drafts): `typst`, `tera`,
`tinytemplate`, `quick-xml`, `lcov`, `glob`, `config`, `thiserror`, `mockito`,
`assert_cmd`. PDF is `printpdf`; XML/coverage/lcov parsing is done by the Runner's
tool containers, not by a Reporter-side crate.

Clippy is configured per crate: `pedantic = warn` with `module_name_repetitions`
and `must_use_candidate` allowed (SRS-QUAL-001).

**Quality gates (CI, `.github/workflows/rust.yml`)**: `cargo clippy --workspace
-- -D warnings` (static-analysis gate, SRS-QUAL-001), `cargo fmt --check`
against `rustfmt.toml` (code-style gate, SRS-QUAL-002 — still advisory,
`continue-on-error: true`, which SRS-CDR-003 does not allow), and the test suite
under `cargo llvm-cov` with nextest (`ci/run-tests.sh`, SRS-QUAL-003). SQT-TN-AP
additionally plans `cargo-deny` (licence gate), `cargo-audit` (advisories) and a
`rust-toolchain.toml` toolchain pin; none of the three is in the repository yet.

The language choice answers SRS-DES-001 (statically typed, memory-safe without a
managed runtime, single self-contained executable); the ADD ("Portability and
lifetime design") relies on it so that a run years later needs no interpreter,
package index or shared-library set.

### 11.2 External Tools

Tools the **Runner** orchestrates (packaged as Docker images under `docker/`, so
they need not be installed on the host):

| Tool | Purpose | Metric |
|------|---------|--------|
| `/usr/bin/time -v` | Execution time, peak RSS, CPU | `time.*`, `memory.peak_rss_kb`, `cpu.*` |
| **Valgrind** | Leak detection | `memory.leaks_bytes` |
| **lcov / gcov** | Line coverage | `coverage.line_pct` |
| **cppcheck** | Static analysis | `static.warnings` |
| **SonarQube Scanner** | Severity-graded issues | `static.critical_issues`, `static.major_issues` |
| **Docker** | Container runtime for all of the above | — |

### 11.3 Project Structure

```
qsmart-sqt/                       # Cargo workspace root (ADD ids: §2.4)
├── Cargo.toml                    # [workspace] + [workspace.dependencies]
├── Cargo.lock
├── LICENSE                       # ESA-PL Permissive v2.4 (SRS-DES-002)
├── rustfmt.toml                  # code-style ruleset (SRS-QUAL-002)
├── .github/workflows/rust.yml    # CI gates: clippy, rustfmt, tests + coverage
├── ARCHITECTURE.md               # This document
├── IMPLEMENTATION.md             # Current implementation state
├── REVIEW_FINDINGS.md            # Gaps / decisions from the 2026-07-03 review
├── job-api.openapi.yaml          # Runner API contract
├── docker/                       # Tool images (Dockerfile.run/.valgrind/.coverage/.static/.sonar)
├── examples/                     # Example reporter/runner configs + specs
├── ci/                           # CI Dockerfile + test scripts
│
├── sqt-types/                    # Shared DTOs (the API contract in Rust)
│   └── src/lib.rs                # JobRequest, JobStatusResponse, JobMetrics,
│                                 #   JobOutput, HardwareInfo, VersionResponse,
│                                 #   DocsResponse, HealthResponse, ErrorResponse
│
├── sqt-reporter/                 # Client CLI (orchestration + evaluation + reporting)
│   └── src/
│       ├── main.rs               # clap CLI: `run` / `preflight`
│       ├── config/               # SqtConfig schema + loader/validator
│       ├── specs/                # loader.rs (explicit-file spec loading) + example specs
│       ├── constraint/           # Constraint, Op, Value, evaluator
│       ├── orchestrator/         # session protocol → submit → poll → evaluate
│       ├── api/                  # ApiClient (REST client for the Runner)
│       ├── accuracy/             # client-side accuracy + precision computation
│       ├── baseline/             # baseline-doc parsing + coverage gate
│       ├── evidence/             # per-run EvidenceStore
│       ├── traceability/         # dual-audience matrix
│       ├── report.rs             # Report model, JSON/PDF/terminal/QDP output
│       ├── preflight.rs          # SRS-OPS-007 pre-flight mode
│       └── metrics/              # specification Target descriptor
│
└── sqt-runner/                   # Server (axum) — the reference SQT Runner
    └── src/
        ├── main.rs               # router, backend factory, startup preflight
        ├── handlers.rs           # jobs / version / docs / health handlers
        ├── config.rs             # ServerConfig (backend/docker/hardware/docs)
        ├── store.rs              # in-memory job store
        ├── executor/             # ExecutionBackend + LifecycleBackend + docker.rs
        ├── tools/                # run_binary, valgrind, coverage, static_analysis, sonarqube, binary_size
        ├── docker.rs             # container run/kill helpers
        └── bin/test_binary.rs    # configurable fixture binary-under-test
```

---

## 12. Key Architectural Decisions

### Decision Log

| ID | Decision | Rationale | Alternatives Considered | Status |
|----|----------|-----------|------------------------|--------|
| **AD-001** | Use Rust for implementation | Memory safety, performance, modern tooling | Python (slower), C++ (less safe) | ✅ Accepted |
| **AD-002** | YAML as the config format | Human-readable, widely supported; one structured, version-controllable artefact per project (SRS-FUNC-001, ADD LLR-001) | JSON (verbose), TOML (less known) | ✅ Accepted |
| **AD-003** | REST API for ML-L interface | Language-agnostic, clear decoupling | gRPC (complex), direct linking (coupling) | ✅ Accepted |
| **AD-004** | Strictdoc for requirements | Klepsydra's proposal, text-based; `GET /api/v1/docs` serves StrictDoc in the contract; parsed by the Reporter's own Rust parser (§4.2) | ReqIF (complex), Excel (not traceable) | ✅ Accepted |
| **AD-005** | Ateleris builds generic SQT Runner (not delegated to ML-L Provider) | ML-L Provider focuses on delivering artifacts; Ateleris controls the test execution layer, enabling a generic and open implementation | Delegate to ML-L Provider (Klepsydra) - rejected: closes the runner to third parties and ties test execution to a single vendor | ✅ Accepted |
| **AD-006** | `printpdf` for the PDF summary | Pure-Rust crate inside the Reporter binary: no external typesetting toolchain at run time, consistent with the single self-contained executable (SRS-DES-001); the PDF is one rendering of the report model (§9.4) | Typst, LaTeX/Tectonic (template engines; would be a substitution behind `write_pdf`), Pandoc (limited) | ✅ Accepted |
| **AD-007** | ~~Convention-based req-to-test mapping~~ | Superseded by AD-024; SRS-MAINT-002 requires explicit listing | See AD-024 | ❌ Rejected |
| **AD-008** | Docker containerization | Reproducibility, isolation | VM (heavy), native (env issues) | ✅ Accepted |
| **AD-009** | Exponential backoff for polling | Efficient, respectful of API | Fixed interval (wasteful) | ✅ Accepted |
| **AD-010** | PDF + JSON + terminal + traceability + QDP outputs | ECSS compliance + machine-readability + CI UX | — | ✅ Accepted — **HTML report deferred** (not implemented, not SRS-required; §9.1) |
| **AD-011** | Declarative test specs (metric constraints) | No boilerplate test code; requirements directly encode what must hold; uniform traceability; the ADD's specification model (C-ORC-02) | Manual test code per requirement (verbose, hard to maintain) | ✅ Accepted |
| **AD-012** | ~~Compile-time metric extensibility via `inventory` crate~~ | Superseded: the ADD routes Runner-declared metrics by name through an open namespace (AD-029, LLR-019) and puts the collector extension point in the reference Runner (§10.1); the Reporter-side `inventory` collectors were never wired | External process/JSON stdio, dynamic `.so`, WASM (still rejected: the ADD allows no run-time plug-in discovery) | ❌ Superseded by AD-029 |
| **AD-013** | Single job endpoint (`/api/v1/jobs`) | Eliminates implicit metric-routing convention; one job covers execution + instrumentation + coverage + static analysis; simpler orchestrator logic | Two endpoints `/runs` + `/analysis` (implemented in an earlier iteration, reverted - routing via name-prefix convention was fragile and implicitly coupled) | ✅ Accepted — the ADD's verification-activity invocation (SRS-FUNC-015) is still to be fitted into the contract (§13.2 item 8) |
| **AD-014** | Workload types as a closed enum | The workload types (object_detection, image_classification, etc.) are taken from the ML Space Profile (RD-5); both Reporter and Runner reason over the same closed set; no open-ended task dispatch needed | Open task-name dispatch (Runner accepts arbitrary strings — fragile, hard to validate, not traceable to APCO) | ✅ Accepted |
| **AD-015** | CPU utilisation normalised Runner-side | Runner knows hardware topology (core count, type); `cpu.avg_normalized_pct` is unambiguous across CPU/GPU/FPGA; hardware descriptor provides raw context | Normalise client-side (Runner would need to expose raw per-core counters, each hardware type differently structured) | ✅ Accepted |
| **AD-016** | ML accuracy metrics computed Reporter-side | Runner returns raw outputs; SQT Reporter computes accuracy/F1/IoU/L2 etc. against reference labels; keeps Runner hardware-agnostic and reference-data-free | Runner computes accuracy (requires Runner-side reference dataset per workload - complex, leaks qualification logic into the Runner) | ✅ Accepted |
| **AD-017** | Unified `outputs` field: one entry per input item, transferred once | Repetitions are for statistics only; outputs are per-input-item, not per-repetition. `result_consistent` captures determinism evidence. Applies uniformly to all workload types including pixel_segmentation. | Split `sample_output` / `all_outputs` per workload type (added complexity, inconsistent semantics) | ✅ Accepted |
| **AD-018** | Model and artifacts configured on Runner at startup, not per job | No per-job model transfer; ONNX models can be hundreds of MB; re-sending per job is impractical. Runner is purpose-configured for one model version. To qualify a different version, restart Runner with updated config or CLI flags. | Per-job `model_zip` field (removed: inconsistent with session assembly model, impractical at production model sizes) | ✅ Accepted |
| **AD-019** | ML-L Provider artifacts pre-placed on Runner filesystem | No network transfer from Provider at qualification time; configurable paths in `sqt-runner.yaml`; simplest possible delivery model | Pull artifacts from Provider API at session start (adds Provider-side API requirement, network dependency, latency) | ✅ Accepted |
| **AD-020** | SQT Reporter fetches docs from Runner (not directly from ML-L Provider) | Runner is the single point of contact; Reporter need not know the Provider's delivery mechanism; consistent with Runner-as-data-source model | Reporter contacts Provider directly for docs (couples Reporter to Provider, breaks single-interface design) | ✅ Accepted |
| **AD-021** | 1:1 Reporter:Runner deployment model | Simplest correct model for the current use case; no concurrent client complexity; job queue is sufficient | Multi-client with session isolation (deferred; revisit if needed) | ✅ Accepted |
| **AD-022** | Runner executes one job at a time (initial); Reporter submits+polls concurrently | SRS-PERF-001 requires concurrent submission with wall-clock ≤ longest job + 10%; the SRS permits any Runner-side concurrency bound ≥ 1. Reporter submits all jobs and polls all IDs concurrently (SRS compliant); Runner queues and runs sequentially for v1 (valid: wall-clock bounded by queue depth per SRS-PERF-001). | Fully parallel Runner execution (deferred; adds resource management complexity on space hardware) | ✅ Accepted (reference Runner; ADD C-RNR-02) |
| **AD-023** | SQT Runner execution-backend abstraction (TAL) | Isolates target-specific code behind the object-safe `ExecutionBackend` trait, with an ergonomic `LifecycleBackend` (prepare/run/collect/teardown) that new backends implement; backend chosen by the `backend:` config key + `build_executor` factory; new targets (GR740/GR765) added without touching handlers, store, or the Reporter (§8.1; ADD C-RNR-03, LLR-DER-002, LLR-DER-003) | Inline target-specific logic in the job handler (simpler now, unextensible later) | ✅ Accepted |
| **AD-024** | Explicit spec registration (no auto-discovery) | SRS-MAINT-002 forbids auto-discovery from filesystem state; explicit listing in config makes the active spec set deterministic, auditable, and source-controllable (ADD LLR-005) | Directory scanning (rejected: environment-dependent, invisible in source history, incompatible with reproducible qualification) | ✅ Accepted |
| **AD-025** | Term *workload type*; the wire field keeps the name `use_case` | The SRS (SRS-FUNC-019), the ADD and the OpenAPI schema (`WorkloadType`) say workload type; renaming the field would change the published contract for no functional gain | APCO "workload types" as the term (superseded: the SRS and ADD use workload type); renaming the field to `workload_type` (rejected: contract change) | ✅ Accepted |
| **AD-026** | Pre-flight mode (`--preflight`) | SRS-OPS-007 requires a mode that validates config + endpoint + auth without submitting jobs; shortens CI feedback loop on configuration changes (ADD LLR-017) | No pre-flight (operator must run a full qualification to detect config errors) | ✅ Accepted |
| **AD-027** | Exit code 2 for interrupted runs | SRS-OPS-008 requires SIGTERM exit distinguishable from completed-pass (0) and completed-fail (1); exit 2 = interrupted (ADD LLR-016, LLR-029; §7.3) | Single non-zero code for all non-pass outcomes (rejected: loses diagnostic information) | ✅ Accepted |
| **AD-028** | API-profile coverage from the elements each workload spec declares; coverage never changes the exit code | Keeps coverage independent of any API, model format or Runner — the Reporter compares names it was given (SRS-FUNC-021 to SRS-FUNC-023; ADD LLR-035). Separates "did the workloads pass" from "how much of the profile do they reach" | Runner reports exercised elements (ties the Runner to the API); coverage gaps fail the run (masks a passing run) | 🟡 Proposed — SQT-SW-1 |
| **AD-029** | Open metric namespace for Runner-declared metrics | SRS-FUNC-006(a): a new metric needs no Orchestrator source change; known metrics stay typed for SRS-IF-007 (§10.2; ADD LLR-019) | Keep the closed vocabulary and reword SRS-FUNC-006 (rejected at the ADD review) | 🟡 Proposed — SQT-SW-1 (#17) |
| **AD-030** | Contract data types in a shared crate (`sqt-types`) that depends on no other workspace crate | Both sides agree on the contract by construction; a third-party Runner can be built against the contract alone (ADD C-SHR-01, LLR-DER-001); `sqt-types/tests/openapi_contract.rs` guards against drift from `job-api.openapi.yaml` | — | ✅ Accepted |

### Trade-offs

#### Configuration: Code vs Files

**Decision**: Configuration files (YAML) only

| Aspect | Files (YAML) | Code (Rust) |
|--------|--------------|-------------|
| **Flexibility** | ⭐⭐⭐⭐⭐ Easy to change without recompilation | ⭐⭐ Requires recompilation |
| **Type Safety** | ⭐⭐ Runtime validation | ⭐⭐⭐⭐⭐ Compile-time checks |
| **Version Control** | ⭐⭐⭐⭐⭐ Easy to track changes | ⭐⭐⭐⭐ Mixed with code |
| **User-Friendliness** | ⭐⭐⭐⭐⭐ Non-developers can edit | ⭐⭐ Requires Rust knowledge |

**Conclusion**: YAML only. The ADD (LLR-001) allows no configuration channel other than the single artefact and the credential reference, so there is no programmatic override; the unwired `SpecFactory` in `specs/` is not a supported channel (§10.1)

---

## 13. Open Questions

### 13.0 Deployment Model - Decided

**1:1 Client:Runner** - The current design targets exactly one SQT Reporter talking to one SQT Runner at a time. Concurrent clients are not supported and not planned. The Runner's job queue ensures that multiple jobs submitted by a single Reporter session are executed sequentially; this is by design and not a limitation to work around. If multi-client support is ever needed, the session concept should be revisited first (see review notes on session isolation).

---

### 13.1 SQT Runner - Technical Questions

1. **AI System Assembly Mechanism**
   - **Question**: How exactly does the ML-L Provider package the inference engine, model, and main program? What does "linking them together" concretely mean?
   - **Concern**: The `prepare()` step in the TAL cannot be fully specified until the ML-L Provider's delivery format is known
   - **Action**: Obtain a sample delivery bundle from Klepsydra; define the assembly interface based on it

2. **Workload Set and API Profile** — *decided*
   - The SQT ships a generic, representative workload set per workload type; a project narrows or extends it (SRS-FUNC-022)
   - The API and the API profile are selected outside the SQT, in ML-TN-T1/T2 (APCO, WP100); the SQT takes the profile as a declared input (SRS-FUNC-021)
   - Mission use cases (WP420, APCO) extend the workload set as they become available

3. **GR740/GR765 Emulation Backend** — *route decided, design open*
   - Decided in SQT-TN-AP: target execution is emulated behind the Runner (QEMU, Renode, SIS) for SQT-SW-2; no hardware is procured; Gaisler-hosted boards over SSH/GRMON are opportunistic (§8.3)
   - **Question**: How does the backend drive the emulators — staging the assembled AI system, starting it, and getting outputs and measurements back — and which measurements are meaningful under emulation (SQT-TN-AP proposes deterministic instruction counts)?
   - **Action**: Design the backend within SQT-SW-2 (§14.1)

4. **Input/Output Data Format**
   - **Question**: What is the format of the reference input/output datasets? How are they structured on the Runner filesystem?
   - **Action**: Align with Klepsydra on dataset conventions; document in Runner config schema. Tied to the open `input` contract decision (§5.1, F-H2) and to the data shipped with the generic workload set (SRS-FUNC-022)

### 13.2 SQT Reporter - Technical Questions

5. **Strictdoc Parser Implementation** — *decided: own Rust parser*
   - The Reporter parses StrictDoc itself (`sqt-reporter/src/baseline/mod.rs`, §4.2); no Python StrictDoc CLI at run time, which keeps the Reporter a single self-contained executable (SRS-DES-001)
   - Scope is deliberately small: UID, TITLE, STATUS and VERIFICATION of `[REQUIREMENT]` / `[COMPOSITE_REQUIREMENT]` blocks. Extend it only if requirement discovery needs more fields

6. **REST API Authentication** — *decided: bearer token*
   - Bearer token on every contract operation (SRS-IF-006; `bearerAuth` in `job-api.openapi.yaml`). Reporter: token from the variable named by `runner.token_env`, redacted in all output (`api/client.rs`). Runner: optional `auth.token_env`, middleware in `sqt-runner/src/auth.rs` on every route except `/health`, `401 UNAUTHORIZED` otherwise (§3.1, §5.1)
   - Transport security is `https://` to the configured endpoint; mTLS is not part of the contract

7. **Report Template Engine** — *decided: `printpdf`*
   - The PDF summary is rendered with `printpdf` (AD-006, §9.4). Typst and LaTeX/Tectonic were not adopted; either would be a substitution behind `write_pdf` if a richer layout is needed

8. **Verification-Activity Invocation** (SRS-FUNC-014, SRS-FUNC-015; SQT-SW-1)
   - **Question**: How does the contract let the Reporter trigger project-declared verification activities and retrieve their outputs verbatim — a dedicated operation, or a field of the job request?
   - **Concern**: The ADD lists it as an EI-1 operation class; today coverage and static analysis run implicitly inside every job of the reference Runner and return only metrics (§5.1, AD-013)

9. **UNCOVERED Test Requirements**
   - **Question**: What is the lifecycle outcome of a Test requirement without a registered spec?
   - **Concern**: The contract text of `GET /api/v1/docs` treats it as a failed qualification with status `uncovered` and a non-zero exit; the code exits 1 before submitting any job and writes no report; the ADD's run lifecycle has no matching terminal state (§7.3). Still to be reconciled

### 13.3 Clarifications Needed from ML-L Provider (Klepsydra)

- [ ] Delivery format for inference engine, model, and main program (ZIP structure? directory layout?)
- [ ] Definition of "Prepare AI System" - what does linking/assembling require from the Runner?
- [ ] Sample Strictdoc requirements file for the ECSS documentation baseline
- [ ] Example unit test results and validation test results (format, content)
- [ ] Declared API profile in a machine-readable form (ML-TN-T1/T2, APCO; needed for SQT-SW-1, §4.7)
- [ ] Agreement on the Runner API contract at PDR (an SQT-TN-AP assumption: Orchestrator and Runner are developed contract-first from then on)
- [x] ~~Connection protocol to GR740/GR765~~ — answered by SQT-TN-AP: emulation behind the Runner, opportunistic access to Gaisler-hosted boards (§8.3, §13.1 item 3)

---

## 14. Next Steps

### 14.1 Release Plan

The releases are those of SQT-TN-AP ("Release plan and alignment with ML-L"),
which holds the authoritative scope, the per-release activities and data package,
and the release log; the Product Owner adjusts the grouping at each release
planning with ESA and the ML-L role. Each release is a tag on this repository
(SRS-CDR-001); no tag exists yet.

| Release | Tag | Window | SRS requirement groups | Delivery focus | State in this repository |
|---|---|---|---|---|---|
| SQT-SW-0 | `v0.1.0` | Sprint 0 to PDR (planned 2026-11-04) | FUNC-001..004, 007..009, 018..020; IF-001..003, 006..008; QUAL-001..003; CDR-001..003; DES-001..005; GEN-001, 002; PORT-001, 002; INST-001 | MVP: Orchestrator core, reference Runner, CI gates, minimal evidence package | Implemented, with the open deltas in `REVIEW_FINDINGS.md` (e.g. default fan-out F-M7, verbatim discovery evidence F-M11, advisory rustfmt gate); not tagged |
| SQT-SW-1 | `v0.2.0` | PDR to project-level CDR | FUNC-005, 006, 010..015, 021..023; PERF-001..004; REL-001..004; OPS-001..008; HF-001, 002; DATA-001..003; RES-001..003; MAINT-001, 002 | Metric collectors, traceability matrix, verification-activity triggering, operator guidance, performance and reliability; fulfilment matrix becomes authoritative | Partly ahead of plan (concurrency and polling, exit codes, pre-flight, partial-failure handling, schema version, dual-audience traceability exist). Open: API-profile qualification (§4.7), open metric namespace (§10.2, #17), verification-activity invocation (§13.2 item 8), per-run evidence location (F-M4) |
| SQT-SW-2 | `v1.0.0` | Project-level CDR to QR | FUNC-016, 017; CDR-004; VAL-001, 002; full non-regression; end-to-end ML-L-QDP generation on GR740 and GR765/NOEL-V emulation | Final release: complete SUM, SRF, SRelD; ISVV-ready artefacts | Accuracy and precision computation exist (`accuracy/`); emulation backend not started (§8.3) |
| SQT-SW-3 (optional) | `v1.x` | QR to FR | Defect fixes and Task 3 (use case) feedback | Consolidation for the FR data package | — |

### 14.2 Immediate Actions (to PDR)

1. **Close SQT-SW-0**: work off the SQT-SW-0 items in `REVIEW_FINDINGS.md`, make
   the rustfmt gate blocking (SRS-CDR-003), and tag `v0.1.0`.
2. **Agree the Runner API contract with the ML-L role** (SQT-TN-AP assumption),
   including the pending `input` decision (§5.1) and the shape of
   verification-activity invocation (§13.2 item 8).
3. **Align on the ML-L Provider delivery format**: obtain a sample artifact bundle
   (inference engine + model + main program) and pin down the "Prepare AI System"
   step (§8.2, §13.1 item 1).
4. **Prepare SQT-SW-1**: obtain the declared API profile from ML-TN-T1/T2 (APCO) in
   machine-readable form, and plan the generic workload set and coverage report
   (§4.7).

---

## Appendices

### Appendix A: Glossary

| Term | Definition |
|------|------------|
| **ECSS** | European Cooperation for Space Standardization |
| **ML-L** | Machine Learning Library (inference engine) |
| **SQT** | Software Qualification Toolkit (collectively: SQT Runner + SQT Reporter) |
| **SQT Runner** | Ateleris-developed REST API server that executes tests on the target hardware |
| **SQT Reporter** | Ateleris-developed CLI that orchestrates qualification sessions and generates reports; the **Orchestrator** of the SRS and ADD |
| **Reference Runner** | The ADD's name for `sqt-runner`: the conformant Runner delivered with the SQT (SRS-INST-001); its internals are non-normative for third-party Runners |
| **Runner API contract** | `job-api.openapi.yaml`; the single, versioned integration boundary, normative for both sides (ADD AD-4, EI-1) |
| **TAL** | Target Abstraction Layer - pluggable backend in the SQT Runner for different execution targets |
| **QDP** | Quality Data Package - the ECSS-compliant qualification output artifact |
| **API profile** | The API elements inside the qualification boundary, declared per ML Space Profile (ML-TN-T1/T2); an SQT input (SRS-FUNC-021) |
| **Workload type** | The kind of AI workload a job runs (`use_case` on the wire), from the ML Space Profile (SRS-FUNC-019) |
| **Workload tests** | Representative workloads the SQT Runner executes to check that the ML-L loads, executes and returns a valid result (SRS-FUNC-022) |
| **PBF** | Performance Benchmark Framework (Klepsydra's internal tool - not used by SQT Runner) |
| **TRL** | Technology Readiness Level (1-9 scale) |
| **Strictdoc** | Requirements management tool (text-based format) |
| **ONNX** | Open Neural Network Exchange (model format) |
| **GR740** | Quad-core LEON4 space-grade processor (Cobham Gaisler) |
| **GR765** | Octa-core space-grade processor (Cobham Gaisler); targeted in its NOEL-V configuration, the LEON5 configuration being excluded (SQT-TN-AP) |
| **NOEL-V** | Cobham Gaisler RISC-V processor core of the GR765 |
| **QEMU, Renode, SIS** | Emulators used behind the Runner for the GR740 and GR765/NOEL-V targets (SQT-TN-AP) |
| **RTEMS** | Real-Time Executive for Multiprocessor Systems (RTOS) |

### Appendix B: References

Project documents (`qsmart-sqt-documentation` repository):

1. SQT Software Requirements Specification (SRS), SQT-SP-0001-ATS, Issue 1 Rev 2
2. SQT Architectural Design Document (ADD), SQT-DD-0001-ATS — the authoritative design
3. SQT-TN-AP — Agile Process, Development, Verification and Validation Plan, SQT-PL-0002-ATS (release plan, emulation environment, quality gates)
4. SQT Tools Identification Technical Note, SQT-TN-0002-ATS
5. Project Glossary, SQT-TN-0001-ATS
6. Runner API contract: `job-api.openapi.yaml` (this repository)

External and customer documents:

7. ML-TN-T1 — APCO Space Profile Technical Note (APCO, WP100): ML Space Profile, workload types
8. ML-TN-T2 — API and ML-L Analysis Report (APCO, WP100): API selection and the API profile
9. ESA Statement of Work (Q-SMART), ESA-GSTP-TECEDS-SOW-2025-001877
10. Q-SMART Proposal, Volume 1, KPSR-2024-13-QSMART-Vol1
11. ECSS-E-ST-40C Rev.1: Software engineering
12. ECSS-Q-ST-80C: Software product assurance
13. ESA Software Community Licence Permissive v2.4 (Type 3)
14. Strictdoc Documentation: https://strictdoc.readthedocs.io/
15. RTEMS Quality Delivery Process: https://docs.rtems.org/


# Notes
Strictdoc supports custom grammars / fields -> We could inject our definitions there

See https://strictdoc.readthedocs.io/en/stable/?a=SDOC-SRS-21 and https://strictdoc.readthedocs.io/en/stable/?a=SDOC-SSS-62 

However, do we really want do provide a set of things that can be done? Or do we want to define that in code?