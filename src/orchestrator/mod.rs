use std::collections::HashMap;

use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};

use crate::api::types::{Instrumentation, JobRequest, MeasurementMode, WorkloadType};
use crate::api::ApiClient;
use crate::config::SqtConfig;
use crate::constraint::ConstraintOutcome;
use crate::constraint::evaluator::evaluate;
use crate::specs::{RunResult, SpecResult, TestSpec};

type GroupKey = (WorkloadType, Option<String>);

/// Build the grouping key for a spec: (workload_type, input_path).
fn group_key(spec: &TestSpec) -> GroupKey {
    (
        spec.workload_type.clone(),
        spec.target.input.as_ref().map(|p| p.to_string_lossy().into_owned()),
    )
}

/// Read and base64-encode the model ZIP file from disk.
///
/// Returns `None` if no path is configured.
fn read_model_zip(config: &SqtConfig) -> Result<Option<String>> {
    let Some(ref path) = config.model_zip_path else {
        return Ok(None);
    };
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read model ZIP: {}", path.display()))?;
    Ok(Some(BASE64.encode(&bytes)))
}

/// Build a `JobRequest` for a group of specs that share the same (workload_type, input).
fn build_job_request(
    workload_type: &WorkloadType,
    input: &Option<String>,
    specs: &[&TestSpec],
    model_zip: Option<String>,
) -> JobRequest {
    let max_timeout = specs.iter().map(|s| s.target.timeout.as_secs()).max().unwrap_or(30);
    JobRequest {
        model: workload_type.clone(),
        model_zip,
        input: input.as_ref().map(|s| serde_json::Value::String(s.clone())),
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

/// Run all specs against the single-endpoint API.
///
/// Groups specs by (workload_type, input), submits one job per group,
/// waits for all results, then evaluates each spec's constraints.
///
/// # Errors
///
/// Returns an error if job submission or polling fails.
pub async fn run(specs: &[TestSpec], client: &ApiClient, config: &SqtConfig) -> Result<RunResult> {
    if specs.is_empty() {
        return Ok(RunResult {
            results: vec![],
            timestamp: now_iso8601(),
            config_version: config.project.version.clone(),
        });
    }

    let model_zip = read_model_zip(config)?;

    // Group specs by (workload_type, input).
    let mut groups: HashMap<GroupKey, Vec<&TestSpec>> = HashMap::new();
    for spec in specs {
        groups.entry(group_key(spec)).or_default().push(spec);
    }

    // Submit one job per group.
    tracing::info!("Submitting {} job(s) to {}", groups.len(), config.api.url);
    let mut job_ids: HashMap<GroupKey, String> = HashMap::new();
    for ((workload_type, input), group) in &groups {
        let request = build_job_request(workload_type, input, group, model_zip.clone());
        let job_id = client.submit_job(&request).await?;
        tracing::info!(job_id, workload = ?workload_type, input = ?input, "Job accepted");
        job_ids.insert((workload_type.clone(), input.clone()), job_id);
    }

    // Wait for all jobs.
    let mut job_responses = HashMap::new();
    for (key, job_id) in &job_ids {
        let response = client.wait_for_job(job_id).await?;
        tracing::info!(job_id, status = ?response.status, "Job finished");
        job_responses.insert(key.clone(), response);
    }

    // Evaluate constraints for each spec.
    let results: Vec<SpecResult> = specs
        .iter()
        .map(|spec| {
            let key = group_key(spec);
            let outcomes: Vec<ConstraintOutcome> = spec
                .constraints
                .iter()
                .map(|constraint| {
                    let name = constraint.metric_name();
                    let maybe_value = job_responses.get(&key).and_then(|r| r.metrics.get(name));
                    match maybe_value {
                        Some(value) => {
                            let mut outcome = evaluate(value, constraint);
                            if let Some(id) = job_ids.get(&key) {
                                outcome.evidence.insert("job_id".into(), id.clone());
                            }
                            outcome
                        }
                        None => failing_outcome(name, "metric unavailable"),
                    }
                })
                .collect();
            SpecResult::new(spec.clone(), outcomes)
        })
        .collect();

    Ok(RunResult {
        results,
        timestamp: now_iso8601(),
        config_version: config.project.version.clone(),
    })
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

    use crate::api::types::{JobMetrics, WorkloadType};
    use crate::constraint::{Constraint, Op};
    use crate::constraint::evaluator::evaluate;
    use crate::metrics::{Platform, Target};
    use crate::specs::TestSpec;

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
        let request = build_job_request(&WorkloadType::ImageClassification, &None, &refs, None);
        assert_eq!(request.timeout_secs, Some(60));
    }

    #[test]
    fn null_metric_maps_to_failing_outcome() {
        let outcome = failing_outcome("time.avg_ms", "metric unavailable");
        assert!(!outcome.passed);
        assert!(outcome.message.contains("time.avg_ms"));
        assert!(outcome.message.contains("FAIL"));
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
    fn build_job_request_embeds_model_zip() {
        // T-042: model ZIP read once and embedded in every JobRequest.
        let spec = make_spec(WorkloadType::ImageClassification, None, 30);
        let refs = vec![&spec];
        let zip = Some("base64encodedzip==".to_string());
        let req = build_job_request(&WorkloadType::ImageClassification, &None, &refs, zip.clone());
        assert_eq!(req.model_zip, zip, "model_zip must be passed through to the request");
    }

    #[test]
    fn build_job_request_no_model_zip_when_absent() {
        // When no ZIP is configured, model_zip must be None in the request.
        let spec = make_spec(WorkloadType::ImageClassification, None, 30);
        let refs = vec![&spec];
        let req = build_job_request(&WorkloadType::ImageClassification, &None, &refs, None);
        assert!(req.model_zip.is_none());
    }

    #[test]
    fn build_job_request_workload_type_propagates() {
        // The workload type from the group key must appear in the request body.
        let spec = make_spec(WorkloadType::ObjectDetection, None, 30);
        let refs = vec![&spec];
        let req = build_job_request(&WorkloadType::ObjectDetection, &None, &refs, None);
        assert_eq!(req.model, WorkloadType::ObjectDetection);
    }

    #[test]
    fn build_job_request_input_becomes_json_string() {
        // The input path must be forwarded as a JSON string value in the request.
        let spec = make_spec(WorkloadType::Regression, Some("data.csv"), 30);
        let refs = vec![&spec];
        let input = Some("data.csv".to_string());
        let req = build_job_request(&WorkloadType::Regression, &input, &refs, None);
        assert_eq!(
            req.input,
            Some(serde_json::Value::String("data.csv".into())),
            "input path must be forwarded as a JSON string"
        );
    }

    #[test]
    fn build_job_request_no_input_is_none() {
        let spec = make_spec(WorkloadType::Classification, None, 30);
        let refs = vec![&spec];
        let req = build_job_request(&WorkloadType::Classification, &None, &refs, None);
        assert!(req.input.is_none());
    }

    // --- Null metric = failing outcome (AD-013) ------------------------------

    #[test]
    fn unavailable_metric_produces_failing_outcome_not_error() {
        // AD-013 / ARCH §5.5.2: a null metric field means a tool was unavailable,
        // not that the job failed. The orchestrator must map null → ConstraintOutcome
        // { passed: false } — not bail with an error.
        let outcome = failing_outcome("memory.leaks_bytes", "metric unavailable");
        assert!(!outcome.passed, "null metric must produce a FAIL outcome");
        assert!(
            outcome.actual.is_nan(),
            "actual value must be NaN when metric was unavailable"
        );
        assert!(outcome.message.contains("FAIL"));
        assert!(outcome.message.contains("memory.leaks_bytes"));
        assert!(
            outcome.evidence.is_empty(),
            "unavailable metric has no evidence to record"
        );
    }
}
