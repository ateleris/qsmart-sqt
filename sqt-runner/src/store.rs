// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! The in-memory job store. Jobs waiting or running are always kept; of the
//! finished ones, only the most recent are kept (`max_finished_jobs`).

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use sqt_types::{JobMetrics, JobOutput, JobRequest, JobStatus, MeasurementMode, WorkloadType};
use tokio::sync::RwLock;

/// Gives every accepted job its place in the order of submission.
static NEXT_SEQ: AtomicU64 = AtomicU64::new(0);

/// In-memory state for a single job.
pub struct JobState {
    /// Job id, a UUID.
    pub id: String,
    /// Order of submission, assigned while the store's write lock is held:
    /// jobs run, and are evicted, in this order.
    pub seq: u64,
    /// Workload type from the request.
    pub workload_type: WorkloadType,
    /// Repetitions from the request.
    pub repetitions: u32,
    /// Measurement mode from the request, echoed in the status.
    pub mode: MeasurementMode,
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
            seq: NEXT_SEQ.fetch_add(1, Ordering::Relaxed),
            workload_type: request.use_case.clone(),
            repetitions: request.repetitions,
            mode: request.mode.clone(),
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

/// The place of job `id` among the waiting jobs, in submission order: 1 for
/// the next job to run, 2 for the one after, and so on. `None` once the job has
/// started, or if it is unknown. The submission order is assigned under the
/// store's write lock, so positions are consistent across simultaneous
/// submissions. The start order follows it closely but not strictly: each job
/// waits on a first-in, first-out semaphore from its own task, and a task can
/// reach the semaphore a moment before an earlier one. The value is therefore
/// the job's position in submission order, a close estimate of when it runs.
pub fn queue_position(jobs: &HashMap<String, JobState>, id: &str) -> Option<u32> {
    let job = jobs.get(id).filter(|j| j.status == JobStatus::Accepted)?;
    let ahead =
        jobs.values().filter(|j| j.status == JobStatus::Accepted && j.seq < job.seq).count();
    Some(u32::try_from(ahead + 1).unwrap_or(u32::MAX))
}

/// Drop the oldest finished (complete or failed) jobs beyond the most recent
/// `keep`, so the store does not grow without bound (backlog F-M5). Jobs that
/// are waiting or running are never dropped. Returns how many were dropped.
pub fn evict_finished(jobs: &mut HashMap<String, JobState>, keep: usize) -> usize {
    let mut finished: Vec<(u64, String)> = jobs
        .values()
        .filter(|j| matches!(j.status, JobStatus::Complete | JobStatus::Failed))
        .map(|j| (j.seq, j.id.clone()))
        .collect();
    if finished.len() <= keep {
        return 0;
    }
    finished.sort_unstable();
    let excess = finished.len() - keep;
    for (_, id) in finished.into_iter().take(excess) {
        jobs.remove(&id);
    }
    excess
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

    fn store_with(statuses: &[JobStatus]) -> (HashMap<String, JobState>, Vec<String>) {
        let mut jobs = HashMap::new();
        let mut ids = Vec::new();
        for (i, status) in statuses.iter().enumerate() {
            let id = format!("job-{i}");
            let mut job = JobState::new(id.clone(), &make_request(WorkloadType::Regression, 1));
            job.status = status.clone();
            jobs.insert(id.clone(), job);
            ids.push(id);
        }
        (jobs, ids)
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn queue_position_counts_the_jobs_waiting_ahead() {
        use JobStatus::{Accepted, Complete, Running};
        let (jobs, ids) = store_with(&[Complete, Running, Accepted, Accepted, Accepted]);
        assert_eq!(queue_position(&jobs, &ids[2]), Some(1));
        assert_eq!(queue_position(&jobs, &ids[3]), Some(2));
        assert_eq!(queue_position(&jobs, &ids[4]), Some(3));
        assert_eq!(queue_position(&jobs, &ids[1]), None, "a running job has no place in the queue");
        assert_eq!(queue_position(&jobs, &ids[0]), None);
        assert_eq!(queue_position(&jobs, "unknown"), None);
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn eviction_drops_only_the_oldest_finished_jobs() {
        use JobStatus::{Accepted, Complete, Failed, Running};
        let (mut jobs, ids) =
            store_with(&[Complete, Failed, Running, Complete, Accepted, Complete]);

        assert_eq!(evict_finished(&mut jobs, 2), 2);

        // The two oldest finished jobs are gone; the two newest stay, and the
        // waiting and running ones are untouched.
        for gone in [&ids[0], &ids[1]] {
            assert!(!jobs.contains_key(gone.as_str()), "{gone} should be evicted");
        }
        for kept in [&ids[2], &ids[3], &ids[4], &ids[5]] {
            assert!(jobs.contains_key(kept.as_str()), "{kept} should be kept");
        }
        assert_eq!(evict_finished(&mut jobs, 2), 0, "nothing left to evict");
    }
}
