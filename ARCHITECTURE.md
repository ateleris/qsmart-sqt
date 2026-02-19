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
- [5. REST API Interface](#5-rest-api-interface)
- [6. Scope Definition](#6-scope-definition)
- [7. Requirements to Code Flow](#7-requirements-to-code-flow)
- [8. Hardware & Emulation Strategy](#8-hardware--emulation-strategy)
- [9. Report Generation](#9-report-generation)
- [10. Technology Stack](#10-technology-stack)
- [11. Key Architectural Decisions](#11-key-architectural-decisions)
- [12. Open Questions](#12-open-questions)
- [13. Next Steps](#13-next-steps)

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
│  │  Parser            │────────▶│    (Test Runner)         │            │
│  │                    │         │                          │            │
│  │  • YAML/JSON       │         │  • Job Scheduling        │            │
│  │  • Strictdoc       │         │  • Parallel Execution    │            │
│  │  • Validation      │         │  • Progress Tracking     │            │
│  └────────────────────┘         └────────────┬─────────────┘            │
│                                              ▼                          │
│  ┌───────────────────────────────────────────┼──────────────────────┐   │
│  │                    Tool Adapters          │                      │   │
│  ├─────────────┬──────────────┬─────────────┬┴───────┬──────────────┤   │
│  │ Static      │ Unit Test    │ Coverage    │ Build  │ Performance  │   │
│  │ Analysis    │ Runners      │ Tools       │ System │ Analysis     │   │
│  │             │              │             │        │              │   │
│  │ •SonarQube  │ •GoogleTest  │ •gcov       │ •CMake │ •Benchmark   │   │
│  │ •clang-tidy │ •Catch2      │ •lcov       │ •Cargo │  aggregation │   │
│  │ •cppcheck   │ •CppUTest    │ •llvm-cov   │ •Make  │              │   │
│  │ •cpplint    │ •pytest      │             │        │              │   │
│  └─────────────┴──────────────┴─────────────┴────────┴──────────────┘   │
│                                                                         │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │          Evidence Collector & Traceability Manager                │  │
│  │                                                                   │  │
│  │  • Requirements ↔ Tests mapping                                   │  │
│  │  • Test results aggregation                                       │  │
│  │  • Coverage metrics collection                                    │  │
│  │  • Static analysis results parsing                                │  │
│  │  • Traceability matrix generation                                 │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                         │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                  Report Generator                                 │  │
│  │                                                                   │  │
│  │  • PDF generation (ECSS-compliant templates)                      │  │
│  │  • HTML reports (developer-friendly)                              │  │
│  │  • JSON artifacts (machine-readable)                              │  │
│  │  • Traceability matrices (HTML/PDF)                               │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                         │
└─────────────────────────────────┬───────────────────────────────────────┘
                                  │
                                  │ REST API (HTTP/JSON)
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                    ML-L Specific Part (Klepsydra)                       │
│                       [Klepsydra Responsibility]                        │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  • Klepsydra AI specific test suite                                     │
│  • Performance Benchmark Framework (PBF)                                │
│  • Hardware-specific validation (GR740, GR765, RISC-V)                  │
│  • Binary generation for target hardware                                │
│  • ML model conversion and optimization                                 │
│  • Hardware execution management                                        │
│                                                                         │
└─────────────────────────────────┬───────────────────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                        Hardware/Emulation Layer                         │
│                                                                         │
│  ┌──────────────┬──────────────┬──────────────┬─────────────────────┐   │
│  │ QEMU         │ TSIM         │ GRMON        │ Physical Hardware   │   │
│  │              │              │              │                     │   │
│  │ •RISC-V      │ •LEON/GR740  │ •GR740       │ •GR740 Eval Board   │   │
│  │ •ARM         │ •GR765       │  debug       │ •GR765 Dev Board    │   │
│  │ •x86 (host)  │              │              │ •Custom targets     │   │
│  └──────────────┴──────────────┴──────────────┴─────────────────────┘   │
│                                                                         │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                  Docker Containers                               │   │
│  │  • Linux-based test environments                                 │   │
│  │  • Toolchain isolation                                           │   │
│  │  • Reproducible builds                                           │   │
│  └──────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Component Responsibilities

#### SQT Generic (Ateleris - Open Source)
- Configuration management
- Requirements parsing and traceability
- Generic tool orchestration
- REST API client
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
Generic (Tool-agnostic)     │  Specific (ML-L/Hardware-specific)
────────────────────────────┼────────────────────────────────────
YAML/JSON config parsing    │  Klepsydra AI test execution
Strictdoc requirements      │  PBF performance benchmarking
SonarQube orchestration     │  Binary compilation for targets
GoogleTest runner           │  Hardware flashing/execution
Coverage aggregation        │  Target-specific metrics
PDF report generation       │  Hardware communication (JTAG/serial)
REST API client             │  REST API server
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

# Hardware Testing (via REST API to ML-L Specific)
hardware_tests:
  enabled: true
  rest_api:
    endpoint: "http://localhost:8080/ml-l-api/v1"
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

# Performance Benchmarking (via ML-L API)
performance_tests:
  enabled: true
  rest_api_endpoint: "http://localhost:8080/ml-l-api/v1/benchmark"

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

---

## 5. REST API Interface

### 5.1 API Contract (OpenAPI Specification)

The interface between SQT Generic and ML-L Specific is defined via REST API.

#### Base URL
```
http://localhost:8080/ml-l-api/v1
```

#### Authentication
```http
Authorization: Bearer <token>
Content-Type: application/json
```

### 5.2 API Endpoints

#### 5.2.1 Submit ML-L Validation Job

**Endpoint**: `POST /api/v1/ml-l/validate`

**Description**: Submit a validation job for the ML-L on a specific target platform.

**Request Body**:
```json
{
  "ml_l_version": "1.0.0",
  "target_platform": "gr740-rtems6",
  "test_suite": "performance",
  "model": {
    "format": "onnx",
    "path": "/path/to/model.onnx",
    "name": "resnet50"
  },
  "test_config": {
    "iterations": 100,
    "warmup_iterations": 10,
    "input_size": [1, 3, 224, 224],
    "measure_memory": true
  },
  "execution_mode": "hardware",
  "timeout_seconds": 600
}
```

**Response** (202 Accepted):
```json
{
  "job_id": "job-a1b2c3d4-e5f6-7890",
  "status": "queued",
  "created_at": "2026-02-13T14:30:00Z",
  "estimated_duration_seconds": 300,
  "status_url": "/api/v1/jobs/job-a1b2c3d4-e5f6-7890"
}
```

#### 5.2.2 Check Job Status

**Endpoint**: `GET /api/v1/jobs/{job_id}`

**Response**:
```json
{
  "job_id": "job-a1b2c3d4-e5f6-7890",
  "status": "running",
  "progress_percent": 65,
  "created_at": "2026-02-13T14:30:00Z",
  "started_at": "2026-02-13T14:30:15Z",
  "current_step": "Hardware execution",
  "logs_url": "/api/v1/jobs/job-a1b2c3d4-e5f6-7890/logs",
  "estimated_completion": "2026-02-13T14:35:00Z"
}
```

**Possible Status Values**:
- `queued`: Job is waiting to be executed
- `running`: Job is currently executing
- `completed`: Job finished successfully
- `failed`: Job encountered an error
- `cancelled`: Job was cancelled by user

#### 5.2.3 Get Job Results

**Endpoint**: `GET /api/v1/jobs/{job_id}/results`

**Response** (when status = completed):
```json
{
  "job_id": "job-a1b2c3d4-e5f6-7890",
  "status": "completed",
  "completed_at": "2026-02-13T14:34:45Z",
  "duration_seconds": 270,

  "results": {
    "functional": {
      "tests_total": 150,
      "tests_passed": 148,
      "tests_failed": 2,
      "tests_skipped": 0,
      "coverage_percent": 87.5,
      "failed_tests": [
        {
          "name": "test_edge_case_1",
          "error": "Assertion failed: expected 0.95, got 0.94"
        },
        {
          "name": "test_overflow_handling",
          "error": "Timeout after 10 seconds"
        }
      ]
    },

    "performance": {
      "inference_latency": {
        "mean_ms": 87.3,
        "std_ms": 2.1,
        "min_ms": 84.5,
        "max_ms": 93.2,
        "p50_ms": 87.0,
        "p95_ms": 91.5,
        "p99_ms": 92.8
      },
      "throughput": {
        "mean_fps": 11.5,
        "std_fps": 0.3
      },
      "memory_usage": {
        "heap_kb": 1847,
        "stack_kb": 128,
        "static_kb": 512,
        "total_kb": 2487
      }
    },

    "artifacts": {
      "binary_path": "/artifacts/ml-l-gr740-v1.0.0.elf",
      "binary_size_kb": 2048,
      "test_report_json": "/artifacts/test-report.json",
      "test_report_html": "/artifacts/test-report.html",
      "profiling_data": "/artifacts/profile.json"
    }
  },

  "metadata": {
    "platform": "gr740-rtems6",
    "ml_l_version": "1.0.0",
    "test_suite": "performance",
    "execution_environment": {
      "hardware": "GR740 Evaluation Board",
      "os": "RTEMS 6.1",
      "cpu_freq_mhz": 250
    }
  }
}
```

#### 5.2.4 Execute Hardware Test

**Endpoint**: `POST /api/v1/hardware/execute`

**Request**:
```json
{
  "platform": "gr740-eval-board",
  "binary_path": "./build/ml-l-test.elf",
  "test_type": "functional",
  "timeout_seconds": 600,
  "collect_metrics": true,
  "serial_output": true
}
```

**Response**:
```json
{
  "job_id": "job-hw-12345",
  "status": "queued",
  "platform": "gr740-eval-board"
}
```

#### 5.2.5 Performance Benchmark

**Endpoint**: `POST /api/v1/performance/benchmark`

**Request**:
```json
{
  "model": "resnet50",
  "platform": "gr740-rtems6",
  "benchmark_config": {
    "input_size": [1, 3, 224, 224],
    "iterations": 100,
    "warmup_iterations": 10,
    "measure_power": false
  }
}
```

**Response**:
```json
{
  "job_id": "job-bench-67890",
  "status": "queued"
}
```

### 5.3 Error Handling

**Standard Error Response**:
```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Invalid target platform: gr740-rtems6",
    "details": "Supported platforms: gr740-rtems6, rv32imac-baremetal",
    "timestamp": "2026-02-13T14:30:00Z"
  }
}
```

**Error Codes**:
- `VALIDATION_ERROR`: Invalid request parameters
- `NOT_FOUND`: Resource not found (job_id, model, etc.)
- `TIMEOUT`: Operation exceeded timeout
- `HARDWARE_ERROR`: Hardware execution failed
- `COMPILATION_ERROR`: Binary compilation failed
- `INTERNAL_ERROR`: Internal server error

### 5.4 Polling Strategy

The SQT Generic should implement exponential backoff for polling:

```rust
async fn wait_for_job_completion(job_id: &str) -> Result<JobResults> {
    let mut delay = Duration::from_secs(1);
    let max_delay = Duration::from_secs(30);

    loop {
        let status = check_job_status(job_id).await?;

        match status.status {
            JobStatus::Completed => {
                return get_job_results(job_id).await;
            }
            JobStatus::Failed => {
                return Err(Error::JobFailed(status.error_message));
            }
            JobStatus::Running | JobStatus::Queued => {
                tokio::time::sleep(delay).await;
                delay = std::cmp::min(delay * 2, max_delay);
            }
        }
    }
}
```

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
   - Invoke SonarQube scanner
   - Run clang-tidy on source code
   - Execute cppcheck
   - Parse and aggregate results

4. **Unit Test Execution**
   - Discover unit tests (pattern matching)
   - Execute GoogleTest/Catch2/CppUTest suites
   - Collect test results (JUnit XML)
   - Handle test failures and timeouts

5. **Coverage Analysis**
   - Run tests with coverage instrumentation
   - Collect coverage data (gcov, lcov)
   - Generate coverage reports (HTML, Cobertura)
   - Compute coverage metrics

6. **REST API Client**
   - Implement HTTP client for ML-L API
   - Submit validation jobs
   - Poll for job status
   - Retrieve results

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
# Hardware via API (Klepsydra responsibility)
hardware_tests:
  rest_api_endpoint: "http://localhost:8080/ml-l-api/v1"

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

### 9.2 Report Content Structure

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

### 9.3 Report Templates

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

### 9.4 Report Generation Implementation

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

## 10. Technology Stack

### 10.1 Rust Dependencies

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

### 10.2 External Tools

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

### 10.3 Project Structure

```
sqt/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── ARCHITECTURE.md          # This document
├── LICENSE
│
├── src/
│   ├── main.rs              # CLI entry point
│   ├── lib.rs               # Library root
│   │
│   ├── config/              # Configuration management
│   │   ├── mod.rs
│   │   ├── parser.rs        # YAML/JSON parsing
│   │   ├── validator.rs     # Config validation
│   │   └── schema.rs        # Config data structures
│   │
│   ├── requirements/        # Requirements management
│   │   ├── mod.rs
│   │   ├── strictdoc.rs     # Strictdoc parser
│   │   ├── traceability.rs  # Traceability matrix
│   │   └── types.rs         # Requirement data structures
│   │
│   ├── orchestrator/        # Test orchestration
│   │   ├── mod.rs
│   │   ├── runner.rs        # Test execution
│   │   ├── scheduler.rs     # Job scheduling
│   │   └── parallel.rs      # Parallel execution
│   │
│   ├── adapters/            # Tool adapters
│   │   ├── mod.rs
│   │   ├── sonarqube.rs
│   │   ├── clang_tidy.rs
│   │   ├── googletest.rs
│   │   ├── coverage.rs
│   │   └── cmake.rs
│   │
│   ├── ml_l_api/            # REST API client
│   │   ├── mod.rs
│   │   ├── client.rs        # HTTP client
│   │   ├── types.rs         # API data structures
│   │   └── polling.rs       # Job polling logic
│   │
│   ├── evidence/            # Evidence collection
│   │   ├── mod.rs
│   │   ├── collector.rs     # Aggregate results
│   │   └── types.rs         # Evidence data structures
│   │
│   ├── reporting/           # Report generation
│   │   ├── mod.rs
│   │   ├── pdf.rs           # PDF generation
│   │   ├── html.rs          # HTML generation
│   │   ├── json.rs          # JSON export
│   │   └── templates/       # Report templates
│   │
│   └── utils/               # Utilities
│       ├── mod.rs
│       ├── logging.rs
│       └── fs.rs
│
├── templates/               # Report templates
│   ├── ecss-report.typ      # Typst PDF template
│   ├── report.html          # HTML template
│   └── traceability.html
│
├── tests/                   # Integration tests
│   ├── integration_tests.rs
│   ├── fixtures/
│   └── snapshots/
│
└── examples/                # Usage examples
    ├── basic_usage.rs
    └── ci_integration.rs
```

---

## 11. Key Architectural Decisions

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

## 12. Open Questions

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

## 13. Next Steps

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