#![allow(dead_code)]

use crate::constraint::{Constraint, ConstraintOutcome};
use crate::metrics::Target;

#[derive(Debug, Clone, Default)]
pub struct TestSpec {
    pub requirement_uid: String,
    pub target: Target,
    pub constraints: Vec<Constraint>,
}

#[derive(Debug, Clone)]
pub struct SpecResult {
    pub spec: TestSpec,
    pub outcomes: Vec<ConstraintOutcome>,
    pub passed: bool,
}

impl SpecResult {
    pub fn new(spec: TestSpec, outcomes: Vec<ConstraintOutcome>) -> Self {
        let passed = outcomes.iter().all(|o| o.passed);
        Self { spec, outcomes, passed }
    }
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub results: Vec<SpecResult>,
    pub timestamp: String,
    pub config_version: String,
}

pub fn all_specs() -> Vec<TestSpec> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::{Constraint, ConstraintOutcome, Op, Value};

    fn passing_outcome() -> ConstraintOutcome {
        ConstraintOutcome {
            passed: true,
            actual: 87.3,
            expected: 100.0,
            message: "87.3 ms <= 100 ms: PASS".to_string(),
        }
    }

    fn failing_outcome() -> ConstraintOutcome {
        ConstraintOutcome {
            passed: false,
            actual: 110.0,
            expected: 100.0,
            message: "110 ms <= 100 ms: FAIL".to_string(),
        }
    }

    fn make_spec() -> TestSpec {
        TestSpec {
            requirement_uid: "REQ-001".to_string(),
            constraints: vec![Constraint::new("cpu", Op::Lte, Value::pct(80.0))],
            ..Default::default()
        }
    }

    #[test]
    fn all_specs_returns_empty_vec() {
        assert!(all_specs().is_empty());
    }

    #[test]
    fn spec_result_derives_passed_from_outcomes() {
        let result = SpecResult::new(make_spec(), vec![passing_outcome(), passing_outcome()]);
        assert!(result.passed);
    }

    #[test]
    fn spec_result_fails_if_any_outcome_fails() {
        let result = SpecResult::new(make_spec(), vec![passing_outcome(), failing_outcome()]);
        assert!(!result.passed);
    }

    #[test]
    fn test_spec_with_default_target() {
        use std::time::Duration;
        let spec = TestSpec { requirement_uid: "REQ-TEST".to_string(), ..Default::default() };
        assert_eq!(spec.requirement_uid, "REQ-TEST");
        assert_eq!(spec.target.timeout, Duration::from_secs(30));
    }
}
