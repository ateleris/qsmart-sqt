#![allow(dead_code)]

use std::fmt;

pub use evaluator::ConstraintOutcome;
pub(crate) mod evaluator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Lte,
    Gte,
    Lt,
    Gt,
    Eq,
    Ne,
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Op::Lte => write!(f, "<="),
            Op::Gte => write!(f, ">="),
            Op::Lt => write!(f, "<"),
            Op::Gt => write!(f, ">"),
            Op::Eq => write!(f, "=="),
            Op::Ne => write!(f, "!="),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Milliseconds(f64),
    Kilobytes(f64),
    Percent(f64),
    Integer(i64),
    Boolean(bool),
}

impl Value {
    pub fn ms(v: f64) -> Self {
        Value::Milliseconds(v)
    }
    pub fn kb(v: f64) -> Self {
        Value::Kilobytes(v)
    }
    pub fn pct(v: f64) -> Self {
        Value::Percent(v)
    }
    pub fn int(v: i64) -> Self {
        Value::Integer(v)
    }
    pub fn bool(b: bool) -> Self {
        Value::Boolean(b)
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn to_f64(&self) -> f64 {
        match self {
            Value::Milliseconds(v) | Value::Kilobytes(v) | Value::Percent(v) => *v,
            Value::Integer(v) => *v as f64,
            Value::Boolean(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    pub fn unit(&self) -> &str {
        match self {
            Value::Milliseconds(_) => "ms",
            Value::Kilobytes(_) => "KB",
            Value::Percent(_) => "%",
            Value::Integer(_) | Value::Boolean(_) => "",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Constraint {
    pub metric_name: String,
    pub op: Op,
    pub value: Value,
    pub config: serde_yaml::Value,
}

impl Constraint {
    pub fn new(name: impl Into<String>, op: Op, value: Value) -> Self {
        Self { metric_name: name.into(), op, value, config: serde_yaml::Value::Null }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_ms_roundtrip() {
        let v = Value::ms(100.0);
        assert_eq!(v.to_f64(), 100.0);
        assert_eq!(v.unit(), "ms");
    }

    #[test]
    fn value_kb_roundtrip() {
        let v = Value::kb(1024.0);
        assert_eq!(v.to_f64(), 1024.0);
        assert_eq!(v.unit(), "KB");
    }

    #[test]
    fn value_pct_roundtrip() {
        let v = Value::pct(85.5);
        assert_eq!(v.to_f64(), 85.5);
        assert_eq!(v.unit(), "%");
    }

    #[test]
    fn value_int_roundtrip() {
        let v = Value::int(42);
        assert_eq!(v.to_f64(), 42.0);
        assert_eq!(v.unit(), "");
    }

    #[test]
    fn value_bool_roundtrip() {
        let v_true = Value::bool(true);
        let v_false = Value::bool(false);
        assert_eq!(v_true.to_f64(), 1.0);
        assert_eq!(v_false.to_f64(), 0.0);
        assert_eq!(v_true.unit(), "");
    }

    #[test]
    fn op_display() {
        assert_eq!(Op::Lte.to_string(), "<=");
        assert_eq!(Op::Gte.to_string(), ">=");
        assert_eq!(Op::Lt.to_string(), "<");
        assert_eq!(Op::Gt.to_string(), ">");
        assert_eq!(Op::Eq.to_string(), "==");
        assert_eq!(Op::Ne.to_string(), "!=");
    }

    #[test]
    fn constraint_new_defaults_config_to_null() {
        let c = Constraint::new("cpu", Op::Lte, Value::pct(80.0));
        assert_eq!(c.config, serde_yaml::Value::Null);
    }
}
