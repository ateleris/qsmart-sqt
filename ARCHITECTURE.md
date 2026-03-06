# Q-SMART SQT Architecture Overview

**Project**: Qualified Software Machine Learning Toolkit for Space Hardware
**Component**: SQT Generic (Ateleris responsibility)
**Technology**: Rust application
**Version**: 1.0
**Date**: 2026-02-13

---

## Table of Contents

- [1. Executive Summary](#1-executive-summary)
- [2. System Architecture](#2-system-architecture)
- [3. Configuration Management](#3-configuration-management)
- [4. Requirements Management](#4-requirements-management)
  - [4.5 Declarative Test Specifications](#45-declarative-test-specifications)
- [5. REST API Interface](#5-rest-api-interface)
- [6. Scope Definition](#6-scope-definition)
- [7. Requirements to Code Flow](#7-requirements-to-code-flow)
- [8. Hardware & Emulation Strategy](#8-hardware--emulation-strategy)
- [9. Report Generation](#9-report-generation)
- [10. Metric Extensibility](#10-metric-extensibility)
- [11. Technology Stack](#11-technology-stack)
- [12. Key Architectural Decisions](#12-key-architectural-decisions)
- [13. Open Questions](#13-open-questions)
- [14. Next Steps](#14-next-steps)

---

## 1. Executive Summary

The Software Qualification Toolkit (SQT) Generic is the open-source component of the Q-SMART qualification infrastructure. It orchestrates the qualification process for Machine Learning Libraries (ML-L) targeting space-grade hardware by:

- **Parsing requirements** from Strictdoc format
- **Orchestrating quality tools** (static analysis, unit testing, coverage)
- **Interfacing with ML-L Specific** via REST API for hardware-specific validation
- **Generating ECSS-compliant reports** (PDF, HTML) with requirements traceability

### Key Principles

1. **Decoupling**: SQT Generic is completely decoupled from ML-L Specific (Klepsydra's closed-source component)
2. **Configuration-driven**: All aspects configurable via YAML/JSON
3. **Standards-compliant**: ECSS E-ST-40C and Q-ST-80C alignment
4. **Evidence-based**: Comprehensive traceability from requirements to test results
5. **Automation-first**: CI/CD-ready with minimal manual intervention

---

## 2. System Architecture

### 2.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         SQT Generic (Rust)                              │
│                      [Ateleris Responsibility]                          │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  ┌────────────────────┐         ┌──────────────────────────┐            │
│  │  Configuration     │         │    Orchestrator          │            │
│  │  Parser            │────────▶│  (API Client + Polling)  │            │
│  │                    │         │                          │            │
│  │  • YAML/JSON       │         │  • Group specs by target │            │
│  │  • Strictdoc       │         │  • Submit jobs in parallel│            │
│  │  • Validation      │         │  • Poll all until done   │            │
│  └────────────────────┘         └────────────┬─────────────┘            │
│                                              │                          │
│  ┌───────────────────────────────────────────┼──────────────────────┐   │
│  │          Constraint Evaluator             │                      │   │
│  │  • Map RunMetrics/AnalysisMetrics fields  │                      │   │
│  │    → MetricValue; null = failed constraint│                      │   │
│  │  • Evaluate constraints against values    │                      │   │
│  └───────────────────────────────────────────┼──────────────────────┘   │
│                                              │                          │
│  ┌───────────────────────────────────────────┼──────────────────────┐   │
│  │          Evidence Collector & Traceability Manager               │   │
│  │  • Requirements ↔ Tests mapping                                  │   │
│  │  • Aggregate per-task evidence from job results                  │   │
│  │  • Traceability matrix generation                                │   │
│  └───────────────────────────────────────────┼──────────────────────┘   │
│                                              │                          │
│  ┌───────────────────────────────────────────┼──────────────────────┐   │
│  │                  Report Generator         │                      │   │
│  │  • PDF generation (ECSS-compliant templates)                     │   │
│  │  • HTML reports (developer-friendly)                             │   │
│  │  • JSON artifacts (machine-readable)                             │   │
│  │  • Traceability matrices (HTML/PDF)                              │   │
│  └───────────────────────────────────────────┼──────────────────────┘   │
│                                              │                          │
└──────────────────────────────────────────────┼──────────────────────────┘
                                               │
                           REST API (POST job, GET status)
                                               │
                                               ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                           Job API Server                                │
│             (ML-L Specific / Local Dev Server)                          │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  Task Runners:                                                          │
│  • return_code, time.execution                                          │
│  • memory.peak_rss (via /usr/bin/time)                                  │
│  • memory.leaks (via valgrind)                                          │
│  • coverage.line_percent (via lcov)                                     │
│  • sonar.critical_issues (via SonarQube API)                            │
│  • (Klepsydra-specific tasks: PBF, hardware execution)                  │
│                                                                         │
│  Job Queue + Parallel Task Execution                                    │
│                                                                         │
└─────────────────────────────────┬───────────────────────────────────────┘
                                  │
                                  ▼
                    Hardware / Emulation Layer
```

### 2.2 Component Responsibilities

#### SQT Generic (Ateleris - Open Source)
- Configuration management
- Requirements parsing and traceability
- Job submission and polling via REST API
- Metric extraction from job results
- Constraint evaluation against extracted values
- Evidence collection
- Report generation

#### ML-L Specific (Klepsydra - Closed Source)
- Klepsydra AI testing
- Performance benchmarking
- Hardware binary generation
- Target hardware execution
- REST API server

#### Clear Separation of Concerns
```
Generic (SQT)                │  Server (ML-L Specific / Local)
─────────────────────────────┼──────────────────────────────────
YAML config parsing           │  Tool execution (valgrind, lcov…)
Specs + constraint evaluation │  SonarQube integration
Job submission + polling      │  Binary execution on hardware
Evidence collection           │  Job queue + parallel task dispatch
Report generation             │  REST API server
```

---

## 3. Configuration Management

### 3.1 Configuration File Format

Primary configuration format: **YAML** (human-readable, widely supported)

#### Main Configuration File: `sqt-config.yaml`

```yaml
# Project Metadata
project:
  name: "Klepsydra AI ML-L Qualification"
  version: "1.0.0"
  ecss_level: "D"                    # A, B, C, or D
  target_trl: 6                      # Technology Readiness Level
  description: "ECSS qualification of Klepsydra AI inference engine"

# Requirements Management
requirements:
  format: "strictdoc"                # strictdoc, yaml, json
  source: "./requirements/ml-l-requirements.sdoc"
  enable_traceability: true
  traceability_output: "./reports/traceability-matrix.html"

# Build System Configuration
build:
  system: "cmake"                    # cmake, cargo, make, meson
  source_dir: "./src"
  build_dir: "./build"

  # Multiple target configurations
  targets:
    - name: "gr740-rtems6"
      architecture: "leon4"
      os: "rtems6"
      toolchain: "sparc-rtems6-gcc"
      compiler_flags:
        - "-O2"
        - "-Wall"
        - "-Werror"
        - "-mcpu=leon4"
      linker_flags:
        - "-Wl,--gc-sections"

    - name: "rv32imac-baremetal"
      architecture: "riscv32"
      os: "baremetal"
      toolchain: "riscv32-unknown-elf-gcc"
      compiler_flags:
        - "-O2"
        - "-march=rv32imac"
        - "-mabi=ilp32"

    - name: "host-linux"
      architecture: "x86_64"
      os: "linux"
      toolchain: "gcc"
      compiler_flags: ["-O2", "-g"]

# Static Analysis Configuration
static_analysis:
  enabled: true

  tools:
    sonarqube:
      enabled: true
      server_url: "http://localhost:9000"
      project_key: "ml-l-qualification"
      config_file: "./sonar-project.properties"
      quality_gate: "strict"
      fail_on_quality_gate: true

    clang_tidy:
      enabled: true
      config_file: "./.clang-tidy"
      checks:
        - "modernize-*"
        - "readability-*"
        - "performance-*"
        - "bugprone-*"
        - "-modernize-use-trailing-return-type"
      header_filter: ".*"

    cppcheck:
      enabled: true
      enable: ["all"]
      suppress: ["unusedFunction"]
      inline_suppr: true

    cpplint:
      enabled: false
      filters: ["-whitespace/indent"]

# Unit Testing Configuration
unit_tests:
  enabled: true
  framework: "googletest"            # googletest, catch2, cpputest, doctest
  test_pattern: "tests/**/*_test.cpp"
  timeout_per_test: 300              # seconds
  parallel_execution: true
  max_parallel_jobs: 4

  coverage:
    enabled: true
    tool: "lcov"                     # lcov, gcov, llvm-cov
    output_format: ["html", "cobertura"]
    threshold:
      line: 80                       # minimum line coverage %
      function: 75                   # minimum function coverage %
      branch: 70                     # minimum branch coverage %
    exclude_patterns:
      - "*/tests/*"
      - "*/third_party/*"

# Integration Testing Configuration
integration_tests:
  enabled: true

  test_suites:
    - name: "functional_validation"
      description: "Functional correctness tests"
      test_dir: "./tests/integration/functional"
      timeout: 600

    - name: "stress_testing"
      description: "Load and stress tests"
      test_dir: "./tests/integration/stress"
      timeout: 1800

    - name: "regression_suite"
      description: "Regression test suite"
      test_dir: "./tests/integration/regression"
      timeout: 900

# Hardware Testing (via Job API — see job-api.openapi.yaml)
hardware_tests:
  enabled: true
  rest_api:
    endpoint: "http://localhost:8080"
    timeout: 3600                    # seconds
    retry_attempts: 3
    authentication:
      type: "bearer"                 # bearer, basic, api_key
      token_env: "ML_L_API_TOKEN"    # environment variable name

  platforms:
    # Physical hardware
    - name: "gr740-eval-board"
      type: "physical"
      description: "GR740 evaluation board"
      connection:
        type: "ssh"
        host: "gr740-board.local"
        port: 22
        user: "root"
      binary_upload: "scp"
      execution_method: "ssh_command"

    # Emulators/Simulators
    - name: "tsim-gr740"
      type: "emulator"
      description: "TSIM GR740 simulator"
      command: "tsim-leon3 -gr740"
      serial_output: true

    - name: "qemu-riscv32"
      type: "emulator"
      description: "QEMU RISC-V 32-bit"
      command: "qemu-system-riscv32 -M virt -nographic"

    - name: "qemu-arm"
      type: "emulator"
      description: "QEMU ARM Cortex-A"
      command: "qemu-system-arm -M virt"

# Performance Benchmarking (via Job API — see job-api.openapi.yaml)
performance_tests:
  enabled: true
  rest_api_endpoint: "http://localhost:8080"

  benchmarks:
    - name: "inference_latency"
      description: "Measure inference latency"
      metrics: ["latency_ms", "latency_std"]
      threshold:
        max_latency_ms: 100

    - name: "throughput"
      description: "Measure inference throughput"
      metrics: ["fps", "throughput_std"]
      threshold:
        min_fps: 10

    - name: "memory_usage"
      description: "Memory consumption analysis"
      metrics: ["heap_kb", "stack_kb", "static_kb"]
      threshold:
        max_heap_kb: 4096

# Report Generation
reports:
  output_dir: "./reports"
  timestamp_format: "%Y-%m-%d_%H-%M-%S"

  formats:
    pdf:
      enabled: true
      template: "./templates/ecss-report-template.tex"
      engine: "typst"                # typst, pdflatex, xelatex

    html:
      enabled: true
      template: "./templates/report-template.html"
      interactive: true

    json:
      enabled: true
      pretty_print: true

  include:
    - requirements_traceability
    - test_results_summary
    - detailed_test_results
    - coverage_reports
    - static_analysis_results
    - performance_metrics
    - build_artifacts_manifest

# ML-L Specific Interface Configuration
ml_l_interface:
  enabled: true
  base_url: "http://localhost:8080"
  api_version: "v1"
  timeout: 600

  endpoints:
    validate: "/api/v1/ml-l/validate"
    job_status: "/api/v1/jobs/{job_id}"
    job_results: "/api/v1/jobs/{job_id}/results"
    hardware_execute: "/api/v1/hardware/execute"
    performance_benchmark: "/api/v1/performance/benchmark"

# CI/CD Integration
ci_integration:
  enabled: true
  provider: "github_actions"         # github_actions, gitlab_ci, jenkins

  on_failure:
    create_issue: true
    notify_email: ["team@ateleris.ch"]

  artifacts:
    upload: true
    retention_days: 90
    paths:
      - "./reports/**"
      - "./build/**/*.elf"
      - "./coverage/**"

# Logging Configuration
logging:
  level: "info"                      # debug, info, warn, error
  output: "stdout"                   # stdout, file, both
  file_path: "./logs/sqt.log"
  format: "json"                     # json, text
  timestamp: true
```

### 3.2 Configuration Validation

The SQT must validate configuration files on startup:

```rust
// Pseudocode for configuration validation
fn validate_config(config: &Config) -> Result<(), ValidationError> {
    // 1. Check required fields
    if config.project.name.is_empty() {
        return Err(ValidationError::MissingField("project.name"));
    }

    // 2. Validate file paths exist
    if !Path::new(&config.requirements.source).exists() {
        return Err(ValidationError::FileNotFound(config.requirements.source));
    }

    // 3. Validate threshold ranges
    if config.unit_tests.coverage.threshold.line > 100 {
        return Err(ValidationError::InvalidValue("coverage threshold > 100%"));
    }

    // 4. Validate tool availability
    if config.static_analysis.tools.sonarqube.enabled {
        check_tool_available("sonar-scanner")?;
    }

    // 5. Validate REST API connectivity (if enabled)
    if config.ml_l_interface.enabled {
        check_api_connectivity(&config.ml_l_interface.base_url)?;
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
        rest_endpoint: "/api/v1/performance/benchmark"
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

Rather than mapping requirements to hand-written test code for every case, SQT Generic supports a **declarative test specification** approach: testable traits (metrics/properties) are defined as built-in or plugin-provided capabilities, and requirements directly specify which constraints must hold — no boilerplate test code required.

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

  - metric: memory.peak_rss
    operator: <=
    value: 512KB

  - metric: memory.leaks
    operator: ==
    value: 0
```

The syntax is intentionally uniform: the user specifies `metric`, `operator`, and `value`. Whether the metric is built-in or provided by a plugin makes no difference to the user.

#### 4.5.3 Core Metric Catalogue

The following built-in metrics ship with SQT Generic. Each is implemented as an internal metric collector (same interface as plugins, but compiled in):

**Memory Metrics**

| Metric | Unit | Description |
|--------|------|-------------|
| `memory.peak_rss` | bytes | Peak resident set size |
| `memory.peak_percent` | % | Peak memory as % of available RAM |
| `memory.allocations` | count | Number of heap allocations |
| `memory.leaks` | bytes | Leaked bytes (Valgrind / ASan) |

**Timing Metrics**

| Metric | Unit | Description |
|--------|------|-------------|
| `time.execution` | ms | Total execution time |
| `time.latency_p50` | ms | 50th percentile latency |
| `time.latency_p95` | ms | 95th percentile latency |
| `time.latency_p99` | ms | 99th percentile latency |
| `time.startup` | ms | Initialization time |
| `time.deadline_misses` | count | Real-time deadline violations |

**Correctness Metrics**

| Metric | Unit | Description |
|--------|------|-------------|
| `accuracy` | % | ML inference accuracy |
| `output.matches_reference` | bool | Output matches reference file |
| `return_code` | int | Process exit code |
| `error_count` | count | Runtime errors / exceptions |

**Code Quality Metrics**

| Metric | Unit | Description |
|--------|------|-------------|
| `sonar.critical_issues` | count | Critical SonarQube violations |
| `sonar.blocker_issues` | count | Blocker violations |
| `sonar.code_smells` | count | Code smell count |
| `coverage.line_percent` | % | Line coverage |
| `coverage.branch_percent` | % | Branch coverage |
| `complexity.cyclomatic_max` | int | Max cyclomatic complexity |

**Hardware Metrics** (collected via Klepsydra API)

| Metric | Unit | Description |
|--------|------|-------------|
| `hardware.temperature` | °C | Operating temperature |
| `hardware.power_draw` | W | Power consumption |
| `hardware.cpu_utilization` | % | CPU usage |
| `hardware.cache_misses` | count | Cache miss rate |

**Phase 1 MVP** targets the 12 most critical metrics: `memory.peak_rss`, `memory.leaks`, `time.execution`, `time.latency_p95`, `accuracy`, `return_code`, `sonar.critical_issues`, `coverage.line_percent`, `hardware.temperature`, `hardware.power_draw`, `hardware.cpu_utilization`, `hardware.cache_misses`.

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

The orchestrator never calls metric collectors directly — all tool execution happens server-side. The `MetricCollector` trait (and Phase 2 built-in collectors) remains valid as the implementation layer for the local dev API server. See Section 10 (Plugin Architecture) for how custom metrics are registered and invoked.

---

## 5. REST API Interface

### 5.1 API Contract (OpenAPI Specification)

The full interface contract is defined in **`job-api.openapi.yaml`** at the root of the
repository. The spec is generic — it is implemented both by the ML-L Specific server
(Klepsydra) and by the local development API server binary.

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

The server is configured at startup with the path to the **binary under test** and the
hardware target. Clients do not specify a binary in their requests — all jobs implicitly
run against the server's configured binary.

A single job type covers all qualification activities: execution runs, instrumented
runs (valgrind/asan/gprof), coverage collection, and static analysis. The client
specifies what workload to run and how; the server returns everything it can measure.

### 5.2 Predefined Workload Types

The set of workload types is defined in SQT Generic and known to both sides of the
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

**Default session**: when no specific workload is requested, SQT Generic submits jobs for
all supported workload types in parallel (one job per type). All jobs execute concurrently;
total wall-clock time is bounded by the slowest job.

### 5.3 Measurement Modes

Each job runs in one of two modes, set by the `mode` field:

| Mode | Behaviour | Populated metrics |
|---|---|---|
| `latency` (default) | Each inference measured independently | `time.avg_ms`, `time.latency_p50/p95/p99_ms` |
| `throughput` | Backend maximises pipeline utilisation | `time.throughput_fps` |

### 5.4 Instrumentation Options

| Value | Tool | Effect |
|---|---|---|
| `none` (default) | — | No overhead; use for benchmarks and accuracy runs |
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

**Response** (terminal — job complete):
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
terminal state. A `complete` job may have `null` metric fields — those indicate
unavailable tools, not infrastructure failure. A `failed` status means the job could not
run at all; the `error` field provides a description.

### 5.6 Output Handling and Accuracy Metrics

The server returns model outputs directly in the job response. `output.outputs`
always contains **one entry per input item** — never one per repetition.
Repetitions exist for statistical measurement only.

For all workload types (including `pixel_segmentation`), the server executes all
`repetitions` and transfers the output batch once. `result_consistent` records
whether every repetition produced bit-identical results:

```
output.result_consistent = true   → all N repetitions agreed
output.result_consistent = false  → non-determinism detected (possible hardware fault)
```

SQT Generic computes accuracy metrics (accuracy, F1, IoU, L2, etc.) client-side by
comparing `output.outputs` against reference labels attached to the spec. An
`outputs_checksum` (SHA-256 of the outputs array) enables cross-session
reproducibility verification without re-transferring the data.

### 5.7 CPU Utilisation Normalisation

**Decision**: normalise server-side (see AD-013).

`cpu.avg_normalized_pct` is computed by the server as:

```
cpu.avg_normalized_pct = (average CPU used across all cores) / (core_count × 100) × 100
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

SQT Generic implements exponential backoff for all polling. Results are embedded
in the GET response — no separate fetch is needed:

```
initial delay: 1 s
backoff factor: ×2 per poll
maximum delay: 30 s
```

All jobs for a qualification session are submitted in parallel; the orchestrator
awaits them concurrently. Total wall-clock time is bounded by the slowest job,
not the sum of all jobs.

---

## 6. Scope Definition

### 6.1 Ateleris Responsibility (SQT Generic)

#### ✅ DO (Your Scope)

1. **Configuration Management**
   - Parse YAML/JSON configuration files
   - Validate configuration correctness
   - Support configuration profiles (dev, ci, production)

2. **Requirements Management**
   - Parse Strictdoc requirements files
   - Extract requirements metadata
   - Build requirements database

3. **Static Analysis Orchestration**
   - Submit tasks (e.g., `sonar.critical_issues`) via the job API
   - Server handles tool invocation (SonarQube, clang-tidy, cppcheck)
   - Parse and evaluate metric values from job results

4. **Unit Test Execution**
   - Submit tasks (e.g., `return_code`, `coverage.line_percent`) via the job API
   - Server handles test discovery, execution, and coverage collection
   - Evaluate pass/fail from returned metric values

5. **Coverage Analysis**
   - Submit `coverage.line_percent` (and related) tasks via the job API
   - Server runs tests with coverage instrumentation and returns results
   - Evaluate against configured thresholds

6. **REST API Client**
   - Implement HTTP client for the job API (see `job-api.openapi.yaml`)
   - Submit one job per spec with all required task names
   - Poll for job status until complete
   - Extract metric values from task results

7. **Evidence Collection**
   - Aggregate all tool outputs
   - Build requirements traceability matrix
   - Compute quality metrics
   - Organize artifacts

8. **Report Generation**
   - Generate ECSS-compliant PDF reports
   - Create HTML developer reports
   - Export JSON data for automation
   - Include traceability matrices

9. **CI/CD Integration**
   - Provide CLI interface
   - Return appropriate exit codes
   - Support common CI systems (GitHub Actions, GitLab CI)

#### ❌ DON'T (Out of Scope)

1. **ML-L Specific Testing**
   - ❌ Implement Klepsydra AI unit tests
   - ❌ Develop ML-L specific test cases
   - ❌ Know internal Klepsydra AI architecture

2. **Hardware Management**
   - ❌ Flash binaries to hardware boards
   - ❌ Manage JTAG/serial connections
   - ❌ Control power cycling
   - ❌ Hardware diagnostics

3. **Binary Compilation**
   - ❌ Build ML-L binaries for space targets
   - ❌ Cross-compilation toolchain management
   - ❌ Linker script generation

4. **Performance Benchmarking**
   - ❌ Implement PBF (Performance Benchmark Framework)
   - ❌ Measure hardware-specific metrics
   - ❌ Layer-wise latency profiling

5. **ML Model Handling**
   - ❌ Convert ML models (ONNX → proprietary)
   - ❌ Optimize models for targets
   - ❌ Quantization

### 6.2 Klepsydra Responsibility (ML-L Specific)

#### ✅ Klepsydra DOES

1. **ML-L Testing**
   - Klepsydra AI unit tests
   - Integration tests
   - Hardware-specific validation

2. **Performance Benchmarking**
   - PBF implementation
   - Latency measurements
   - Throughput analysis
   - Memory profiling

3. **Binary Generation**
   - Compile ML-L for target hardware
   - Link with RTEMS/baremetal
   - Generate .elf files

4. **Hardware Execution**
   - Upload binaries to boards
   - Execute tests on hardware
   - Collect results via serial/network

5. **REST API Server**
   - Implement API endpoints
   - Job queue management
   - Results storage

### 6.3 Interface Contract

```
SQT Generic                     ML-L Specific
────────────────────────────────────────────────
"Please validate ML-L v1.0.0    "OK, job queued"
 on GR740 with ResNet50"

"What's the status?"            "Running... 65%"

"Are you done?"                 "Completed! Here are
                                 the results: ..."

"Here's my report with          (ML-L Specific doesn't
 your results integrated"        generate final report)
```

---

## 7. Requirements to Code Flow

### 7.1 Complete Workflow

```
┌─────────────────────────────────────────────────────────────────┐
│ 1. Requirements Definition (Strictdoc)                          │
│                                                                 │
│    [REQUIREMENT]                                                │
│    UID: REQ-ML-L-001                                            │
│    STATEMENT: Support multi-core inference                      │
│    VERIFICATION: Test                                           │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ 2. Parse Requirements                                           │
│                                                                 │
│    SQT reads .sdoc file → Extracts requirements → Builds DB     │
│                                                                 │
│    struct Requirement {                                         │
│      uid: "REQ-ML-L-001",                                       │
│      statement: "Support multi-core...",                        │
│      verification: [Test]                                       │
│    }                                                            │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ 3. Map to Test Cases                                            │
│                                                                 │
│    Option A (Convention):                                       │
│      REQ-ML-L-001 → tests/unit/test_multicore.cpp               │
│                                                                 │
│    Option B (Explicit):                                         │
│      test_mappings.yaml:                                        │
│        - req: REQ-ML-L-001                                      │
│          tests: [test_multicore.cpp::test_parallel]             │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ 4. Test Discovery                                               │
│                                                                 │
│    • Scan test directories                                      │
│    • Find matching test files                                   │
│    • Parse test metadata (GoogleTest fixtures)                  │
│    • Build test execution plan                                  │
│                                                                 │
│    Found: 150 unit tests, 45 integration tests                  │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ 5. Test Execution                                               │
│                                                                 │
│    For each test:                                               │
│      • Compile test binary                                      │
│      • Execute test                                             │
│      • Collect results (pass/fail/skip)                         │
│      • Collect coverage data                                    │
│      • Collect execution time                                   │
│                                                                 │
│    Unit tests:        ✓ 148 passed, ✗ 2 failed                  │
│    Coverage:          87.5% line coverage                       │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ 6. Hardware Tests (via REST API)                                │
│                                                                 │
│    For requirements needing hardware validation:                │
│      • POST /api/v1/ml-l/validate                               │
│      • Poll GET /api/v1/jobs/{id}                               │
│      • Retrieve GET /api/v1/jobs/{id}/results                   │
│                                                                 │
│    Hardware test:     ✓ Passed (latency: 87ms < 100ms)          │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ 7. Traceability Matrix Generation                               │
│                                                                 │
│    REQ-ML-L-001 → test_multicore.cpp → ✓ Passed → 100% coverage │
│    REQ-ML-L-002 → test_onnx.cpp      → ✓ Passed → 95% coverage  │
│    REQ-ML-L-003 → Hardware Test      → ✓ Passed → N/A           │
│                                                                 │
│    All 45 requirements verified: ✓                              │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ 8. Report Generation                                            │
│                                                                 │
│    Generate ECSS-compliant report:                              │
│      • Executive summary                                        │
│      • Requirements traceability                                │
│      • Test results                                             │
│      • Coverage analysis                                        │
│      • Static analysis results                                  │
│      • Performance metrics                                      │
│                                                                 │
│    Output: validation-report.pdf                                │
└─────────────────────────────────────────────────────────────────┘
```

### 7.2 Example: End-to-End Trace

**Requirement**:
```strictdoc
[REQUIREMENT]
UID: REQ-ML-L-003
STATEMENT: Inference latency on GR740 shall be < 100ms for ResNet50
VERIFICATION: Test
```

**Test Mapping** (explicit):
```yaml
test_mappings:
  - requirement_uid: "REQ-ML-L-003"
    hardware_tests:
      - platform: "gr740-eval"
        rest_endpoint: "/api/v1/performance/benchmark"
        config:
          model: "resnet50"
          iterations: 100
        success_criteria:
          latency_mean_ms: "<= 100"
```

**Execution**:
1. SQT sends POST to `/api/v1/performance/benchmark`
2. Klepsydra executes benchmark on GR740 hardware
3. Returns result: `{ "latency_mean_ms": 87.3 }`
4. SQT checks: `87.3 <= 100` → ✓ Pass

**Traceability Matrix Entry**:
```
REQ-ML-L-003 | HW Test (GR740) | ✓ Passed | 87.3ms < 100ms
```

**Report Section**:
```
Requirement REQ-ML-L-003: VERIFIED
  Statement: Inference latency on GR740 shall be < 100ms
  Test Method: Hardware benchmark on GR740 Eval Board
  Result: PASS (measured: 87.3ms, threshold: 100ms)
  Evidence: See Appendix A, Test Report job-12345
```

---

## 8. Hardware & Emulation Strategy

### 8.1 Emulation for Generic Targets

For **non-space-specific** targets (ARM, RISC-V, x86), the SQT can use emulation directly:

```yaml
# Direct emulation (SQT responsibility)
emulation:
  qemu:
    riscv32:
      command: "qemu-system-riscv32 -M virt -nographic -kernel {binary}"
      timeout: 300

    arm_cortex_a:
      command: "qemu-system-arm -M virt -cpu cortex-a15 -kernel {binary}"
      serial_output: true
```

**Use Case**: Functional tests on generic architectures for portability validation.

### 8.2 Hardware via ML-L API (Space-Specific)

For **space-specific hardware** (GR740, GR765, LEON), delegate to ML-L Specific:

```yaml
# Hardware via Job API (Klepsydra responsibility — see job-api.openapi.yaml)
hardware_tests:
  rest_api_endpoint: "http://localhost:8080"

  platforms:
    - name: "gr740-eval-board"
      type: "physical"
      # SQT doesn't know HOW to access the board
      # It just sends a request to Klepsydra's API
```

**Rationale**:
- Hardware access is complex (JTAG, serial, network, power control)
- Space hardware requires specialized tools (TSIM, GRMON)
- Klepsydra has expertise and tooling for this
- Clean separation of concerns

### 8.3 Decision Matrix

| Target | Method | Who Handles | Rationale |
|--------|--------|-------------|-----------|
| GR740 (physical) | REST API | Klepsydra | Specialized hardware access |
| GR765 (simulator) | REST API | Klepsydra | Requires TSIM license |
| RISC-V (QEMU) | Direct emulation | SQT | Standard QEMU, simple |
| ARM Cortex-A (QEMU) | Direct emulation | SQT | Standard QEMU, simple |
| x86 Linux (host) | Direct execution | SQT | Native execution |

### 8.4 Docker Containerization

SQT should support Docker for reproducible environments:

```yaml
docker:
  enabled: true

  images:
    build_environment:
      image: "ateleris/ml-l-build:latest"
      volumes:
        - "./src:/workspace/src"
        - "./build:/workspace/build"

    test_environment:
      image: "ateleris/ml-l-test:latest"
      network: "host"  # For REST API access
```

**Benefits**:
- Reproducible builds
- Toolchain isolation
- Easy CI/CD integration

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
fans results back to each spec. The server knows which binary to test — it is
configured at startup, not sent per request.

The report table shows one row per requirement, citing its evidence source:

```
REQ-ML-L-001  PASS  return_code=0 ✓  time=87ms ✓    [ml_l_inference + reference.bin]
REQ-ML-L-002  FAIL  return_code=1 ✗                  [ml_l_inference + edge_case.bin]
REQ-ML-L-003  PASS  peak_rss=3.9MB ✓                 [ml_l_inference + reference.bin]
REQ-STATIC-001 PASS sonar.critical=0 ✓               [ml_l_inference, static analysis]
```

The run is evidence *for* the requirement — not the unit of reporting. Multiple
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

SQT Generic ships with a set of built-in metric collectors (Section 4.5.3). When a project needs a metric that is not built-in, users extend the tool by adding Rust code directly to the source tree, then recompiling. There is no runtime plugin protocol, no dynamic loading, and no external process bridge.

**Architectural decision: compile-time extensibility only**

| Approach | Chosen? | Reason |
|---|---|---|
| Dynamic `.so` / `.dll` | ❌ | No stable Rust ABI; version coupling; crash risk |
| External process (stdio/JSON) | ❌ | Runtime overhead; IPC complexity; out-of-process failure modes |
| WebAssembly (WASM) | ❌ | Complex toolchain for what is fundamentally a build-time concern |
| Embedded scripting (Rhai/Lua) | ❌ | Limited capability for real measurement work |
| **Compile-time Rust (`inventory` crate)** | ✅ | Zero runtime overhead; full Rust type safety; no central registration file; trivial to add, trivial to test |

The intended extensibility model is: **clone the repository, add a Rust file, compile**. SQT Generic is a qualification tool used by small, technically capable teams. Requiring a recompile is not a constraint — it is a feature, because it means every deployment is a single, auditable, statically-linked binary with a fixed set of collectors.

### 10.1 The `MetricCollector` Trait

Every collector — built-in or user-added — implements the same trait:

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

The [`inventory`](https://crates.io/crates/inventory) crate uses linker sections to collect distributed static items at link time. Each collector file self-registers by calling one macro — no central list is ever maintained.

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

**Registry construction (in `src/metrics/registry.rs` — never edited):**

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
│   ├── registry.rs         ← build_registry() — never edit this file
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

# Process Execution
tokio-process = "0.2"

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
sqt/                              # Main SQT Generic application
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
│   │   ├── scheduler.rs          # Job scheduling
│   │   └── parallel.rs           # Parallel execution
│   │
│   ├── plugins/                  # Plugin system (Section 10)
│   │   ├── mod.rs                # MetricCollector trait + registry
│   │   ├── external.rs           # ExternalPluginCollector (child process)
│   │   ├── discovery.rs          # Auto-discovery from plugin_dirs
│   │   └── builtin/              # Built-in metric collectors
│   │       ├── memory.rs         # memory.peak_rss, memory.leaks, ...
│   │       ├── timing.rs         # time.execution, time.latency_*, ...
│   │       ├── accuracy.rs       # accuracy, output.matches_reference, ...
│   │       ├── sonarqube.rs      # sonar.* metrics
│   │       └── coverage.rs       # coverage.line_percent, ...
│   │
│   ├── adapters/                 # Tool adapters (local execution)
│   │   ├── mod.rs
│   │   ├── clang_tidy.rs
│   │   ├── googletest.rs
│   │   ├── coverage.rs
│   │   └── cmake.rs
│   │
│   ├── ml_l_api/                 # REST API client for Klepsydra
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

sqt-plugin-sdk/                   # Rust plugin SDK (separate crate → crates.io)
├── Cargo.toml
└── src/
    ├── lib.rs                    # Plugin trait, PluginInfo, CollectRequest/Result
    └── main_macro.rs             # plugin_main!() macro

sqt-plugin-sdk-python/            # Python plugin SDK (→ PyPI)
└── sqt_plugin_sdk/
    └── __init__.py

plugins/                          # Official bundled plugins
├── sqt-metric-valgrind/          # Memory leak detection
├── sqt-metric-sonarqube-full/    # Extended SonarQube metrics
└── sqt-metric-hw-power/          # Hardware power measurement

examples/plugins/                 # Plugin authoring examples
├── plugin_bash/                  # Minimal Bash plugin
├── plugin_python/                # Python SDK example
└── plugin_rust/                  # Rust SDK example
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
| **AD-005** | Delegate hardware testing to ML-L | Complexity, hardware expertise | Implement in SQT (too complex) | ✅ Accepted |
| **AD-006** | Use Typst for PDF generation | Modern, fast, programmable | LaTeX (complex), Pandoc (limited) | 🟡 Proposed |
| **AD-007** | Convention-based req-to-test mapping | Simplicity, maintainability | Only explicit mapping (verbose) | 🟡 Proposed |
| **AD-008** | Docker containerization | Reproducibility, isolation | VM (heavy), native (env issues) | ✅ Accepted |
| **AD-009** | Exponential backoff for polling | Efficient, respectful of API | Fixed interval (wasteful) | ✅ Accepted |
| **AD-010** | Both PDF and HTML reports | ECSS compliance + developer UX | Only PDF (not dev-friendly) | ✅ Accepted |
| **AD-011** | Declarative test specs (metric constraints) | No boilerplate test code; requirements directly encode what must hold; uniform traceability | Manual test code per requirement (verbose, hard to maintain) | 🟡 Proposed |
| **AD-012** | Compile-time metric extensibility via `inventory` crate | Zero runtime overhead; full Rust type safety; no central registration file; single auditable binary per deployment | External process/JSON stdio (IPC overhead, OOP failure modes), dynamic `.so` (no stable ABI), WASM (complex toolchain) | ✅ Accepted |
| **AD-013** | Single job endpoint (`/api/v1/jobs`) | Eliminates implicit metric-routing convention; one job covers execution + instrumentation + coverage + static analysis; simpler orchestrator logic | Two endpoints `/runs` + `/analysis` (implemented in Phase 4, reverted — routing via name-prefix convention was fragile and implicitly coupled) | ✅ Accepted |
| **AD-014** | Predefined workload types in SQT Generic | Workloads (object_detection, image_classification, etc.) are a closed, known set; both client and server reason over the same enum; no open-ended task dispatch needed | Open task-name dispatch (server accepts arbitrary task strings — fragile, hard to validate) | ✅ Accepted |
| **AD-015** | CPU utilisation normalised server-side | Server knows hardware topology (core count, type); `cpu.avg_normalized_pct` is unambiguous across CPU/GPU/FPGA; hardware descriptor provides raw context | Normalise client-side (server would need to expose raw per-core counters, each hardware type differently structured) | ✅ Accepted |
| **AD-016** | ML accuracy metrics computed client-side | Server returns raw outputs; SQT Generic computes accuracy/F1/IoU/L2 etc. against reference labels; keeps server hardware-agnostic and reference-data-free | Server computes accuracy (requires server-side reference dataset per workload — complex, leaks qualification logic into the ML-L Specific component) | ✅ Accepted |
| **AD-017** | Unified `outputs` field: one entry per input item, transferred once | Repetitions are for statistics only; outputs are per-input-item, not per-repetition. `result_consistent` captures determinism evidence. Applies uniformly to all workload types including segmentation masks. | Split `sample_output` / `all_outputs` per workload type (added complexity, inconsistent semantics) | ✅ Accepted |

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

### 12.1 Technical Questions

1. **Strictdoc Parser Implementation**
   - **Question**: Should we implement a full Strictdoc parser or use their Python tool?
   - **Options**:
     - A) Write custom Rust parser (full control, no dependencies)
     - B) Use Strictdoc CLI tool (mature, but Python dependency)
   - **Recommendation**: Start with Option B, migrate to A if needed

2. **REST API Authentication**
   - **Question**: What authentication method should the API use?
   - **Options**:
     - A) Bearer token (simple, stateless)
     - B) API key (simpler)
     - C) mTLS (most secure, complex)
   - **Recommendation**: Bearer token for development, mTLS for production

3. **Report Template Engine**
   - **Question**: Typst vs LaTeX for PDF generation?
   - **Decision needed**: Verify Typst maturity for ECSS compliance
   - **Fallback**: LaTeX with Tectonic

4. **Hardware Test Timeout**
   - **Question**: What should be the default timeout for hardware tests?
   - **Concern**: Space hardware tests can be slow (flashing, execution, collection)
   - **Recommendation**: 10 minutes default, configurable per platform

### 12.2 Process Questions

5. **CI/CD Pipeline**
   - **Question**: Which CI system will be primary?
   - **Recommendation**: GitHub Actions (start), add GitLab CI if needed

6. **Artifact Storage**
   - **Question**: Where should large artifacts be stored (binaries, reports)?
   - **Options**:
     - A) Local filesystem
     - B) S3-compatible storage
     - C) GitLab artifacts
   - **Recommendation**: Local + optional S3 for long-term storage

7. **API Versioning**
   - **Question**: How to handle API versioning (SQT ↔ ML-L)?
   - **Recommendation**: Semantic versioning in URL (/api/v1/, /api/v2/)

### 12.3 Clarifications Needed from Klepsydra

- [ ] Confirm REST API contract (endpoints, authentication)
- [ ] Provide example PBF output format
- [ ] Define hardware platforms available for testing
- [ ] Clarify binary upload mechanism (direct or via API)
- [ ] Specify timeout expectations for hardware tests
- [ ] Provide sample Strictdoc requirements file

---

## 14. Next Steps

### 13.1 Immediate Actions (Week 1-2)

1. **Validate Architecture with Klepsydra**
   - Schedule architecture review meeting
   - Confirm REST API contract
   - Agree on responsibilities

2. **Create Project Repository**
   - Initialize Git repository
   - Set up Cargo project structure
   - Add CI/CD pipeline (GitHub Actions)

3. **Implement Configuration Parser**
   - Define YAML schema
   - Implement parser with serde_yaml
   - Write validation logic
   - Add unit tests

4. **Draft OpenAPI Specification**
   - Document REST API endpoints
   - Define request/response schemas
   - Share with Klepsydra for approval

### 13.2 Phase 1: Proof of Concept (Week 3-4)

5. **Minimal Working Prototype**
   - Parse simple YAML config
   - Run one static analysis tool (clang-tidy)
   - Generate simple text report
   - Command-line interface

6. **REST API Client (Mock)**
   - Implement HTTP client
   - Create mock server for testing
   - Test job submission and polling

### 13.3 Phase 2: Core Functionality (Week 5-8)

7. **Tool Adapters**
   - Implement SonarQube adapter
   - Implement GoogleTest runner
   - Implement coverage collector

8. **Requirements Management**
   - Implement Strictdoc parser
   - Build traceability logic
   - Test with sample requirements

9. **Evidence Collection**
   - Aggregate tool results
   - Generate traceability matrix
   - Basic HTML report

### 13.4 Phase 3: Integration (Week 9-12)

10. **ML-L API Integration**
    - Integrate with real Klepsydra API
    - Test on actual hardware platforms
    - Validate results

11. **Report Generation**
    - Implement PDF generation
    - Create ECSS-compliant templates
    - Generate comprehensive reports

12. **End-to-End Testing**
    - Full qualification workflow
    - Performance testing
    - Error handling

### 13.5 Phase 4: Refinement (Week 13-15)

13. **Documentation**
    - User guide
    - API documentation
    - Configuration examples

14. **CI/CD Integration**
    - GitHub Actions workflow
    - Docker images
    - Artifact management

15. **Handover & Training**
    - Training session for Klepsydra
    - Deployment guide
    - Maintenance plan

---

## Appendices

### Appendix A: Glossary

| Term | Definition |
|------|------------|
| **ECSS** | European Cooperation for Space Standardization |
| **ML-L** | Machine Learning Library (inference engine) |
| **SQT** | Software Qualification Toolkit |
| **PBF** | Performance Benchmark Framework (Klepsydra's tool) |
| **TRL** | Technology Readiness Level (1-9 scale) |
| **Strictdoc** | Requirements management tool (text-based) |
| **ONNX** | Open Neural Network Exchange (model format) |
| **GR740** | Quad-core LEON4 space-grade processor |
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