use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Enumerations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadType {
    ObjectDetection,
    ImageClassification,
    PixelSegmentation,
    Classification,
    Regression,
    TimeSeriesForecasting,
}

impl Default for WorkloadType {
    fn default() -> Self {
        Self::ImageClassification
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Instrumentation {
    None,
    Asan,
    Valgrind,
    Gprof,
}

impl Default for Instrumentation {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeasurementMode {
    Latency,
    Throughput,
}

impl Default for MeasurementMode {
    fn default() -> Self {
        Self::Latency
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Accepted,
    Running,
    Complete,
    Failed,
}

impl JobStatus {
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
    pub model: WorkloadType,
    /// Base64-encoded ZIP containing the customer's AI model and
    /// pre/post-processing code. `None` falls back to server-configured model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_zip: Option<String>,
    /// Workload-specific input data (images, tabular rows, time series, or
    /// dataset reference). `None` when the server has a default dataset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    #[serde(default = "default_repetitions")]
    pub repetitions: u32,
    #[serde(default)]
    pub instrumentation: Instrumentation,
    #[serde(default)]
    pub mode: MeasurementMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warmup_repetitions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
}

fn default_repetitions() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// Accepted response
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
pub struct HardwareInfo {
    #[serde(rename = "type")]
    pub hw_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cores: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_total_kb: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accelerator_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_mhz: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
}

// ---------------------------------------------------------------------------
// Unified metrics (all fields nullable; null = tool unavailable)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct JobMetrics {
    pub return_code: Option<i64>,

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
    #[serde(rename = "time.startup_ms")]
    pub time_startup_ms: Option<f64>,
    #[serde(rename = "time.first_inference_ms")]
    pub time_first_inference_ms: Option<f64>,

    #[serde(rename = "memory.avg_rss_kb")]
    pub memory_avg_rss_kb: Option<f64>,
    #[serde(rename = "memory.peak_rss_kb")]
    pub memory_peak_rss_kb: Option<f64>,
    #[serde(rename = "memory.leaks_bytes")]
    pub memory_leaks_bytes: Option<i64>,
    #[serde(rename = "memory.stack_kb")]
    pub memory_stack_kb: Option<f64>,
    #[serde(rename = "memory.heap_allocations")]
    pub memory_heap_allocations: Option<i64>,

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

    #[serde(rename = "power.avg_watts")]
    pub power_avg_watts: Option<f64>,
    #[serde(rename = "power.peak_watts")]
    pub power_peak_watts: Option<f64>,

    #[serde(rename = "hardware.temperature_celsius")]
    pub hardware_temperature_celsius: Option<f64>,
}

impl JobMetrics {
    /// Look up a metric value by its wire name. Returns `None` when the field
    /// is absent or the name is unknown.
    #[allow(clippy::cast_precision_loss)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobOutput {
    #[serde(rename = "type")]
    pub output_type: String,
    pub result_consistent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs_checksum: Option<String>,
}

// ---------------------------------------------------------------------------
// Status response
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatusResponse {
    pub job_id: String,
    pub model: WorkloadType,
    pub mode: MeasurementMode,
    pub repetitions: u32,
    pub status: JobStatus,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware: Option<HardwareInfo>,
    #[serde(default)]
    pub metrics: JobMetrics,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<JobOutput>,
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: ApiError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    pub timestamp: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_status_is_terminal() {
        assert!(JobStatus::Complete.is_terminal());
        assert!(JobStatus::Failed.is_terminal());
        assert!(!JobStatus::Accepted.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
    }

    #[test]
    fn job_accepted_deserializes() {
        let json =
            r#"{"job_id":"job-abc123","status":"accepted","created_at":"2026-03-06T10:00:00Z"}"#;
        let accepted: JobAccepted = serde_json::from_str(json).unwrap();
        assert_eq!(accepted.job_id, "job-abc123");
        assert_eq!(accepted.status, "accepted");
    }

    #[test]
    fn job_status_response_deserializes() {
        let json = r#"{
            "job_id": "job-abc123",
            "model": "image_classification",
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
        assert_eq!(resp.model, WorkloadType::ImageClassification);
        assert_eq!(resp.status, JobStatus::Complete);
        assert!(resp.status.is_terminal());
        assert_eq!(resp.metrics.return_code, Some(0));
        assert_eq!(resp.metrics.time_avg_ms, Some(87.3));
        assert_eq!(resp.metrics.memory_peak_rss_kb, Some(4096.0));
        assert!(resp.metrics.memory_leaks_bytes.is_none());
    }

    #[test]
    fn job_metrics_all_null_deserializes() {
        let json = r#"{
            "job_id": "job-xyz",
            "model": "regression",
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

    #[test]
    fn job_metrics_get_unknown_returns_none() {
        let m = JobMetrics::default();
        assert!(m.get("no_such_metric").is_none());
        assert!(m.get("return_code").is_none());
    }

    #[test]
    fn job_request_model_zip_optional() {
        let json = r#"{"model":"image_classification"}"#;
        let req: JobRequest = serde_json::from_str(json).unwrap();
        assert!(req.model_zip.is_none());

        let with_zip = r#"{"model":"image_classification","model_zip":"abc123=="}"#;
        let req2: JobRequest = serde_json::from_str(with_zip).unwrap();
        assert_eq!(req2.model_zip.as_deref(), Some("abc123=="));
    }

    #[test]
    fn error_response_deserializes() {
        let json = r#"{"error":{"code":"NOT_FOUND","message":"Job not found","timestamp":"2026-03-06T10:00:00Z"}}"#;
        let err: ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error.code, "NOT_FOUND");
        assert!(err.error.details.is_none());
    }

    // ----- Wire-name tests (ARCH §5.2, §5.3, §5.4) --------------------------
    // These pin the exact JSON strings that the Klepsydra API server expects.
    // A changed `rename_all` attribute would silently break the contract.

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

    #[test]
    fn measurement_mode_wire_names() {
        // ARCH §5.3: latency / throughput.
        assert_eq!(serde_json::to_string(&MeasurementMode::Latency).unwrap(), "\"latency\"");
        assert_eq!(
            serde_json::to_string(&MeasurementMode::Throughput).unwrap(),
            "\"throughput\""
        );
    }

    // ----- JobRequest serialization (ARCH §5.5.1) ----------------------------

    #[test]
    fn job_request_defaults_are_spec_compliant() {
        // ARCH §5.5.1: when only model is given, repetitions=1,
        // instrumentation=none, mode=latency must be assumed.
        let json = r#"{"model":"regression"}"#;
        let req: JobRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.repetitions, 1, "default repetitions must be 1");
        assert_eq!(req.instrumentation, Instrumentation::None, "default instrumentation must be none");
        assert_eq!(req.mode, MeasurementMode::Latency, "default mode must be latency");
        assert!(req.model_zip.is_none());
        assert!(req.input.is_none());
    }

    #[test]
    fn job_request_absent_model_zip_omitted_from_json() {
        // ARCH §5.5.1: `model_zip` must NOT appear in the request body when None
        // (skip_serializing_if). Sending `"model_zip": null` would be a protocol error.
        let req = JobRequest {
            model: WorkloadType::Classification,
            model_zip: None,
            input: None,
            repetitions: 1,
            instrumentation: Instrumentation::default(),
            mode: MeasurementMode::default(),
            warmup_repetitions: None,
            timeout_secs: None,
            tags: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(
            !json.contains("model_zip"),
            "absent model_zip must be omitted from JSON, got: {json}"
        );
    }

    #[test]
    fn job_request_model_zip_present_serializes_correctly() {
        // model_zip must appear as a base64 string when present.
        let req = JobRequest {
            model: WorkloadType::ImageClassification,
            model_zip: Some("aGVsbG8=".into()), // base64("hello")
            input: None,
            repetitions: 1,
            instrumentation: Instrumentation::default(),
            mode: MeasurementMode::default(),
            warmup_repetitions: None,
            timeout_secs: None,
            tags: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"model_zip\":\"aGVsbG8=\""), "model_zip missing in: {json}");
        let back: JobRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model_zip.as_deref(), Some("aGVsbG8="));
    }

    // ----- JobStatusResponse edge cases (ARCH §5.5.2) -----------------------

    #[test]
    fn job_status_response_with_hardware_deserializes() {
        // ARCH §5.5.2 example: hardware descriptor with GR740 fields.
        let json = r#"{
            "job_id": "job-hw-test",
            "model": "image_classification",
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

    #[test]
    fn job_output_result_consistent_field() {
        // ARCH §5.6 (AD-017): result_consistent records whether every repetition
        // produced bit-identical results.
        let json = r#"{
            "job_id": "job-out",
            "model": "classification",
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

    #[test]
    fn failed_job_carries_error_field() {
        // ARCH §5.5.2: a failed job has an `error` field with a description.
        let json = r#"{
            "job_id": "job-fail",
            "model": "regression",
            "mode": "latency",
            "repetitions": 1,
            "status": "failed",
            "created_at": "2026-03-06T10:00:00Z",
            "error": "binary not found: /opt/model.onnx",
            "metrics": {}
        }"#;
        let resp: JobStatusResponse = serde_json::from_str(json).unwrap();
        assert!(resp.status.is_terminal());
        assert_eq!(
            resp.error.as_deref(),
            Some("binary not found: /opt/model.onnx")
        );
    }

    #[test]
    fn complete_job_null_metric_field_is_none() {
        // ARCH §5.5.2: "A complete job may have null metric fields — those indicate
        // unavailable tools, not infrastructure failure."
        // Explicitly null fields must deserialize to None, not error.
        let json = r#"{
            "job_id": "job-partial",
            "model": "image_classification",
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

    #[test]
    fn workload_type_default_is_image_classification() {
        // TestSpec derives Default which falls through to WorkloadType::default().
        // This must be ImageClassification so unset specs have a defined workload.
        assert_eq!(WorkloadType::default(), WorkloadType::ImageClassification);
    }
}
