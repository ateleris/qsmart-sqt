use anyhow::Result;

use crate::metrics::process::run_process;
use crate::metrics::{CollectorFactory, MetricCollector, MetricValue, Target};

pub struct ReturnCodeCollector;

impl MetricCollector for ReturnCodeCollector {
    fn name(&self) -> &'static str {
        "return_code"
    }

    fn collect(&self, target: &Target, _config: &serde_yaml::Value) -> Result<MetricValue> {
        let output = run_process(target, None, &[], "return_code")?;

        let stdout_path =
            output.stdout_path.as_deref().map(|p| p.display().to_string()).unwrap_or_default();
        let stderr_path =
            output.stderr_path.as_deref().map(|p| p.display().to_string()).unwrap_or_default();

        Ok(MetricValue::new(f64::from(output.exit_code), "")
            .with_evidence("command", target.binary.display().to_string())
            .with_evidence("stdout_path", stdout_path)
            .with_evidence("stderr_path", stderr_path))
    }
}

fn create() -> Box<dyn MetricCollector> {
    Box::new(ReturnCodeCollector)
}

inventory::submit! {
    CollectorFactory { name: "return_code", create }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;
    use tempfile::tempdir;

    use crate::metrics::Platform;

    fn target_for(dir: &std::path::Path, binary: &str) -> Target {
        Target {
            binary: PathBuf::from(binary),
            platform: Platform::Local,
            input: None,
            working_dir: dir.to_path_buf(),
            timeout: Duration::from_secs(10),
        }
    }

    #[test]
    fn name_is_return_code() {
        assert_eq!(ReturnCodeCollector.name(), "return_code");
    }

    #[test]
    fn true_returns_zero() {
        let dir = tempdir().unwrap();
        let target = target_for(dir.path(), "/bin/true");
        let mv = ReturnCodeCollector.collect(&target, &serde_yaml::Value::Null).unwrap();
        assert_eq!(mv.value, 0.0);
    }

    #[test]
    fn false_returns_nonzero() {
        let dir = tempdir().unwrap();
        let target = target_for(dir.path(), "/bin/false");
        let mv = ReturnCodeCollector.collect(&target, &serde_yaml::Value::Null).unwrap();
        assert_ne!(mv.value, 0.0);
    }

    #[test]
    fn evidence_contains_command() {
        let dir = tempdir().unwrap();
        let target = target_for(dir.path(), "/bin/true");
        let mv = ReturnCodeCollector.collect(&target, &serde_yaml::Value::Null).unwrap();
        assert!(mv.evidence.contains_key("command"));
        assert!(mv.evidence["command"].contains("true"));
    }
}
