use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use sqt::api::types::{
    JobAccepted, JobRequest, JobStatus, JobStatusResponse, MeasurementMode,
};
use uuid::Uuid;

use crate::config::{BinaryConfig, ToolConfig};
use crate::store::{self, JobStore};

/// Shared application state passed to all handlers.
#[derive(Clone)]
pub struct AppState {
    pub store: JobStore,
    pub workspace: PathBuf,
    pub binary_config: Arc<BinaryConfig>,
    pub tool_config: Arc<ToolConfig>,
}

// ---------------------------------------------------------------------------
// POST /api/v1/jobs
// ---------------------------------------------------------------------------

pub async fn create_job(
    State(state): State<AppState>,
    Json(req): Json<JobRequest>,
) -> (StatusCode, Json<JobAccepted>) {
    let job_id = Uuid::new_v4().to_string();
    let timeout_secs = req.timeout_secs.unwrap_or(60);

    let job = store::JobState::new(job_id.clone(), req.model.clone());
    let created_at = job.created_at.clone();

    {
        let mut store = state.store.write().await;
        store.insert(job_id.clone(), job);
    }

    let input = req.input.as_ref().and_then(|v| v.as_str()).map(|s| s.to_string());
    let model_zip = req.model_zip.clone();

    tracing::info!(
        job_id = %job_id,
        model = ?req.model,
        input = ?input,
        has_model_zip = model_zip.is_some(),
        timeout_secs,
        "POST /api/v1/jobs - accepted"
    );

    // Spawn background execution
    let store = state.store.clone();
    let workspace = state.workspace.clone();
    let config = state.tool_config.clone();
    let binary_config = state.binary_config.clone();
    let id = job_id.clone();

    tokio::spawn(async move {
        {
            let mut s = store.write().await;
            if let Some(job) = s.get_mut(&id) {
                job.status = JobStatus::Running;
                job.started_at = Some(crate::now_iso());
            }
        }
        tracing::info!(job_id = %id, "job started");

        let result = tokio::task::spawn_blocking({
            let workspace = workspace.clone();
            let config = config.clone();
            let binary_config = binary_config.clone();
            let input = input.clone();
            let model_zip = model_zip.clone();
            move || {
                crate::run_job::execute(
                    &binary_config,
                    input.as_deref(),
                    model_zip.as_deref(),
                    timeout_secs,
                    &workspace,
                    &config,
                )
            }
        })
        .await;

        let mut s = store.write().await;
        if let Some(job) = s.get_mut(&id) {
            job.completed_at = Some(crate::now_iso());
            match result {
                Ok(Ok(metrics)) => {
                    job.status = JobStatus::Complete;
                    tracing::info!(
                        job_id = %id,
                        return_code = ?metrics.return_code,
                        time_ms = ?metrics.time_avg_ms,
                        peak_rss_kb = ?metrics.memory_peak_rss_kb,
                        cpu_pct = ?metrics.cpu_avg_normalized_pct,
                        binary_size_kb = ?metrics.binary_size_kb,
                        "job complete"
                    );
                    job.metrics = Some(metrics);
                }
                Ok(Err(e)) => {
                    job.status = JobStatus::Failed;
                    tracing::error!(job_id = %id, error = %e, "job failed");
                    job.error = Some(format!("{e:#}"));
                }
                Err(e) => {
                    job.status = JobStatus::Failed;
                    tracing::error!(job_id = %id, error = %e, "job panicked");
                    job.error = Some(format!("task panicked: {e}"));
                }
            }
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(JobAccepted {
            job_id,
            status: "accepted".into(),
            created_at,
        }),
    )
}

// ---------------------------------------------------------------------------
// GET /api/v1/jobs/{job_id}
// ---------------------------------------------------------------------------

pub async fn get_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<JobStatusResponse>, StatusCode> {
    let store = state.store.read().await;
    let job = store.get(&job_id).ok_or(StatusCode::NOT_FOUND)?;

    tracing::debug!(
        job_id = %job_id,
        status = ?job.status,
        "GET /api/v1/jobs/{job_id}"
    );

    Ok(Json(JobStatusResponse {
        job_id: job.id.clone(),
        model: job.workload_type.clone(),
        mode: MeasurementMode::default(),
        repetitions: 1,
        status: job.status.clone(),
        created_at: job.created_at.clone(),
        started_at: job.started_at.clone(),
        completed_at: job.completed_at.clone(),
        error: job.error.clone(),
        tags: None,
        hardware: None,
        metrics: job.metrics.clone().unwrap_or_default(),
        output: None,
    }))
}
