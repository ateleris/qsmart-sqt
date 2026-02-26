use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Enumerations
// ---------------------------------------------------------------------------

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
        matches!(self, JobStatus::Complete | JobStatus::Failed)
    }
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    pub binary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisRequest {
    pub binary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

// ---------------------------------------------------------------------------
// Accepted responses
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunAccepted {
    pub run_id: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisAccepted {
    pub analysis_id: String,
    pub status: String,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Fixed-schema metrics
// ---------------------------------------------------------------------------

/// Per-run metrics returned by `GET /api/v1/runs/{run_id}`.
///
/// All fields are `Option` so that in-progress responses (with no results yet)
/// deserialize without error.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RunMetrics {
    pub return_code: Option<i64>,
    #[serde(rename = "time.execution")]
    pub time_execution: Option<f64>,
    #[serde(rename = "memory.peak_rss")]
    pub memory_peak_rss: Option<f64>,
    #[serde(rename = "memory.avg_rss")]
    pub memory_avg_rss: Option<f64>,
    #[serde(rename = "cpu.max_pct")]
    pub cpu_max_pct: Option<f64>,
    #[serde(rename = "cpu.avg_pct")]
    pub cpu_avg_pct: Option<f64>,
    #[serde(rename = "memory.leaks")]
    pub memory_leaks: Option<f64>,
    #[serde(rename = "coverage.line_pct")]
    pub coverage_line_pct: Option<f64>,
    #[serde(rename = "ml.accuracy")]
    pub ml_accuracy: Option<f64>,
    #[serde(rename = "ml.f1_score")]
    pub ml_f1_score: Option<f64>,
    #[serde(rename = "ml.recall")]
    pub ml_recall: Option<f64>,
    #[serde(rename = "ml.precision")]
    pub ml_precision: Option<f64>,
}

impl RunMetrics {
    /// Look up a metric value by its wire name.
    ///
    /// Returns `None` if the field is absent or the name is unknown.
    #[allow(clippy::cast_precision_loss)]
    pub fn get(&self, name: &str) -> Option<f64> {
        match name {
            "return_code" => self.return_code.map(|v| v as f64),
            "time.execution" => self.time_execution,
            "memory.peak_rss" => self.memory_peak_rss,
            "memory.avg_rss" => self.memory_avg_rss,
            "cpu.max_pct" => self.cpu_max_pct,
            "cpu.avg_pct" => self.cpu_avg_pct,
            "memory.leaks" => self.memory_leaks,
            "coverage.line_pct" => self.coverage_line_pct,
            "ml.accuracy" => self.ml_accuracy,
            "ml.f1_score" => self.ml_f1_score,
            "ml.recall" => self.ml_recall,
            "ml.precision" => self.ml_precision,
            _ => None,
        }
    }
}

/// Per-binary analysis metrics returned by `GET /api/v1/analysis/{analysis_id}`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisMetrics {
    #[serde(rename = "sonar.critical_issues")]
    pub sonar_critical_issues: Option<i64>,
    #[serde(rename = "sonar.major_issues")]
    pub sonar_major_issues: Option<i64>,
    #[serde(rename = "static.warnings")]
    pub static_warnings: Option<i64>,
    #[serde(rename = "binary.size_kb")]
    pub binary_size_kb: Option<f64>,
}

impl AnalysisMetrics {
    /// Look up a metric value by its wire name.
    ///
    /// Returns `None` if the field is absent or the name is unknown.
    #[allow(clippy::cast_precision_loss)]
    pub fn get(&self, name: &str) -> Option<f64> {
        match name {
            "sonar.critical_issues" => self.sonar_critical_issues.map(|v| v as f64),
            "sonar.major_issues" => self.sonar_major_issues.map(|v| v as f64),
            "static.warnings" => self.static_warnings.map(|v| v as f64),
            "binary.size_kb" => self.binary_size_kb,
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Status responses
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStatusResponse {
    pub run_id: String,
    pub binary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    pub status: JobStatus,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub metrics: RunMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisStatusResponse {
    pub analysis_id: String,
    pub binary: String,
    pub status: JobStatus,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub metrics: AnalysisMetrics,
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
    fn job_status_is_terminal_for_complete_and_failed() {
        assert!(JobStatus::Complete.is_terminal());
        assert!(JobStatus::Failed.is_terminal());
        assert!(!JobStatus::Accepted.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
    }

    #[test]
    fn run_accepted_deserializes() {
        let json =
            r#"{"run_id":"run-abc123","status":"accepted","created_at":"2026-02-26T10:00:00Z"}"#;
        let accepted: RunAccepted = serde_json::from_str(json).unwrap();
        assert_eq!(accepted.run_id, "run-abc123");
        assert_eq!(accepted.status, "accepted");
    }

    #[test]
    fn run_status_response_deserializes() {
        let json = r#"{
            "run_id": "run-abc123",
            "binary": "/usr/bin/my-app",
            "input": "test_input.bin",
            "status": "complete",
            "created_at": "2026-02-26T10:00:00Z",
            "started_at": "2026-02-26T10:00:01Z",
            "completed_at": "2026-02-26T10:00:05Z",
            "metrics": {
                "return_code": 0,
                "time.execution": 87.3,
                "memory.peak_rss": 4096.0
            }
        }"#;
        let resp: RunStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.run_id, "run-abc123");
        assert_eq!(resp.status, JobStatus::Complete);
        assert!(resp.status.is_terminal());
        assert_eq!(resp.metrics.return_code, Some(0));
        assert_eq!(resp.metrics.time_execution, Some(87.3));
        assert_eq!(resp.metrics.memory_peak_rss, Some(4096.0));
        assert!(resp.metrics.memory_leaks.is_none());
    }

    #[test]
    fn run_metrics_all_null_deserializes() {
        let json = r#"{
            "run_id": "run-xyz",
            "binary": "/bin/app",
            "status": "running",
            "created_at": "2026-02-26T10:00:00Z",
            "metrics": {}
        }"#;
        let resp: RunStatusResponse = serde_json::from_str(json).unwrap();
        assert!(resp.metrics.return_code.is_none());
        assert!(resp.metrics.time_execution.is_none());
        assert!(resp.metrics.memory_peak_rss.is_none());
    }

    #[test]
    fn run_metrics_get_returns_correct_field() {
        let m = RunMetrics {
            return_code: Some(0),
            time_execution: Some(87.3),
            memory_peak_rss: Some(4096.0),
            memory_avg_rss: Some(2048.0),
            cpu_max_pct: Some(95.0),
            cpu_avg_pct: Some(50.0),
            memory_leaks: Some(0.0),
            coverage_line_pct: Some(85.5),
            ml_accuracy: Some(0.97),
            ml_f1_score: Some(0.95),
            ml_recall: Some(0.94),
            ml_precision: Some(0.96),
        };
        assert_eq!(m.get("return_code"), Some(0.0));
        assert_eq!(m.get("time.execution"), Some(87.3));
        assert_eq!(m.get("memory.peak_rss"), Some(4096.0));
        assert_eq!(m.get("memory.avg_rss"), Some(2048.0));
        assert_eq!(m.get("cpu.max_pct"), Some(95.0));
        assert_eq!(m.get("cpu.avg_pct"), Some(50.0));
        assert_eq!(m.get("memory.leaks"), Some(0.0));
        assert_eq!(m.get("coverage.line_pct"), Some(85.5));
        assert_eq!(m.get("ml.accuracy"), Some(0.97));
        assert_eq!(m.get("ml.f1_score"), Some(0.95));
        assert_eq!(m.get("ml.recall"), Some(0.94));
        assert_eq!(m.get("ml.precision"), Some(0.96));
    }

    #[test]
    fn run_metrics_get_unknown_returns_none() {
        let m = RunMetrics::default();
        assert!(m.get("no_such_metric").is_none());
        assert!(m.get("return_code").is_none()); // field present but None
    }

    #[test]
    fn analysis_status_response_deserializes() {
        let json = r#"{
            "analysis_id": "analysis-001",
            "binary": "/usr/bin/my-app",
            "status": "complete",
            "created_at": "2026-02-26T10:00:00Z",
            "metrics": {
                "sonar.critical_issues": 2,
                "sonar.major_issues": 5,
                "static.warnings": 3,
                "binary.size_kb": 1024.5
            }
        }"#;
        let resp: AnalysisStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.analysis_id, "analysis-001");
        assert_eq!(resp.status, JobStatus::Complete);
        assert_eq!(resp.metrics.sonar_critical_issues, Some(2));
        assert_eq!(resp.metrics.sonar_major_issues, Some(5));
        assert_eq!(resp.metrics.static_warnings, Some(3));
        assert_eq!(resp.metrics.binary_size_kb, Some(1024.5));
    }

    #[test]
    fn analysis_metrics_get_returns_correct_field() {
        let m = AnalysisMetrics {
            sonar_critical_issues: Some(2),
            sonar_major_issues: Some(5),
            static_warnings: Some(3),
            binary_size_kb: Some(1024.5),
        };
        assert_eq!(m.get("sonar.critical_issues"), Some(2.0));
        assert_eq!(m.get("sonar.major_issues"), Some(5.0));
        assert_eq!(m.get("static.warnings"), Some(3.0));
        assert_eq!(m.get("binary.size_kb"), Some(1024.5));
        assert!(m.get("unknown").is_none());
    }

    #[test]
    fn error_response_deserializes() {
        let json = r#"{"error":{"code":"NOT_FOUND","message":"Run not found","timestamp":"2026-02-26T10:00:00Z"}}"#;
        let err: ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error.code, "NOT_FOUND");
        assert_eq!(err.error.message, "Run not found");
        assert!(err.error.details.is_none());
    }
}
