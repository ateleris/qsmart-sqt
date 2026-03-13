use std::collections::HashMap;

use anyhow::Result;

use crate::api::types::{AnalysisRequest, RunRequest, RunStatusResponse};
use crate::api::ApiClient;
use crate::config::SqtConfig;
use crate::constraint::ConstraintOutcome;
use crate::constraint::evaluator::evaluate;
use crate::specs::{RunResult, SpecResult, TestSpec};

/// Build the grouping key for a target: the input path.
///
/// The server knows its own binary, so grouping is by input only.
fn run_key(target: &crate::metrics::Target) -> Option<String> {
    target.input.as_ref().map(|p| p.to_string_lossy().into_owned())
}

/// Build a `RunRequest` for a group of specs that share the same input.
///
/// The timeout is the maximum across all specs in the group.
fn build_run_request(input: &Option<String>, specs: &[&TestSpec]) -> RunRequest {
    let max_timeout = specs.iter().map(|s| s.target.timeout.as_secs()).max().unwrap_or(30);
    RunRequest { input: input.clone(), timeout_secs: Some(max_timeout) }
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
/// Groups specs by input, submits one run job per unique input and one
/// analysis job for the server's configured binary, waits for all results,
/// then evaluates each spec's constraints.
///
/// The server knows which binary to test - the client does not send it.
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

    // Group specs by input.
    let mut groups: HashMap<Option<String>, Vec<&TestSpec>> = HashMap::new();
    for spec in specs {
        groups.entry(run_key(&spec.target)).or_default().push(spec);
    }

    // Submit one run job per input group.
    tracing::info!("Submitting {} run job(s) to {}", groups.len(), config.api.url);
    let mut run_ids: HashMap<Option<String>, String> = HashMap::new();
    for (input, group) in &groups {
        let request = build_run_request(input, group);
        let run_id = client.submit_run(&request).await?;
        tracing::info!(run_id, input = ?input, "Run accepted");
        run_ids.insert(input.clone(), run_id);
    }

    // Submit a single analysis job (the server knows which binary to analyse).
    let analysis_timeout = specs.iter().map(|s| s.target.timeout.as_secs()).max().unwrap_or(30);
    let analysis_request = AnalysisRequest { timeout_secs: Some(analysis_timeout) };
    let analysis_id = client.submit_analysis(&analysis_request).await?;
    tracing::info!(analysis_id, "Analysis accepted");

    // Wait for all run jobs.
    let mut run_responses: HashMap<Option<String>, RunStatusResponse> = HashMap::new();
    for (input, run_id) in &run_ids {
        let response = client.wait_for_run(run_id).await?;
        tracing::info!(run_id, status = ?response.status, "Run finished");
        run_responses.insert(input.clone(), response);
    }

    // Wait for the analysis job.
    let analysis_response = client.wait_for_analysis(&analysis_id).await?;
    tracing::info!(analysis_id, status = ?analysis_response.status, "Analysis finished");

    // Evaluate constraints for each spec.
    let results: Vec<SpecResult> = specs
        .iter()
        .map(|spec| {
            let input = run_key(&spec.target);
            let outcomes: Vec<ConstraintOutcome> = spec
                .constraints
                .iter()
                .map(|constraint| {
                    let name = constraint.metric_name();
                    let maybe_value = if constraint.is_analysis() {
                        analysis_response.metrics.get(name)
                    } else {
                        run_responses.get(&input).and_then(|r| r.metrics.get(name))
                    };
                    match maybe_value {
                        Some(value) => {
                            let mut outcome = evaluate(value, constraint);
                            if constraint.is_analysis() {
                                outcome.evidence.insert("analysis_id".into(), analysis_id.clone());
                            } else if let Some(id) = run_ids.get(&input) {
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
    use crate::constraint::{Constraint, Op};
    use crate::constraint::evaluator::evaluate;
    use crate::metrics::{Platform, Target};
    use crate::specs::TestSpec;

    use super::{build_run_request, failing_outcome, run_key};

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

    // --- constraint routing ---

    #[test]
    fn analysis_constraints_are_analysis() {
        assert!(Constraint::sonar_critical_issues(Op::Eq, 0).is_analysis());
        assert!(Constraint::sonar_major_issues(Op::Eq, 0).is_analysis());
        assert!(Constraint::static_warnings(Op::Lte, 5).is_analysis());
        assert!(Constraint::binary_size_kb(Op::Lte, 1024.0).is_analysis());
    }

    #[test]
    fn run_constraints_are_not_analysis() {
        assert!(!Constraint::return_code(Op::Eq, 0).is_analysis());
        assert!(!Constraint::execution_time(Op::Lte, 100.0).is_analysis());
        assert!(!Constraint::memory_peak_rss(Op::Lte, 4096.0).is_analysis());
        assert!(!Constraint::memory_leaks(Op::Eq, 0.0).is_analysis());
        assert!(!Constraint::coverage_line_pct(Op::Gte, 80.0).is_analysis());
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

    // --- run_key / grouping ---

    #[test]
    fn grouping_deduplicates_same_input() {
        let t1 = make_target("/bin/app", Some("input.bin"), 30);
        let t2 = make_target("/bin/app", Some("input.bin"), 60);
        assert_eq!(run_key(&t1), run_key(&t2));
        assert_eq!(run_key(&t1), Some("input.bin".to_string()));
    }

    #[test]
    fn grouping_separates_different_inputs() {
        let t_a = make_target("/bin/app", Some("input_a.bin"), 30);
        let t_b = make_target("/bin/app", Some("input_b.bin"), 30);
        assert_ne!(run_key(&t_a), run_key(&t_b));
    }

    #[test]
    fn grouping_none_input() {
        let t = make_target("/bin/app", None, 30);
        assert_eq!(run_key(&t), None);
    }

    #[test]
    fn grouping_takes_max_timeout() {
        let spec30 = make_spec("/bin/app", None, 30);
        let spec60 = make_spec("/bin/app", None, 60);
        let spec45 = make_spec("/bin/app", None, 45);

        let refs: Vec<&TestSpec> = vec![&spec30, &spec60, &spec45];
        let input = run_key(&spec30.target);
        let request = build_run_request(&input, &refs);
        assert_eq!(request.timeout_secs, Some(60));
    }
}
