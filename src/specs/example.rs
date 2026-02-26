use crate::constraint::{Constraint, Op, Value};
use crate::metrics::Target;
use crate::specs::TestSpec;

pub fn spec() -> TestSpec {
    TestSpec {
        requirement_uid: "REQ-EXAMPLE-001".into(),
        target: Target { binary: "build/example_binary".into(), ..Default::default() },
        constraints: vec![
            Constraint::new("return_code", Op::Eq, Value::int(0)),
            Constraint::new("time.execution", Op::Lte, Value::ms(100.0)),
        ],
    }
}
