use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use sqt::api::types::{
    AnalysisAccepted, AnalysisRequest, AnalysisStatusResponse, JobStatus, RunAccepted, RunRequest,
    RunStatusResponse,
};
use uuid::Uuid;

use crate::ToolConfig;
use crate::store::{self, JobKind, JobStore};

/// Shared application state passed to all handlers.
#[derive(Clone)]
pub struct AppState {
    pub store: JobStore,
    pub workspace: PathBuf,
    pub binary: String,
    pub tool_config: Arc<ToolConfig>,
}

// ---------------------------------------------------------------------------
// POST /api/v1/runs
// ---------------------------------------------------------------------------

pub async fn create_run(
    State(state): State<AppState>,
    Json(req): Json<RunRequest>,
) -> (StatusCode, Json<RunAccepted>) {
    let run_id = Uuid::new_v4().to_string();
    let timeout_secs = req.timeout_secs.unwrap_or(60);

    let job = store::JobState::new_run(
        run_id.clone(),
        req.input.clone(),
        timeout_secs,
    );
    let created_at = job.created_at.clone();

    {
        let mut store = state.store.write().await;
        store.insert(run_id.clone(), job);
    }

    tracing::info!(run_id = %run_id, input = ?req.input, "accepted run");

    // Spawn background execution
    let store = state.store.clone();
    let workspace = state.workspace.clone();
    let config = state.tool_config.clone();
    let binary = state.binary.clone();
    let id = run_id.clone();
    let input = req.input.clone();

    tokio::spawn(async move {
        // Mark as running
        {
            let mut s = store.write().await;
            if let Some(job) = s.get_mut(&id) {
                job.status = JobStatus::Running;
                job.started_at = Some(crate::now_iso());
            }
        }

        // Execute in blocking thread pool
        let result = tokio::task::spawn_blocking({
            let workspace = workspace.clone();
            let config = config.clone();
            let binary = binary.clone();
            let input = input.clone();
            move || {
                crate::run_job::execute(
                    &binary,
                    input.as_deref(),
                    timeout_secs,
                    &workspace,
                    &config,
                )
            }
        })
        .await;

        // Update job state
        let mut s = store.write().await;
        if let Some(job) = s.get_mut(&id) {
            job.completed_at = Some(crate::now_iso());
            match result {
                Ok(Ok(metrics)) => {
                    job.status = JobStatus::Complete;
                    job.run_metrics = Some(metrics);
                }
                Ok(Err(e)) => {
                    job.status = JobStatus::Failed;
                    job.error = Some(format!("{e:#}"));
                }
                Err(e) => {
                    job.status = JobStatus::Failed;
                    job.error = Some(format!("task panicked: {e}"));
                }
            }
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(RunAccepted {
            run_id,
            status: "accepted".into(),
            created_at,
        }),
    )
}

// ---------------------------------------------------------------------------
// GET /api/v1/runs/{run_id}
// ---------------------------------------------------------------------------

pub async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunStatusResponse>, StatusCode> {
    let store = state.store.read().await;
    let job = store.get(&run_id).ok_or(StatusCode::NOT_FOUND)?;

    let input = match &job.kind {
        JobKind::Run { input, .. } => input.clone(),
        _ => return Err(StatusCode::NOT_FOUND),
    };

    Ok(Json(RunStatusResponse {
        run_id: job.id.clone(),
        input,
        status: job.status.clone(),
        created_at: job.created_at.clone(),
        started_at: job.started_at.clone(),
        completed_at: job.completed_at.clone(),
        error: job.error.clone(),
        metrics: job.run_metrics.clone().unwrap_or_default(),
    }))
}

// ---------------------------------------------------------------------------
// POST /api/v1/analysis
// ---------------------------------------------------------------------------

pub async fn create_analysis(
    State(state): State<AppState>,
    Json(req): Json<AnalysisRequest>,
) -> (StatusCode, Json<AnalysisAccepted>) {
    let analysis_id = Uuid::new_v4().to_string();
    let timeout_secs = req.timeout_secs.unwrap_or(120);

    let job = store::JobState::new_analysis(
        analysis_id.clone(),
        timeout_secs,
    );
    let created_at = job.created_at.clone();

    {
        let mut store = state.store.write().await;
        store.insert(analysis_id.clone(), job);
    }

    tracing::info!(analysis_id = %analysis_id, "accepted analysis");

    // Spawn background execution
    let store = state.store.clone();
    let workspace = state.workspace.clone();
    let config = state.tool_config.clone();
    let binary = state.binary.clone();
    let id = analysis_id.clone();

    tokio::spawn(async move {
        // Mark as running
        {
            let mut s = store.write().await;
            if let Some(job) = s.get_mut(&id) {
                job.status = JobStatus::Running;
                job.started_at = Some(crate::now_iso());
            }
        }

        // Execute in blocking thread pool
        let result = tokio::task::spawn_blocking({
            let workspace = workspace.clone();
            let config = config.clone();
            let binary = binary.clone();
            move || {
                crate::analysis_job::execute(
                    &binary,
                    timeout_secs,
                    &workspace,
                    &config,
                )
            }
        })
        .await;

        // Update job state
        let mut s = store.write().await;
        if let Some(job) = s.get_mut(&id) {
            job.completed_at = Some(crate::now_iso());
            match result {
                Ok(Ok(metrics)) => {
                    job.status = JobStatus::Complete;
                    job.analysis_metrics = Some(metrics);
                }
                Ok(Err(e)) => {
                    job.status = JobStatus::Failed;
                    job.error = Some(format!("{e:#}"));
                }
                Err(e) => {
                    job.status = JobStatus::Failed;
                    job.error = Some(format!("task panicked: {e}"));
                }
            }
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(AnalysisAccepted {
            analysis_id,
            status: "accepted".into(),
            created_at,
        }),
    )
}

// ---------------------------------------------------------------------------
// GET /api/v1/analysis/{analysis_id}
// ---------------------------------------------------------------------------

pub async fn get_analysis(
    State(state): State<AppState>,
    Path(analysis_id): Path<String>,
) -> Result<Json<AnalysisStatusResponse>, StatusCode> {
    let store = state.store.read().await;
    let job = store.get(&analysis_id).ok_or(StatusCode::NOT_FOUND)?;

    match &job.kind {
        JobKind::Analysis { .. } => {}
        _ => return Err(StatusCode::NOT_FOUND),
    };

    Ok(Json(AnalysisStatusResponse {
        analysis_id: job.id.clone(),
        status: job.status.clone(),
        created_at: job.created_at.clone(),
        started_at: job.started_at.clone(),
        completed_at: job.completed_at.clone(),
        error: job.error.clone(),
        metrics: job.analysis_metrics.clone().unwrap_or_default(),
    }))
}
