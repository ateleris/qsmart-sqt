// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Data types of the Runner API contract (`job-api.openapi.yaml`), shared by
//! the Orchestrator (`sqt-reporter`) and the reference Runner (`sqt-runner`).
//!
//! Field names and wire names follow the contract exactly. Response types
//! reject unknown fields (`deny_unknown_fields`), so a Runner response that
//! does not conform to the contract fails to deserialise (SRS-IF-007).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Enumerations
// ---------------------------------------------------------------------------

/// The kind of ML workload a job runs. It tells the Runner how to interpret
/// the job input and what output to produce (SRS-FUNC-019).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadType {
    /// Images in, bounding boxes and class labels out.
    ObjectDetection,
    /// Images in, a class label and confidence out.
    #[default]
    ImageClassification,
    /// Images in, a per-pixel mask out.
    PixelSegmentation,
    /// Feature vectors in, a class label out.
    Classification,
    /// Feature vectors in, a scalar value out.
    Regression,
    /// A time series in, a forecast sequence out.
    TimeSeriesForecasting,
}

/// Instrumentation the Runner attaches to the binary under test; one per job.
/// Instrumented runs are slower, so their timing is not comparable with
/// uninstrumented runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Instrumentation {
    /// No instrumentation (default).
    #[default]
    None,
    /// `AddressSanitizer`: memory errors such as overflows and use-after-free.
    Asan,
    /// Valgrind memcheck: reports `memory.leaks_bytes`.
    Valgrind,
    /// GNU gprof call-graph profile; no effect on memory metrics.
    Gprof,
}

/// How the Runner schedules repetitions and which timing metrics it fills.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeasurementMode {
    /// Each inference call is timed on its own: `time.avg_ms` and the latency
    /// percentiles (default).
    #[default]
    Latency,
    /// The Runner maximises pipeline utilisation and reports `time.throughput_fps`.
    Throughput,
}

/// State of a job on the Runner. `Complete` and `Failed` are terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    /// Queued behind the job that is running.
    Accepted,
    /// Executing its repetitions.
    Running,
    /// All repetitions finished; metrics and outputs are final.
    Complete,
    /// Infrastructure failure: the job could not run. `error` says why.
    Failed,
}

impl JobStatus {
    /// Whether the job has reached a final state, so polling can stop (SRS-IF-003).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Complete | Self::Failed)
    }
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Body for `POST /api/v1/jobs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    /// Workload type of the job.
    pub use_case: WorkloadType,
    /// Workload-specific input data (images, tabular rows, time series, or
    /// dataset reference). `None` when the server has a default dataset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    /// How often the binary is run on the same input; metrics are aggregated
    /// over all repetitions. Defaults to 1.
    #[serde(default = "default_repetitions")]
    pub repetitions: u32,
    /// Instrumentation to attach (default: none).
    #[serde(default)]
    pub instrumentation: Instrumentation,
    /// Measurement mode (default: latency).
    #[serde(default)]
    pub mode: MeasurementMode,
    /// Calls made before measurement starts; their results are discarded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warmup_repetitions: Option<u32>,
    /// Upper bound in seconds for the whole job, warm-up included; the Runner
    /// fails the job when it is exceeded. `None` lets the Runner choose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// Free key-value metadata, echoed back in the job status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
}

fn default_repetitions() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// Accepted response
// ---------------------------------------------------------------------------

/// Response body for `POST /api/v1/jobs` (202 Accepted).
///
/// `deny_unknown_fields` implements SRS-IF-007: unknown fields in a Runner
/// response are rejected as a schema violation rather than silently ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobAccepted {
    /// Identifier for polling the job.
    pub job_id: String,
    /// Always `accepted`.
    pub status: String,
    /// Position in the execution queue; 1 means next.
    pub queue_position: u32,
    /// Submission time, RFC 3339.
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Hardware descriptor
// ---------------------------------------------------------------------------

/// Hardware descriptor embedded in `JobStatusResponse`.
///
/// `deny_unknown_fields` implements SRS-IF-007.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardwareInfo {
    /// Primary compute element: `cpu`, `gpu`, `fpga`, `npu`, `other`, or
    /// `unknown` when the Runner does not describe its hardware.
    #[serde(rename = "type")]
    pub hw_type: String,
    /// Human-readable hardware model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Number of compute units; the basis of `cpu.avg_normalized_pct`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cores: Option<u32>,
    /// Total available memory in kilobytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_total_kb: Option<u64>,
    /// Attached accelerator, if inference does not run on the primary element.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accelerator_model: Option<String>,
    /// Clock of the primary compute element in MHz.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_mhz: Option<f64>,
    /// Operating system or runtime on the target, e.g. `RTEMS 6`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
}

impl Default for HardwareInfo {
    fn default() -> Self {
        Self {
            hw_type: "unknown".into(),
            model: None,
            cores: None,
            memory_total_kb: None,
            accelerator_model: None,
            clock_mhz: None,
            os: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Unified metrics (all fields nullable; null = tool unavailable)
// ---------------------------------------------------------------------------

/// Aggregated metrics returned in `JobStatusResponse`.
///
/// `deny_unknown_fields` implements SRS-IF-007: an unrecognised metric key in
/// a Runner response is treated as a schema violation (the affected job is
/// recorded as infrastructure-failed) rather than silently dropped.
///
/// Note: this means adding a new metric field to the Runner without a
/// corresponding update to this struct will fail deserialization. This is
/// intentional — it enforces that both sides of the API contract are kept
/// in sync.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JobMetrics {
    /// Exit code of the last repetition; 0 is success.
    pub return_code: Option<i64>,

    /// Average execution time per repetition in milliseconds: the binary's own
    /// elapsed time, without container overhead.
    #[serde(rename = "time.avg_ms")]
    pub time_avg_ms: Option<f64>,
    /// Median latency in milliseconds; needs at least 2 repetitions in latency mode.
    #[serde(rename = "time.latency_p50_ms")]
    pub time_latency_p50_ms: Option<f64>,
    /// 95th-percentile latency in milliseconds; needs at least 20 repetitions.
    #[serde(rename = "time.latency_p95_ms")]
    pub time_latency_p95_ms: Option<f64>,
    /// 99th-percentile latency in milliseconds; needs at least 100 repetitions.
    #[serde(rename = "time.latency_p99_ms")]
    pub time_latency_p99_ms: Option<f64>,
    /// Inferences per second; throughput mode only.
    #[serde(rename = "time.throughput_fps")]
    pub time_throughput_fps: Option<f64>,
    /// Time from process start to the first inference call in milliseconds.
    #[serde(rename = "time.startup_ms")]
    pub time_startup_ms: Option<f64>,
    /// Duration of the first (cold) inference call in milliseconds.
    #[serde(rename = "time.first_inference_ms")]
    pub time_first_inference_ms: Option<f64>,

    /// Average resident set size across repetitions in kilobytes.
    #[serde(rename = "memory.avg_rss_kb")]
    pub memory_avg_rss_kb: Option<f64>,
    /// Peak resident set size across repetitions in kilobytes.
    #[serde(rename = "memory.peak_rss_kb")]
    pub memory_peak_rss_kb: Option<f64>,
    /// Bytes leaked according to Valgrind or `ASan`; `null` without that
    /// instrumentation.
    #[serde(rename = "memory.leaks_bytes")]
    pub memory_leaks_bytes: Option<i64>,
    /// Peak stack usage in kilobytes.
    #[serde(rename = "memory.stack_kb")]
    pub memory_stack_kb: Option<f64>,
    /// Heap allocation calls across all repetitions.
    #[serde(rename = "memory.heap_allocations")]
    pub memory_heap_allocations: Option<i64>,

    /// Average CPU use as a share of all cores, 0 to 100 %.
    #[serde(rename = "cpu.avg_normalized_pct")]
    pub cpu_avg_normalized_pct: Option<f64>,

    /// Line coverage, 0 to 100 %; `null` if the binary is not instrumented.
    #[serde(rename = "coverage.line_pct")]
    pub coverage_line_pct: Option<f64>,
    /// Branch coverage, 0 to 100 %; `null` if the binary is not instrumented.
    #[serde(rename = "coverage.branch_pct")]
    pub coverage_branch_pct: Option<f64>,
    /// Function coverage, 0 to 100 %; `null` if the binary is not instrumented.
    #[serde(rename = "coverage.function_pct")]
    pub coverage_function_pct: Option<f64>,
    /// Source lines executed; `null` if the binary is not instrumented.
    #[serde(rename = "coverage.lines_covered")]
    pub coverage_lines_covered: Option<i64>,
    /// Instrumented source lines; `null` if the binary is not instrumented.
    #[serde(rename = "coverage.lines_total")]
    pub coverage_lines_total: Option<i64>,

    /// Warnings from static analysers such as cppcheck.
    #[serde(rename = "static.warnings")]
    pub static_warnings: Option<i64>,
    /// CRITICAL-severity `SonarQube` issues.
    #[serde(rename = "static.critical_issues")]
    pub static_critical_issues: Option<i64>,
    /// MAJOR-severity `SonarQube` issues.
    #[serde(rename = "static.major_issues")]
    pub static_major_issues: Option<i64>,

    /// Size of the binary on disk in kilobytes.
    #[serde(rename = "binary.size_kb")]
    pub binary_size_kb: Option<f64>,

    /// Harness/Docker overhead (outer `docker run` wall clock − the binary's
    /// own elapsed time), in milliseconds. Diagnostic: lets the reporter see
    /// how much of a naive wall-clock measurement is container lifecycle rather
    /// than the workload (SRS-FUNC-005). `null` when it could not be measured.
    #[serde(rename = "harness.docker_overhead_ms")]
    pub harness_docker_overhead_ms: Option<f64>,

    /// Average power draw during inference in watts.
    #[serde(rename = "power.avg_watts")]
    pub power_avg_watts: Option<f64>,
    /// Peak power draw across repetitions in watts.
    #[serde(rename = "power.peak_watts")]
    pub power_peak_watts: Option<f64>,

    /// Average die or board temperature in degrees Celsius.
    #[serde(rename = "hardware.temperature_celsius")]
    pub hardware_temperature_celsius: Option<f64>,
}

impl JobMetrics {
    /// Look up a metric value by its wire name. Returns `None` when the field
    /// is absent or the name is unknown.
    #[expect(
        clippy::cast_precision_loss,
        reason = "integer metrics (return code, counts) are far below 2^52 and convert exactly"
    )]
    pub fn get(&self, name: &str) -> Option<f64> {
        match name {
            "return_code" => self.return_code.map(|v| v as f64),
            "time.avg_ms" => self.time_avg_ms,
            "time.latency_p50_ms" => self.time_latency_p50_ms,
            "time.latency_p95_ms" => self.time_latency_p95_ms,
            "time.latency_p99_ms" => self.time_latency_p99_ms,
            "time.throughput_fps" => self.time_throughput_fps,
            "time.startup_ms" => self.time_startup_ms,
            "time.first_inference_ms" => self.time_first_inference_ms,
            "memory.avg_rss_kb" => self.memory_avg_rss_kb,
            "memory.peak_rss_kb" => self.memory_peak_rss_kb,
            "memory.leaks_bytes" => self.memory_leaks_bytes.map(|v| v as f64),
            "memory.stack_kb" => self.memory_stack_kb,
            "memory.heap_allocations" => self.memory_heap_allocations.map(|v| v as f64),
            "cpu.avg_normalized_pct" => self.cpu_avg_normalized_pct,
            "coverage.line_pct" => self.coverage_line_pct,
            "coverage.branch_pct" => self.coverage_branch_pct,
            "coverage.function_pct" => self.coverage_function_pct,
            "coverage.lines_covered" => self.coverage_lines_covered.map(|v| v as f64),
            "coverage.lines_total" => self.coverage_lines_total.map(|v| v as f64),
            "static.warnings" => self.static_warnings.map(|v| v as f64),
            "static.critical_issues" => self.static_critical_issues.map(|v| v as f64),
            "static.major_issues" => self.static_major_issues.map(|v| v as f64),
            "binary.size_kb" => self.binary_size_kb,
            "harness.docker_overhead_ms" => self.harness_docker_overhead_ms,
            "power.avg_watts" => self.power_avg_watts,
            "power.peak_watts" => self.power_peak_watts,
            "hardware.temperature_celsius" => self.hardware_temperature_celsius,
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Model output
// ---------------------------------------------------------------------------

/// Model output block embedded in `JobStatusResponse`.
///
/// `deny_unknown_fields` implements SRS-IF-007.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobOutput {
    /// Semantic type of the outputs, e.g. `classification_labels`.
    #[serde(rename = "type")]
    pub output_type: String,
    /// Whether all repetitions produced bit-identical outputs (SRS-REL-004).
    pub result_consistent: bool,
    /// One output per input item, not per repetition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<serde_json::Value>>,
    /// SHA-256 of the canonical JSON of `outputs`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs_checksum: Option<String>,
}

// ---------------------------------------------------------------------------
// Status response
// ---------------------------------------------------------------------------

/// Response body for `GET /api/v1/jobs/{job_id}`.
///
/// `deny_unknown_fields` implements SRS-IF-007: unknown fields from the Runner
/// cause deserialization to fail, which the API client maps to an
/// infrastructure-failed outcome for the affected job.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobStatusResponse {
    /// Identifier from the submission.
    pub job_id: String,
    /// Workload type of the job.
    pub use_case: WorkloadType,
    /// Measurement mode of the job.
    pub mode: MeasurementMode,
    /// Repetitions requested.
    pub repetitions: u32,
    /// Current state.
    pub status: JobStatus,
    /// Non-null only while `status == "accepted"` (queued, not yet running).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_position: Option<u32>,
    /// Submission time, RFC 3339.
    pub created_at: String,
    /// Start of execution; absent while queued.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// End of execution; present only in terminal states.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// Infrastructure error; present only when the job failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Metadata from the request, echoed unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
    /// Hardware that executed the job; absent while queued.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware: Option<HardwareInfo>,
    /// Aggregated metrics; all `None` until the job has run.
    #[serde(default)]
    pub metrics: JobMetrics,
    /// Model outputs; absent while the job is running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<JobOutput>,
}

// ---------------------------------------------------------------------------
// Version response
// ---------------------------------------------------------------------------

/// Response body for `GET /api/v1/version`.
///
/// `deny_unknown_fields` implements SRS-IF-007.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionResponse {
    /// API major.minor version string, e.g. `"1.0"`.
    pub api_version: String,
    /// Crate version of the runner binary (from `CARGO_PKG_VERSION`).
    pub runner_version: String,
    /// Hardware target identifier configured in the runner, e.g. `"gr740-leon4"`.
    pub target_id: String,
    /// Operating system name, e.g. `"linux"` or `"macos"`.
    pub os: String,
    /// Versions of key tools available on the runner, e.g. `{"onnxruntime": "1.17"}`.
    pub toolchain_versions: std::collections::HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// GET /api/v1/health
// ---------------------------------------------------------------------------

/// Overall runner health (`GET /api/v1/health`).
///
/// `healthy` (200): all checks passed. `degraded` (200): core functionality
/// operational but a non-critical check (docs) failed. `unhealthy` (503): a
/// critical check failed — do not submit jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// All checks passed (HTTP 200).
    Healthy,
    /// Operational, but a non-critical check failed (HTTP 200).
    Degraded,
    /// A critical check failed; do not submit jobs (HTTP 503).
    Unhealthy,
}

/// Result of an individual health check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthCheckValue {
    /// Check passed.
    Ok,
    /// The checked resource (directory, etc.) is absent.
    Missing,
    /// The resource is optional and not configured on this runner.
    NotConfigured,
    /// Check could not be completed (unexpected error).
    Error,
}

/// Per-subsystem check results for `GET /api/v1/health`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthChecks {
    /// In-memory job store is accessible. Critical.
    pub job_store: HealthCheckValue,
    /// Configured workspace directory exists. Critical.
    pub workspace: HealthCheckValue,
    /// Configured docs directory exists; `not_configured` when unset. Non-critical.
    pub docs_dir: HealthCheckValue,
}

/// Response body for `GET /api/v1/health`.
///
/// `deny_unknown_fields` implements SRS-IF-007.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthResponse {
    /// Overall health.
    pub status: HealthStatus,
    /// Crate version of the running runner binary.
    pub runner_version: String,
    /// Hardware target identifier configured in the runner.
    pub target_id: String,
    /// Per-subsystem check results.
    pub checks: HealthChecks,
}

// ---------------------------------------------------------------------------
// GET /api/v1/docs
// ---------------------------------------------------------------------------

/// Response body for `GET /api/v1/docs`.
///
/// `deny_unknown_fields` implements SRS-IF-007.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DocsResponse {
    /// Requirement documents the Runner declares (SRS-FUNC-018).
    pub documents: Vec<StrictdocDocument>,
    /// Further evidence files, kept verbatim in the QDP.
    pub attachments: Vec<Attachment>,
}

/// A `StrictDoc` `.sdoc` file served as raw text for client-side parsing.
///
/// `deny_unknown_fields` implements SRS-IF-007.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrictdocDocument {
    /// File name on the Runner.
    pub filename: String,
    /// Title extracted from `[DOCUMENT] TITLE:` as a convenience; `null` if
    /// the server does not pre-parse the document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Full raw `StrictDoc` text, UTF-8, LF line endings.
    pub content: String,
}

/// A non-StrictDoc evidence file (PDF, spreadsheet, etc.) included verbatim
/// in the QDP. Not parsed by SQT Reporter.
///
/// `deny_unknown_fields` implements SRS-IF-007.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    /// File name on the Runner.
    pub filename: String,
    /// MIME type of the file.
    pub content_type: String,
    /// Base64-encoded file contents (standard encoding, no line breaks).
    pub data: String,
}

// ---------------------------------------------------------------------------
// Error response
// ---------------------------------------------------------------------------

/// Standard error envelope returned by the Runner on 4xx/5xx responses.
///
/// `deny_unknown_fields` implements SRS-IF-007.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorResponse {
    /// Error details.
    pub error: ApiError,
}

/// Detail block inside `ErrorResponse`.
///
/// `deny_unknown_fields` implements SRS-IF-007.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiError {
    /// Machine-readable code, e.g. `VALIDATION_ERROR` or `NOT_FOUND`.
    pub code: String,
    /// Human-readable description.
    pub message: String,
    /// Optional further context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    /// Time of the error, RFC 3339.
    pub timestamp: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies: SRS-IF-003
    #[test]
    fn job_status_is_terminal() {
        assert!(JobStatus::Complete.is_terminal());
        assert!(JobStatus::Failed.is_terminal());
        assert!(!JobStatus::Accepted.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
    }

    /// Verifies: SRS-IF-007
    #[test]
    fn job_accepted_deserializes() {
        let json = r#"{"job_id":"job-abc123","status":"accepted","queue_position":0,"created_at":"2026-03-06T10:00:00Z"}"#;
        let accepted: JobAccepted = serde_json::from_str(json).unwrap();
        assert_eq!(accepted.job_id, "job-abc123");
        assert_eq!(accepted.status, "accepted");
        assert_eq!(accepted.queue_position, 0);
    }

    /// Verifies: SRS-IF-007
    #[test]
    fn job_status_response_deserializes() {
        let json = r#"{
            "job_id": "job-abc123",
            "use_case": "image_classification",
            "mode": "latency",
            "repetitions": 10,
            "status": "complete",
            "created_at": "2026-03-06T10:00:00Z",
            "started_at": "2026-03-06T10:00:01Z",
            "completed_at": "2026-03-06T10:00:05Z",
            "metrics": {
                "return_code": 0,
                "time.avg_ms": 87.3,
                "memory.peak_rss_kb": 4096.0
            }
        }"#;
        let resp: JobStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.job_id, "job-abc123");
        assert_eq!(resp.use_case, WorkloadType::ImageClassification);
        assert_eq!(resp.status, JobStatus::Complete);
        assert!(resp.status.is_terminal());
        assert_eq!(resp.metrics.return_code, Some(0));
        assert_eq!(resp.metrics.time_avg_ms, Some(87.3));
        assert_eq!(resp.metrics.memory_peak_rss_kb, Some(4096.0));
        assert!(resp.metrics.memory_leaks_bytes.is_none());
    }

    /// Verifies: SRS-IF-007
    #[test]
    fn job_metrics_all_null_deserializes() {
        let json = r#"{
            "job_id": "job-xyz",
            "use_case": "regression",
            "mode": "latency",
            "repetitions": 1,
            "status": "running",
            "created_at": "2026-03-06T10:00:00Z",
            "metrics": {}
        }"#;
        let resp: JobStatusResponse = serde_json::from_str(json).unwrap();
        assert!(resp.metrics.return_code.is_none());
        assert!(resp.metrics.time_avg_ms.is_none());
        assert!(resp.metrics.memory_peak_rss_kb.is_none());
    }

    /// Verifies: SRS-FUNC-005
    #[test]
    fn job_metrics_get_returns_correct_field() {
        let m = JobMetrics {
            return_code: Some(0),
            time_avg_ms: Some(87.3),
            memory_peak_rss_kb: Some(4096.0),
            memory_avg_rss_kb: Some(2048.0),
            memory_leaks_bytes: Some(0),
            cpu_avg_normalized_pct: Some(24.3),
            coverage_line_pct: Some(87.2),
            static_critical_issues: Some(0),
            static_major_issues: Some(3),
            static_warnings: Some(1),
            binary_size_kb: Some(412.0),
            ..Default::default()
        };
        assert_eq!(m.get("return_code"), Some(0.0));
        assert_eq!(m.get("time.avg_ms"), Some(87.3));
        assert_eq!(m.get("memory.peak_rss_kb"), Some(4096.0));
        assert_eq!(m.get("memory.avg_rss_kb"), Some(2048.0));
        assert_eq!(m.get("memory.leaks_bytes"), Some(0.0));
        assert_eq!(m.get("cpu.avg_normalized_pct"), Some(24.3));
        assert_eq!(m.get("coverage.line_pct"), Some(87.2));
        assert_eq!(m.get("static.critical_issues"), Some(0.0));
        assert_eq!(m.get("static.major_issues"), Some(3.0));
        assert_eq!(m.get("static.warnings"), Some(1.0));
        assert_eq!(m.get("binary.size_kb"), Some(412.0));
    }

    /// Verifies: SRS-FUNC-005
    #[test]
    fn job_metrics_get_unknown_returns_none() {
        let m = JobMetrics::default();
        assert!(m.get("no_such_metric").is_none());
        assert!(m.get("return_code").is_none());
    }

    /// Verifies: SRS-REL-003
    #[test]
    fn error_response_deserializes() {
        let json = r#"{"error":{"code":"NOT_FOUND","message":"Job not found","timestamp":"2026-03-06T10:00:00Z"}}"#;
        let err: ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error.code, "NOT_FOUND");
        assert!(err.error.details.is_none());
    }

    // ----- Wire-name tests (ARCH §5.2, §5.3, §5.4) --------------------------
    // These pin the exact JSON strings that the SQT Runner API expects.
    // A changed `rename_all` attribute would silently break the contract.

    /// Verifies: SRS-IF-001, SRS-FUNC-019
    #[test]
    fn workload_type_wire_names() {
        // ARCH §5.2 defines these exact snake_case names.
        let cases: &[(WorkloadType, &str)] = &[
            (WorkloadType::ObjectDetection, "object_detection"),
            (WorkloadType::ImageClassification, "image_classification"),
            (WorkloadType::PixelSegmentation, "pixel_segmentation"),
            (WorkloadType::Classification, "classification"),
            (WorkloadType::Regression, "regression"),
            (WorkloadType::TimeSeriesForecasting, "time_series_forecasting"),
        ];
        for (variant, wire) in cases {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(json, format!("\"{wire}\""), "{variant:?} wire name wrong");
            let round: WorkloadType = serde_json::from_str(&json).unwrap();
            assert_eq!(&round, variant, "{variant:?} round-trip failed");
        }
    }

    /// Verifies: SRS-IF-001
    #[test]
    fn job_status_wire_names_all_variants() {
        // All four status values must serialize to lowercase (API contract).
        let cases: &[(JobStatus, &str)] = &[
            (JobStatus::Accepted, "\"accepted\""),
            (JobStatus::Running, "\"running\""),
            (JobStatus::Complete, "\"complete\""),
            (JobStatus::Failed, "\"failed\""),
        ];
        for (variant, wire) in cases {
            assert_eq!(&serde_json::to_string(variant).unwrap(), wire, "{variant:?}");
            let round: JobStatus = serde_json::from_str(wire).unwrap();
            assert_eq!(&round, variant, "{variant:?} round-trip failed");
        }
    }

    /// Verifies: SRS-IF-001
    #[test]
    fn instrumentation_wire_names() {
        // ARCH §5.4: instrumentation values sent to server must be lowercase.
        let cases: &[(Instrumentation, &str)] = &[
            (Instrumentation::None, "\"none\""),
            (Instrumentation::Asan, "\"asan\""),
            (Instrumentation::Valgrind, "\"valgrind\""),
            (Instrumentation::Gprof, "\"gprof\""),
        ];
        for (variant, wire) in cases {
            assert_eq!(&serde_json::to_string(variant).unwrap(), wire, "{variant:?}");
        }
    }

    /// Verifies: SRS-IF-001
    #[test]
    fn measurement_mode_wire_names() {
        // ARCH §5.3: latency / throughput.
        assert_eq!(serde_json::to_string(&MeasurementMode::Latency).unwrap(), "\"latency\"");
        assert_eq!(serde_json::to_string(&MeasurementMode::Throughput).unwrap(), "\"throughput\"");
    }

    // ----- JobRequest serialization (ARCH §5.5.1) ----------------------------

    /// Verifies: SRS-IF-002
    #[test]
    fn job_request_defaults_are_spec_compliant() {
        // ARCH §5.5.1: when only use_case is given, repetitions=1,
        // instrumentation=none, mode=latency must be assumed.
        let json = r#"{"use_case":"regression"}"#;
        let req: JobRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.repetitions, 1, "default repetitions must be 1");
        assert_eq!(
            req.instrumentation,
            Instrumentation::None,
            "default instrumentation must be none"
        );
        assert_eq!(req.mode, MeasurementMode::Latency, "default mode must be latency");
        assert!(req.input.is_none());
    }

    // ----- JobStatusResponse edge cases (ARCH §5.5.2) -----------------------

    /// Verifies: SRS-IF-007
    #[test]
    fn job_status_response_with_hardware_deserializes() {
        // ARCH §5.5.2 example: hardware descriptor with GR740 fields.
        let json = r#"{
            "job_id": "job-hw-test",
            "use_case": "image_classification",
            "mode": "latency",
            "repetitions": 10,
            "status": "complete",
            "created_at": "2026-03-06T10:00:00Z",
            "hardware": {
                "type": "cpu",
                "model": "GR740 Leon4",
                "cores": 4,
                "memory_total_kb": 131072,
                "clock_mhz": 250.0,
                "os": "RTEMS 6"
            },
            "metrics": {
                "time.avg_ms": 87.3,
                "cpu.avg_normalized_pct": 24.3
            }
        }"#;
        let resp: JobStatusResponse = serde_json::from_str(json).unwrap();
        let hw = resp.hardware.as_ref().expect("hardware block must be present");
        assert_eq!(hw.hw_type, "cpu");
        assert_eq!(hw.model.as_deref(), Some("GR740 Leon4"));
        assert_eq!(hw.cores, Some(4));
        assert_eq!(hw.clock_mhz, Some(250.0));
        assert_eq!(hw.os.as_deref(), Some("RTEMS 6"));
    }

    /// Verifies: SRS-REL-004
    #[test]
    fn job_output_result_consistent_field() {
        // ARCH §5.6 (AD-017): result_consistent records whether every repetition
        // produced bit-identical results.
        let json = r#"{
            "job_id": "job-out",
            "use_case": "classification",
            "mode": "latency",
            "repetitions": 5,
            "status": "complete",
            "created_at": "2026-03-06T10:00:00Z",
            "metrics": {},
            "output": {
                "type": "classification_labels",
                "result_consistent": true,
                "outputs_checksum": "a3f1c2d4"
            }
        }"#;
        let resp: JobStatusResponse = serde_json::from_str(json).unwrap();
        let out = resp.output.as_ref().expect("output block must be present");
        assert!(out.result_consistent, "result_consistent must be true");
        assert_eq!(out.outputs_checksum.as_deref(), Some("a3f1c2d4"));
        assert_eq!(out.output_type, "classification_labels");
    }

    /// Verifies: SRS-REL-003
    #[test]
    fn failed_job_carries_error_field() {
        // ARCH §5.5.2: a failed job has an `error` field with a description.
        let json = r#"{
            "job_id": "job-fail",
            "use_case": "regression",
            "mode": "latency",
            "repetitions": 1,
            "status": "failed",
            "created_at": "2026-03-06T10:00:00Z",
            "error": "binary not found: /opt/model.onnx",
            "metrics": {}
        }"#;
        let resp: JobStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, JobStatus::Failed);
        assert_eq!(resp.error.as_deref(), Some("binary not found: /opt/model.onnx"));
    }

    /// Verifies: SRS-FUNC-005, SRS-REL-002
    #[test]
    fn complete_job_null_metric_field_is_none() {
        // ARCH §5.5.2: "A complete job may have null metric fields — those indicate
        // unavailable tools, not infrastructure failure."
        // Explicitly null fields must deserialize to None, not error.
        let json = r#"{
            "job_id": "job-partial",
            "use_case": "image_classification",
            "mode": "latency",
            "repetitions": 1,
            "status": "complete",
            "created_at": "2026-03-06T10:00:00Z",
            "metrics": {
                "return_code": 0,
                "time.avg_ms": 87.3,
                "memory.leaks_bytes": null,
                "coverage.line_pct": null
            }
        }"#;
        let resp: JobStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.metrics.return_code, Some(0));
        assert_eq!(resp.metrics.time_avg_ms, Some(87.3));
        assert!(
            resp.metrics.memory_leaks_bytes.is_none(),
            "explicit null must deserialize as None"
        );
        assert!(resp.metrics.coverage_line_pct.is_none());
    }

    /// Verifies: SRS-REL-003
    #[test]
    fn error_response_with_details_deserializes() {
        // ARCH §5.8: details field is optional.
        let json = r#"{
            "error": {
                "code": "VALIDATION_ERROR",
                "message": "unknown model type: 'depth_estimation'",
                "details": "Supported types: object_detection, image_classification, ...",
                "timestamp": "2026-03-06T10:00:00Z"
            }
        }"#;
        let err: ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error.code, "VALIDATION_ERROR");
        assert!(err.error.details.is_some());
        assert!(err.error.details.unwrap().contains("object_detection"));
    }

    /// Verifies: SRS-IF-003
    #[test]
    fn failed_job_is_terminal_but_not_complete() {
        // wait_for_job branches on Failed vs Complete separately.
        // Complete → return Ok; Failed → bail! with error message.
        assert!(JobStatus::Failed.is_terminal());
        assert!(JobStatus::Complete.is_terminal());
        assert_ne!(JobStatus::Failed, JobStatus::Complete);
        assert!(!JobStatus::Accepted.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
    }

    /// Verifies: SRS-FUNC-019
    #[test]
    fn workload_type_default_is_image_classification() {
        // TestSpec derives Default which falls through to WorkloadType::default().
        // This must be ImageClassification so unset specs have a defined workload.
        assert_eq!(WorkloadType::default(), WorkloadType::ImageClassification);
    }
}
