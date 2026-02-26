use std::collections::HashMap;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Running,
    Complete,
    Failed,
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    pub target: ApiTarget,
    pub tasks: Vec<TaskSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiTarget {
    pub binary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobAccepted {
    pub job_id: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatusResponse {
    pub job_id: String,
    pub status: JobStatus,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub tasks: HashMap<String, TaskResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<HashMap<String, String>>,
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
    fn job_accepted_deserializes() {
        let json = r#"{"job_id":"job-abc123","status":"accepted","created_at":"2026-02-26T10:00:00Z"}"#;
        let accepted: JobAccepted = serde_json::from_str(json).unwrap();
        assert_eq!(accepted.job_id, "job-abc123");
        assert_eq!(accepted.status, "accepted");
    }

    #[test]
    fn job_status_response_deserializes() {
        let json = r#"{
            "job_id": "job-abc123",
            "status": "complete",
            "created_at": "2026-02-26T10:00:00Z",
            "started_at": "2026-02-26T10:00:01Z",
            "tasks": {
                "return_code": {"status": "complete", "value": 0.0, "unit": ""},
                "time.execution": {"status": "complete", "value": 87.3, "unit": "ms"},
                "memory.leaks": {"status": "failed", "error": "valgrind not found"}
            }
        }"#;
        let resp: JobStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.job_id, "job-abc123");
        assert_eq!(resp.status, JobStatus::Complete);
        assert!(resp.status.is_terminal());
        assert_eq!(resp.tasks.len(), 3);

        let rc = &resp.tasks["return_code"];
        assert_eq!(rc.status, TaskStatus::Complete);
        assert_eq!(rc.value, Some(0.0));

        let leaks = &resp.tasks["memory.leaks"];
        assert_eq!(leaks.status, TaskStatus::Failed);
        assert_eq!(leaks.error.as_deref(), Some("valgrind not found"));
    }

    #[test]
    fn error_response_deserializes() {
        let json = r#"{"error":{"code":"NOT_FOUND","message":"Job not found","timestamp":"2026-02-26T10:00:00Z"}}"#;
        let err: ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error.code, "NOT_FOUND");
        assert_eq!(err.error.message, "Job not found");
        assert!(err.error.details.is_none());
    }

    #[test]
    fn task_spec_skips_null_config_on_serialization() {
        let spec = TaskSpec { name: "return_code".into(), config: None };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(!json.contains("config"));
    }
}
