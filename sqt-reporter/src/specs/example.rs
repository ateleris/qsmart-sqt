use sqt_types::WorkloadType;
use crate::constraint::{Constraint, Op};
use crate::metrics::Target;
use crate::specs::{SpecFactory, TestSpec};

pub fn spec() -> TestSpec {
    TestSpec {
        requirement_uid: "REQ-EXAMPLE-001".into(),
        workload_type: WorkloadType::ImageClassification,
        target: Target { binary: "build/example_binary".into(), ..Default::default() },
        constraints: vec![
            Constraint::return_code(Op::Eq, 0),
            Constraint::execution_time(Op::Lte, 500.0),
            Constraint::memory_peak_rss(Op::Lte, 5000.0),
            Constraint::cpu_avg_normalized_pct(Op::Lte, 50.0),
        ],
    }
}

inventory::submit! {
    SpecFactory { create: spec }
}
