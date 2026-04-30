use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::types::{JobMetrics, JobOutput, JobStatus, Hardware, MeasurementMode, WorkloadType};

// ---------------------------------------------------------------------------
// Internal job record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct JobRecord {
    pub id: String,
    pub model: WorkloadType,
    pub mode: MeasurementMode,
    pub repetitions: u32,
    pub status: JobStatus,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub hardware: Option<Hardware>,
    pub metrics: Option<JobMetrics>,
    pub output: Option<JobOutput>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Shared job store — an RwLock-protected map of job_id -> JobRecord
// ---------------------------------------------------------------------------

pub type JobStore = Arc<RwLock<HashMap<String, JobRecord>>>;

pub fn new_store() -> JobStore {
    Arc::new(RwLock::new(HashMap::new()))
}
