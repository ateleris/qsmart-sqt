// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

mod example;
mod example2;
pub mod loader;

use std::collections::HashMap;

use crate::constraint::{Constraint, ConstraintOutcome};
use crate::metrics::Target;
use sqt_types::WorkloadType;

#[derive(Debug, Clone, Default)]
pub struct TestSpec {
    pub requirement_uid: String,
    pub workload_type: WorkloadType,
    pub target: Target,
    pub constraints: Vec<Constraint>,
    /// Constraints whose value unit does not reduce to the metric's unit. Each
    /// is reported as a failing outcome (SRS-FUNC-004).
    pub unreconcilable: Vec<UnreconcilableConstraint>,
    /// Reference outputs for client-side ml.* accuracy evaluation (T-054).
    pub reference_outputs: Option<Vec<serde_json::Value>>,
    /// Reference outputs for numerical precision comparison (SRS-FUNC-016).
    pub precision_reference: Option<Vec<serde_json::Value>>,
    /// Absolute tolerance for `ml.output_within_tolerance` (SRS-FUNC-016).
    pub precision_tolerance: Option<f64>,
}

/// A constraint that was declared but cannot be evaluated because its value's
/// unit does not reduce to the unit of its metric.
#[derive(Debug, Clone)]
pub struct UnreconcilableConstraint {
    pub metric: String,
    /// Display form of the operator, e.g. `<=`.
    pub operator: String,
    pub reason: String,
}

/// A factory that produces a `TestSpec`.
///
/// Spec modules register themselves via `inventory::submit!` so that
/// `all_specs()` discovers them automatically - no manual wiring needed.
pub struct SpecFactory {
    pub create: fn() -> TestSpec,
}

inventory::collect!(SpecFactory);

/// Three-state outcome for a spec: pass, fail, or infrastructure error.
///
/// `InfraError` means the Runner failed to execute the job (network failure,
/// internal server error, job timeout on the runner side). It is distinct from
/// a constraint `Fail`, which means the binary-under-test did not meet its
/// performance requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutcomeKind {
    Pass,
    Fail,
    InfraError,
}

#[derive(Debug, Clone)]
pub struct SpecResult {
    pub spec: TestSpec,
    pub outcomes: Vec<ConstraintOutcome>,
    pub outcome: OutcomeKind,
    /// Backward-compat helper: `true` only for `OutcomeKind::Pass`.
    pub passed: bool,
    /// Ids of the evidence artefacts behind this result (SRS-FUNC-009): the
    /// Runner's final response for the job, then the persisted outcomes.
    pub evidence: Vec<String>,
}

impl SpecResult {
    /// Construct a `SpecResult` from evaluated constraint outcomes.
    ///
    /// An empty `outcomes` slice always produces `OutcomeKind::Fail` (vacuous
    /// pass prevention).  All outcomes passing → `Pass`.  Any failure → `Fail`.
    pub fn new(spec: TestSpec, outcomes: Vec<ConstraintOutcome>) -> Self {
        let kind = if !outcomes.is_empty() && outcomes.iter().all(|o| o.passed) {
            OutcomeKind::Pass
        } else {
            OutcomeKind::Fail
        };
        let passed = kind == OutcomeKind::Pass;
        Self { spec, outcomes, outcome: kind, passed, evidence: Vec::new() }
    }

    /// Construct a synthetic `SpecResult` representing a Runner infrastructure
    /// error (e.g. network failure, job timeout, internal server error).
    ///
    /// The single outcome's `evidence` map carries the structured error fields
    /// from the Runner's `ErrorResponse` so they surface in both the terminal
    /// summary and the JSON report.
    pub fn infra_error(spec: TestSpec, message: String, evidence: HashMap<String, String>) -> Self {
        let outcome = ConstraintOutcome {
            passed: false,
            actual: f64::NAN,
            expected: f64::NAN,
            message,
            evidence,
        };
        Self {
            spec,
            outcomes: vec![outcome],
            outcome: OutcomeKind::InfraError,
            passed: false,
            evidence: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub results: Vec<SpecResult>,
    pub timestamp: String,
    pub config_version: String,
    /// `true` if the run was cut short by SIGTERM or Ctrl-C.
    pub interrupted: bool,
    /// Baseline requirement set parsed during this run, if available.
    pub requirement_set: Option<crate::baseline::RequirementSet>,
    /// What the Runner's discovery operation returned (SRS-FUNC-018).
    pub discovery: Option<crate::baseline::DiscoveryRecord>,
}

/// Collect all registered specs via inventory.
pub fn all_specs() -> Vec<TestSpec> {
    inventory::iter::<SpecFactory>.into_iter().map(|f| (f.create)()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::{Constraint, ConstraintOutcome, Op};

    fn passing_outcome() -> ConstraintOutcome {
        ConstraintOutcome {
            passed: true,
            actual: 87.3,
            expected: 100.0,
            message: "87.3 ms <= 100 ms: PASS".to_string(),
            evidence: std::collections::HashMap::new(),
        }
    }

    fn failing_outcome() -> ConstraintOutcome {
        ConstraintOutcome {
            passed: false,
            actual: 110.0,
            expected: 100.0,
            message: "110 ms <= 100 ms: FAIL".to_string(),
            evidence: std::collections::HashMap::new(),
        }
    }

    fn make_spec() -> TestSpec {
        TestSpec {
            requirement_uid: "REQ-001".to_string(),
            constraints: vec![Constraint::cpu_avg_normalized_pct(Op::Lte, 80.0)],
            ..Default::default()
        }
    }

    /// Verifies: SRS-REL-001
    #[test]
    fn spec_result_fails_if_no_outcomes() {
        let result = SpecResult::new(make_spec(), vec![]);
        assert!(!result.passed);
    }

    /// Verifies: SRS-FUNC-003
    #[test]
    fn all_specs_returns_nonempty_vec() {
        assert!(!all_specs().is_empty());
    }

    /// Verifies: SRS-FUNC-003
    #[test]
    fn all_specs_discovers_both_examples() {
        let specs = all_specs();
        let uids: Vec<&str> = specs.iter().map(|s| s.requirement_uid.as_str()).collect();
        assert!(uids.contains(&"REQ-EXAMPLE-001"), "missing REQ-EXAMPLE-001");
        assert!(uids.contains(&"REQ-EXAMPLE-002"), "missing REQ-EXAMPLE-002");
    }

    /// Verifies: SRS-FUNC-007
    #[test]
    fn spec_result_derives_passed_from_outcomes() {
        let result = SpecResult::new(make_spec(), vec![passing_outcome(), passing_outcome()]);
        assert!(result.passed);
        assert_eq!(result.outcome, OutcomeKind::Pass);
    }

    /// Verifies: SRS-FUNC-007
    #[test]
    fn spec_result_fails_if_any_outcome_fails() {
        let result = SpecResult::new(make_spec(), vec![passing_outcome(), failing_outcome()]);
        assert!(!result.passed);
        assert_eq!(result.outcome, OutcomeKind::Fail);
    }

    /// Verifies: SRS-OPS-006
    #[test]
    fn spec_result_infra_error_is_not_passed() {
        let result = SpecResult::infra_error(
            make_spec(),
            "runner crashed".to_string(),
            std::collections::HashMap::new(),
        );
        assert!(!result.passed);
        assert_eq!(result.outcome, OutcomeKind::InfraError);
        assert_eq!(result.outcomes.len(), 1);
        assert!(result.outcomes[0].message.contains("runner crashed"));
    }

    /// Verifies: SRS-OPS-006, SRS-FUNC-008
    #[test]
    fn spec_result_infra_error_carries_evidence() {
        let mut evidence = std::collections::HashMap::new();
        evidence.insert("error_code".to_string(), "RUNNER_TIMEOUT".to_string());
        evidence.insert("error_message".to_string(), "job timed out".to_string());
        let result = SpecResult::infra_error(make_spec(), "job timed out".to_string(), evidence);
        assert_eq!(result.outcome, OutcomeKind::InfraError);
        assert_eq!(
            result.outcomes[0].evidence.get("error_code").map(String::as_str),
            Some("RUNNER_TIMEOUT")
        );
    }

    /// Verifies: SRS-FUNC-003
    #[test]
    fn test_spec_with_default_target() {
        use std::time::Duration;
        let spec = TestSpec { requirement_uid: "REQ-TEST".to_string(), ..Default::default() };
        assert_eq!(spec.requirement_uid, "REQ-TEST");
        assert_eq!(spec.target.timeout, Duration::from_secs(30));
    }

    /// Verifies: SRS-FUNC-019
    #[test]
    fn discovered_specs_carry_image_classification_workload_type() {
        // Both example specs must explicitly declare ImageClassification.
        // If workload_type were left defaulted they'd silently use the default
        // value; this test pins the explicit choice so a future change is noticed.
        use sqt_types::WorkloadType;
        let specs = all_specs();
        let uid_001 = specs.iter().find(|s| s.requirement_uid == "REQ-EXAMPLE-001");
        let uid_002 = specs.iter().find(|s| s.requirement_uid == "REQ-EXAMPLE-002");
        assert!(uid_001.is_some(), "REQ-EXAMPLE-001 must be registered");
        assert!(uid_002.is_some(), "REQ-EXAMPLE-002 must be registered");
        assert_eq!(uid_001.unwrap().workload_type, WorkloadType::ImageClassification);
        assert_eq!(uid_002.unwrap().workload_type, WorkloadType::ImageClassification);
    }
}
