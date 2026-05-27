use std::collections::HashMap;
use std::sync::Arc;

use sqt_api_types::{JobMetrics, JobStatus, WorkloadType};
use tokio::sync::RwLock;

/// In-memory state for a single job.
pub struct JobState {
    pub id: String,
    pub workload_type: WorkloadType,
    pub status: JobStatus,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error: Option<String>,
    pub metrics: Option<JobMetrics>,
}

impl JobState {
    pub fn new(id: String, workload_type: WorkloadType) -> Self {
        Self {
            id,
            workload_type,
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
