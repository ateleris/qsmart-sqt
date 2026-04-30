use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::specs::{RunResult, SpecResult};

// ---------------------------------------------------------------------------
// Report DTO
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct Report {
    pub timestamp: String,
    pub config_version: String,
    pub summary: Summary,
    pub results: Vec<SpecReport>,
}

#[derive(Serialize)]
pub struct Summary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
}

#[derive(Serialize)]
pub struct SpecReport {
    pub requirement_uid: String,
    pub passed: bool,
    pub target: TargetReport,
    pub outcomes: Vec<OutcomeReport>,
}

#[derive(Serialize)]
pub struct TargetReport {
    pub binary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    pub timeout_secs: u64,
}

#[derive(Serialize)]
pub struct OutcomeReport {
    pub metric: String,
    pub operator: String,
    pub passed: bool,
    pub actual: f64,
    pub expected: f64,
    pub unit: String,
    pub message: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub evidence: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

impl Report {
    pub fn from_run_result(result: &RunResult) -> Self {
        let results: Vec<SpecReport> = result.results.iter().map(spec_report).collect();
        let total = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        let failed = total - passed;
        Self {
            timestamp: result.timestamp.clone(),
            config_version: result.config_version.clone(),
            summary: Summary { total, passed, failed },
            results,
        }
    }
}

fn spec_report(sr: &SpecResult) -> SpecReport {
    let outcomes = sr
        .spec
        .constraints
        .iter()
        .zip(sr.outcomes.iter())
        .map(|(constraint, outcome)| OutcomeReport {
            metric: constraint.metric_name().to_owned(),
            operator: constraint.op().to_string(),
            passed: outcome.passed,
            actual: outcome.actual,
            expected: outcome.expected,
            unit: constraint.unit().to_owned(),
            message: outcome.message.clone(),
            evidence: outcome.evidence.clone(),
        })
        .collect();

    SpecReport {
        requirement_uid: sr.spec.requirement_uid.clone(),
        passed: sr.passed,
        target: TargetReport {
            binary: sr.spec.target.binary.to_string_lossy().into_owned(),
            input: sr.spec.target.input.as_ref().map(|p| p.to_string_lossy().into_owned()),
            timeout_secs: sr.spec.target.timeout.as_secs(),
        },
        outcomes,
    }
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// Print a human-readable summary to stdout.
pub fn print_terminal(report: &Report) {
    for spec in &report.results {
        let status = if spec.passed { "PASS" } else { "FAIL" };
        println!("{:<60} {status}", spec.requirement_uid);
        for outcome in &spec.outcomes {
            println!("    {:<20} {}", outcome.metric, outcome.message);
        }
    }
    println!("{}", "─".repeat(50));
    println!(
        "{}/{} specs passed, {} failed",
        report.summary.passed, report.summary.total, report.summary.failed
    );
}

/// Serialize the report as pretty-printed JSON to `path`.
///
/// # Errors
///
/// Returns an error if the file cannot be created or serialization fails.
pub fn write_json(report: &Report, path: &Path) -> Result<()> {
    let file = std::fs::File::create(path)?;
    serde_json::to_writer_pretty(file, report)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::constraint::{Constraint, ConstraintOutcome, Op};
    use crate::metrics::{Platform, Target};
    use crate::specs::{RunResult, SpecResult, TestSpec};

    fn make_target(binary: &str) -> Target {
        Target {
            binary: PathBuf::from(binary),
            input: None,
            platform: Platform::Local,
            working_dir: PathBuf::from("."),
            timeout: Duration::from_secs(30),
        }
    }

    fn make_spec(uid: &str, binary: &str) -> TestSpec {
        TestSpec {
            requirement_uid: uid.to_string(),
            target: make_target(binary),
            constraints: vec![Constraint::return_code(Op::Eq, 0)],
            ..Default::default()
        }
    }

    fn passing_spec_result(uid: &str, binary: &str) -> SpecResult {
        let spec = make_spec(uid, binary);
        let outcome = ConstraintOutcome {
            passed: true,
            actual: 0.0,
            expected: 0.0,
            message: "0 == 0: PASS".to_string(),
            evidence: HashMap::new(),
        };
        SpecResult::new(spec, vec![outcome])
    }

    fn failing_spec_result(uid: &str, binary: &str) -> SpecResult {
        let spec = make_spec(uid, binary);
        let outcome = ConstraintOutcome {
            passed: false,
            actual: 1.0,
            expected: 0.0,
            message: "1 == 0: FAIL".to_string(),
            evidence: HashMap::new(),
        };
        SpecResult::new(spec, vec![outcome])
    }

    fn make_run_result(results: Vec<SpecResult>) -> RunResult {
        RunResult {
            results,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            config_version: "1.0.0".to_string(),
        }
    }

    #[test]
    fn report_from_run_result_computes_summary() {
        let run_result = make_run_result(vec![
            passing_spec_result("REQ-001", "/bin/app"),
            failing_spec_result("REQ-002", "/bin/app"),
        ]);
        let report = Report::from_run_result(&run_result);
        assert_eq!(report.summary.total, 2);
        assert_eq!(report.summary.passed, 1);
        assert_eq!(report.summary.failed, 1);
    }

    #[test]
    fn report_outcome_carries_evidence() {
        let spec = make_spec("REQ-001", "/bin/app");
        let mut evidence = HashMap::new();
        evidence.insert("run_id".to_string(), "abc-123".to_string());
        let outcome = ConstraintOutcome {
            passed: true,
            actual: 0.0,
            expected: 0.0,
            message: "0 == 0: PASS".to_string(),
            evidence,
        };
        let sr = SpecResult::new(spec, vec![outcome]);
        let run_result = make_run_result(vec![sr]);
        let report = Report::from_run_result(&run_result);
        let ev = &report.results[0].outcomes[0].evidence;
        assert_eq!(ev.get("run_id").map(String::as_str), Some("abc-123"));
    }

    #[test]
    fn report_serializes_to_json() {
        let run_result = make_run_result(vec![passing_spec_result("REQ-001", "/bin/app")]);
        let report = Report::from_run_result(&run_result);
        let json = serde_json::to_string(&report).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed["summary"]["total"], 1);
        assert_eq!(parsed["summary"]["passed"], 1);
        assert_eq!(parsed["config_version"], "1.0.0");
        assert_eq!(parsed["results"][0]["requirement_uid"], "REQ-001");
    }

    #[test]
    fn report_empty_results() {
        let run_result = make_run_result(vec![]);
        let report = Report::from_run_result(&run_result);
        assert_eq!(report.summary.total, 0);
        assert_eq!(report.summary.passed, 0);
        assert_eq!(report.summary.failed, 0);
        assert!(report.results.is_empty());
    }
}
