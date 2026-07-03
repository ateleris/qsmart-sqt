# Q-SMART SQT Architecture Overview

**Project**: Qualified Software Machine Learning Toolkit for Space Hardware
**Components**: SQT Runner + SQT Reporter (both Ateleris responsibility)
**Technology**: Rust applications
**Version**: 1.2
**Date**: 2026-07-03

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
- [3. Configuration Management](#3-configuration-management)
- [4. Requirements Management](#4-requirements-management)
  - [4.5 Declarative Test Specifications](#45-declarative-test-specifications)
  - [4.6 Compliant Requirements (Non-Testable)](#46-compliant-requirements-non-testable)
- [5. REST API Interface (SQT Runner)](#5-rest-api-interface-sqt-runner)
  - [5.2 APCO Space Profile Use Cases](#52-apco-space-profile-use-cases)
  - [5.5 API Endpoints](#55-api-endpoints)
  - [5.5.3 Health Check](#553-health-check)
  - [5.10 Requirements Documents Endpoint](#510-requirements-documents-endpoint)
- [6. Scope Definition](#6-scope-definition)
- [7. Qualification Workflow](#7-qualification-workflow)
- [8. SQT Runner Target Architecture](#8-sqt-runner-target-architecture)
- [9. Report Generation](#9-report-generation)
- [10. Metric Extensibility](#10-metric-extensibility)
- [11. Technology Stack](#11-technology-stack)
- [12. Key Architectural Decisions](#12-key-architectural-decisions)
- [13. Open Questions](#13-open-questions)
- [14. Next Steps](#14-next-steps)

---

## 1. Executive Summary

The Software Qualification Toolkit (SQT) is the Ateleris-developed component of the Q-SMART qualification infrastructure. It qualifies Machine Learning Libraries (ML-L) targeting space-grade hardware and consists of **two distinct Rust applications**:

### SQT Runner

The SQT Runner is a REST API server deployed on **customer-provided hardware**. It is the component that actually executes tests against the AI system under qualification. It:

- **Assembles the AI system** from the ML-L Provider's artifacts (inference engine, trained model, main program) into a fully runnable task
- **Executes Space Profile tests** (functional tests defined by ESA and Klepsydra) and **Performance tests** (timing, memory, throughput) against the assembled system
- **Serves static qualification artifacts** (ECSS documentation baseline, unit test results, validation test results) pre-placed on its filesystem by the ML-L Provider
- **Exposes a REST API** (`job-api.openapi.yaml`) that the SQT Reporter calls to trigger and monitor test jobs
- **Supports multiple execution targets** via a pluggable target layer (x86\_64/Linux native in scope; GR740 and GR765 planned for future extension)

### SQT Reporter

The SQT Reporter is a CLI application that **orchestrates the full qualification session**. It:

- **Fetches and validates** the ECSS documentation baseline and static test results from the SQT Runner
- **Checks requirements compliance** before allowing test execution to proceed
- **Submits all test jobs concurrently** to the SQT Runner via REST API (receives job IDs immediately; a configurable concurrency bound limits in-flight submissions) and polls all outstanding jobs concurrently until completion
- **Evaluates metric constraints** against returned results (declarative test specifications), with unit-aware comparison
- **Generates ECSS-compliant reports** (PDF, HTML) and the QDP (Quality Data Package) with full requirements traceability

### Key Principles

1. **Separation of concerns**: The SQT Runner handles test execution and artifact serving; the SQT Reporter handles orchestration, requirements checking, and reporting
2. **Deployment flexibility**: The SQT Runner deploys on the customer's hardware (where access to the target systems exists); the SQT Reporter can run anywhere
3. **Target extensibility**: The Runner's target abstraction layer allows adding new execution targets (GR740, GR765) without changing the Reporter or the REST API contract
4. **Configuration-driven**: All aspects configurable via YAML; test specifications registered by explicit declaration (no auto-discovery)
5. **Standards-compliant**: ECSS E-ST-40C and Q-ST-80C alignment; Use Cases per the APCO Space Profile (RD-5 / ML-TN-T1)
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
║  │  (implements job-api.openapi)    │◄──┤  │ Space Profile Tests  │    │     ║
║  │                                  │   │  │ (Ateleris-defined    │    │     ║
║  │  GET  /api/v1/version → compat   │   │  │  functional tests)   │    │     ║
║  │  GET  /api/v1/health → liveness  │   │  └─────────────────────┘    │     ║
║  │  POST /api/v1/jobs  → enqueue    │   │  ┌─────────────────────┐    │     ║
║  │  GET  /api/v1/jobs/{id} → poll   │   │  │ Performance Tests    │    │     ║
║  │  GET  /api/v1/docs  → serve docs │   │  │ (timing, memory,     │    │     ║
║  └──────────────────────────────────┘   │  │ (timing, memory,     │    │     ║
║                                         │  │ (timing, memory,     │    │     ║
║                                         │  │  throughput)         │    │     ║
║                                         │  └─────────────────────┘    │     ║
║                                         └─────────────┬───────────────┘     ║
║                                                       │                     ║
║  ┌────────────────────────────────────────────────────▼─────────────────┐   ║
║  │                      Target Abstraction Layer                         │   ║
║  │                                                                       │   ║
║  │   ┌──────────────────┐   ┌─────────────────────┐  ┌───────────────┐  │   ║
║  │   │ x86_64 / Linux   │   │ GR740  [Out of Scope]│  │ GR765  [OOS] │  │   ║
║  │   │ Native execution │   │ Remote attachment    │  │ Remote attach│  │   ║
║  │   │ [In Scope]       │   │ (serial/network TBD) │  │ (TBD)        │  │   ║
║  │   └──────────────────┘   └─────────────────────┘  └───────────────┘  │   ║
║  └───────────────────────────────────────────────────────────────────────┘   ║
╚══════════════════════════════════════════════════════════════════════════════╝
                               │
         REST API  (job-api.openapi.yaml)
         • GET  /api/v1/health    - liveness / readiness probe
         • POST /api/v1/jobs      - submit test request
         • GET  /api/v1/jobs/{id} - poll for results + metrics
         • GET  /api/v1/docs      - fetch document baseline
                               │
                               ▼
╔══════════════════════════════════════════════════════════════════════════════╗
║        SQT Reporter  (Ateleris - Rust)  [Any x86_64 system]                 ║
╠══════════════════════════════════════════════════════════════════════════════╣
║                                                                              ║
║  ┌──────────────────┐  ┌──────────────────────┐  ┌────────────────────────┐ ║
║  │ Baseline Checker │  │  Orchestrator        │  │  Report Generator      │ ║
║  │                  │  │                      │  │                        │ ║
║  │ • GET /api/v1/   │  │ • Submit test jobs   │  │ • System Test Report   │ ║
║  │   docs           │  │   via POST           │  │ • QDP                  │ ║
║  │ • Parse ECSS     │  │ • Poll until done    │  │ • ECSS PDF / HTML      │ ║
║  │   docs & static  │  │ • Evaluate metric    │  │ • Traceability matrix  │ ║
║  │   test results   │  │   constraints        │  │                        │ ║
║  │ • Gate: halt if  │  │ • Collect evidence   │  │                        │ ║
║  │   not compliant  │  └──────────────────────┘  └────────────────────────┘ ║
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

3. Parse docs; check ECSS compliance and static test results
   Gate: if not compliant → halt with error report

4. Parse Strictdoc requirements from fetched docs
   Identify testable requirements (VERIFICATION: Test)

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

8. Generate System Test Report + QDP
```

### 2.3 Component Responsibilities

#### SQT Runner (Ateleris - new, generic implementation)

| Responsibility | Detail |
|---|---|
| REST API server | Implements `job-api.openapi.yaml`; accepts jobs, returns metrics |
| Document store | Serves pre-placed ML-L Provider artifacts via `GET /api/v1/docs` |
| AI system assembly | Links inference engine + model + main program into runnable task |
| Space Profile test execution | functional tests for space AI systems, defined by ESA and Klepsydra |
| Performance test execution | Timing, memory, throughput benchmarks |
| Target abstraction | Pluggable backend: x86\_64 native (in scope); GR740/GR765 (planned) |
| Configuration | All artifact paths and target settings configurable via a config file; minimal changes needed to deploy |

#### SQT Reporter (Ateleris - formerly "SQT Generic")

| Responsibility | Detail |
|---|---|
| Configuration management | Parse and validate YAML config |
| Document baseline check | Fetch docs from Runner, verify ECSS compliance before proceeding |
| Requirements management | Parse Strictdoc from fetched docs; build traceability database |
| Job orchestration | Submit test jobs; poll for results; evaluate metric constraints |
| Evidence collection | Aggregate results; build traceability matrix |
| Report generation | ECSS-compliant PDF/HTML; QDP |

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
Job submission + polling        │  Space Profile test execution
Metric constraint evaluation    │  Performance test execution
Evidence collection             │  Target abstraction (x64, GR740, GR765)
Report generation (PDF/HTML)    │  Job queue + sequential execution
QDP production                  │  Metric collection and reporting
```

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
  retry_max: 3                       # transient-error retry ceiling (SRS-IF-009)
  retry_backoff_initial_secs: 1      # initial retry backoff (SRS-IF-009)
  poll_interval_initial_secs: 1      # first polling interval (SRS-PERF-003)
  poll_interval_max_secs: 30         # polling backoff cap (SRS-PERF-003)

# Report generation
report:
  output_dir: "./reports"            # relative paths resolve against this config file
```

The Reporter emits, into `report.output_dir`: the machine-readable JSON report,
a PDF summary, the traceability matrix (`.json` + `.txt`), per-job evidence under
`evidence/`, and the QDP bundle (`qdp.zip`). Report *format selection* (choosing
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
    onnxruntime: "1.17"
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
field so the operator can fix it without reading logs. Deeper checks (endpoint
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
TITLE: ONNX model format support
STATEMENT: The ML-L shall support loading and executing models in ONNX format version 1.13 or later.
RATIONALE: ONNX is the industry-standard format for ML model interchange.
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

```rust
// Rust data structure for requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub uid: String,
    pub status: RequirementStatus,
    pub title: String,
    pub statement: String,
    pub rationale: Option<String>,
    pub verification_method: Vec<VerificationMethod>,
    pub relations: Vec<RequirementRelation>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone)]
pub enum RequirementStatus {
    Active,
    Draft,
    Deprecated,
}

#[derive(Debug, Clone)]
pub enum VerificationMethod {
    Test,
    Inspection,
    Analysis,
    Demonstration,
}

#[derive(Debug, Clone)]
pub struct RequirementRelation {
    pub relation_type: RelationType,
    pub target_uid: String,
}

#[derive(Debug, Clone)]
pub enum RelationType {
    Parent,
    Child,
    Refines,
    DerivedFrom,
}
```

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
use_case: image_classification      # APCO Space Profile Use Case
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

This is similar to property-based testing and contract testing. Strictdoc's custom grammar feature (see Notes section) can be used to embed these specs directly in `.sdoc` files, or they can live in sidecar YAML files alongside requirements. Regardless of location, each spec file must be explicitly listed in `sqt-reporter.yaml` — no auto-discovery from filesystem state is performed (SRS-MAINT-002).

#### 4.5.2 Spec YAML Format

Test spec files are listed explicitly in the Reporter config (see §3.1):

```yaml
# specs/REQ-ML-L-003.test.yaml
requirement_uid: REQ-ML-L-003
use_case: image_classification        # APCO Space Profile Use Case (SRS-IF-005)
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
explanatory message. A spec with zero constraints at evaluation time is recorded
as FAIL — vacuous passes are rejected (SRS-REL-001).

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

**Power / Thermal Metrics**

| Metric key | Unit | Description |
|------------|------|-------------|
| `power.avg_watts` | W | Average power draw during inference |
| `power.peak_watts` | W | Peak power draw across all repetitions |
| `hardware.temperature_celsius` | °C | Average die/board temperature during the run |

**Phase 1 MVP** targets the most critical metrics: `memory.peak_rss_kb`, `memory.leaks_bytes`, `time.avg_ms`, `time.latency_p95_ms`, `return_code`, `static.critical_issues`, `coverage.line_pct`, `cpu.avg_normalized_pct`.

#### 4.5.4 Execution Flow

```rust
// Pseudo-code: API-driven orchestration

// Submit one job per testable requirement
let job_id = api_client.submit_job(&JobRequest {
    use_case: spec.use_case,           // APCO Space Profile Use Case
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

## 5. REST API Interface (SQT Runner)

### 5.1 API Contract (OpenAPI Specification)

The full interface contract is defined in **`job-api.openapi.yaml`** at the root of
the repository. The spec is implemented by the **SQT Runner** (the `sqt-runner`
crate is the reference implementation); the **SQT Reporter** is the API client.

> **Known contract drift**: the OpenAPI file still names the workload field `model`
> whereas the implementation (and this document) use `use_case`; `input` is typed
> as a discriminated union in the contract but sent as a bare path string. The
> OpenAPI file is the item to regenerate. Tracked as F-H2 in `REVIEW_FINDINGS.md`.

#### Base URL
```
http://localhost:8080
```

#### Authentication
```http
Authorization: Bearer <token>
Content-Type: application/json
```

#### Design Principles

The Runner is configured at startup (via `sqt-runner.yaml` or CLI flags) with the
hardware target, inference engine, trained model, and main program. These are
**not** transmitted per job. The client (SQT Reporter) specifies only the Use Case
(§5.2) and measurement parameters; the Runner returns everything it can measure.

A single job type covers all qualification activities: execution runs, instrumented
runs (valgrind/asan/gprof), coverage collection, and static analysis.

**Session start protocol (SRS-IF-008):** Before submitting any job, the Reporter
calls `GET /api/v1/version` to verify the Runner's API version is within the set
declared by the Reporter binary. An incompatible version causes an immediate
non-zero exit with a clear mismatch message.

**Response schema validation (SRS-IF-007):** Every Runner response is validated
against the published contract schema before consumption. A non-conformant response
causes the affected job to be recorded as infrastructure-failed.

**Transient error retry (SRS-IF-009):** Network resets, connection timeouts, and
recoverable 5xx responses trigger bounded retries with exponential backoff (up to
`runner.retry_max` attempts). After exhausting retries, the affected job is
recorded as infrastructure-failed. This is distinct from the steady-state polling
backoff in §5.9.

**Credential redaction (SRS-OPS-005):** Bearer tokens are redacted from all
operational output (log lines, terminal summary, persisted evidence, verbose
request/response traces) at every verbosity level without exception.

### 5.2 APCO Space Profile Use Cases

The set of supported Use Cases is defined by the **APCO Space Profile** (RD-5 /
ML-TN-T1) and enumerated in the SQT Reporter. Both Reporter and Runner reason over
the same closed set; the Runner maps each Use Case to the appropriate binary
invocation, input handling, and output interpretation (SRS-IF-005).

| Use Case | Domain | Typical Input | Typical Output |
|---|---|---|---|
| `object_detection` | Computer vision | Images | Bounding boxes + class labels |
| `image_classification` | Computer vision | Images | Class label + confidence score |
| `pixel_segmentation` | Computer vision | Images | Per-pixel class mask |
| `classification` | Tabular ML | Feature vectors | Class label |
| `regression` | Tabular ML | Feature vectors | Scalar prediction |
| `time_series_forecasting` | Sequential ML | Time series sequences | Forecast sequence |

**Default session**: when a spec does not declare a Use Case, the SQT Reporter
submits one job per supported Use Case (SRS-IF-005). Jobs are queued on the Runner
side and polled concurrently by the Reporter.

### 5.3 Measurement Modes

Each job runs in one of two modes, set by the `mode` field. The Use Case is set by the `use_case` field (§5.2):

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
The Reporter does not transmit the model — it only specifies the APCO Space Profile
Use Case and measurement parameters.

For large image batches (hundreds or thousands of images), the request body will be
proportionally large. The server's HTTP frontend must be configured to accept it
(e.g. `client_max_body_size 2g` in nginx). Alternatively, reference a server-side
pre-loaded dataset via `input.type = "dataset_ref"` with a server-assigned identifier.

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

For all Use Cases (including `pixel_segmentation`), the server executes all
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
    "message": "unknown Use Case: 'depth_estimation'",
    "details": "Supported types: object_detection, image_classification, ...",
    "timestamp": "2026-03-06T10:00:00Z"
  }
}
```

**Error Codes**:
- `VALIDATION_ERROR`: Invalid request parameters (e.g., unknown Use Case)
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

4. **Space Profile Test Execution**
   - Execute Space Profile functional tests (ESA/Klepsydra-defined) against the assembled AI system
   - Collect and return results as job metrics

5. **Performance Test Execution**
   - Execute timing, memory, and throughput benchmarks
   - Collect and return metrics in the standard job response schema

6. **Target Abstraction Layer**
   - x86\_64 / Linux native execution **[In Scope]**
   - GR740 via remote attachment (serial/network) **[Planned - Out of Scope for now]**
   - GR765 via remote attachment (serial/network) **[Planned - Out of Scope for now]**
   - Architecture designed to add new targets without changing the Reporter or API contract

#### ❌ OUT OF SCOPE for SQT Runner

1. **Report Generation** - the Reporter produces all reports and the QDP
2. **Requirements Management** - requirements are parsed and evaluated by the Reporter
3. **ML model conversion / quantization** - the ML-L Provider supplies the model as-is
4. **Cross-compilation of ML-L binaries** - the ML-L Provider supplies pre-built binaries
5. **Direct GR740/GR765 hardware access for v1** - designed for, but deferred

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

4. **Document Baseline Check**
   - Fetch documents from the SQT Runner via `GET /api/v1/docs`
   - If empty document set → proceed with project-declared specs + emit diagnostic (SRS-IF-004)
   - Parse ECSS documentation baseline and static test results
   - Gate the qualification session: halt if compliance criteria are not met

5. **Requirements Management**
   - Parse Strictdoc requirements from fetched documents
   - Classify by verification method: Test-verifiable vs compliant-by-Runner-declaration (SRS-IF-004)
   - Build requirements database and traceability matrix

6. **REST API Client (to SQT Runner)**
   - Concurrent job submission: submit all jobs, receive job IDs (202 Accepted) (SRS-PERF-001)
   - Bounded concurrency: `max_concurrent_jobs` semaphore (SRS-PERF-002)
   - Concurrent polling of all outstanding IDs with exponential backoff (SRS-PERF-003)
   - Per-job timeout enforcement; project-default applied when spec omits one (SRS-PERF-004)
   - Response schema validation before consuming (SRS-IF-007)
   - Transient error retry with bounded back-off (SRS-IF-009)

7. **Metric Constraint Evaluation**
   - Unit-aware evaluation: reconcile compatible units (ms↔s, KB↔bytes) (SRS-FUNC-004)
   - Map null/absent metrics to failed constraints (SRS-REL-002)
   - Reject vacuous passes: zero-constraint specs recorded as FAIL (SRS-REL-001)
   - Numerical output precision comparison against reference values (SRS-FUNC-016)
   - Client-side accuracy metric computation per Use Case (SRS-FUNC-017):
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
    - Organize qualification artifacts per run under a timestamped, uniquely identified location (SRS-OPS-004)

11. **Report Generation**
    - ECSS-compliant PDF reports, HTML developer reports, JSON machine-readable output
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

#### ❌ OUT OF SCOPE for SQT Reporter

1. **Test execution** — the Runner handles all test execution
2. **Hardware access** — the Runner abstracts all target hardware
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

### 6.4 Interface Contract

```
SQT Reporter                    SQT Runner
────────────────────────────────────────────────────────────────────
"Fetch the document baseline"   → ECSS docs + static test results

"Are documents compliant?"      Gate in Reporter: proceed or halt

"Execute Use Case: image_classification"
                                → Assemble AI system
                                  Run Space Profile + Performance tests
                                  Return metrics

"What's the status?"            → "Running… 65%"

"Done - give me results"        → Metrics + hardware descriptor

"Here is the qualification      (Runner does not generate reports)
 report and QDP"
```

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
│    (SRS-IF-004)                                                  │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 3. Baseline Compliance Check  [SQT Reporter]                     │
│                                                                  │
│    Parse fetched documents; verify required ECSS artefacts       │
│    are present and structurally valid.                           │
│                                                                  │
│    Gate: if not compliant → halt, emit error report              │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 4. Parse Requirements  [SQT Reporter]                            │
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
│ 5. Load Test Specs  [SQT Reporter]                               │
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
│ 6. Submit ALL Jobs Concurrently  [SQT Reporter → SQT Runner]     │
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
│ 7. Execute & Poll Concurrently  [Runner + Reporter polling]       │
│                                                                  │
│  Runner (one job at a time in current implementation):           │
│    • Assemble AI system (inference engine + model + main prog)   │
│    • Execute Space Profile tests (ESA/Klepsydra-defined)         │
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
│ 8. Evaluate Metric Constraints  [SQT Reporter]                   │
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
│ 9. Build Traceability Matrix  [SQT Reporter]                     │
│                                                                  │
│    REQ-ML-L-001  PASS  return_code=0 ✓  time=87ms ✓             │
│    REQ-ML-L-002  FAIL  return_code=1 ✗                          │
│    REQ-ML-L-NFR-001  COMPLIANT (Review - server declaration)     │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 10. Generate System Test Report + QDP  [SQT Reporter]            │
│                                                                  │
│    • Stamp SQT version + Runner provenance in every output       │
│    • Include schema_version in machine-readable output           │
│    • Render ECSS-compliant PDF (Typst) and HTML                  │
│    • Produce QDP artefact                                        │
│    (SRS-DATA-001, SRS-DATA-003)                                  │
│                                                                  │
│    Output: system-test-report.pdf, report.html, qdp.zip          │
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
use_case: image_classification         # APCO Space Profile Use Case
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

---

## 8. SQT Runner Target Architecture

The SQT Runner is built around a pluggable **execution backend**. All
target-specific logic (how a job is provisioned, run, and measured) sits behind a
trait, so a new target — different hardware, a different ML-L framework — can be
added without touching the HTTP layer, the job store, or the SQT Reporter. This is
the concrete realisation of the Target Abstraction Layer (AD-023).

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
| `docker` (x86_64/Linux) | **In Scope (v1)** | Compile/execute inside Docker containers on the host | Development and CI baseline; the only shipped `Backend` variant |
| GR740 | **Planned - Out of Scope for v1** | New backend: remote attachment (protocol TBD: serial / GDB stub / network) | Customer provides the hardware; backend connects to it |
| GR765 | **Planned - Out of Scope for v1** | New backend: remote attachment (protocol TBD) | Same model as the GR740 backend |

**Design constraint for GR740/GR765**: Ateleris does not have access to these
processors. Support would be added as a new `Backend` variant implementing
`LifecycleBackend` in "remote execution" mode: `prepare` transfers the assembled
AI system to the customer-provided board over an attached link, `run` triggers
execution, `collect` gathers results back. The exact connection protocol (JTAG,
GDB remote stub, serial, network) is TBD and will be defined once a concrete
delivery from the ML-L Provider is available. The backend abstraction (§8.1)
ensures this can be added without touching the API, handlers, or store.

**Note on emulation**: Gaisler provides **TSIM** (Time-accurate SIMulator), a commercial tool that emulates LEON4 (GR740) and exposes a GDB remote stub interface. In principle, the GR740 Runner target could support TSIM as a sub-mode alongside physical hardware - the binary and execution model are identical, only the connection differs. TSIM3 covers LEON4/GR740; LEON5/GR765 support (TSIM5) should be verified with Gaisler. This route is not currently planned but the TAL design does not preclude it.

### 8.4 Space Profile Tests and Performance Tests

Both test types are executed by the Runner's Test Execution Engine for each job:

**Space Profile Tests** (defined by ESA and Klepsydra; implemented in the Runner by Ateleris):
- Functional correctness tests specific to space AI applications
- Defined and maintained by Ateleris
- Verify that the AI system behaves correctly on the target hardware (e.g. input/output format compliance, determinism, error handling)
- Results reported as standard job metrics

**Performance Tests**:
- Timing benchmarks (latency, throughput)
- Memory benchmarks (peak RSS, stack usage, heap allocations)
- Power and temperature metrics (where the target exposes them)
- Results reported as standard job metrics

Both test types use the same job submission flow; the `use_case` field in the job request (an APCO Space Profile Use Case, §5.2) determines which test suite is exercised.

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
image is not configured is simply skipped, and its metrics are reported as `null`.

---

## 9. Report Generation

### 9.1 Report Outputs

Every `sqt run` writes the following into `report.output_dir`. All are produced on
every run — there is no per-format on/off switch.

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
- **Engine**: rendered with `printpdf` today; a richer template engine (e.g.
  Typst) is a future substitution behind the same report data model (AD-006)
- **Sections**: executive summary, requirements traceability, test-results
  summary, per-requirement detail

#### Traceability Matrix (dual-audience)
- Emitted as both `traceability.json` and `traceability.txt` from one data model
  (SRS-FUNC-010, SRS-DATA-002); produced when the Runner serves a baseline

#### Terminal Summary
- Printed at the end of every run: per-spec PASS / FAIL / INFRA_ERROR with counts
  and evidence location (SRS-OPS-003)

#### QDP (Quality Data Package)
- `qdp.zip` bundling the report, traceability, and per-job evidence — the ECSS
  qualification output artefact

> **HTML report**: not implemented. It is a possible future developer-facing
> output; it is not an SRS requirement and no code path emits it today. Earlier
> drafts of this document listed it as a first-class format (AD-010) — treat that
> as deferred, not current.

### 9.2 Report Structure Principles

The report is **requirement-centric**: the outer loop is requirements, not runs.

A single qualification session may test the binary with multiple inputs
(e.g. `reference.bin`, `edge_case.bin`, `stress.bin`). Each input produces one
execution run. Each requirement's `TestSpec` references the input relevant to it.
The orchestrator groups specs by input, submits one run job per unique input, and
fans results back to each spec. The Runner uses the model and inference engine
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
- `traceability::write_json` / `write_text` — the dual-audience matrix
- `write_qdp` — zips report + traceability + evidence into `qdp.zip`

There is no template-engine layer today: the PDF is emitted directly by
`printpdf`. Substituting a template engine (Typst/LaTeX) later is a change behind
`write_pdf` only; the `Report` data model and every other output are unaffected.

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

**(b) A new *metric* from a new tool** on the existing Docker backend → add a step
under `sqt-runner/src/tools/`, populate the corresponding `JobMetrics` field in
`collect`, and add a matching constraint name on the Reporter side. There is no
runtime plugin protocol, no dynamic loading, and no external process bridge — the
Runner is a single auditable statically-linked binary, which suits a qualification
tool. (The historic reporter-side `MetricCollector`/`inventory` collector tree in
`sqt-reporter/src/metrics/` is **legacy reference code from the pre-workspace
single-crate era**; it is not wired into either the orchestrator or the runner and
is a deletion candidate — see `REVIEW_FINDINGS.md`.)

### 10.2 The Closed-Vocabulary Constraint (open design point)

The current implementation fixes the metric vocabulary on **both** sides:

- `sqt-types::JobMetrics` uses `#[serde(deny_unknown_fields)]` — a Runner that
  returns a metric the DTO does not know fails deserialization by design (this is
  the SRS-IF-007 strictness guarantee).
- The Reporter's `Constraint::from_name` recognises only a hardcoded set of metric
  names; the spec loader rejects unknown metric names.

**Consequence**: axis (b) is *not* purely local — a genuinely new metric name
requires editing `sqt-types` (add the field) **and** `sqt-reporter`'s constraint
vocabulary, then recompiling both. This tension with **SRS-FUNC-006**
("project-extensible metric collection" without modifying core source) is a
deliberate, currently-unresolved design decision, not an oversight. Two ways
forward — (i) accept it and reword SRS-FUNC-006 to "extensible by recompilation of
the open-source workspace", or (ii) introduce an explicit extension namespace
(e.g. an open `custom.*` metric map on `JobMetrics` plus a generic constraint kind)
— are tracked as a decision item in `REVIEW_FINDINGS.md`. This document does not
pre-judge the outcome.

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
inventory             # (legacy metric registry — see §10.1)

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
ats-sqt/                          # Cargo workspace root
├── Cargo.toml                    # [workspace] + [workspace.dependencies]
├── Cargo.lock
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
│       └── metrics/              # legacy collectors (unwired; see §10.1)
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
| **AD-002** | YAML as primary config format | Human-readable, widely supported | JSON (verbose), TOML (less known) | 🟡 Proposed |
| **AD-003** | REST API for ML-L interface | Language-agnostic, clear decoupling | gRPC (complex), direct linking (coupling) | ✅ Accepted |
| **AD-004** | Strictdoc for requirements | Klepsydra's proposal, text-based | ReqIF (complex), Excel (not traceable) | 🟡 Proposed |
| **AD-005** | Ateleris builds generic SQT Runner (not delegated to ML-L Provider) | ML-L Provider focuses on delivering artifacts; Ateleris controls the test execution layer, enabling a generic and open implementation | Delegate to ML-L Provider (Klepsydra) - rejected: closes the runner to third parties and ties test execution to a single vendor | ✅ Accepted |
| **AD-006** | Typst for PDF generation | Modern, fast, programmable | LaTeX (complex), Pandoc (limited) | 🟡 Proposed — **current implementation uses `printpdf`**; Typst is a future substitution behind `write_pdf` (§9.4) |
| **AD-007** | ~~Convention-based req-to-test mapping~~ | Superseded by AD-024; SRS-MAINT-002 requires explicit listing | See AD-024 | ❌ Rejected |
| **AD-008** | Docker containerization | Reproducibility, isolation | VM (heavy), native (env issues) | ✅ Accepted |
| **AD-009** | Exponential backoff for polling | Efficient, respectful of API | Fixed interval (wasteful) | ✅ Accepted |
| **AD-010** | PDF + JSON + terminal + traceability + QDP outputs | ECSS compliance + machine-readability + CI UX | — | ✅ Accepted — **HTML report deferred** (not implemented, not SRS-required; §9.1) |
| **AD-011** | Declarative test specs (metric constraints) | No boilerplate test code; requirements directly encode what must hold; uniform traceability | Manual test code per requirement (verbose, hard to maintain) | 🟡 Proposed |
| **AD-012** | Compile-time metric extensibility via `inventory` crate | Zero runtime overhead; full Rust type safety; no central registration file; single auditable binary per deployment | External process/JSON stdio (IPC overhead, OOP failure modes), dynamic `.so` (no stable ABI), WASM (complex toolchain) | ✅ Accepted |
| **AD-013** | Single job endpoint (`/api/v1/jobs`) | Eliminates implicit metric-routing convention; one job covers execution + instrumentation + coverage + static analysis; simpler orchestrator logic | Two endpoints `/runs` + `/analysis` (implemented in Phase 4, reverted - routing via name-prefix convention was fragile and implicitly coupled) | ✅ Accepted |
| **AD-014** | APCO Space Profile Use Cases as a closed enum | Use Cases (object_detection, image_classification, etc.) are defined by the APCO Space Profile (RD-5); both Reporter and Runner reason over the same closed set; no open-ended task dispatch needed | Open task-name dispatch (Runner accepts arbitrary strings — fragile, hard to validate, not traceable to APCO) | ✅ Accepted |
| **AD-015** | CPU utilisation normalised Runner-side | Runner knows hardware topology (core count, type); `cpu.avg_normalized_pct` is unambiguous across CPU/GPU/FPGA; hardware descriptor provides raw context | Normalise client-side (Runner would need to expose raw per-core counters, each hardware type differently structured) | ✅ Accepted |
| **AD-016** | ML accuracy metrics computed Reporter-side | Runner returns raw outputs; SQT Reporter computes accuracy/F1/IoU/L2 etc. against reference labels; keeps Runner hardware-agnostic and reference-data-free | Runner computes accuracy (requires Runner-side reference dataset per workload - complex, leaks qualification logic into the Runner) | ✅ Accepted |
| **AD-017** | Unified `outputs` field: one entry per input item, transferred once | Repetitions are for statistics only; outputs are per-input-item, not per-repetition. `result_consistent` captures determinism evidence. Applies uniformly to all Use Cases including pixel_segmentation. | Split `sample_output` / `all_outputs` per Use Case (added complexity, inconsistent semantics) | ✅ Accepted |
| **AD-018** | Model and artifacts configured on Runner at startup, not per job | No per-job model transfer; ONNX models can be hundreds of MB; re-sending per job is impractical. Runner is purpose-configured for one model version. To qualify a different version, restart Runner with updated config or CLI flags. | Per-job `model_zip` field (removed: inconsistent with session assembly model, impractical at production model sizes) | ✅ Accepted |
| **AD-019** | ML-L Provider artifacts pre-placed on Runner filesystem | No network transfer from Provider at qualification time; configurable paths in `sqt-runner.yaml`; simplest possible delivery model | Pull artifacts from Provider API at session start (adds Provider-side API requirement, network dependency, latency) | ✅ Accepted |
| **AD-020** | SQT Reporter fetches docs from Runner (not directly from ML-L Provider) | Runner is the single point of contact; Reporter need not know the Provider's delivery mechanism; consistent with Runner-as-data-source model | Reporter contacts Provider directly for docs (couples Reporter to Provider, breaks single-interface design) | ✅ Accepted |
| **AD-021** | 1:1 Reporter:Runner deployment model | Simplest correct model for the current use case; no concurrent client complexity; job queue is sufficient | Multi-client with session isolation (deferred; revisit if needed) | ✅ Accepted |
| **AD-022** | Runner executes one job at a time (initial); Reporter submits+polls concurrently | SRS-PERF-001 requires concurrent submission with wall-clock ≤ longest job + 10%; the SRS permits any Runner-side concurrency bound ≥ 1. Reporter submits all jobs and polls all IDs concurrently (SRS compliant); Runner queues and runs sequentially for v1 (valid: wall-clock bounded by queue depth per SRS-PERF-001). | Fully parallel Runner execution (deferred; adds resource management complexity on space hardware) | ✅ Accepted |
| **AD-023** | SQT Runner execution-backend abstraction (TAL) | Isolates target-specific code behind the object-safe `ExecutionBackend` trait, with an ergonomic `LifecycleBackend` (prepare/run/collect/teardown) that new backends implement; backend chosen by the `backend:` config key + `build_executor` factory; new targets (GR740/GR765) added without touching handlers, store, or the Reporter (§8.1) | Inline target-specific logic in the job handler (simpler now, unextensible later) | ✅ Accepted |
| **AD-024** | Explicit spec registration (no auto-discovery) | SRS-MAINT-002 forbids auto-discovery from filesystem state; explicit listing in config makes the active spec set deterministic, auditable, and source-controllable | Directory scanning (rejected: environment-dependent, invisible in source history, incompatible with reproducible qualification) | ✅ Accepted |
| **AD-025** | "Use Cases" per APCO Space Profile (RD-5), not "workload types" | SRS-IF-005 ties the job routing to APCO Space Profile Use Cases; aligning terminology ensures traceability to the standard | "Workload types" (rejected: generic term not traceable to APCO Space Profile) | ✅ Accepted |
| **AD-026** | Pre-flight mode (`--preflight`) | SRS-OPS-007 requires a mode that validates config + endpoint + auth without submitting jobs; shortens CI feedback loop on configuration changes | No pre-flight (operator must run a full qualification to detect config errors) | ✅ Accepted |
| **AD-027** | Exit code 2 for interrupted runs | SRS-OPS-008 requires SIGTERM exit distinguishable from completed-pass (0) and completed-fail (1); exit 2 = interrupted | Single non-zero code for all non-pass outcomes (rejected: loses diagnostic information) | ✅ Accepted |

### Trade-offs

#### Configuration: Code vs Files

**Decision**: Configuration files (YAML) with optional programmatic override

| Aspect | Files (YAML) | Code (Rust) |
|--------|--------------|-------------|
| **Flexibility** | ⭐⭐⭐⭐⭐ Easy to change without recompilation | ⭐⭐ Requires recompilation |
| **Type Safety** | ⭐⭐ Runtime validation | ⭐⭐⭐⭐⭐ Compile-time checks |
| **Version Control** | ⭐⭐⭐⭐⭐ Easy to track changes | ⭐⭐⭐⭐ Mixed with code |
| **User-Friendliness** | ⭐⭐⭐⭐⭐ Non-developers can edit | ⭐⭐ Requires Rust knowledge |

**Conclusion**: YAML primary, with Rust API for advanced users

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

2. **Space Profile Test Definition**
   - **Question**: What exactly are the Space Profile tests? What functional properties do they verify?
   - **Action**: Obtain the Space Profile test specification from ESA/Klepsydra; Ateleris implements it in the Runner

3. **GR740/GR765 Connection Protocol**
   - **Question**: What protocol connects the Runner to the GR740/GR765? (JTAG, GDB remote stub, UART serial, Ethernet?)
   - **Concern**: The TAL design (Section 8.1) accommodates this, but the concrete implementation is blocked on the answer
   - **Action**: Define with Klepsydra/customer when GR target support is scoped

4. **Input/Output Data Format**
   - **Question**: What is the format of the reference input/output datasets? How are they structured on the Runner filesystem?
   - **Action**: Align with Klepsydra on dataset conventions; document in Runner config schema

### 13.2 SQT Reporter - Technical Questions

5. **Strictdoc Parser Implementation**
   - **Question**: Should we implement a full Strictdoc parser in Rust or invoke the Python CLI tool?
   - **Options**:
     - A) Write custom Rust parser (full control, no dependency)
     - B) Use Strictdoc CLI tool (mature, but Python dependency)
   - **Recommendation**: Start with Option B, migrate to A if needed

6. **REST API Authentication**
   - **Question**: What authentication method should the Reporter ↔ Runner API use?
   - **Options**: Bearer token (simple), API key (simpler), mTLS (most secure)
   - **Recommendation**: Bearer token for development, mTLS for production

7. **Report Template Engine**
   - **Question**: Typst vs LaTeX for PDF generation?
   - **Decision needed**: Verify Typst maturity for ECSS report compliance
   - **Fallback**: LaTeX with Tectonic

### 13.3 Clarifications Needed from ML-L Provider (Klepsydra)

- [ ] Delivery format for inference engine, model, and main program (ZIP structure? directory layout?)
- [ ] Definition of "Prepare AI System" - what does linking/assembling require from the Runner?
- [ ] Sample Strictdoc requirements file for the ECSS documentation baseline
- [ ] Example unit test results and validation test results (format, content)
- [ ] Connection protocol to GR740/GR765 (for future Runner target implementation)

---

## 14. Next Steps

### 14.1 Immediate Actions

1. **Align on ML-L Provider delivery format**
   - Obtain a sample artifact bundle from Klepsydra (inference engine + model + main program)
   - Understand the "Prepare AI System" step concretely
   - Document in Runner config schema and TAL design

2. **Obtain Space Profile Test Specification**
   - ESA and Klepsydra define the Space Profile; Ateleris implements it in the Runner
   - Obtain the formal Space Profile test specification from Klepsydra/ESA
   - Agree on pass/fail criteria and metric mapping for the Runner's job response schema

3. **Validate REST API contract**
   - Share `job-api.openapi.yaml` with Klepsydra for review
   - Confirm document format for `GET /api/v1/docs`

### 14.2 Phase 1: SQT Runner - x86_64 Baseline (Rust)

4. **Runner: REST API server scaffold**
   - Implement `POST /api/v1/jobs` and `GET /api/v1/jobs/{id}` (async, job queue)
   - Implement `GET /api/v1/docs` serving configured document paths

5. **Runner: x86\_64 target**
   - Implement `Target` trait for native Linux execution
   - AI system assembly for x86\_64 (based on agreed delivery format)
   - Basic Performance test execution (timing, memory via `/usr/bin/time` / valgrind)

6. **Runner: Space Profile tests (first set)**
   - Implement first Space Profile tests for x86\_64
   - Map results to job metric schema

### 14.3 Phase 2: SQT Reporter - Core Functionality

7. **Reporter: baseline check**
   - Implement `GET /api/v1/docs` client
   - Parse ECSS docs and static test results
   - Implement compliance gate

8. **Reporter: requirements management**
   - Strictdoc parser (CLI wrapper or native Rust)
   - Traceability matrix builder

9. **Reporter: orchestrator**
   - Job submission and polling
   - Metric constraint evaluation
   - Evidence collector

### 14.4 Phase 3: Integration and Report Generation

10. **End-to-end qualification session**
    - Connect Reporter → Runner on x86\_64
    - Full qualification workflow with real ML-L artifacts

11. **Report generation**
    - System Test Report (PDF + HTML)
    - QDP
    - Traceability matrix

12. **CI/CD integration**
    - GitHub Actions workflow
    - Docker images for Runner and Reporter

### 14.5 Phase 4: GR740/GR765 Target (Future)

13. **Define connection protocol** (blocked on hardware availability and Klepsydra alignment)
14. **Implement GR740 target** in the Runner TAL
15. **Validate end-to-end** on customer hardware

---

## Appendices

### Appendix A: Glossary

| Term | Definition |
|------|------------|
| **ECSS** | European Cooperation for Space Standardization |
| **ML-L** | Machine Learning Library (inference engine) |
| **SQT** | Software Qualification Toolkit (collectively: SQT Runner + SQT Reporter) |
| **SQT Runner** | Ateleris-developed REST API server that executes tests on the target hardware |
| **SQT Reporter** | Ateleris-developed CLI that orchestrates qualification sessions and generates reports |
| **TAL** | Target Abstraction Layer - pluggable backend in the SQT Runner for different execution targets |
| **QDP** | Quality Data Package - the ECSS-compliant qualification output artifact |
| **Space Profile Tests** | functional tests for space AI systems, defined by ESA and Klepsydra, executed by the SQT Runner |
| **PBF** | Performance Benchmark Framework (Klepsydra's internal tool - not used by SQT Runner) |
| **TRL** | Technology Readiness Level (1-9 scale) |
| **Strictdoc** | Requirements management tool (text-based format) |
| **ONNX** | Open Neural Network Exchange (model format) |
| **GR740** | Quad-core LEON4 space-grade processor (Cobham Gaisler) |
| **GR765** | Octa-core LEON5 space-grade processor (Cobham Gaisler) |
| **RTEMS** | Real-Time Executive for Multiprocessor Systems (RTOS) |

### Appendix B: References

1. ECSS-E-ST-40C: Software Engineering Standards
2. ECSS-Q-ST-80C: Software Product Assurance
3. Q-SMART Project Proposal (KPSR-2024-13-Vol1)
4. Strictdoc Documentation: https://strictdoc.readthedocs.io/
5. RTEMS Quality Delivery Process: https://docs.rtems.org/


# Notes
Strictdoc supports custom grammars / fields -> We could inject our definitions there

See https://strictdoc.readthedocs.io/en/stable/?a=SDOC-SRS-21 and https://strictdoc.readthedocs.io/en/stable/?a=SDOC-SSS-62 

However, do we really want do provide a set of things that can be done? Or do we want to define that in code?