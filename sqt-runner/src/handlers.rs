use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use sqt_types::{
    ApiError as ApiErrorBody, Attachment, DocsResponse, ErrorResponse, JobAccepted, JobRequest,
    JobStatus, JobStatusResponse, MeasurementMode, StrictdocDocument, VersionResponse,
};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::executor::ExecutionBackend;
use crate::store::{self, JobStore};

/// Shared application state passed to all handlers.
#[derive(Clone)]
pub struct AppState {
    pub store: JobStore,
    pub workspace: PathBuf,
    pub executor: Arc<dyn ExecutionBackend>,
    /// Limits concurrent job execution to one at a time (sequential queue).
    pub job_semaphore: Arc<Semaphore>,
    /// Directory scanned by GET /api/v1/docs (`.sdoc` → documents, rest → attachments).
    pub docs_dir: Option<PathBuf>,
    /// Runner crate version, populated from `env!("CARGO_PKG_VERSION")` at startup.
    pub runner_version: String,
    /// Operating system name, e.g. `"linux"`.
    ///
    /// `target_id` and `toolchain_versions` are sourced from the backend via
    /// [`ExecutionBackend::describe`], not stored here.
    pub os: String,
}

// ---------------------------------------------------------------------------
// POST /api/v1/jobs
// ---------------------------------------------------------------------------

pub async fn create_job(
    State(state): State<AppState>,
    Json(req): Json<JobRequest>,
) -> (StatusCode, Json<JobAccepted>) {
    let job_id = Uuid::new_v4().to_string();

    let job = store::JobState::new(job_id.clone(), &req);
    let created_at = job.created_at.clone();

    {
        let mut store = state.store.write().await;
        store.insert(job_id.clone(), job);
    }

    tracing::info!(
        job_id = %job_id,
        use_case = ?req.use_case,
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
                job.repetitions,
                job.tags.clone(),
                job.status.clone(),
                job.created_at.clone(),
                job.started_at.clone(),
                job.completed_at.clone(),
                job.error.clone(),
                job.metrics.clone(),
            )
        })
    }; // read lock released here

    let (
        id,
        workload_type,
        repetitions,
        tags,
        status,
        created_at,
        started_at,
        completed_at,
        error,
        metrics,
    ) = snapshot.ok_or_else(|| {
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
    let hardware = (status != JobStatus::Accepted).then(|| state.executor.describe().hardware);

    Ok(Json(JobStatusResponse {
        job_id: id,
        use_case: workload_type,
        mode: MeasurementMode::default(),
        repetitions,
        status,
        queue_position,
        created_at,
        started_at,
        completed_at,
        error,
        tags,
        hardware,
        metrics: metrics.unwrap_or_default(),
        output: None,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/v1/version
// ---------------------------------------------------------------------------

pub async fn get_version(State(state): State<AppState>) -> Json<VersionResponse> {
    let descriptor = state.executor.describe();
    Json(VersionResponse {
        api_version: "1.0".to_string(),
        runner_version: state.runner_version.clone(),
        target_id: descriptor.target_id,
        os: state.os.clone(),
        toolchain_versions: descriptor.toolchain_versions,
    })
}

// ---------------------------------------------------------------------------
// GET /api/v1/docs
// ---------------------------------------------------------------------------

pub async fn get_docs(State(state): State<AppState>) -> Response {
    let Some(ref docs_dir) = state.docs_dir else {
        tracing::warn!("GET /api/v1/docs - no docs directory configured");
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

    let dir = docs_dir.clone();
    let scan_result = tokio::task::spawn_blocking(move || scan_docs_dir(&dir)).await;

    match scan_result {
        Ok(Ok(response)) => {
            tracing::info!(
                documents = response.documents.len(),
                attachments = response.attachments.len(),
                "GET /api/v1/docs - serving docs"
            );
            Json(response).into_response()
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "GET /api/v1/docs - failed to scan docs directory");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: ApiErrorBody {
                        code: "DOCS_READ_ERROR".into(),
                        message: format!("failed to read docs directory: {e}"),
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

/// Scan `dir` for docs files (non-recursive).
///
/// `.sdoc` files are read as UTF-8 text and placed in `documents`.
/// All other files are base64-encoded and placed in `attachments`.
/// Entries are sorted by filename for deterministic responses.
fn scan_docs_dir(dir: &std::path::Path) -> anyhow::Result<DocsResponse> {
    use base64::Engine as _;

    let mut documents: Vec<StrictdocDocument> = Vec::new();
    let mut attachments: Vec<Attachment> = Vec::new();

    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read docs directory: {}", dir.display()))?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .collect();

    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let path = entry.path();
        let filename = entry.file_name().to_string_lossy().into_owned();

        if path.extension().and_then(|e| e.to_str()) == Some("sdoc") {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("cannot read {filename}"))?;
            let title = extract_sdoc_title(&content);
            documents.push(StrictdocDocument { filename, title, content });
        } else {
            let bytes = std::fs::read(&path).with_context(|| format!("cannot read {filename}"))?;
            let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let content_type = mime_type_for(&filename).to_string();
            attachments.push(Attachment { filename, content_type, data });
        }
    }

    Ok(DocsResponse { documents, attachments })
}

/// Extract the title from the first `[DOCUMENT]` block's `TITLE:` field.
fn extract_sdoc_title(content: &str) -> Option<String> {
    let mut in_document = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[DOCUMENT]" {
            in_document = true;
            continue;
        }
        if in_document {
            if let Some(title) = trimmed.strip_prefix("TITLE:") {
                return Some(title.trim().to_string());
            }
            // Stop at the next block marker.
            if trimmed.starts_with('[') {
                break;
            }
        }
    }
    None
}

fn mime_type_for(filename: &str) -> &'static str {
    let ext = filename.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "zip" => "application/zip",
        "txt" => "text/plain",
        "html" => "text/html",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::Router;
    use axum::routing::{get, post};
    use http_body_util::BodyExt as _;
    use sqt_types::{HardwareInfo, JobMetrics};
    use tokio::sync::Semaphore;
    use tower::ServiceExt as _;

    use crate::executor::ExecutionBackend;
    use crate::store;

    struct StubExecutor;
    impl ExecutionBackend for StubExecutor {
        fn preflight(&self) -> anyhow::Result<()> {
            Ok(())
        }
        fn execute(
            &self,
            _req: &sqt_types::JobRequest,
            _ws: &std::path::Path,
        ) -> anyhow::Result<JobMetrics> {
            Ok(JobMetrics::default())
        }
        fn describe(&self) -> crate::executor::BackendDescriptor {
            crate::executor::BackendDescriptor {
                hardware: HardwareInfo::default(),
                target_id: "test-target".to_string(),
                toolchain_versions: std::collections::HashMap::new(),
            }
        }
    }

    fn test_app(docs_dir: Option<std::path::PathBuf>) -> Router {
        let state = AppState {
            store: store::new_store(),
            workspace: std::path::PathBuf::from("/tmp"),
            executor: Arc::new(StubExecutor),
            job_semaphore: Arc::new(Semaphore::new(1)),
            docs_dir,
            runner_version: "0.0.0-test".to_string(),
            os: "linux".to_string(),
        };
        Router::new()
            .route("/api/v1/jobs", post(create_job))
            .route("/api/v1/jobs/{job_id}", get(get_job))
            .route("/api/v1/docs", get(get_docs))
            .route("/api/v1/version", get(get_version))
            .with_state(state)
    }

    async fn body_json(body: axum::body::Body) -> serde_json::Value {
        let bytes = body.collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    // ---- GET /api/v1/jobs/{job_id} ------------------------------------------

    #[tokio::test]
    async fn get_job_unknown_id_returns_404() {
        let app = test_app(None);
        let req = axum::http::Request::builder()
            .uri("/api/v1/jobs/no-such-job")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["error"]["code"], "JOB_NOT_FOUND");
    }

    // ---- GET /api/v1/docs ---------------------------------------------------

    #[tokio::test]
    async fn get_docs_not_configured_returns_404() {
        let app = test_app(None);
        let req = axum::http::Request::builder()
            .uri("/api/v1/docs")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["error"]["code"], "DOCS_NOT_CONFIGURED");
    }

    #[tokio::test]
    async fn get_docs_empty_directory_returns_empty_arrays() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(Some(dir.path().to_path_buf()));
        let req = axum::http::Request::builder()
            .uri("/api/v1/docs")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["documents"], serde_json::json!([]));
        assert_eq!(json["attachments"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn get_docs_returns_sdoc_in_documents_and_pdf_in_attachments() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("reqs.sdoc"),
            "[DOCUMENT]\nTITLE: Test Reqs\n\n[REQUIREMENT]\nUID: R-001\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("evidence.pdf"), b"%PDF placeholder").unwrap();

        let app = test_app(Some(dir.path().to_path_buf()));
        let req = axum::http::Request::builder()
            .uri("/api/v1/docs")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp.into_body()).await;

        assert_eq!(json["documents"].as_array().unwrap().len(), 1);
        assert_eq!(json["documents"][0]["filename"], "reqs.sdoc");
        assert_eq!(json["documents"][0]["title"], "Test Reqs");
        assert!(json["documents"][0]["content"].as_str().unwrap().contains("R-001"));

        assert_eq!(json["attachments"].as_array().unwrap().len(), 1);
        assert_eq!(json["attachments"][0]["filename"], "evidence.pdf");
        assert_eq!(json["attachments"][0]["content_type"], "application/pdf");
        assert!(json["attachments"][0]["data"].as_str().unwrap().len() > 0);
    }

    #[tokio::test]
    async fn get_docs_nonexistent_directory_returns_500() {
        let app = test_app(Some(std::path::PathBuf::from("/nonexistent/path/12345")));
        let req = axum::http::Request::builder()
            .uri("/api/v1/docs")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["error"]["code"], "DOCS_READ_ERROR");
    }

    // ---- POST /api/v1/jobs --------------------------------------------------

    #[tokio::test]
    async fn post_job_accepted_returns_202() {
        let app = test_app(None);
        let body = serde_json::json!({ "use_case": "image_classification" });
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/jobs")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["status"], "accepted");
        assert!(json["job_id"].as_str().unwrap().len() > 0);
        assert_eq!(json["queue_position"], 1);
    }

    #[tokio::test]
    async fn post_job_returns_job_id_that_is_retrievable() {
        let state = AppState {
            store: store::new_store(),
            workspace: std::path::PathBuf::from("/tmp"),
            executor: Arc::new(StubExecutor),
            job_semaphore: Arc::new(Semaphore::new(1)),
            docs_dir: None,
            runner_version: "0.0.0-test".to_string(),
            os: "linux".to_string(),
        };
        let app = Router::new()
            .route("/api/v1/jobs", post(create_job))
            .route("/api/v1/jobs/{job_id}", get(get_job))
            .with_state(state);

        // Submit a job and capture the job_id.
        let body = serde_json::json!({ "use_case": "regression" });
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/jobs")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let json = body_json(resp.into_body()).await;
        let job_id = json["job_id"].as_str().unwrap().to_string();

        // Poll for the job - it should be found (accepted or already complete).
        let req = axum::http::Request::builder()
            .uri(format!("/api/v1/jobs/{job_id}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["job_id"], job_id);
        assert_eq!(json["use_case"], "regression");
    }

    #[test]
    fn extract_title_from_document_block() {
        let sdoc =
            "[DOCUMENT]\nTITLE: ML-L Requirements\nVERSION: 1.0\n\n[REQUIREMENT]\nUID: R-001\n";
        assert_eq!(extract_sdoc_title(sdoc), Some("ML-L Requirements".into()));
    }

    #[test]
    fn extract_title_ignores_requirement_title() {
        let sdoc = "[DOCUMENT]\nTITLE: Doc Title\n\n[REQUIREMENT]\nTITLE: Req Title\n";
        assert_eq!(extract_sdoc_title(sdoc), Some("Doc Title".into()));
    }

    #[test]
    fn extract_title_returns_none_when_absent() {
        let sdoc = "[DOCUMENT]\nVERSION: 1.0\n\n[REQUIREMENT]\nUID: R-001\n";
        assert_eq!(extract_sdoc_title(sdoc), None);
    }

    #[test]
    fn mime_type_known_extensions() {
        assert_eq!(mime_type_for("report.pdf"), "application/pdf");
        assert_eq!(
            mime_type_for("data.xlsx"),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
        assert_eq!(mime_type_for("notes.txt"), "text/plain");
    }

    #[test]
    fn mime_type_unknown_falls_back_to_octet_stream() {
        assert_eq!(mime_type_for("binary.bin"), "application/octet-stream");
        assert_eq!(mime_type_for("noextension"), "application/octet-stream");
    }

    #[test]
    fn scan_docs_dir_splits_sdoc_from_attachments() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("reqs.sdoc"), "[DOCUMENT]\nTITLE: Reqs\n").unwrap();
        let mut pdf = std::fs::File::create(dir.path().join("evidence.pdf")).unwrap();
        pdf.write_all(b"%PDF-1.4 placeholder").unwrap();

        let result = scan_docs_dir(dir.path()).unwrap();

        assert_eq!(result.documents.len(), 1);
        assert_eq!(result.documents[0].filename, "reqs.sdoc");
        assert_eq!(result.documents[0].title.as_deref(), Some("Reqs"));

        assert_eq!(result.attachments.len(), 1);
        assert_eq!(result.attachments[0].filename, "evidence.pdf");
        assert_eq!(result.attachments[0].content_type, "application/pdf");
        // data is valid base64
        use base64::Engine as _;
        assert!(
            base64::engine::general_purpose::STANDARD.decode(&result.attachments[0].data).is_ok()
        );
    }

    #[test]
    fn scan_docs_dir_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let result = scan_docs_dir(dir.path()).unwrap();
        assert!(result.documents.is_empty());
        assert!(result.attachments.is_empty());
    }

    #[test]
    fn scan_docs_dir_sorted_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("z-last.sdoc"), "[DOCUMENT]\nTITLE: Z\n").unwrap();
        std::fs::write(dir.path().join("a-first.sdoc"), "[DOCUMENT]\nTITLE: A\n").unwrap();

        let result = scan_docs_dir(dir.path()).unwrap();
        assert_eq!(result.documents[0].filename, "a-first.sdoc");
        assert_eq!(result.documents[1].filename, "z-last.sdoc");
    }

    // ---- GET /api/v1/version ------------------------------------------------

    #[tokio::test]
    async fn get_version_returns_200_with_required_fields() {
        let app = test_app(None);
        let req = axum::http::Request::builder()
            .uri("/api/v1/version")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert!(json["api_version"].as_str().is_some(), "api_version must be present");
        assert!(json["runner_version"].as_str().is_some(), "runner_version must be present");
        assert!(json["target_id"].as_str().is_some(), "target_id must be present");
        assert!(json["os"].as_str().is_some(), "os must be present");
        assert!(json["toolchain_versions"].is_object(), "toolchain_versions must be an object");
    }

    #[tokio::test]
    async fn get_version_api_version_is_1_0() {
        let app = test_app(None);
        let req = axum::http::Request::builder()
            .uri("/api/v1/version")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["api_version"], "1.0");
    }

    #[tokio::test]
    async fn get_version_runner_version_matches_stub() {
        let app = test_app(None);
        let req = axum::http::Request::builder()
            .uri("/api/v1/version")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let json = body_json(resp.into_body()).await;
        // The test_app uses "0.0.0-test" as the runner_version.
        assert_eq!(json["runner_version"], "0.0.0-test");
    }

    #[tokio::test]
    async fn get_version_target_id_and_os_from_state() {
        let app = test_app(None);
        let req = axum::http::Request::builder()
            .uri("/api/v1/version")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["target_id"], "test-target");
        assert_eq!(json["os"], "linux");
    }
}
