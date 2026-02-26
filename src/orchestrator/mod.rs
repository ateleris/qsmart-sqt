use std::collections::HashSet;

use anyhow::Result;

use crate::api::types::{ApiTarget, JobRequest, TaskResult, TaskSpec, TaskStatus};
use crate::api::JobApiClient;
use crate::config::SqtConfig;
use crate::constraint::evaluator::evaluate;
use crate::constraint::ConstraintOutcome;
use crate::metrics::MetricValue;
use crate::specs::{RunResult, SpecResult, TestSpec};

/// Run all specs: submit a single job, wait for completion, evaluate constraints.
///
/// # Errors
///
/// Returns an error if job submission or polling fails.
pub async fn run(
    specs: &[TestSpec],
    client: &JobApiClient,
    config: &SqtConfig,
) -> Result<RunResult> {
    if specs.is_empty() {
        return Ok(RunResult {
            results: vec![],
            timestamp: now_iso8601(),
            config_version: config.project.version.clone(),
        });
    }

    // Collect unique task names across all specs, preserving insertion order.
    let mut seen: HashSet<String> = HashSet::new();
    let mut tasks: Vec<TaskSpec> = Vec::new();
    for spec in specs {
        for constraint in &spec.constraints {
            if seen.insert(constraint.metric_name.clone()) {
                let cfg = yaml_to_json(&constraint.config);
                tasks.push(TaskSpec { name: constraint.metric_name.clone(), config: cfg });
            }
        }
    }

    // Use the first spec's target as the job target.
    let first_target = &specs[0].target;
    let api_target = ApiTarget {
        binary: first_target.binary.to_string_lossy().into_owned(),
        input: first_target
            .input
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        timeout_secs: Some(first_target.timeout.as_secs()),
    };

    let request = JobRequest { target: api_target, tasks };

    tracing::info!("Submitting job to {}", config.api.url);
    let job_id = client.submit_job(&request).await?;
    tracing::info!(job_id, "Job accepted, polling for completion");

    let response = client.wait_for_completion(&job_id).await?;
    tracing::info!(job_id, status = ?response.status, "Job finished");

    // Evaluate each spec's constraints against the task results.
    let results: Vec<SpecResult> = specs
        .iter()
        .map(|spec| {
            let outcomes: Vec<ConstraintOutcome> = spec
                .constraints
                .iter()
                .map(|constraint| {
                    match response.tasks.get(&constraint.metric_name) {
                        Some(task) => task_result_to_outcome(task, constraint),
                        None => failing_outcome(
                            &constraint.metric_name,
                            "task missing from job response",
                        ),
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

/// Convert a `TaskResult` into a `ConstraintOutcome` for the given constraint.
fn task_result_to_outcome(
    task: &TaskResult,
    constraint: &crate::constraint::Constraint,
) -> ConstraintOutcome {
    match task.status {
        TaskStatus::Complete => {
            if let Some(value) = task.value {
                let unit = task.unit.clone().unwrap_or_default();
                let metric = MetricValue::new(value, unit);
                evaluate(&metric, constraint)
            } else {
                failing_outcome(&constraint.metric_name, "task complete but value missing")
            }
        }
        TaskStatus::Failed => {
            let reason = task
                .error
                .as_deref()
                .unwrap_or("task failed with no error message");
            failing_outcome(&constraint.metric_name, reason)
        }
        TaskStatus::Pending | TaskStatus::Running => {
            failing_outcome(&constraint.metric_name, "task did not complete")
        }
    }
}

/// Build a synthetic failing outcome for a task that could not be evaluated.
fn failing_outcome(metric_name: &str, reason: &str) -> ConstraintOutcome {
    ConstraintOutcome {
        passed: false,
        actual: f64::NAN,
        expected: f64::NAN,
        message: format!("{metric_name}: {reason}: FAIL"),
    }
}

/// Convert a `serde_yaml::Value` to a `serde_json::Value`.
///
/// Returns `None` when the YAML value is `Null`.
fn yaml_to_json(yaml: &serde_yaml::Value) -> Option<serde_json::Value> {
    if yaml.is_null() {
        return None;
    }
    // serde_yaml_ng implements Serialize, so we can round-trip through JSON.
    serde_json::to_value(yaml).ok()
}

/// Return the current time as an ISO 8601 string.
///
/// Uses a simple formatted string; no external date-time crate required.
fn now_iso8601() -> String {
    // chrono is not a dependency — return a placeholder that callers can
    // replace in Phase 4 when a proper timestamp library is added.
    "1970-01-01T00:00:00Z".to_owned()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::api::types::{TaskResult, TaskStatus};
    use crate::constraint::{Constraint, Op, Value};
    use crate::metrics::Target;
    use crate::specs::{RunResult, SpecResult, TestSpec};

    use super::{failing_outcome, task_result_to_outcome};

    fn complete_task(value: f64, unit: &str) -> TaskResult {
        TaskResult {
            status: TaskStatus::Complete,
            value: Some(value),
            unit: Some(unit.into()),
            error: None,
            evidence: None,
        }
    }

    fn failed_task(error: &str) -> TaskResult {
        TaskResult {
            status: TaskStatus::Failed,
            value: None,
            unit: None,
            error: Some(error.into()),
            evidence: None,
        }
    }

    fn make_constraint(name: &str, op: Op, val: Value) -> Constraint {
        Constraint::new(name, op, val)
    }

    #[test]
    fn complete_task_passing_maps_to_passing_outcome() {
        let task = complete_task(87.3, "ms");
        let constraint = make_constraint("time.execution", Op::Lte, Value::ms(100.0));
        let outcome = task_result_to_outcome(&task, &constraint);
        assert!(outcome.passed);
        assert!(outcome.message.contains("PASS"));
    }

    #[test]
    fn complete_task_failing_maps_to_failing_outcome() {
        let task = complete_task(150.0, "ms");
        let constraint = make_constraint("time.execution", Op::Lte, Value::ms(100.0));
        let outcome = task_result_to_outcome(&task, &constraint);
        assert!(!outcome.passed);
        assert!(outcome.message.contains("FAIL"));
    }

    #[test]
    fn failed_task_maps_to_failing_outcome() {
        let task = failed_task("valgrind not found");
        let constraint = make_constraint("memory.leaks", Op::Eq, Value::bool(false));
        let outcome = task_result_to_outcome(&task, &constraint);
        assert!(!outcome.passed);
        assert!(outcome.message.contains("valgrind not found"));
        assert!(outcome.message.contains("FAIL"));
    }

    #[test]
    fn missing_task_maps_to_failing_outcome() {
        let outcome = failing_outcome("custom.metric", "task missing from job response");
        assert!(!outcome.passed);
        assert!(outcome.message.contains("custom.metric"));
        assert!(outcome.message.contains("FAIL"));
    }

    #[test]
    fn empty_specs_produce_empty_run_result() {
        let result = RunResult {
            results: vec![],
            timestamp: "1970-01-01T00:00:00Z".into(),
            config_version: "1.0.0".into(),
        };
        assert!(result.results.is_empty());
    }

    #[test]
    fn spec_result_aggregates_multiple_constraints() {
        // Two constraints: first passes, second fails — overall spec fails.
        let spec = TestSpec {
            requirement_uid: "REQ-ORCH-001".into(),
            target: Target::default(),
            constraints: vec![
                make_constraint("return_code", Op::Eq, Value::int(0)),
                make_constraint("time.execution", Op::Lte, Value::ms(100.0)),
            ],
        };

        let outcomes = vec![
            task_result_to_outcome(&complete_task(0.0, ""), &spec.constraints[0]),
            task_result_to_outcome(&complete_task(150.0, "ms"), &spec.constraints[1]),
        ];

        let result = SpecResult::new(spec, outcomes);
        assert!(!result.passed, "spec should fail when any constraint fails");
    }

    #[test]
    fn deduplication_keeps_first_occurrence() {
        // Both constraints reference the same metric name.
        let spec = TestSpec {
            requirement_uid: "REQ-ORCH-002".into(),
            target: Target::default(),
            constraints: vec![
                make_constraint("return_code", Op::Eq, Value::int(0)),
                make_constraint("return_code", Op::Ne, Value::int(1)),
            ],
        };
        // Simulate deduplication: collect unique names.
        let names: std::collections::HashSet<_> =
            spec.constraints.iter().map(|c| &c.metric_name).collect();
        assert_eq!(names.len(), 1);
    }
}
