use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::accuracy::AccuracyMetrics;
use crate::accuracy::compute as accuracy_compute;
use crate::api::ApiClient;
use crate::baseline;
use crate::config::SqtConfig;
use crate::constraint::ConstraintOutcome;
use crate::constraint::evaluator::evaluate;
use crate::evidence::EvidenceStore;
use crate::report::{ConfigSummary, RunProvenance};
use crate::specs::{RunResult, SpecResult, TestSpec};
use sqt_types::{
    Instrumentation, JobRequest, JobStatus, MeasurementMode, VersionResponse, WorkloadType,
};

/// The API major version the Reporter was compiled against.
const SUPPORTED_API_MAJOR: &str = "1";

type GroupKey = (WorkloadType, Option<String>);

/// Build the grouping key for a spec: (`workload_type`, `input_path`).
fn group_key(spec: &TestSpec) -> GroupKey {
    (
        spec.workload_type.clone(),
        spec.target.input.as_ref().map(|p| p.to_string_lossy().into_owned()),
    )
}

/// Build a `JobRequest` for a group of specs that share the same (`workload_type`, `input`).
fn build_job_request(
    workload_type: &WorkloadType,
    input: Option<&String>,
    specs: &[&TestSpec],
) -> JobRequest {
    let max_timeout = specs.iter().map(|s| s.target.timeout.as_secs()).max().unwrap_or(30);
    JobRequest {
        use_case: workload_type.clone(),
        input: input.map(|s| serde_json::Value::String(s.clone())),
        repetitions: 1,
        instrumentation: Instrumentation::default(),
        mode: MeasurementMode::default(),
        warmup_repetitions: None,
        timeout_secs: Some(max_timeout),
        tags: None,
    }
}

/// Build a synthetic failing outcome for a metric that could not be evaluated.
fn failing_outcome(metric_name: &str, reason: &str) -> ConstraintOutcome {
    ConstraintOutcome {
        passed: false,
        actual: f64::NAN,
        expected: f64::NAN,
        message: format!("{metric_name}: {reason}: FAIL"),
        evidence: HashMap::new(),
    }
}

/// Build the evidence map for an `INFRA_ERROR` from a plain error message.
fn infra_error_evidence(message: &str, job_id: Option<&str>) -> HashMap<String, String> {
    let mut evidence = HashMap::new();
    evidence.insert("error_message".to_string(), message.to_string());
    if let Some(id) = job_id {
        evidence.insert("job_id".to_string(), id.to_string());
    }
    evidence
}

/// Build a `RunProvenance` from the runner version response and config.
fn build_provenance(version: &VersionResponse, config: &SqtConfig) -> RunProvenance {
    RunProvenance {
        sqt_version: env!("CARGO_PKG_VERSION").to_string(),
        runner_version: version.runner_version.clone(),
        runner_target_id: version.target_id.clone(),
        runner_os: version.os.clone(),
        runner_toolchain_versions: version.toolchain_versions.clone(),
        config_summary: ConfigSummary {
            runner_url: config.runner.url.clone(),
            token_env: config.runner.token_env.clone(),
            max_concurrent_jobs: config.runner.max_concurrent_jobs,
        },
    }
}

/// Run all specs against the single-endpoint API.
///
/// Groups specs by (`workload_type`, input), submits one job per group,
/// waits for all results, then evaluates each spec's constraints.
///
/// On SIGTERM / Ctrl-C the `shutdown` token is cancelled.  In-flight polls
/// finish (or time out), partial results are collected, and the returned
/// `RunResult` will have `interrupted: true`.
///
/// Returns `(RunResult, RunProvenance)` so the caller can build a report
/// with full provenance information.
///
/// # Errors
///
/// Returns an error only for unrecoverable setup failures (version check,
/// job submission I/O).  Individual job failures are recorded as
/// `OutcomeKind::InfraError` in the spec results rather than propagated.
#[allow(clippy::too_many_lines)]
pub async fn run(
    specs: &[TestSpec],
    client: &ApiClient,
    config: &SqtConfig,
    shutdown: CancellationToken,
    evidence_store: &mut EvidenceStore,
) -> Result<(RunResult, RunProvenance)> {
    // -----------------------------------------------------------------------
    // Session start: version check (SRS-IF-008)
    // -----------------------------------------------------------------------
    tracing::info!(
        reporter_version = env!("CARGO_PKG_VERSION"),
        runner_url = %config.runner.url,
        "SQT Reporter {} -- checking Runner at {}",
        env!("CARGO_PKG_VERSION"),
        config.runner.url,
    );

    let version = client.get_version().await?;
    tracing::info!(
        runner_version = %version.runner_version,
        api_version = %version.api_version,
        target_id = %version.target_id,
        "Runner connected",
    );

    let runner_major = version.api_version.split('.').next().unwrap_or("0");
    if runner_major != SUPPORTED_API_MAJOR {
        anyhow::bail!(
            "Runner API version {} incompatible with supported {}.x",
            version.api_version,
            SUPPORTED_API_MAJOR,
        );
    }

    tracing::info!(
        "SQT Reporter {} -- Runner {} at {}",
        env!("CARGO_PKG_VERSION"),
        version.runner_version,
        config.runner.url,
    );

    let provenance = build_provenance(&version, config);

    // -----------------------------------------------------------------------
    // Baseline coverage check (T-043)
    // -----------------------------------------------------------------------
    let docs = client.get_docs().await.unwrap_or_else(|e| {
        tracing::warn!("docs fetch failed: {e}");
        sqt_types::DocsResponse { documents: vec![], attachments: vec![] }
    });
    let baseline_req_set: Option<crate::baseline::RequirementSet> = if docs.documents.is_empty() {
        tracing::warn!("No baseline documents available — coverage check skipped");
        None
    } else {
        let req_set = baseline::parse_requirements(&docs);
        tracing::info!(
            testable = req_set.testable.len(),
            by_declaration = req_set.compliant_by_declaration.len(),
            "Baseline requirements parsed"
        );
        let spec_uids: Vec<String> = specs.iter().map(|s| s.requirement_uid.clone()).collect();
        baseline::check_coverage(&req_set, &spec_uids)?;
        Some(req_set)
    };

    if specs.is_empty() {
        return Ok((
            RunResult {
                results: vec![],
                timestamp: now_iso8601(),
                config_version: config.project.version.clone(),
                interrupted: false,
                requirement_set: baseline_req_set,
            },
            provenance,
        ));
    }

    tracing::info!(count = specs.len(), "Specs loaded");

    // -----------------------------------------------------------------------
    // Group specs by (workload_type, input).
    // -----------------------------------------------------------------------
    let mut groups: HashMap<GroupKey, Vec<&TestSpec>> = HashMap::new();
    for spec in specs {
        groups.entry(group_key(spec)).or_default().push(spec);
    }

    // -----------------------------------------------------------------------
    // Concurrency semaphore (SRS-PERF-002)
    // -----------------------------------------------------------------------
    let semaphore = Arc::new(Semaphore::new(config.runner.max_concurrent_jobs as usize));

    // Submit one job per group, tracking the per-group server timeout.
    tracing::info!("Submitting {} job(s) to {}", groups.len(), config.runner.url);
    let total = groups.len();
    let mut job_ids: HashMap<GroupKey, (String, u64)> = HashMap::new();
    for (i, ((workload_type, input), group)) in groups.iter().enumerate() {
        let request = build_job_request(workload_type, input.as_ref(), group);
        let timeout_secs = request.timeout_secs.unwrap_or(30);

        // Acquire semaphore slot before submission; release immediately after.
        let permit = semaphore.acquire().await?;
        let job_id = client.submit_job(&request).await?;
        drop(permit);

        tracing::info!(
            job = i + 1,
            total,
            use_case = ?workload_type,
            input = ?input,
            job_id = %job_id,
            "[{}/{}] Submitting use_case={:?} input={:?} -> job_id={}",
            i + 1,
            total,
            workload_type,
            input,
            job_id,
        );
        job_ids.insert((workload_type.clone(), input.clone()), (job_id, timeout_secs));
    }

    // -----------------------------------------------------------------------
    // Poll all jobs, respecting the shutdown token (SRS-OPS-008)
    // -----------------------------------------------------------------------
    let mut job_responses: HashMap<GroupKey, Result<sqt_types::JobStatusResponse>> = HashMap::new();
    let mut interrupted = false;

    for (key, (job_id, timeout_secs)) in &job_ids {
        let poll_timeout = Duration::from_secs(*timeout_secs + 60);
        let start = Instant::now();

        let result = tokio::select! {
            res = client.wait_for_job(job_id, poll_timeout) => res,
            () = shutdown.cancelled() => {
                interrupted = true;
                Err(anyhow::anyhow!("shutdown signal received while waiting for job {job_id}"))
            }
        };

        match &result {
            Ok(resp) => {
                let elapsed = start.elapsed().as_secs_f64();
                tracing::info!(
                    job_id = %job_id,
                    status = ?resp.status,
                    "[job_id={job_id}] {:?} in {elapsed:.1}s",
                    resp.status,
                );
                if let Err(e) = evidence_store.store_json(job_id, "job_response.json", resp) {
                    tracing::warn!(job_id = %job_id, error = %e, "Failed to persist job response evidence");
                }
            }
            Err(e) => {
                tracing::warn!(job_id = %job_id, error = %e, "Job polling failed");
            }
        }

        job_responses.insert(key.clone(), result);

        if interrupted {
            break;
        }
    }

    // -----------------------------------------------------------------------
    // Evaluate constraints for each spec (SRS-OPS-006: INFRA_ERROR)
    // -----------------------------------------------------------------------
    let results: Vec<SpecResult> = specs
        .iter()
        .map(|spec| {
            let key = group_key(spec);

            // Check if we even got a response for this group.
            let response_result = job_responses.get(&key);
            let Some(response_result) = response_result else {
                // No response recorded -- job was skipped due to shutdown.
                return SpecResult::infra_error(
                    spec.clone(),
                    "job was not submitted or polled (interrupted)".to_string(),
                    HashMap::new(),
                );
            };

            // Handle transport / polling errors.
            let response = match response_result {
                Err(e) => {
                    let mut evidence = infra_error_evidence(
                        &e.to_string(),
                        job_ids.get(&key).map(|(id, _)| id.as_str()),
                    );
                    evidence.insert("error_code".to_string(), "TRANSPORT_ERROR".to_string());
                    return SpecResult::infra_error(spec.clone(), e.to_string(), evidence);
                }
                Ok(r) => r,
            };

            // Handle server-side job failure (SRS-OPS-006, SRS-REL-003).
            if response.status == JobStatus::Failed {
                let error_msg = response
                    .error
                    .as_deref()
                    .unwrap_or("job failed with no error details")
                    .to_string();
                let mut evidence = HashMap::new();
                evidence.insert("error_code".to_string(), "RUNNER_JOB_FAILED".to_string());
                evidence.insert("error_message".to_string(), error_msg.clone());
                if let Some((id, _)) = job_ids.get(&key) {
                    evidence.insert("job_id".to_string(), id.clone());
                }
                let msg = format!("RUNNER_JOB_FAILED: {error_msg}");
                return SpecResult::infra_error(spec.clone(), msg, evidence);
            }

            // Capture result_consistent from the job output (SRS-REL-004).
            // Records true/false when the output block is present, or "null" when absent.
            let result_consistent_str = match response.output.as_ref().map(|o| o.result_consistent)
            {
                Some(rc) => rc.to_string(),
                None => "null".to_string(),
            };

            // Compute client-side ml.* accuracy metrics (T-054).
            let computed_accuracy: AccuracyMetrics = if let Some(ref out) = response.output {
                if let Some(ref outputs) = out.outputs {
                    if let Some(ref refs) = spec.reference_outputs {
                        accuracy_compute::compute(outputs, refs, &spec.workload_type)
                    } else {
                        AccuracyMetrics::new()
                    }
                } else {
                    AccuracyMetrics::new()
                }
            } else {
                AccuracyMetrics::new()
            };

            // Precision comparison (SRS-FUNC-016).
            let precision_results: AccuracyMetrics = match (
                response.output.as_ref().and_then(|o| o.outputs.as_ref()),
                spec.precision_reference.as_ref(),
                spec.precision_tolerance,
            ) {
                (Some(outs), Some(prec_ref), Some(tol)) => {
                    accuracy_compute::precision_compare(outs, prec_ref, tol)
                }
                _ => AccuracyMetrics::new(),
            };

            // Normal path: evaluate constraints.
            let outcomes: Vec<ConstraintOutcome> = spec
                .constraints
                .iter()
                .map(|constraint| {
                    let name = constraint.metric_name();
                    tracing::info!(
                        uid = %spec.requirement_uid,
                        "Evaluating {} constraints for {}",
                        spec.constraints.len(),
                        spec.requirement_uid,
                    );
                    if !constraint.is_server_side() {
                        // Look up from client-side accuracy metrics (T-054).
                        let maybe_computed =
                            computed_accuracy.get(name).or_else(|| precision_results.get(name));
                        return match maybe_computed {
                            Some(&val) => {
                                let mut outcome = evaluate(val, constraint);
                                if let Some((id, _)) = job_ids.get(&key) {
                                    outcome.evidence.insert("job_id".into(), id.clone());
                                }
                                outcome.evidence.insert(
                                    "result_consistent".into(),
                                    result_consistent_str.clone(),
                                );
                                outcome
                            }
                            None => failing_outcome(name, "reference_outputs not provided in spec"),
                        };
                    }
                    let maybe_value = response.metrics.get(name);
                    match maybe_value {
                        Some(value) => {
                            let mut outcome = evaluate(value, constraint);
                            if let Some((id, _)) = job_ids.get(&key) {
                                outcome.evidence.insert("job_id".into(), id.clone());
                            }
                            // Record result_consistent in every outcome's evidence
                            // (SRS-REL-004): null/absent -> "null", true/false as string.
                            outcome
                                .evidence
                                .insert("result_consistent".into(), result_consistent_str.clone());
                            outcome
                        }
                        None => failing_outcome(name, "metric unavailable"),
                    }
                })
                .collect();
            SpecResult::new(spec.clone(), outcomes)
        })
        .collect();

    Ok((
        RunResult {
            results,
            timestamp: now_iso8601(),
            config_version: config.project.version.clone(),
            interrupted,
            requirement_set: baseline_req_set,
        },
        provenance,
    ))
}

fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use crate::constraint::evaluator::evaluate;
    use crate::constraint::{Constraint, Op};
    use crate::metrics::{Platform, Target};
    use crate::specs::{OutcomeKind, SpecResult, TestSpec};
    use sqt_types::{JobMetrics, WorkloadType};

    use super::{build_job_request, failing_outcome, group_key};

    fn make_target(input: Option<&str>, timeout_secs: u64) -> Target {
        Target {
            binary: PathBuf::from("build/example"),
            input: input.map(PathBuf::from),
            platform: Platform::Local,
            working_dir: PathBuf::from("."),
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    fn make_spec(workload: WorkloadType, input: Option<&str>, timeout_secs: u64) -> TestSpec {
        TestSpec {
            requirement_uid: format!("REQ-{input:?}"),
            workload_type: workload,
            target: make_target(input, timeout_secs),
            constraints: vec![],
            ..Default::default()
        }
    }

    #[test]
    fn grouping_deduplicates_same_input() {
        let t1 = make_spec(WorkloadType::ImageClassification, Some("input.bin"), 30);
        let t2 = make_spec(WorkloadType::ImageClassification, Some("input.bin"), 60);
        assert_eq!(group_key(&t1), group_key(&t2));
    }

    #[test]
    fn grouping_separates_different_inputs() {
        let t_a = make_spec(WorkloadType::ImageClassification, Some("input_a.bin"), 30);
        let t_b = make_spec(WorkloadType::ImageClassification, Some("input_b.bin"), 30);
        assert_ne!(group_key(&t_a), group_key(&t_b));
    }

    #[test]
    fn grouping_separates_different_workloads() {
        let t_a = make_spec(WorkloadType::ImageClassification, None, 30);
        let t_b = make_spec(WorkloadType::ObjectDetection, None, 30);
        assert_ne!(group_key(&t_a), group_key(&t_b));
    }

    #[test]
    fn grouping_none_input() {
        let t = make_spec(WorkloadType::ImageClassification, None, 30);
        assert_eq!(group_key(&t).1, None);
    }

    #[test]
    fn grouping_takes_max_timeout() {
        let spec30 = make_spec(WorkloadType::ImageClassification, None, 30);
        let spec60 = make_spec(WorkloadType::ImageClassification, None, 60);
        let spec45 = make_spec(WorkloadType::ImageClassification, None, 45);
        let refs: Vec<&TestSpec> = vec![&spec30, &spec60, &spec45];
        let request = build_job_request(&WorkloadType::ImageClassification, None, &refs);
        assert_eq!(request.timeout_secs, Some(60));
    }

    #[test]
    fn null_metric_maps_to_failing_outcome() {
        // AD-013 / ARCH 5.5.2: null metric -> failing outcome, not an error.
        let outcome = failing_outcome("time.avg_ms", "metric unavailable");
        assert!(!outcome.passed);
        assert!(outcome.actual.is_nan(), "actual must be NaN when metric unavailable");
        assert!(outcome.message.contains("time.avg_ms"));
        assert!(outcome.message.contains("FAIL"));
        assert!(outcome.evidence.is_empty(), "unavailable metric has no evidence");
    }

    #[test]
    fn present_metric_maps_to_passing_outcome() {
        let constraint = Constraint::execution_time(Op::Lte, 100.0);
        let outcome = evaluate(87.3, &constraint);
        assert!(outcome.passed);
        assert!(outcome.message.contains("PASS"));
    }

    #[test]
    fn present_metric_maps_to_failing_outcome() {
        let constraint = Constraint::execution_time(Op::Lte, 100.0);
        let outcome = evaluate(150.0, &constraint);
        assert!(!outcome.passed);
        assert!(outcome.message.contains("FAIL"));
    }

    #[test]
    fn job_metrics_get_works() {
        let m = JobMetrics {
            return_code: Some(0),
            time_avg_ms: Some(87.3),
            memory_peak_rss_kb: Some(4096.0),
            ..Default::default()
        };
        assert_eq!(m.get("return_code"), Some(0.0));
        assert_eq!(m.get("time.avg_ms"), Some(87.3));
        assert_eq!(m.get("memory.peak_rss_kb"), Some(4096.0));
        assert!(m.get("no.such.metric").is_none());
    }

    // --- JobRequest construction (T-042) -------------------------------------

    #[test]
    fn build_job_request_workload_type_propagates() {
        // The workload type from the group key must appear in the request body.
        let spec = make_spec(WorkloadType::ObjectDetection, None, 30);
        let refs = vec![&spec];
        let req = build_job_request(&WorkloadType::ObjectDetection, None, &refs);
        assert_eq!(req.use_case, WorkloadType::ObjectDetection);
    }

    #[test]
    fn build_job_request_input_becomes_json_string() {
        // The input path must be forwarded as a JSON string value in the request.
        let spec = make_spec(WorkloadType::Regression, Some("data.csv"), 30);
        let refs = vec![&spec];
        let input = Some("data.csv".to_string());
        let req = build_job_request(&WorkloadType::Regression, input.as_ref(), &refs);
        assert_eq!(
            req.input,
            Some(serde_json::Value::String("data.csv".into())),
            "input path must be forwarded as a JSON string"
        );
    }

    // --- ml.* constraint guard (T-054) ---------------------------------------

    #[test]
    fn ml_constraint_produces_failing_outcome_without_reference() {
        // T-054: ml.* metrics are client-side. Without reference_outputs, the
        // orchestrator must produce a failing outcome distinguishing
        // "reference not provided" from "metric unavailable" (a server-side tool
        // that was disabled).
        let outcome = failing_outcome("ml.accuracy", "reference_outputs not provided in spec");
        assert!(!outcome.passed);
        assert!(outcome.message.contains("ml.accuracy"));
        assert!(
            outcome.message.contains("reference_outputs"),
            "outcome must mention reference_outputs: {}",
            outcome.message
        );
    }

    // --- INFRA_ERROR does not propagate as Err (SRS-OPS-006) -----------------

    #[test]
    fn infra_error_spec_result_does_not_pass() {
        // T-074: INFRA_ERROR must be recorded as a non-passing SpecResult,
        // not as an Err that aborts the whole run.
        let spec = make_spec(WorkloadType::ImageClassification, None, 30);
        let result = SpecResult::infra_error(
            spec,
            "RUNNER_ERROR: internal server error".to_string(),
            std::collections::HashMap::new(),
        );
        assert!(!result.passed, "INFRA_ERROR must not pass");
        assert_eq!(result.outcome, OutcomeKind::InfraError, "outcome must be InfraError");
        assert_eq!(result.outcomes.len(), 1, "must carry exactly one synthetic outcome");
    }

    #[test]
    fn zero_constraint_spec_produces_fail_not_pass() {
        // Vacuous-pass prevention: a spec with no constraints must never PASS.
        let spec = make_spec(WorkloadType::ImageClassification, None, 30);
        let result = SpecResult::new(spec, vec![]);
        assert!(!result.passed, "zero-constraint spec must fail (vacuous pass prevention)");
        assert_eq!(result.outcome, OutcomeKind::Fail);
    }
}
