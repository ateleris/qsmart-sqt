use std::collections::HashMap;
use std::sync::Arc;

use sqt::api::types::{AnalysisMetrics, JobStatus, RunMetrics};
use tokio::sync::RwLock;

/// What kind of job was submitted.
pub enum JobKind {
    Run {
        binary: String,
        input: Option<String>,
        timeout_secs: u64,
    },
    Analysis {
        binary: String,
        timeout_secs: u64,
    },
}

/// In-memory state for a single job.
pub struct JobState {
    pub id: String,
    pub kind: JobKind,
    pub status: JobStatus,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error: Option<String>,
    pub run_metrics: Option<RunMetrics>,
    pub analysis_metrics: Option<AnalysisMetrics>,
}

impl JobState {
    pub fn new_run(id: String, binary: String, input: Option<String>, timeout_secs: u64) -> Self {
        Self {
            id,
            kind: JobKind::Run {
                binary,
                input,
                timeout_secs,
            },
            status: JobStatus::Accepted,
            created_at: crate::now_iso(),
            started_at: None,
            completed_at: None,
            error: None,
            run_metrics: None,
            analysis_metrics: None,
        }
    }

    pub fn new_analysis(id: String, binary: String, timeout_secs: u64) -> Self {
        Self {
            id,
            kind: JobKind::Analysis {
                binary,
                timeout_secs,
            },
            status: JobStatus::Accepted,
            created_at: crate::now_iso(),
            started_at: None,
            completed_at: None,
            error: None,
            run_metrics: None,
            analysis_metrics: None,
        }
    }

    /// Helper: extract the binary path regardless of job kind.
    pub fn binary(&self) -> &str {
        match &self.kind {
            JobKind::Run { binary, .. } | JobKind::Analysis { binary, .. } => binary,
        }
    }
}

pub type JobStore = Arc<RwLock<HashMap<String, JobState>>>;

pub fn new_store() -> JobStore {
    Arc::new(RwLock::new(HashMap::new()))
}
