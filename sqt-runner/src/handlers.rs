use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use sqt_api_types::{
    ApiError as ApiErrorBody, ErrorResponse, JobAccepted, JobRequest, JobStatus,
    JobStatusResponse, MeasurementMode,
};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::executor::JobExecutor;
use crate::store::{self, JobStore};

/// Shared application state passed to all handlers.
#[derive(Clone)]
pub struct AppState {
    pub store: JobStore,
    pub workspace: PathBuf,
    pub executor: Arc<dyn JobExecutor>,
    /// Limits concurrent job execution to one at a time (sequential queue).
    pub job_semaphore: Arc<Semaphore>,
    /// Path to the qualification documents zip file (served by GET /api/v1/docs).
    pub docs_zip: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// POST /api/v1/jobs
// ---------------------------------------------------------------------------

pub async fn create_job(
    State(state): State<AppState>,
    Json(req): Json<JobRequest>,
) -> (StatusCode, Json<JobAccepted>) {
    let job_id = Uuid::new_v4().to_string();

    let job = store::JobState::new(job_id.clone(), req.model.clone());
    let created_at = job.created_at.clone();

    {
        let mut store = state.store.write().await;
        store.insert(job_id.clone(), job);
    }

    tracing::info!(
        job_id = %job_id,
        model = ?req.model,
        input = ?req.input,
        timeout_secs = ?req.timeout_secs,
        "POST /api/v1/jobs - accepted"
    );

    // Spawn background execution.  The task waits for the semaphore before
    // running so jobs execute one at a time (sequential queue per AD-022).
    let store = state.store.clone();
    let workspace = state.workspace.join(&job_id); // per-job subdirectory
    let executor = state.executor.clone();
    let semaphore = state.job_semaphore.clone();
    let id = job_id.clone();

    tokio::spawn(async move {
        // Wait for an execution slot.  The permit is held for the duration of
        // the job and dropped automatically when this block exits.
        let _permit = semaphore.acquire().await.expect("semaphore closed");

        {
            let mut s = store.write().await;
            if let Some(job) = s.get_mut(&id) {
                job.status = JobStatus::Running;
                job.started_at = Some(crate::now_iso());
            }
        }
        tracing::info!(job_id = %id, "job started");

        // Create the per-job workspace directory.
        if let Err(e) = std::fs::create_dir_all(&workspace) {
            tracing::error!(job_id = %id, error = %e, "failed to create workspace");
            let mut s = store.write().await;
            if let Some(job) = s.get_mut(&id) {
                job.status = JobStatus::Failed;
                job.completed_at = Some(crate::now_iso());
                job.error = Some(format!("failed to create workspace: {e}"));
            }
            return;
        }

        let result = tokio::task::spawn_blocking({
            let workspace = workspace.clone();
            let req = req.clone();
            move || executor.execute(&req, &workspace)
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
        // _permit dropped here — next queued job can acquire the semaphore.
    });

    (
        StatusCode::ACCEPTED,
        Json(JobAccepted {
            job_id,
            status: "accepted".into(),
            // TODO(auth): enforce Bearer token validation before this handler
            // is reachable. Tracked for a dedicated auth session.
            queue_position: 1,
            created_at,
        }),
    )
}

// ---------------------------------------------------------------------------
// GET /api/v1/jobs/{job_id}
// ---------------------------------------------------------------------------

type NotFound = (StatusCode, Json<ErrorResponse>);

pub async fn get_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<JobStatusResponse>, NotFound> {
    let snapshot = {
        let store = state.store.read().await;
        store.get(&job_id).map(|job| {
            (
                job.id.clone(),
                job.workload_type.clone(),
                job.status.clone(),
                job.created_at.clone(),
                job.started_at.clone(),
                job.completed_at.clone(),
                job.error.clone(),
                job.metrics.clone(),
            )
        })
    }; // read lock released here

    let (id, workload_type, status, created_at, started_at, completed_at, error, metrics) =
        snapshot.ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: ApiErrorBody {
                        code: "JOB_NOT_FOUND".into(),
                        message: format!("job '{job_id}' not found"),
                        details: None,
                        timestamp: crate::now_iso(),
                    },
                }),
            )
        })?;

    tracing::debug!(
        job_id = %job_id,
        status = ?status,
        "GET /api/v1/jobs/{job_id}"
    );

    // `queue_position` is non-null only while the job is still accepted (queued).
    // `hardware` is null until the job starts running.
    let queue_position = (status == JobStatus::Accepted).then_some(1);
    let hardware = (status != JobStatus::Accepted).then(|| state.executor.hardware_info());

    Ok(Json(JobStatusResponse {
        job_id: id,
        model: workload_type,
        mode: MeasurementMode::default(),
        repetitions: 1,
        status,
        queue_position,
        created_at,
        started_at,
        completed_at,
        error,
        tags: None,
        hardware,
        metrics: metrics.unwrap_or_default(),
        output: None,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/v1/docs
// ---------------------------------------------------------------------------

pub async fn get_docs(State(state): State<AppState>) -> Response {
    let Some(ref zip_path) = state.docs_zip else {
        tracing::warn!("GET /api/v1/docs - no docs configured");
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: ApiErrorBody {
                    code: "DOCS_NOT_CONFIGURED".into(),
                    message: "no qualification documents configured on this runner".into(),
                    details: None,
                    timestamp: crate::now_iso(),
                },
            }),
        )
            .into_response();
    };

    let path = zip_path.clone();
    let read_result =
        tokio::task::spawn_blocking(move || std::fs::read(&path)).await;

    match read_result {
        Ok(Ok(bytes)) => {
            tracing::info!(bytes = bytes.len(), "GET /api/v1/docs - serving docs zip");
            ([(header::CONTENT_TYPE, "application/zip")], bytes).into_response()
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "GET /api/v1/docs - failed to read zip");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: ApiErrorBody {
                        code: "DOCS_READ_ERROR".into(),
                        message: format!("failed to read docs zip: {e}"),
                        details: None,
                        timestamp: crate::now_iso(),
                    },
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "GET /api/v1/docs - task panicked");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: ApiErrorBody {
                        code: "INTERNAL_ERROR".into(),
                        message: format!("internal error: {e}"),
                        details: None,
                        timestamp: crate::now_iso(),
                    },
                }),
            )
                .into_response()
        }
    }
}
