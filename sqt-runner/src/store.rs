// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! The in-memory job store. Jobs are kept until the Runner stops.

use std::{collections::HashMap, sync::Arc};

use sqt_types::{JobMetrics, JobOutput, JobRequest, JobStatus, WorkloadType};
use tokio::sync::RwLock;

/// In-memory state for a single job.
pub struct JobState {
    /// Job id, a UUID.
    pub id: String,
    /// Workload type from the request.
    pub workload_type: WorkloadType,
    /// Repetitions from the request.
    pub repetitions: u32,
    /// Tags from the request, echoed in the status.
    pub tags: Option<HashMap<String, String>>,
    /// Current state.
    pub status: JobStatus,
    /// Submission time, RFC 3339.
    pub created_at: String,
    /// Start of execution.
    pub started_at: Option<String>,
    /// End of execution.
    pub completed_at: Option<String>,
    /// Why the job failed.
    pub error: Option<String>,
    /// Metrics, once the job has run.
    pub metrics: Option<JobMetrics>,
    /// Inference output, once the job has run.
    pub output: Option<JobOutput>,
}

impl JobState {
    /// A newly accepted job.
    pub fn new(id: String, request: &JobRequest) -> Self {
        Self {
            id,
            workload_type: request.use_case.clone(),
            repetitions: request.repetitions,
            tags: request.tags.clone(),
            status: JobStatus::Accepted,
            created_at: crate::now_iso(),
            started_at: None,
            completed_at: None,
            error: None,
            metrics: None,
            output: None,
        }
    }
}

/// Shared, lockable map of all jobs by id.
pub type JobStore = Arc<RwLock<HashMap<String, JobState>>>;

/// An empty job store.
pub fn new_store() -> JobStore {
    Arc::new(RwLock::new(HashMap::new()))
}

#[cfg(test)]
mod tests {
    use sqt_types::{Instrumentation, JobRequest, JobStatus, MeasurementMode, WorkloadType};

    use super::*;

    fn make_request(use_case: WorkloadType, repetitions: u32) -> JobRequest {
        JobRequest {
            use_case,
            input: None,
            repetitions,
            instrumentation: Instrumentation::default(),
            mode: MeasurementMode::default(),
            warmup_repetitions: None,
            timeout_secs: None,
            tags: None,
        }
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn job_state_new_sets_accepted_status() {
        let req = make_request(WorkloadType::ImageClassification, 1);
        let job = JobState::new("job-1".into(), &req);
        assert_eq!(job.status, JobStatus::Accepted);
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn job_state_new_copies_fields_from_request() {
        let mut tags = HashMap::new();
        tags.insert("env".into(), "ci".into());
        let mut req = make_request(WorkloadType::ObjectDetection, 5);
        req.tags = Some(tags.clone());

        let job = JobState::new("job-2".into(), &req);

        assert_eq!(job.id, "job-2");
        assert_eq!(job.workload_type, WorkloadType::ObjectDetection);
        assert_eq!(job.repetitions, 5);
        assert_eq!(job.tags, Some(tags));
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn job_state_new_optional_fields_are_none() {
        let req = make_request(WorkloadType::Regression, 1);
        let job = JobState::new("job-3".into(), &req);
        assert!(job.started_at.is_none());
        assert!(job.completed_at.is_none());
        assert!(job.error.is_none());
        assert!(job.metrics.is_none());
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn new_store_is_empty() {
        let store = new_store();
        let guard = store.try_read().unwrap();
        assert!(guard.is_empty());
    }
}
