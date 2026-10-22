use std::collections::HashMap;

use super::{Constraint, Op};

#[derive(Debug, Clone)]
pub struct ConstraintOutcome {
    pub passed: bool,
    pub actual: f64,
    pub expected: f64,
    pub message: String,
    pub evidence: HashMap<String, String>,
}

/// Evaluate a constraint against an actual metric value.
///
/// The metric name and unit are derived from the constraint variant itself,
/// so the caller only needs to supply the raw `f64` from the API response.
pub fn evaluate(actual: f64, constraint: &Constraint) -> ConstraintOutcome {
    let expected = constraint.expected();
    let op = constraint.op();
    let unit = constraint.unit();

    let passed = match op {
        Op::Lte => actual <= expected,
        Op::Gte => actual >= expected,
        Op::Lt => actual < expected,
        Op::Gt => actual > expected,
        Op::Eq => (actual - expected).abs() < f64::EPSILON,
        Op::Ne => (actual - expected).abs() >= f64::EPSILON,
    };

    let status = if passed { "PASS" } else { "FAIL" };
    let message = if unit.is_empty() {
        format!("{actual} {op} {expected}: {status}")
    } else {
        format!("{actual} {unit} {op} {expected} {unit}: {status}")
    };

    ConstraintOutcome { passed, actual, expected, message, evidence: HashMap::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::{Constraint, Op};

    /// Verifies: SRS-FUNC-004
    #[test]
    fn lte_pass_when_less() {
        let outcome = evaluate(87.3, &Constraint::execution_time(Op::Lte, 100.0));
        assert!(outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn lte_pass_when_equal() {
        let outcome = evaluate(100.0, &Constraint::execution_time(Op::Lte, 100.0));
        assert!(outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn lte_fail_when_greater() {
        let outcome = evaluate(101.0, &Constraint::execution_time(Op::Lte, 100.0));
        assert!(!outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn gte_pass_when_greater() {
        let outcome = evaluate(110.0, &Constraint::execution_time(Op::Gte, 100.0));
        assert!(outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn gte_pass_when_equal() {
        let outcome = evaluate(100.0, &Constraint::execution_time(Op::Gte, 100.0));
        assert!(outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn gte_fail_when_less() {
        let outcome = evaluate(90.0, &Constraint::execution_time(Op::Gte, 100.0));
        assert!(!outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn lt_pass_when_less() {
        let outcome = evaluate(99.0, &Constraint::execution_time(Op::Lt, 100.0));
        assert!(outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn lt_fail_when_equal() {
        let outcome = evaluate(100.0, &Constraint::execution_time(Op::Lt, 100.0));
        assert!(!outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn lt_fail_when_greater() {
        let outcome = evaluate(101.0, &Constraint::execution_time(Op::Lt, 100.0));
        assert!(!outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn gt_pass_when_greater() {
        let outcome = evaluate(101.0, &Constraint::execution_time(Op::Gt, 100.0));
        assert!(outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn gt_fail_when_equal() {
        let outcome = evaluate(100.0, &Constraint::execution_time(Op::Gt, 100.0));
        assert!(!outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn gt_fail_when_less() {
        let outcome = evaluate(99.0, &Constraint::execution_time(Op::Gt, 100.0));
        assert!(!outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn eq_pass_when_equal() {
        let outcome = evaluate(0.0, &Constraint::return_code(Op::Eq, 0));
        assert!(outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn eq_fail_when_not_equal() {
        let outcome = evaluate(1.0, &Constraint::return_code(Op::Eq, 0));
        assert!(!outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn ne_pass_when_different() {
        let outcome = evaluate(1.0, &Constraint::return_code(Op::Ne, 0));
        assert!(outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn ne_fail_when_equal() {
        let outcome = evaluate(0.0, &Constraint::return_code(Op::Ne, 0));
        assert!(!outcome.passed);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn message_contains_pass_on_pass() {
        let outcome = evaluate(87.3, &Constraint::execution_time(Op::Lte, 100.0));
        assert!(outcome.message.contains("PASS"));
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn message_contains_fail_on_fail() {
        let outcome = evaluate(101.0, &Constraint::execution_time(Op::Lte, 100.0));
        assert!(outcome.message.contains("FAIL"));
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn message_contains_unit() {
        let outcome = evaluate(87.3, &Constraint::execution_time(Op::Lte, 100.0));
        assert!(outcome.message.contains("ms"));
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn unitless_message_has_no_unit() {
        let outcome = evaluate(0.0, &Constraint::return_code(Op::Eq, 0));
        assert!(!outcome.message.contains("ms"));
        assert!(outcome.message.contains("0 == 0: PASS"));
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn kb_constraint_message() {
        let outcome = evaluate(512.0, &Constraint::memory_peak_rss(Op::Lte, 1024.0));
        assert!(outcome.passed);
        assert!(outcome.message.contains("KB"));
    }
}
