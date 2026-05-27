# Q-SMART SQT Architecture Overview

**Project**: Qualified Software Machine Learning Toolkit for Space Hardware
**Components**: SQT Runner + SQT Reporter (both Ateleris responsibility)
**Technology**: Rust applications
**Version**: 1.1
**Date**: 2026-05-27

---

## Table of Contents

- [1. Executive Summary](#1-executive-summary)
- [2. System Architecture](#2-system-architecture)
- [3. Configuration Management](#3-configuration-management)
- [4. Requirements Management](#4-requirements-management)
  - [4.5 Declarative Test Specifications](#45-declarative-test-specifications)
  - [4.6 Compliant Requirements (Non-Testable)](#46-compliant-requirements-non-testable)
- [5. REST API Interface (SQT Runner)](#5-rest-api-interface-sqt-runner)
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
- **Submits test jobs** to the SQT Runner via REST API and polls for results
- **Evaluates metric constraints** against returned results (declarative test specifications)
- **Generates ECSS-compliant reports** (PDF, HTML) and the QDP (Quality Data Package) with full requirements traceability

### Key Principles

1. **Separation of concerns**: The SQT Runner handles test execution and artifact serving; the SQT Reporter handles orchestration, requirements checking, and reporting
2. **Deployment flexibility**: The SQT Runner deploys on the customer's hardware (where access to the target systems exists); the SQT Reporter can run anywhere
3. **Target extensibility**: The Runner's target abstraction layer allows adding new execution targets (GR740, GR765) without changing the Reporter or the REST API contract
4. **Configuration-driven**: All aspects configurable via YAML
5. **Standards-compliant**: ECSS E-ST-40C and Q-ST-80C alignment
6. **Evidence-based**: Comprehensive traceability from requirements to test results

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
║  │  POST /api/v1/jobs  → enqueue    │   │  │  functional tests)   │    │     ║
║  │  GET  /api/v1/jobs/{id} → poll   │   │  └─────────────────────┘    │     ║
║  │  GET  /api/v1/docs  → serve docs │   │  ┌─────────────────────┐    │     ║
║  └──────────────────────────────────┘   │  │ Performance Tests    │    │     ║
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
1. GET /api/v1/docs
                            ◄── ECSS baseline + static test results

2. Parse docs; check ECSS compliance and static test results
   Gate: if not compliant → halt with error report

3. Parse Strictdoc requirements from fetched docs
   Identify testable requirements (VERIFICATION: Test)

4. For each testable requirement:
   POST /api/v1/jobs  ─────────────────────────────────────────────►
                            ◄── 202 Accepted { job_id, queue_position }
   (poll with exponential backoff)
   GET /api/v1/jobs/{id}  ─────────────────────────────────────────►
                            ◄── status: "accepted" (queue_position: N)
                            ◄── status: "running"
                            ◄── status: "complete" | "failed"
                                 + metrics, outputs, hardware descriptor

5. Evaluate metric constraints against results

6. Generate System Test Report + QDP
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

```yaml
# Project Metadata
project:
  name: "Klepsydra AI ML-L Qualification"
  version: "1.0.0"
  ecss_level: "D"                    # A, B, C, or D
  target_trl: 6                      # Technology Readiness Level

# SQT Runner connection
runner:
  url: "http://runner-host:8080"
  auth:
    type: "bearer"                   # bearer, none
    token_env: "SQT_RUNNER_TOKEN"    # environment variable holding the token
  poll_interval_initial_secs: 1
  poll_interval_max_secs: 30
  job_timeout_secs: 3600

# Test specifications (declarative .test.yaml files)
test_specs:
  directory: "./specs"               # scanned recursively for *.test.yaml files

# Report Generation
reports:
  output_dir: "./reports"
  timestamp_format: "%Y-%m-%d_%H-%M-%S"

  formats:
    pdf:
      enabled: true
      engine: "typst"                # typst, pdflatex, xelatex

    html:
      enabled: true

    json:
      enabled: true

# CI/CD Integration
ci_integration:
  provider: "github_actions"         # github_actions, gitlab_ci, jenkins, none

  artifacts:
    upload: true
    retention_days: 90
    paths:
      - "./reports/**"

# Logging
logging:
  level: "info"                      # debug, info, warn, error
  output: "stdout"                   # stdout, file, both
  file_path: "./logs/sqt-reporter.log"
  format: "json"
```

#### SQT Runner Configuration: `sqt-runner.yaml`

```yaml
# Execution target
runner:
  target: x86_64-linux               # x86_64-linux | gr740 (planned) | gr765 (planned)

  # Paths to ML-L Provider artifacts (pre-placed on filesystem before session)
  artifacts:
    inference_engine: ./artifacts/klepsydra-engine
    trained_model:    ./artifacts/model.onnx
    main_program:     ./artifacts/main
    input_data:       ./artifacts/input/
    output_reference: ./artifacts/reference_output/

  # Qualification documents served via GET /api/v1/docs
  docs:
    ecss_baseline:           ./docs/ecss-baseline.sdoc
    unit_test_results:       ./docs/unit-test-results.sdoc
    validation_test_results: ./docs/validation-test-results.sdoc

  # Tool configuration (all runner-side)
  tools:
    valgrind:       /usr/bin/valgrind
    lcov:           /usr/bin/lcov
    sonar_scanner:  /usr/local/bin/sonar-scanner
    sonar_url:      "http://localhost:9000"
    sonar_project:  "ml-l-qualification"

# REST API server settings
server:
  host: "0.0.0.0"
  port: 8080
  auth:
    type: "bearer"
    token_env: "SQT_RUNNER_TOKEN"

# Logging
logging:
  level: "info"
  output: "stdout"
  file_path: "./logs/sqt-runner.log"
  format: "json"
```

The model and artifacts are configured at Runner startup, not per job. To qualify a different model version, restart the Runner with updated artifact paths (or override them via CLI flags, e.g. `--trained-model ./artifacts/model-v2.onnx`).

### 3.2 Configuration Validation

Both components validate their configuration files on startup:

```rust
// Reporter: validate sqt-reporter.yaml
fn validate_reporter_config(config: &ReporterConfig) -> Result<(), ValidationError> {
    if config.project.name.is_empty() {
        return Err(ValidationError::MissingField("project.name"));
    }
    if !Path::new(&config.test_specs.directory).exists() {
        return Err(ValidationError::FileNotFound(config.test_specs.directory));
    }
    check_api_connectivity(&config.runner.url)?;
    Ok(())
}

// Runner: validate sqt-runner.yaml
fn validate_runner_config(config: &RunnerConfig) -> Result<(), ValidationError> {
    for path in [&config.runner.artifacts.inference_engine,
                 &config.runner.artifacts.trained_model,
                 &config.runner.artifacts.main_program] {
        if !Path::new(path).exists() {
            return Err(ValidationError::FileNotFound(path));
        }
    }
    for doc in [&config.runner.docs.ecss_baseline,
                &config.runner.docs.unit_test_results,
                &config.runner.docs.validation_test_results] {
        if !Path::new(doc).exists() {
            return Err(ValidationError::FileNotFound(doc));
        }
    }
    Ok(())
}
```

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

Two approaches:

#### Approach 1: Convention-based (Automatic)
```
Requirements UID pattern → Test file pattern

REQ-ML-L-001  →  tests/unit/test_multicore.cpp
REQ-ML-L-002  →  tests/unit/test_onnx_loader.cpp
REQ-ML-L-003  →  tests/performance/test_inference_latency.cpp
```

#### Approach 2: Explicit Mapping (Configuration)
```yaml
# test-mappings.yaml
test_mappings:
  - requirement_uid: "REQ-ML-L-001"
    test_cases:
      - "tests/unit/test_multicore.cpp::TestMulticoreInference::test_parallel_execution"
      - "tests/integration/test_gr740_multicore.cpp::TestGR740Multicore::test_quad_core"

  - requirement_uid: "REQ-ML-L-002"
    test_cases:
      - "tests/unit/test_onnx_loader.cpp::TestONNXLoader::test_load_resnet50"
      - "tests/unit/test_onnx_loader.cpp::TestONNXLoader::test_onnx_version_support"

  - requirement_uid: "REQ-ML-L-003"
    hardware_tests:
      - platform: "gr740-eval"
        rest_endpoint: "/api/v1/jobs"
        config:
          model: "resnet50"
          input_size: [224, 224]
          iterations: 100
```

### 4.4 Traceability Matrix Generation

Generate comprehensive traceability matrix:

```
┌──────────────┬─────────────────────────────┬─────────────┬────────────┐
│ Requirement  │ Test Cases                  │ Status      │ Coverage   │
├──────────────┼─────────────────────────────┼─────────────┼────────────┤
│ REQ-ML-L-001 │ • test_multicore.cpp::L45   │ ✓ Passed    │ 100%       │
│              │ • test_gr740_multicore::L12 │ ✓ Passed    │            │
├──────────────┼─────────────────────────────┼─────────────┼────────────┤
│ REQ-ML-L-002 │ • test_onnx_loader.cpp::L23 │ ✓ Passed    │ 95%        │
│              │ • test_onnx_loader.cpp::L67 │ ✓ Passed    │            │
├──────────────┼─────────────────────────────┼─────────────┼────────────┤
│ REQ-ML-L-003 │ • Hardware Test (GR740)     │ ✓ Passed    │ N/A        │
│              │   Job ID: job-12345         │ (87ms)      │            │
├──────────────┼─────────────────────────────┼─────────────┼────────────┤
│ REQ-ML-L-004 │ • test_memory.cpp::L89      │ ✓ Passed    │ 100%       │
│              │ • Hardware Test (Memory)    │ ✓ Passed    │            │
│              │   Job ID: job-12346         │ (1.8MB)     │            │
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

This is similar to property-based testing and contract testing. Strictdoc's custom grammar feature (see Notes section) can be used to embed these specs directly in `.sdoc` files, or they can live in sidecar YAML files alongside requirements.

#### 4.5.2 Sidecar YAML Format (Recommended)

Test specs live in `tests/<req-uid>.test.yaml` alongside the requirements:

```yaml
# tests/REQ-ML-L-003.test.yaml
requirement_uid: REQ-ML-L-003
test_type: performance
target:
  binary: ml_l_inference
  platform: gr740
  input: datasets/reference_mnist.bin

constraints:
  - metric: time.execution
    operator: <=
    value: 100ms

  - metric: accuracy
    operator: ">="
    value: 95%

  - metric: memory.peak_rss_kb
    operator: <=
    value: 512

  - metric: memory.leaks_bytes
    operator: ==
    value: 0
```

The syntax is intentionally uniform: the user specifies `metric`, `operator`, and `value`. Whether the metric is built-in or provided by a plugin makes no difference to the user.

#### 4.5.3 Core Metric Catalogue

The following built-in metrics ship with SQT Reporter. Each is implemented as an internal metric collector (same interface as plugins, but compiled in):

These names correspond directly to keys in the `JobStatusResponse.metrics` object (see `job-api.openapi.yaml`). The Reporter evaluates constraints by looking up each key in the returned metrics map.

**Memory Metrics**

| Metric key | Unit | Description |
|------------|------|-------------|
| `memory.peak_rss_kb` | KB | Peak resident set size |
| `memory.avg_rss_kb` | KB | Average resident set size across repetitions |
| `memory.leaks_bytes` | bytes | Leaked bytes (Valgrind / ASan); 0 = no leaks |

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

**Phase 1 MVP** targets the most critical metrics: `memory.peak_rss_kb`, `memory.leaks_bytes`, `time.avg_ms`, `time.latency_p95_ms`, `return_code`, `static.critical_issues`, `coverage.line_pct`, `cpu.avg_normalized_pct`.

#### 4.5.4 Execution Flow

```rust
// Pseudo-code: API-driven orchestration

// Collect all required task names from specs
let tasks: Vec<TaskSpec> = specs.iter()
    .flat_map(|s| s.constraints.iter())
    .map(|c| TaskSpec { name: c.metric_name.clone(), config: c.config.clone() })
    .collect();

// Submit ONE job with all tasks
let job_id = api_client.submit_job(&target, &tasks).await?;

// Poll until complete (exponential backoff)
let job = api_client.wait_for_completion(&job_id).await?;

// Evaluate constraints against task results
for spec in &specs {
    for constraint in &spec.constraints {
        let task_result = job.tasks.get(&constraint.metric_name)?;
        let actual = MetricValue::new(task_result.value, &task_result.unit);
        let outcome = evaluate(&actual, constraint);
        results.push(outcome);
    }
}
```

The orchestrator never calls metric collectors directly - all tool execution happens server-side. The `MetricCollector` trait (and Phase 2 built-in collectors) remains valid as the implementation layer for the local dev API server. See Section 10 (Plugin Architecture) for how custom metrics are registered and invoked.

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

The full interface contract is defined in **`job-api.openapi.yaml`** at the root of the
repository. The spec is implemented by the **SQT Runner**. The **SQT Reporter** is the
API client. During development, the existing local dev-server stub can stand in for the
Runner until the real implementation is ready.

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
**not** transmitted per job. The client (SQT Reporter) specifies only what workload
to run and how; the Runner returns everything it can measure.

A single job type covers all qualification activities: execution runs, instrumented
runs (valgrind/asan/gprof), coverage collection, and static analysis.

### 5.2 Predefined Workload Types

The set of workload types is defined in SQT Reporter and known to both sides of the
API. The server maps each workload type to the appropriate binary invocation,
input handling, and output interpretation.

| Workload | Domain | Typical Input | Typical Output |
|---|---|---|---|
| `object_detection` | Computer vision | Images | Bounding boxes + class labels |
| `image_classification` | Computer vision | Images | Class label + confidence score |
| `pixel_segmentation` | Computer vision | Images | Per-pixel class mask (large output) |
| `classification` | Tabular ML | Feature vectors | Class label |
| `regression` | Tabular ML | Feature vectors | Scalar prediction |
| `time_series_forecasting` | Sequential ML | Time series sequences | Forecast sequence |

**Default session**: when no specific workload is requested, SQT Reporter submits jobs for
all supported workload types. Jobs queue up in the Runner and execute sequentially;
total wall-clock time is the sum of all job durations.

### 5.3 Measurement Modes

Each job runs in one of two modes, set by the `mode` field:

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

Two endpoints; both follow the same asynchronous pattern:
submit → receive a job ID (202 Accepted) → poll until `status` is terminal.
Results are embedded directly in the GET response; there is no separate `/results` endpoint.

#### 5.5.1 Submit Job

**Endpoint**: `POST /api/v1/jobs`

**Request Body**:
```json
{
  "model": "image_classification",
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
The Reporter does not transmit the model - it only specifies the workload type and
measurement parameters.

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
  "model": "image_classification",
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
    "message": "unknown model type: 'depth_estimation'",
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

### 5.9 Polling Strategy

SQT Reporter implements exponential backoff for all polling. Results are embedded
in the GET response - no separate fetch is needed:

```
initial delay: 1 s
backoff factor: x2 per poll
maximum delay: 30 s
```

Jobs for a qualification session are submitted by the Reporter and queue up in
the Runner. The Runner executes one job at a time. The Reporter polls each
outstanding job ID sequentially (or interleaved with backoff); total wall-clock
time is the sum of all job durations.

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
   - Contains `Test` → schedule for normal constraint evaluation (requires a `.test.yaml`).
   - No `Test` → record as `compliant` with `evidence_source: server_declaration`.
4. Include all requirements in the report traceability matrix, grouped by verification path.

See **Section 4.6** for the full rationale and report output format.

---

## 6. Scope Definition

### 6.1 SQT Runner Scope (Ateleris)

#### ✅ IN SCOPE

1. **REST API Server**
   - Implement `job-api.openapi.yaml` endpoints (`POST /api/v1/jobs`, `GET /api/v1/jobs/{id}`, `GET /api/v1/docs`)
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

#### ✅ IN SCOPE

1. **Configuration Management**
   - Parse YAML configuration files
   - Validate configuration correctness
   - Support configuration profiles (dev, ci, production)

2. **Document Baseline Check**
   - Fetch documents from the SQT Runner via `GET /api/v1/docs`
   - Parse ECSS documentation baseline and static test results
   - Gate the qualification session: halt if compliance criteria are not met

3. **Requirements Management**
   - Parse Strictdoc requirements from fetched documents
   - Extract requirements metadata and verification methods
   - Build requirements database and traceability matrix

4. **REST API Client (to SQT Runner)**
   - Submit test jobs via `POST /api/v1/jobs`
   - Poll for job status via `GET /api/v1/jobs/{id}`
   - Extract metric values from results

5. **Metric Constraint Evaluation**
   - Evaluate declarative test specs (`.test.yaml`) against job result metrics
   - Map null metrics to failed constraints

6. **Evidence Collection**
   - Aggregate all job results and static documents
   - Build requirements traceability matrix
   - Organize qualification artifacts

7. **Report Generation**
   - Generate ECSS-compliant PDF reports
   - Create HTML developer reports
   - Produce QDP (Quality Data Package)
   - Include traceability matrices

8. **CI/CD Integration**
   - Provide CLI interface
   - Return appropriate exit codes
   - Support common CI systems (GitHub Actions, GitLab CI)

#### ❌ OUT OF SCOPE for SQT Reporter

1. **Test execution** - the Runner handles all test execution
2. **Hardware access** - the Runner abstracts all target hardware
3. **ML-L artifact management** - the Reporter consumes artifacts already served by the Runner
4. **REST API server** - the Reporter is a client only

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

"Execute test on x86_64"        → Assemble AI system
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
│ 1. Fetch Document Baseline  [SQT Reporter → SQT Runner]          │
│                                                                  │
│    GET /api/v1/docs                                              │
│    ← ECSS documentation baseline (.sdoc)                        │
│    ← Unit test results (static, .sdoc)                          │
│    ← Validation test results (static, .sdoc)                    │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 2. Baseline Compliance Check  [SQT Reporter]                     │
│                                                                  │
│    Parse fetched documents; verify required ECSS artefacts       │
│    are present and structurally valid.                           │
│                                                                  │
│    Gate: if not compliant → halt, emit error report              │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 3. Parse Requirements  [SQT Reporter]                            │
│                                                                  │
│    Parse Strictdoc from fetched documents.                       │
│    For each [REQUIREMENT]:                                       │
│      • VERIFICATION includes Test  → must have .test.yaml        │
│      • VERIFICATION is non-Test    → mark compliant by server    │
│        (Inspection / Review / Analysis)        declaration       │
│                                                                  │
│    REQ-ML-L-001  VERIFICATION: Test        → needs .test.yaml    │
│    REQ-ML-L-NFR-001  VERIFICATION: Review  → compliant           │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 4. Load Test Specs  [SQT Reporter]                               │
│                                                                  │
│    Scan specs/ directory for *.test.yaml files.                  │
│    Match each spec to a requirement by uid.                      │
│    Unmatched testable requirements → UNCOVERED → halt            │
│                                                                  │
│    # specs/REQ-ML-L-001.test.yaml                                │
│    requirement_uid: REQ-ML-L-001                                 │
│    constraints:                                                  │
│      - metric: return_code    op: eq   value: 0                  │
│      - metric: time.avg_ms    op: lte  value: 100                │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 5. Submit Jobs  [SQT Reporter → SQT Runner]                      │
│                                                                  │
│    For each test spec, POST /api/v1/jobs.                        │
│    Jobs are queued in the Runner; Reporter receives a job_id     │
│    immediately (status: accepted, queue_position: N).            │
│                                                                  │
│    POST /api/v1/jobs  { model: "image_classification",          │
│                         repetitions: 10, mode: "latency" }      │
│    ← { job_id: "job-abc123", status: "accepted",                │
│        queue_position: 1 }                                       │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 6. Execute & Poll  [SQT Runner + SQT Reporter polling]           │
│                                                                  │
│  Runner (sequential, one job at a time):                         │
│    • Assemble AI system (inference engine + model + main prog)   │
│    • Execute Space Profile tests (ESA/Klepsydra-defined)         │
│    • Execute Performance tests (timing, memory, throughput)      │
│    • Collect all metrics; update job status → complete           │
│                                                                  │
│  Reporter polls with exponential backoff:                        │
│    GET /api/v1/jobs/{id}                                         │
│    ← status: "accepted" (queue_position: 1)                      │
│    ← status: "running"                                           │
│    ← status: "complete"  { metrics: { ... }, output: { ... } }  │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ 7. Evaluate Metric Constraints  [SQT Reporter]                   │
│                                                                  │
│    For each spec constraint, look up the metric in job result.   │
│    null metric → FAILED (tool unavailable counts as failure).    │
│                                                                  │
│    REQ-ML-L-001:                                                 │
│      return_code = 0   == 0  → ✓ PASS                           │
│      time.avg_ms = 87.3 <= 100 → ✓ PASS                        │
│    REQ-ML-L-002:                                                 │
│      return_code = 1   == 0  → ✗ FAIL                           │
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
│    • Format job metrics + traceability into report structures    │
│    • Render ECSS-compliant PDF (Typst) and HTML                  │
│    • Produce QDP artefact                                        │
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
constraints:
  - metric: time.avg_ms
    op: lte
    value: 100
  - metric: return_code
    op: eq
    value: 0
```

**Execution**:
1. Reporter submits `POST /api/v1/jobs` - Runner queues it, returns `job-12345`
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

The SQT Runner is designed from the start to support multiple execution targets. The **Target Abstraction Layer** (TAL) isolates all target-specific logic so that new targets can be added without touching the REST API server, test execution engine, or the SQT Reporter.

### 8.1 Target Abstraction Layer Design

Each target implements a common `Target` trait:

```rust
pub trait Target: Send + Sync {
    /// Unique identifier used in Runner config (e.g. "x86_64-linux", "gr740")
    fn id(&self) -> &str;

    /// Assemble the AI system (inference engine + model + main program)
    /// into a state ready for test execution.
    fn prepare(&self, config: &AiSystemConfig) -> anyhow::Result<PreparedSystem>;

    /// Execute the given test run and return collected metrics.
    fn execute(&self, system: &PreparedSystem, job: &JobRequest) -> anyhow::Result<JobMetrics>;

    /// Clean up any resources allocated during prepare/execute.
    fn teardown(&self, system: PreparedSystem) -> anyhow::Result<()>;
}
```

The active target is selected by a single config key; no code changes are needed to switch targets.

```yaml
# sqt-runner.yaml
runner:
  target: x86_64-linux        # or: gr740, gr765

  artifacts:
    inference_engine: ./artifacts/klepsydra-engine
    trained_model:    ./artifacts/model.onnx
    main_program:     ./artifacts/main
    input_data:       ./artifacts/input/
    output_reference: ./artifacts/reference_output/

  docs:
    ecss_baseline:          ./docs/ecss-baseline.sdoc
    unit_test_results:      ./docs/unit-test-results.sdoc
    validation_test_results: ./docs/validation-test-results.sdoc
```

### 8.2 AI System Assembly

Before test execution the Runner assembles the AI system. The exact mechanism depends on the ML-L Provider's delivery format, which is not yet fully defined. The assembly step may involve:

- Copying artifacts into a working directory
- Setting environment variables (paths to inference engine, model file, etc.)
- Linking or composing components into a single runnable process
- Compiling a thin wrapper that binds the engine and model (if required by the delivery format)

Assembly is performed once per qualification session (not per job) and cached for subsequent jobs. The `prepare()` method on the target trait encapsulates this step.

### 8.3 Supported Targets

| Target | Status | Execution Method | Notes |
|--------|--------|-----------------|-------|
| `x86_64-linux` | **In Scope (v1)** | Native process execution | Development and CI baseline |
| `gr740` | **Planned - Out of Scope for v1** | Remote attachment (connection protocol TBD: serial / GDB server / network) | Customer provides the hardware; Runner connects to it |
| `gr765` | **Planned - Out of Scope for v1** | Remote attachment (connection protocol TBD) | Same model as GR740 target |

**Design constraint for GR740/GR765**: Ateleris does not have access to these processors. The Runner therefore operates in a "remote execution" mode: it transfers the assembled AI system to the customer-provided board over an attached link, triggers execution, and collects results back. The exact connection protocol (JTAG, GDB remote stub, serial, network) is TBD and will be defined once a concrete delivery from the ML-L Provider is available. The target abstraction ensures this can be added later without API changes.

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

Both test types use the same job submission flow; the `model` (workload type) field in the job request determines which test suite is exercised.

### 8.5 Docker Containerization

The SQT Runner should support Docker for reproducible environments on x86\_64:

```yaml
docker:
  enabled: true
  image: "ateleris/sqt-runner:latest"
  volumes:
    - "./artifacts:/runner/artifacts"
    - "./docs:/runner/docs"
  network: "host"   # Reporter and Runner on the same host during development
```

**Benefits**:
- Reproducible test environment
- Toolchain isolation (valgrind, lcov, etc.)
- Easy CI/CD integration for x86\_64 tests

---

## 9. Report Generation

### 9.1 Report Types

#### PDF Report (ECSS-Compliant)
- **Target audience**: ESA, customers, certification
- **Format**: Professional, structured, traceable
- **Template**: LaTeX or Typst
- **Sections**:
  1. Executive Summary
  2. Project Overview
  3. Requirements Traceability Matrix
  4. Test Results Summary
  5. Detailed Test Results
  6. Coverage Analysis
  7. Static Analysis Results
  8. Performance Metrics
  9. Appendices (raw data, artifacts)

#### HTML Report (Developer-Friendly)
- **Target audience**: Developers, CI dashboards
- **Format**: Interactive, searchable
- **Features**:
  - Collapsible sections
  - Search functionality
  - Links to source code
  - Trend graphs

#### JSON Report (Machine-Readable)
- **Target audience**: Automation, other tools
- **Format**: Structured JSON
- **Use case**: CI/CD integration, metrics tracking

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
═══════════════════════════════════════════════════════════════

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

### 9.4 Report Templates

#### PDF Template (Typst)
```typst
// ecss-report-template.typ
#set document(title: "ML-L Qualification Report", author: "Ateleris GmbH")
#set page(numbering: "1", number-align: center)

#align(center)[
  #text(size: 24pt, weight: "bold")[
    Software Qualification Report
  ]
  #v(1em)
  #text(size: 18pt)[
    #project_name
  ]
  #v(2em)
  #text(size: 12pt)[
    Version: #version \
    Date: #datetime.today().display()
  ]
]

#pagebreak()

= Executive Summary

#executive_summary_content

= Requirements Traceability Matrix

#table(
  columns: 4,
  [Requirement], [Test Cases], [Status], [Coverage],
  ..#traceability_rows
)

// ... more sections ...
```

### 9.5 Report Generation Implementation

```rust
pub struct ReportGenerator {
    config: ReportConfig,
    template_engine: TemplateEngine,
}

impl ReportGenerator {
    pub fn generate(&self, evidence: &Evidence) -> Result<Report> {
        // 1. Prepare data model
        let data = ReportData::from_evidence(evidence);

        // 2. Generate sections
        let sections = vec![
            self.generate_executive_summary(&data)?,
            self.generate_test_results(&data)?,
            self.generate_traceability_matrix(&data)?,
            self.generate_coverage_report(&data)?,
            self.generate_static_analysis(&data)?,
            self.generate_performance_metrics(&data)?,
        ];

        // 3. Render to formats
        let pdf = self.render_pdf(&sections)?;
        let html = self.render_html(&sections)?;
        let json = self.render_json(&data)?;

        Ok(Report { pdf, html, json })
    }
}
```

---

## 10. Metric Extensibility

### 10.0 Where Metrics Live

**All metric collection (tool execution) happens in the SQT Runner.** The Runner runs valgrind, lcov, sonar-scanner, and any other analysis tools on the target system and returns results as `JobMetrics` fields in the job response. The tools are configured in `sqt-runner.yaml` and run server-side; the Reporter never invokes them directly.

**The SQT Reporter is responsible for consuming and formatting those metrics** for reports and constraint evaluation. On the Reporter side, the `MetricCollector` trait and the `inventory`-based registry described below are the extensibility mechanism for the **Runner** - not the Reporter. When someone adds a new tool to the Runner (e.g. a custom profiler), they add a new collector in the Runner's source tree. The Reporter side only needs a corresponding field in the `.test.yaml` spec schema and a label in the report template.

To summarise:
```
SQT Runner                           SQT Reporter
─────────────────────────────────────────────────────
MetricCollector trait                Job response deserialization
inventory-based registry             Constraint evaluation (metric vs. spec)
Tool execution (valgrind, lcov, …)   Report formatting (metric → PDF/HTML row)
Metric collection + JobMetrics fill  Traceability mapping
```

### Extensibility Decision: Compile-Time Only (Runner)

The SQT Runner ships with a set of built-in metric collectors. When a project needs a metric that is not built-in, users extend the Runner by adding Rust code directly to its source tree, then recompiling. There is no runtime plugin protocol, no dynamic loading, and no external process bridge.

**Architectural decision: compile-time extensibility only**

| Approach | Chosen? | Reason |
|---|---|---|
| Dynamic `.so` / `.dll` | ❌ | No stable Rust ABI; version coupling; crash risk |
| External process (stdio/JSON) | ❌ | Runtime overhead; IPC complexity; out-of-process failure modes |
| WebAssembly (WASM) | ❌ | Complex toolchain for what is fundamentally a build-time concern |
| Embedded scripting (Rhai/Lua) | ❌ | Limited capability for real measurement work |
| **Compile-time Rust (`inventory` crate)** | ✅ | Zero runtime overhead; full Rust type safety; no central registration file; trivial to add, trivial to test |

The intended extensibility model is: **clone the Runner repository, add a Rust file, compile**. The Runner is a qualification tool used by small, technically capable teams. Requiring a recompile is not a constraint - it is a feature, because it means every deployment is a single, auditable, statically-linked binary with a fixed set of collectors.

### 10.1 The `MetricCollector` Trait

Every collector - built-in or user-added - implements the same trait:

```rust
// src/metrics/mod.rs

pub trait MetricCollector: Send + Sync {
    /// Unique metric name, e.g. "memory.peak_rss" or "custom.model_drift"
    fn name(&self) -> &str;

    /// Execute the measurement and return the value + evidence artifacts.
    /// `config` carries the optional per-constraint YAML block from the test spec;
    /// collectors that need no configuration may ignore it.
    fn collect(
        &self,
        target: &Target,
        config: &serde_yaml::Value,
    ) -> anyhow::Result<MetricValue>;
}
```

`MetricValue` carries the measured `f64`, its unit string, and an evidence map (label → file path or inline text) that is written into the QDP report.

### 10.2 Auto-Registration via `inventory`

The [`inventory`](https://crates.io/crates/inventory) crate uses linker sections to collect distributed static items at link time. Each collector file self-registers by calling one macro - no central list is ever maintained.

**Declaration (once, in `src/metrics/mod.rs`):**

```rust
pub struct CollectorFactory {
    pub name:   &'static str,
    pub create: fn() -> Box<dyn MetricCollector>,
}

inventory::collect!(CollectorFactory);
```

**Per-collector self-registration (bottom of each collector file):**

```rust
// src/metrics/builtin/memory_rss.rs

pub struct MemoryPeakRssCollector;

impl MetricCollector for MemoryPeakRssCollector {
    fn name(&self) -> &str { "memory.peak_rss" }
    fn collect(&self, target: &Target, _config: &serde_yaml::Value) -> anyhow::Result<MetricValue> {
        // spawn /usr/bin/time -v, parse "Maximum resident set size"
    }
}

inventory::submit! {
    CollectorFactory { name: "memory.peak_rss", create: || Box::new(MemoryPeakRssCollector) }
}
```

**Registry construction (in `src/metrics/registry.rs` - never edited):**

```rust
pub fn build_registry() -> MetricRegistry {
    let mut r = MetricRegistry::new();
    for factory in inventory::iter::<CollectorFactory> {
        r.register_boxed(factory.name, (factory.create)());
    }
    r
}
```

Adding a new collector requires no changes to `registry.rs` or any other existing file. The linker picks up the new `inventory::submit!` entry automatically.

### 10.3 How Users Add a Custom Metric

```
1. Create  src/metrics/custom/my_metric.rs
2. Implement MetricCollector for your type
3. Call inventory::submit! { CollectorFactory { ... } } at the bottom
4. Add `mod my_metric;` to src/metrics/custom/mod.rs
5. Run cargo build
```

That is the complete workflow. There is no configuration file to update, no manifest to register, and no process boundary to cross. The resulting binary contains the new collector as a first-class citizen alongside all built-ins.

### 10.4 Project Layout (Metric-Related)

```
src/
├── metrics/
│   ├── mod.rs              ← MetricCollector trait, Target, MetricValue,
│   │                         CollectorFactory + inventory::collect!
│   ├── registry.rs         ← build_registry() - never edit this file
│   ├── builtin/
│   │   ├── mod.rs
│   │   ├── return_code.rs
│   │   ├── execution_time.rs
│   │   ├── memory_rss.rs
│   │   ├── memory_leaks.rs
│   │   ├── coverage_line.rs
│   │   └── sonar_issues.rs
│   └── custom/
│       └── mod.rs          ← users add their collectors here
```

There is no `sqt-plugin-sdk` crate, no Python package, and no separate plugin binary. The source tree is the SDK.

---

## 11. Technology Stack

### 11.1 Rust Dependencies

```toml
[package]
name = "sqt"
version = "1.0.0"
edition = "2024"

[dependencies]
# CLI Framework
clap = { version = "4.5", features = ["derive", "env"] }
clap_complete = "4.5"

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml = "0.9"
toml = "0.8"

# HTTP Client (for REST API)
reqwest = { version = "0.11", features = ["json", "blocking"] }
tokio = { version = "1.35", features = ["full"] }

# Async Runtime
async-trait = "0.1"

# Error Handling
anyhow = "1.0"
thiserror = "1.0"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# Report Generation
typst = "0.11"              # PDF generation (modern alternative to LaTeX)
tera = "1.19"               # Template engine for HTML
tinytemplate = "1.2"        # Lightweight templating

# XML Parsing (for test results)
quick-xml = { version = "0.31", features = ["serialize"] }

# Code Coverage Parsing
lcov = "0.8"

# File System Operations
walkdir = "2.4"
glob = "0.3"

# Date/Time
chrono = { version = "0.4", features = ["serde"] }

# Configuration Management
config = "0.14"

# Testing Frameworks
[dev-dependencies]
mockito = "1.2"
assert_cmd = "2.0"
predicates = "3.0"
tempfile = "3.8"
```

### 11.2 External Tools

Tools that SQT orchestrates (must be installed separately):

| Tool | Purpose | Installation |
|------|---------|--------------|
| **SonarQube Scanner** | Static analysis | `sonar-scanner` |
| **clang-tidy** | C++ linting | Part of LLVM/Clang |
| **cppcheck** | Static analysis | `apt install cppcheck` |
| **GoogleTest** | Unit testing | Built with CMake |
| **lcov** | Coverage reporting | `apt install lcov` |
| **CMake** | Build system | `apt install cmake` |
| **QEMU** | Emulation | `apt install qemu-system` |
| **Docker** | Containerization | Docker Desktop |

### 11.3 Project Structure

```
sqt-reporter/                     # SQT Reporter application (orchestration + reporting)
├── Cargo.toml
├── Cargo.lock
├── README.md
├── ARCHITECTURE.md               # This document
├── LICENSE
│
├── src/
│   ├── main.rs                   # CLI entry point
│   ├── lib.rs                    # Library root
│   │
│   ├── config/                   # Configuration management
│   │   ├── mod.rs
│   │   ├── parser.rs             # YAML/JSON parsing
│   │   ├── validator.rs          # Config validation
│   │   └── schema.rs             # Config data structures
│   │
│   ├── requirements/             # Requirements management
│   │   ├── mod.rs
│   │   ├── strictdoc.rs          # Strictdoc parser
│   │   ├── traceability.rs       # Traceability matrix
│   │   ├── test_spec.rs          # Declarative test spec parser
│   │   └── types.rs              # Requirement data structures
│   │
│   ├── orchestrator/             # Test orchestration
│   │   ├── mod.rs
│   │   ├── runner.rs             # Constraint-driven execution
│   │   └── scheduler.rs          # Job scheduling (sequential queue)
│   │
│   ├── runner_api/               # REST API client for SQT Runner
│   │   ├── mod.rs
│   │   ├── client.rs             # HTTP client
│   │   ├── types.rs              # API data structures
│   │   └── polling.rs            # Job polling with exponential backoff
│   │
│   ├── evidence/                 # Evidence collection
│   │   ├── mod.rs
│   │   ├── collector.rs          # Aggregate results
│   │   └── types.rs              # Evidence data structures
│   │
│   ├── reporting/                # Report generation
│   │   ├── mod.rs
│   │   ├── pdf.rs                # PDF generation (Typst)
│   │   ├── html.rs               # HTML generation
│   │   ├── json.rs               # JSON export
│   │   └── templates/            # Report templates
│   │
│   └── utils/                    # Utilities
│       ├── mod.rs
│       ├── logging.rs
│       └── fs.rs
│
├── templates/                    # Report templates
│   ├── ecss-report.typ           # Typst PDF template
│   ├── report.html               # HTML template
│   └── traceability.html
│
├── tests/                        # Integration tests
│   ├── integration_tests.rs
│   ├── fixtures/
│   └── snapshots/
│
└── examples/                     # Usage examples
    ├── basic_usage.rs
    └── ci_integration.rs

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
| **AD-006** | Use Typst for PDF generation | Modern, fast, programmable | LaTeX (complex), Pandoc (limited) | 🟡 Proposed |
| **AD-007** | Convention-based req-to-test mapping | Simplicity, maintainability | Only explicit mapping (verbose) | 🟡 Proposed |
| **AD-008** | Docker containerization | Reproducibility, isolation | VM (heavy), native (env issues) | ✅ Accepted |
| **AD-009** | Exponential backoff for polling | Efficient, respectful of API | Fixed interval (wasteful) | ✅ Accepted |
| **AD-010** | Both PDF and HTML reports | ECSS compliance + developer UX | Only PDF (not dev-friendly) | ✅ Accepted |
| **AD-011** | Declarative test specs (metric constraints) | No boilerplate test code; requirements directly encode what must hold; uniform traceability | Manual test code per requirement (verbose, hard to maintain) | 🟡 Proposed |
| **AD-012** | Compile-time metric extensibility via `inventory` crate | Zero runtime overhead; full Rust type safety; no central registration file; single auditable binary per deployment | External process/JSON stdio (IPC overhead, OOP failure modes), dynamic `.so` (no stable ABI), WASM (complex toolchain) | ✅ Accepted |
| **AD-013** | Single job endpoint (`/api/v1/jobs`) | Eliminates implicit metric-routing convention; one job covers execution + instrumentation + coverage + static analysis; simpler orchestrator logic | Two endpoints `/runs` + `/analysis` (implemented in Phase 4, reverted - routing via name-prefix convention was fragile and implicitly coupled) | ✅ Accepted |
| **AD-014** | Predefined workload types in SQT Reporter | Workloads (object_detection, image_classification, etc.) are a closed, known set; both Reporter and Runner reason over the same enum; no open-ended task dispatch needed | Open task-name dispatch (Runner accepts arbitrary task strings - fragile, hard to validate) | ✅ Accepted |
| **AD-015** | CPU utilisation normalised Runner-side | Runner knows hardware topology (core count, type); `cpu.avg_normalized_pct` is unambiguous across CPU/GPU/FPGA; hardware descriptor provides raw context | Normalise client-side (Runner would need to expose raw per-core counters, each hardware type differently structured) | ✅ Accepted |
| **AD-016** | ML accuracy metrics computed Reporter-side | Runner returns raw outputs; SQT Reporter computes accuracy/F1/IoU/L2 etc. against reference labels; keeps Runner hardware-agnostic and reference-data-free | Runner computes accuracy (requires Runner-side reference dataset per workload - complex, leaks qualification logic into the Runner) | ✅ Accepted |
| **AD-017** | Unified `outputs` field: one entry per input item, transferred once | Repetitions are for statistics only; outputs are per-input-item, not per-repetition. `result_consistent` captures determinism evidence. Applies uniformly to all workload types including segmentation masks. | Split `sample_output` / `all_outputs` per workload type (added complexity, inconsistent semantics) | ✅ Accepted |
| **AD-018** | Model and artifacts configured on Runner at startup, not per job | No per-job model transfer; ONNX models can be hundreds of MB; re-sending per job is impractical. Runner is purpose-configured for one model version. To qualify a different version, restart Runner with updated config or CLI flags. | Per-job `model_zip` field (removed: inconsistent with session assembly model, impractical at production model sizes) | ✅ Accepted |
| **AD-019** | ML-L Provider artifacts pre-placed on Runner filesystem | No network transfer from Provider at qualification time; configurable paths in `sqt-runner.yaml`; simplest possible delivery model | Pull artifacts from Provider API at session start (adds Provider-side API requirement, network dependency, latency) | ✅ Accepted |
| **AD-020** | SQT Reporter fetches docs from Runner (not directly from ML-L Provider) | Runner is the single point of contact; Reporter need not know the Provider's delivery mechanism; consistent with Runner-as-data-source model | Reporter contacts Provider directly for docs (couples Reporter to Provider, breaks single-interface design) | ✅ Accepted |
| **AD-021** | 1:1 Reporter:Runner deployment model | Simplest correct model for the current use case; no concurrent client complexity; job queue is sufficient | Multi-client with session isolation (deferred; revisit if needed) | ✅ Accepted |
| **AD-022** | Job queue: sequential execution, one job at a time | Predictable resource usage on target hardware; simplest correct implementation; `queue_position` gives Reporter full visibility | Parallel job execution (complex resource management, especially on constrained space hardware) | ✅ Accepted |
| **AD-023** | SQT Runner target abstraction layer (TAL) | Isolates target-specific code (x86\_64, GR740, GR765) behind a Rust trait; new targets can be added without touching the Reporter or the REST API; GR740/GR765 deferred but architecturally catered for | Inline target-specific logic in the job handler (simpler now, unextensible later) | ✅ Accepted |

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