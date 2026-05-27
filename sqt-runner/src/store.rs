use std::collections::HashMap;
use std::sync::Arc;

use sqt_types::{JobMetrics, JobRequest, JobStatus, WorkloadType};
use tokio::sync::RwLock;

/// In-memory state for a single job.
pub struct JobState {
    pub id: String,
    pub workload_type: WorkloadType,
    pub repetitions: u32,
    pub tags: Option<HashMap<String, String>>,
    pub status: JobStatus,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error: Option<String>,
    pub metrics: Option<JobMetrics>,
}

impl JobState {
    pub fn new(id: String, request: &JobRequest) -> Self {
        Self {
            id,
            workload_type: request.model.clone(),
            repetitions: request.repetitions,
            tags: request.tags.clone(),
            status: JobStatus::Accepted,
            created_at: crate::now_iso(),
            started_at: None,
            completed_at: None,
            error: None,
            metrics: None,
        }
    }
}

pub type JobStore = Arc<RwLock<HashMap<String, JobState>>>;

pub fn new_store() -> JobStore {
    Arc::new(RwLock::new(HashMap::new()))
}

#[cfg(test)]
mod tests {
    use sqt_types::{Instrumentation, JobRequest, JobStatus, MeasurementMode, WorkloadType};

    use super::*;

    fn make_request(model: WorkloadType, repetitions: u32) -> JobRequest {
        JobRequest {
            model,
            input: None,
            repetitions,
            instrumentation: Instrumentation::default(),
            mode: MeasurementMode::default(),
            warmup_repetitions: None,
            timeout_secs: None,
            tags: None,
        }
    }

    #[test]
    fn job_state_new_sets_accepted_status() {
        let req = make_request(WorkloadType::ImageClassification, 1);
        let job = JobState::new("job-1".into(), &req);
        assert_eq!(job.status, JobStatus::Accepted);
    }

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

    #[test]
    fn job_state_new_optional_fields_are_none() {
        let req = make_request(WorkloadType::Regression, 1);
        let job = JobState::new("job-3".into(), &req);
        assert!(job.started_at.is_none());
        assert!(job.completed_at.is_none());
        assert!(job.error.is_none());
        assert!(job.metrics.is_none());
    }

    #[test]
    fn new_store_is_empty() {
        let store = new_store();
        let guard = store.try_read().unwrap();
        assert!(guard.is_empty());
    }
}
