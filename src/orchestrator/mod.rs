use std::collections::HashMap;

use anyhow::Result;

use crate::api::types::{AnalysisRequest, AnalysisStatusResponse, RunRequest, RunStatusResponse};
use crate::api::ApiClient;
use crate::config::SqtConfig;
use crate::constraint::evaluator::evaluate;
use crate::constraint::ConstraintOutcome;
use crate::metrics::{MetricValue, Target};
use crate::specs::{RunResult, SpecResult, TestSpec};

/// Metric name prefixes that belong to analysis jobs (binary-level, not per-run).
const ANALYSIS_PREFIXES: &[&str] = &["sonar.", "static.", "binary."];

/// Return `true` if `name` should be routed to an analysis job rather than a run job.
fn is_analysis_metric(name: &str) -> bool {
    ANALYSIS_PREFIXES.iter().any(|p| name.starts_with(p))
}

/// Return the unit string for a metric, matching `Value::unit()`.
fn metric_unit(name: &str) -> &'static str {
    match name {
        "time.execution" => "ms",
        "memory.peak_rss" | "memory.avg_rss" | "binary.size_kb" => "KB",
        "cpu.max_pct" | "cpu.avg_pct" | "coverage.line_pct" => "%",
        "memory.leaks" => "bytes",
        _ => "",
    }
}

/// Build the grouping key for a target: `(binary_path, input_path)`.
fn run_key(target: &Target) -> (String, Option<String>) {
    (
        target.binary.to_string_lossy().into_owned(),
        target.input.as_ref().map(|p| p.to_string_lossy().into_owned()),
    )
}

/// Build a `RunRequest` for a group of specs that share the same `(binary, input)`.
///
/// The timeout is the maximum across all specs in the group.
fn build_run_request(key: &(String, Option<String>), specs: &[&TestSpec]) -> RunRequest {
    let max_timeout = specs.iter().map(|s| s.target.timeout.as_secs()).max().unwrap_or(30);
    let (binary, input) = key.clone();
    RunRequest { binary, input, timeout_secs: Some(max_timeout) }
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

/// Run all specs against the two-endpoint API.
///
/// Groups specs by `(binary, input)`, submits one run job per group and one
/// analysis job per unique binary, waits for all results, then evaluates each
/// spec's constraints.
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

    // Group specs by (binary, input).
    let mut groups: HashMap<(String, Option<String>), Vec<&TestSpec>> = HashMap::new();
    for spec in specs {
        groups.entry(run_key(&spec.target)).or_default().push(spec);
    }

    // Submit one run job per group.
    tracing::info!("Submitting {} run job(s) to {}", groups.len(), config.api.url);
    let mut run_ids: HashMap<(String, Option<String>), String> = HashMap::new();
    for (key, group) in &groups {
        let request = build_run_request(key, group);
        let run_id = client.submit_run(&request).await?;
        tracing::info!(run_id, binary = %key.0, "Run accepted");
        run_ids.insert(key.clone(), run_id);
    }

    // Submit one analysis job per unique binary.
    let mut seen_binaries: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut analysis_ids: HashMap<String, String> = HashMap::new();
    for spec in specs {
        let binary = spec.target.binary.to_string_lossy().into_owned();
        if seen_binaries.insert(binary.clone()) {
            let max_timeout = specs
                .iter()
                .filter(|s| s.target.binary.to_string_lossy().as_ref() == binary.as_str())
                .map(|s| s.target.timeout.as_secs())
                .max()
                .unwrap_or(30);
            let request = AnalysisRequest { binary: binary.clone(), timeout_secs: Some(max_timeout) };
            let analysis_id = client.submit_analysis(&request).await?;
            tracing::info!(analysis_id, binary = %binary, "Analysis accepted");
            analysis_ids.insert(binary, analysis_id);
        }
    }

    // Wait for all run jobs.
    let mut run_responses: HashMap<(String, Option<String>), RunStatusResponse> = HashMap::new();
    for (key, run_id) in &run_ids {
        let response = client.wait_for_run(run_id).await?;
        tracing::info!(run_id, status = ?response.status, "Run finished");
        run_responses.insert(key.clone(), response);
    }

    // Wait for all analysis jobs.
    let mut analysis_responses: HashMap<String, AnalysisStatusResponse> = HashMap::new();
    for (binary, analysis_id) in &analysis_ids {
        let response = client.wait_for_analysis(analysis_id).await?;
        tracing::info!(analysis_id, status = ?response.status, "Analysis finished");
        analysis_responses.insert(binary.clone(), response);
    }

    // Evaluate constraints for each spec.
    let results: Vec<SpecResult> = specs
        .iter()
        .map(|spec| {
            let key = run_key(&spec.target);
            let binary = key.0.clone();
            let outcomes: Vec<ConstraintOutcome> = spec
                .constraints
                .iter()
                .map(|constraint| {
                    let name = &constraint.metric_name;
                    let maybe_value = if is_analysis_metric(name) {
                        analysis_responses.get(&binary).and_then(|r| r.metrics.get(name))
                    } else {
                        run_responses.get(&key).and_then(|r| r.metrics.get(name))
                    };
                    match maybe_value {
                        Some(value) => {
                            let metric = MetricValue::new(value, metric_unit(name));
                            let mut outcome = evaluate(&metric, constraint);
                            if is_analysis_metric(name) {
                                if let Some(id) = analysis_ids.get(&binary) {
                                    outcome.evidence.insert("analysis_id".into(), id.clone());
                                }
                            } else if let Some(id) = run_ids.get(&key) {
                                outcome.evidence.insert("run_id".into(), id.clone());
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

/// Return the current time as an ISO 8601 string (UTC, second precision).
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

    use crate::api::types::{AnalysisMetrics, RunMetrics};
    use crate::constraint::{Constraint, Op, Value};
    use crate::constraint::evaluator::evaluate;
    use crate::metrics::{MetricValue, Platform, Target};
    use crate::specs::TestSpec;

    use super::{
        build_run_request, failing_outcome, is_analysis_metric, metric_unit, run_key,
    };

    fn make_target(binary: &str, input: Option<&str>, timeout_secs: u64) -> Target {
        Target {
            binary: PathBuf::from(binary),
            input: input.map(PathBuf::from),
            platform: Platform::Local,
            working_dir: PathBuf::from("."),
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    fn make_spec(binary: &str, input: Option<&str>, timeout_secs: u64) -> TestSpec {
        TestSpec {
            requirement_uid: format!("REQ-{binary}"),
            target: make_target(binary, input, timeout_secs),
            constraints: vec![],
        }
    }

    // --- is_analysis_metric ---

    #[test]
    fn is_analysis_metric_routing() {
        assert!(is_analysis_metric("sonar.critical_issues"));
        assert!(is_analysis_metric("sonar.major_issues"));
        assert!(is_analysis_metric("static.warnings"));
        assert!(is_analysis_metric("binary.size_kb"));
        assert!(!is_analysis_metric("return_code"));
        assert!(!is_analysis_metric("time.execution"));
        assert!(!is_analysis_metric("memory.peak_rss"));
        assert!(!is_analysis_metric("memory.leaks"));
        assert!(!is_analysis_metric("coverage.line_pct"));
    }

    // --- metric_unit ---

    #[test]
    fn metric_unit_matches_known_metrics() {
        assert_eq!(metric_unit("return_code"), "");
        assert_eq!(metric_unit("time.execution"), "ms");
        assert_eq!(metric_unit("memory.peak_rss"), "KB");
        assert_eq!(metric_unit("memory.avg_rss"), "KB");
        assert_eq!(metric_unit("binary.size_kb"), "KB");
        assert_eq!(metric_unit("cpu.max_pct"), "%");
        assert_eq!(metric_unit("cpu.avg_pct"), "%");
        assert_eq!(metric_unit("coverage.line_pct"), "%");
        assert_eq!(metric_unit("memory.leaks"), "bytes");
        assert_eq!(metric_unit("sonar.critical_issues"), "");
        assert_eq!(metric_unit("ml.accuracy"), "");
        assert_eq!(metric_unit("unknown_metric"), "");
    }

    // --- RunMetrics.get ---

    #[test]
    fn run_metric_extraction_complete() {
        let m = RunMetrics {
            return_code: Some(0),
            time_execution: Some(87.3),
            ..Default::default()
        };
        assert_eq!(m.get("return_code"), Some(0.0));
        assert_eq!(m.get("time.execution"), Some(87.3));
    }

    #[test]
    fn run_metric_extraction_null() {
        let m = RunMetrics::default();
        assert!(m.get("return_code").is_none());
        assert!(m.get("time.execution").is_none());
        assert!(m.get("memory.peak_rss").is_none());
    }

    // --- AnalysisMetrics.get ---

    #[test]
    fn analysis_metric_extraction() {
        let m = AnalysisMetrics {
            sonar_critical_issues: Some(3),
            ..Default::default()
        };
        assert_eq!(m.get("sonar.critical_issues"), Some(3.0));
        assert!(m.get("sonar.major_issues").is_none());
        assert!(m.get("binary.size_kb").is_none());
    }

    // --- failing_outcome ---

    #[test]
    fn null_metric_maps_to_failing_outcome() {
        let outcome = failing_outcome("time.execution", "metric unavailable");
        assert!(!outcome.passed);
        assert!(outcome.message.contains("time.execution"));
        assert!(outcome.message.contains("FAIL"));
    }

    // --- constraint evaluation ---

    #[test]
    fn present_metric_maps_to_passing_outcome() {
        let constraint = Constraint::new("time.execution", Op::Lte, Value::ms(100.0));
        let metric = MetricValue::new(87.3, metric_unit("time.execution"));
        let outcome = evaluate(&metric, &constraint);
        assert!(outcome.passed);
        assert!(outcome.message.contains("PASS"));
    }

    #[test]
    fn present_metric_maps_to_failing_outcome() {
        let constraint = Constraint::new("time.execution", Op::Lte, Value::ms(100.0));
        let metric = MetricValue::new(150.0, metric_unit("time.execution"));
        let outcome = evaluate(&metric, &constraint);
        assert!(!outcome.passed);
        assert!(outcome.message.contains("FAIL"));
    }

    // --- run_key / grouping ---

    #[test]
    fn grouping_deduplicates_same_binary_input() {
        let t1 = make_target("/bin/app", Some("input.bin"), 30);
        let t2 = make_target("/bin/app", Some("input.bin"), 60);
        assert_eq!(run_key(&t1), run_key(&t2));
        let key = run_key(&t1);
        assert_eq!(key.0, "/bin/app");
        assert_eq!(key.1.as_deref(), Some("input.bin"));
    }

    #[test]
    fn grouping_separates_different_inputs() {
        let t_a = make_target("/bin/app", Some("input_a.bin"), 30);
        let t_b = make_target("/bin/app", Some("input_b.bin"), 30);
        assert_ne!(run_key(&t_a), run_key(&t_b));
    }

    #[test]
    fn grouping_takes_max_timeout() {
        let spec30 = make_spec("/bin/app", None, 30);
        let spec60 = make_spec("/bin/app", None, 60);
        let spec45 = make_spec("/bin/app", None, 45);

        let refs: Vec<&TestSpec> = vec![&spec30, &spec60, &spec45];
        let key = run_key(&spec30.target);
        let request = build_run_request(&key, &refs);
        assert_eq!(request.timeout_secs, Some(60));
    }
}
