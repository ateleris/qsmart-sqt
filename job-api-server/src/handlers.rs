use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use uuid::Uuid;

use crate::{
    state::{JobRecord, JobStore},
    types::*,
};

// ---------------------------------------------------------------------------
// GET /api/v1/docs
// ---------------------------------------------------------------------------
//
// Returns the StrictDoc requirements documents that this server is responsible
// for. SQT Generic calls this once per qualification session to:
//
//   1. discover the full list of requirements
//   2. identify which are testable (VERIFICATION: Test) vs. non-testable
//   3. mark non-testable requirements as compliant without submitting a job
//   4. flag testable requirements with no matching .test.yaml as UNCOVERED gaps
//
// Each document must be valid StrictDoc text. SQT Generic parses it to extract
// [REQUIREMENT] blocks, UIDs, titles, statements, and VERIFICATION methods.

pub async fn get_docs() -> Json<DocsResponse> {
    // TODO(Klepsydra): load your actual .sdoc files here and return them.
    //   let content = std::fs::read_to_string("/etc/ml-l/requirements.sdoc")?;

    Json(DocsResponse {
        documents: vec![],
    })
}

// ---------------------------------------------------------------------------
// POST /api/v1/jobs
// ---------------------------------------------------------------------------
//
// TODO(Klepsydra): implement `execute_job` below. That function is the main
// place that interfaces with the infrastructure.

pub async fn submit_job(
    State(store): State<JobStore>,
    Json(request): Json<JobRequest>,
) -> (StatusCode, Json<JobAccepted>) {

    let job_id = format!("job-{}", Uuid::new_v4());
    let now = chrono::Utc::now().to_rfc3339();

    let record = JobRecord {
        id: job_id.clone(),
        model: request.model.clone(),
        mode: request.mode.clone(),
        repetitions: request.repetitions,
        status: JobStatus::Accepted,
        created_at: now.clone(),
        started_at: None,
        completed_at: None,
        hardware: None,
        metrics: None,
        output: None,
        error: None,
    };

    store.write().await.insert(job_id.clone(), record);

    let store_clone = store.clone();
    let id = job_id.clone();
    tokio::spawn(async move {
        {
            let mut s = store_clone.write().await;
            if let Some(job) = s.get_mut(&id) {
                job.status = JobStatus::Running;
                job.started_at = Some(chrono::Utc::now().to_rfc3339());
            }
        }

        let result = tokio::task::spawn_blocking({
            let request = request.clone();
            move || execute_job(request)
        })
        .await;

        let mut s = store_clone.write().await;
        if let Some(job) = s.get_mut(&id) {
            job.completed_at = Some(chrono::Utc::now().to_rfc3339());
            match result {
                Ok(Ok(outcome)) => {
                    job.status = JobStatus::Complete;
                    job.hardware = Some(outcome.hardware);
                    job.metrics = Some(outcome.metrics);
                    job.output = outcome.output;
                }
                Ok(Err(e)) => {
                    job.status = JobStatus::Failed;
                    job.error = Some(e);
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
        Json(JobAccepted {
            job_id,
            status: "accepted".into(),
            created_at: now,
        }),
    )
}

// ---------------------------------------------------------------------------
// execute_job — main interface
// ---------------------------------------------------------------------------
//
// Return:
//   Ok(JobOutcome)  — execution succeeded; fill in hardware + metrics + output
//   Err(String)     — infrastructure failure; message goes into the error field

pub struct JobOutcome {
    pub hardware: Hardware,
    pub metrics: JobMetrics,
    pub output: Option<JobOutput>,
}

fn execute_job(_request: JobRequest) -> Result<JobOutcome, String> {
    // TODO(Klepsydra): replace this entire function body with your implementation.
    //
    // Suggested steps:
    //
    //  1. Select the binary / application package for `request.model`.
    //
    //  2. Apply `request.instrumentation`:
    //       none     — bare execution, lowest overhead
    //       asan     — run with AddressSanitizer; populates memory error data
    //       valgrind — run under Valgrind memcheck; populates memory.leaks_bytes
    //       gprof    — run with gprof instrumentation
    //
    //  3. Honour `request.warmup_repetitions`: run N times, discard results,
    //     then run `request.repetitions` times for measurement.
    //
    //  4. Honour `request.mode`:
    //       latency    — measure each inference independently
    //                    → populate time.avg_ms, time.latency_p50/p95/p99_ms
    //       throughput — maximise pipeline utilisation
    //                    → populate time.throughput_fps
    //
    //  5. Collect as many metrics as your infrastructure can provide.
    //     Leave fields None only if the tool genuinely is not available —
    //     SQT Generic treats a null metric as a failed constraint.
    //
    //  6. Fill in the Hardware descriptor (type, model, cores, memory, clock, OS).
    //
    //  7. Return the model output in `output.outputs` — one entry per input item.
    //     Set `output.result_consistent` to true if all repetitions produced
    //     bit-identical results (non-determinism = possible hardware fault).
    //
    // On failure (binary not found, hardware unreachable, …)
    // return Err(description) — do NOT return Ok with null metrics.

    Ok(JobOutcome {
        hardware: Hardware {
            hw_type: "cpu".into(),
            model: "placeholder".into(),
            cores: 1,
            memory_total_kb: 0,
            accelerator_model: None,
            clock_mhz: None,
            os: None,
        },
        // All metrics null — implement execute_job to populate real values.
        metrics: JobMetrics::default(),
        output: None,
    })
}

// ---------------------------------------------------------------------------
// GET /api/v1/jobs/{job_id}
// ---------------------------------------------------------------------------

pub async fn get_job(
    State(store): State<JobStore>,
    Path(job_id): Path<String>,
) -> Result<Json<JobStatusResponse>, StatusCode> {
    let store = store.read().await;
    match store.get(&job_id) {
        Some(job) => Ok(Json(JobStatusResponse {
            job_id: job.id.clone(),
            model: job.model.clone(),
            mode: job.mode.clone(),
            repetitions: job.repetitions,
            status: job.status.clone(),
            created_at: job.created_at.clone(),
            started_at: job.started_at.clone(),
            completed_at: job.completed_at.clone(),
            hardware: job.hardware.clone(),
            metrics: job.metrics.clone(),
            output: job.output.clone(),
            error: job.error.clone(),
        })),
        None => Err(StatusCode::NOT_FOUND),
    }
}
