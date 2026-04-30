use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Enumerations — must match the values in job-api.openapi.yaml exactly
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadType {
    ObjectDetection,
    ImageClassification,
    PixelSegmentation,
    Classification,
    Regression,
    TimeSeriesForecasting,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Instrumentation {
    #[default]
    None,
    Asan,
    Valgrind,
    Gprof,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementMode {
    #[default]
    Latency,
    Throughput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Accepted,
    Running,
    Complete,
    Failed,
}

// ---------------------------------------------------------------------------
// Job request
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    pub model: WorkloadType,

    /// Workload input (images, tabular data, time series, or a dataset reference).
    /// `None` when the server has a default dataset configured for this workload.
    pub input: Option<serde_json::Value>,

    #[serde(default = "default_repetitions")]
    pub repetitions: u32,

    #[serde(default)]
    pub instrumentation: Instrumentation,

    #[serde(default)]
    pub mode: MeasurementMode,

    #[serde(default)]
    pub warmup_repetitions: u32,

    pub timeout_secs: Option<u64>,

    /// Arbitrary key-value tags passed through to the response for traceability
    /// (e.g. requirement UID, git SHA).
    pub tags: Option<HashMap<String, String>>,
}

fn default_repetitions() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// Accepted response (202)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobAccepted {
    pub job_id: String,
    pub status: String,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Hardware descriptor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hardware {
    #[serde(rename = "type")]
    pub hw_type: String,
    pub model: String,
    pub cores: u32,
    pub memory_total_kb: u64,
    pub accelerator_model: Option<String>,
    pub clock_mhz: Option<f64>,
    pub os: Option<String>,
}

// ---------------------------------------------------------------------------
// Metrics — all fields nullable; null means tool unavailable or not configured.
// Field names must match the OpenAPI spec exactly (dotted notation via rename).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobMetrics {
    pub return_code: Option<i32>,

    #[serde(rename = "time.avg_ms")]
    pub time_avg_ms: Option<f64>,
    #[serde(rename = "time.latency_p50_ms")]
    pub time_latency_p50_ms: Option<f64>,
    #[serde(rename = "time.latency_p95_ms")]
    pub time_latency_p95_ms: Option<f64>,
    #[serde(rename = "time.latency_p99_ms")]
    pub time_latency_p99_ms: Option<f64>,
    #[serde(rename = "time.throughput_fps")]
    pub time_throughput_fps: Option<f64>,

    #[serde(rename = "memory.avg_rss_kb")]
    pub memory_avg_rss_kb: Option<f64>,
    #[serde(rename = "memory.peak_rss_kb")]
    pub memory_peak_rss_kb: Option<f64>,
    #[serde(rename = "memory.leaks_bytes")]
    pub memory_leaks_bytes: Option<i64>,

    #[serde(rename = "cpu.avg_normalized_pct")]
    pub cpu_avg_normalized_pct: Option<f64>,

    #[serde(rename = "coverage.line_pct")]
    pub coverage_line_pct: Option<f64>,
    #[serde(rename = "coverage.branch_pct")]
    pub coverage_branch_pct: Option<f64>,
    #[serde(rename = "coverage.function_pct")]
    pub coverage_function_pct: Option<f64>,
    #[serde(rename = "coverage.lines_covered")]
    pub coverage_lines_covered: Option<i64>,
    #[serde(rename = "coverage.lines_total")]
    pub coverage_lines_total: Option<i64>,

    #[serde(rename = "static.warnings")]
    pub static_warnings: Option<i64>,
    #[serde(rename = "static.critical_issues")]
    pub static_critical_issues: Option<i64>,
    #[serde(rename = "static.major_issues")]
    pub static_major_issues: Option<i64>,

    #[serde(rename = "binary.size_kb")]
    pub binary_size_kb: Option<f64>,
}

// ---------------------------------------------------------------------------
// Job output
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobOutput {
    #[serde(rename = "type")]
    pub output_type: String,
    pub result_consistent: Option<bool>,
    /// One entry per input item regardless of workload type.
    pub outputs: Option<Vec<serde_json::Value>>,
    /// SHA-256 hex digest of the outputs array for reproducibility verification.
    pub outputs_checksum: Option<String>,
}

// ---------------------------------------------------------------------------
// Full job status response (returned by GET /api/v1/jobs/{job_id})
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatusResponse {
    pub job_id: String,
    pub model: WorkloadType,
    pub mode: MeasurementMode,
    pub repetitions: u32,
    pub status: JobStatus,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    /// Null until the job reaches a terminal state.
    pub hardware: Option<Hardware>,
    /// Null until the job reaches a terminal state.
    pub metrics: Option<JobMetrics>,
    /// Null until the job reaches a terminal state.
    pub output: Option<JobOutput>,
    /// Set on `failed` status; null otherwise.
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Requirements documents (GET /api/v1/docs)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrictdocDocument {
    pub filename: String,
    pub title: Option<String>,
    /// Full raw StrictDoc text. SQT Generic parses this client-side.
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocsResponse {
    pub documents: Vec<StrictdocDocument>,
}

