#![allow(dead_code)]

use super::{Constraint, Op};
use crate::metrics::MetricValue;

#[derive(Debug, Clone)]
pub struct ConstraintOutcome {
    pub passed: bool,
    pub actual: f64,
    pub expected: f64,
    pub message: String,
}

pub fn evaluate(actual: &MetricValue, constraint: &Constraint) -> ConstraintOutcome {
    let actual_val = actual.value;
    let expected_val = constraint.value.to_f64();
    let unit = &actual.unit;

    let passed = match constraint.op {
        Op::Lte => actual_val <= expected_val,
        Op::Gte => actual_val >= expected_val,
        Op::Lt => actual_val < expected_val,
        Op::Gt => actual_val > expected_val,
        Op::Eq => (actual_val - expected_val).abs() < f64::EPSILON,
        Op::Ne => (actual_val - expected_val).abs() >= f64::EPSILON,
    };

    let status = if passed { "PASS" } else { "FAIL" };
    let op = &constraint.op;
    let message = if unit.is_empty() {
        format!("{actual_val} {op} {expected_val}: {status}")
    } else {
        format!("{actual_val} {unit} {op} {expected_val} {unit}: {status}")
    };

    ConstraintOutcome { passed, actual: actual_val, expected: expected_val, message }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::{Constraint, Op, Value};
    use crate::metrics::MetricValue;

    fn mv(value: f64, unit: &str) -> MetricValue {
        MetricValue::new(value, unit)
    }

    fn c(op: Op, val: Value) -> Constraint {
        Constraint::new("test_metric", op, val)
    }

    #[test]
    fn lte_pass_when_less() {
        let outcome = evaluate(&mv(87.3, "ms"), &c(Op::Lte, Value::ms(100.0)));
        assert!(outcome.passed);
    }

    #[test]
    fn lte_pass_when_equal() {
        let outcome = evaluate(&mv(100.0, "ms"), &c(Op::Lte, Value::ms(100.0)));
        assert!(outcome.passed);
    }

    #[test]
    fn lte_fail_when_greater() {
        let outcome = evaluate(&mv(101.0, "ms"), &c(Op::Lte, Value::ms(100.0)));
        assert!(!outcome.passed);
    }

    #[test]
    fn gte_pass_when_greater() {
        let outcome = evaluate(&mv(110.0, "ms"), &c(Op::Gte, Value::ms(100.0)));
        assert!(outcome.passed);
    }

    #[test]
    fn gte_pass_when_equal() {
        let outcome = evaluate(&mv(100.0, "ms"), &c(Op::Gte, Value::ms(100.0)));
        assert!(outcome.passed);
    }

    #[test]
    fn gte_fail_when_less() {
        let outcome = evaluate(&mv(90.0, "ms"), &c(Op::Gte, Value::ms(100.0)));
        assert!(!outcome.passed);
    }

    #[test]
    fn lt_pass_when_less() {
        let outcome = evaluate(&mv(99.0, "ms"), &c(Op::Lt, Value::ms(100.0)));
        assert!(outcome.passed);
    }

    #[test]
    fn lt_fail_when_equal() {
        let outcome = evaluate(&mv(100.0, "ms"), &c(Op::Lt, Value::ms(100.0)));
        assert!(!outcome.passed);
    }

    #[test]
    fn lt_fail_when_greater() {
        let outcome = evaluate(&mv(101.0, "ms"), &c(Op::Lt, Value::ms(100.0)));
        assert!(!outcome.passed);
    }

    #[test]
    fn gt_pass_when_greater() {
        let outcome = evaluate(&mv(101.0, "ms"), &c(Op::Gt, Value::ms(100.0)));
        assert!(outcome.passed);
    }

    #[test]
    fn gt_fail_when_equal() {
        let outcome = evaluate(&mv(100.0, "ms"), &c(Op::Gt, Value::ms(100.0)));
        assert!(!outcome.passed);
    }

    #[test]
    fn gt_fail_when_less() {
        let outcome = evaluate(&mv(99.0, "ms"), &c(Op::Gt, Value::ms(100.0)));
        assert!(!outcome.passed);
    }

    #[test]
    fn eq_pass_when_equal() {
        let outcome = evaluate(&mv(100.0, "ms"), &c(Op::Eq, Value::ms(100.0)));
        assert!(outcome.passed);
    }

    #[test]
    fn eq_fail_when_not_equal() {
        let outcome = evaluate(&mv(99.0, "ms"), &c(Op::Eq, Value::ms(100.0)));
        assert!(!outcome.passed);
    }

    #[test]
    fn ne_pass_when_different() {
        let outcome = evaluate(&mv(99.0, "ms"), &c(Op::Ne, Value::ms(100.0)));
        assert!(outcome.passed);
    }

    #[test]
    fn ne_fail_when_equal() {
        let outcome = evaluate(&mv(100.0, "ms"), &c(Op::Ne, Value::ms(100.0)));
        assert!(!outcome.passed);
    }

    #[test]
    fn message_contains_pass_on_pass() {
        let outcome = evaluate(&mv(87.3, "ms"), &c(Op::Lte, Value::ms(100.0)));
        assert!(outcome.message.contains("PASS"));
    }

    #[test]
    fn message_contains_fail_on_fail() {
        let outcome = evaluate(&mv(101.0, "ms"), &c(Op::Lte, Value::ms(100.0)));
        assert!(outcome.message.contains("FAIL"));
    }

    #[test]
    fn message_contains_unit() {
        let outcome = evaluate(&mv(87.3, "ms"), &c(Op::Lte, Value::ms(100.0)));
        assert!(outcome.message.contains("ms"));
    }

    #[test]
    fn boolean_value_treated_as_1_or_0() {
        let outcome = evaluate(&MetricValue::new(1.0, ""), &c(Op::Eq, Value::bool(true)));
        assert!(outcome.passed);
    }

    #[test]
    fn kb_constraint_test() {
        let outcome = evaluate(&mv(512.0, "KB"), &c(Op::Lte, Value::kb(1024.0)));
        assert!(outcome.passed);
        assert!(outcome.message.contains("KB"));
    }
}
