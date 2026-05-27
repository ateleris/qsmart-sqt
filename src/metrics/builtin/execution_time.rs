use anyhow::Result;

use crate::metrics::process::run_process;
use crate::metrics::{CollectorFactory, MetricCollector, MetricValue, Target};

pub struct ExecutionTimeCollector;

impl MetricCollector for ExecutionTimeCollector {
    fn name(&self) -> &'static str {
        "time.avg_ms"
    }

    fn collect(&self, target: &Target, _config: &serde_yaml::Value) -> Result<MetricValue> {
        let output = run_process(target, None, &[], "time_execution")?;
        let ms = output.wall_time.as_secs_f64() * 1000.0;

        Ok(MetricValue::new(ms, "ms").with_evidence("duration_ms", ms.to_string()))
    }
}

fn create() -> Box<dyn MetricCollector> {
    Box::new(ExecutionTimeCollector)
}

inventory::submit! {
    CollectorFactory { name: "time.avg_ms", create }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;
    use tempfile::tempdir;

    use crate::metrics::Platform;

    fn target_true(dir: &std::path::Path) -> Target {
        Target {
            binary: PathBuf::from("/bin/true"),
            platform: Platform::Local,
            input: None,
            working_dir: dir.to_path_buf(),
            timeout: Duration::from_secs(10),
        }
    }

    #[test]
    fn name_is_time_avg_ms() {
        assert_eq!(ExecutionTimeCollector.name(), "time.avg_ms");
    }

    #[test]
    fn true_returns_positive_ms() {
        let dir = tempdir().unwrap();
        let target = target_true(dir.path());
        let mv = ExecutionTimeCollector.collect(&target, &serde_yaml::Value::Null).unwrap();
        assert!(mv.value >= 0.0);
        assert_eq!(mv.unit, "ms");
    }

    #[test]
    fn evidence_contains_duration() {
        let dir = tempdir().unwrap();
        let target = target_true(dir.path());
        let mv = ExecutionTimeCollector.collect(&target, &serde_yaml::Value::Null).unwrap();
        assert!(mv.evidence.contains_key("duration_ms"));
    }
}
