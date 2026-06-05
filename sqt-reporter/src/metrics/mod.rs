use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub mod builtin;
pub mod custom;
pub mod process;
pub mod registry;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Platform {
    Local,
    Remote(String),
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Target {
    pub binary: PathBuf,
    pub platform: Platform,
    pub input: Option<PathBuf>,
    pub working_dir: PathBuf,
    pub timeout: Duration,
}

impl Default for Target {
    fn default() -> Self {
        Self {
            binary: PathBuf::new(),
            platform: Platform::Local,
            input: None,
            working_dir: PathBuf::from("."),
            timeout: Duration::from_secs(30),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MetricValue {
    pub value: f64,
    pub unit: String,
    pub evidence: HashMap<String, String>,
}

#[allow(dead_code)]
impl MetricValue {
    pub fn new(value: f64, unit: impl Into<String>) -> Self {
        Self { value, unit: unit.into(), evidence: HashMap::new() }
    }

    #[must_use]
    pub fn with_evidence(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.evidence.insert(key.into(), value.into());
        self
    }
}

#[allow(dead_code)]
pub trait MetricCollector: Send + Sync {
    fn name(&self) -> &'static str;
    /// # Errors
    ///
    /// Returns an error if the underlying tool fails to run or its output cannot be parsed.
    fn collect(&self, target: &Target, config: &serde_yaml::Value) -> anyhow::Result<MetricValue>;
}

#[allow(dead_code)]
pub struct CollectorFactory {
    pub name: &'static str,
    pub create: fn() -> Box<dyn MetricCollector>,
}

inventory::collect!(CollectorFactory);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_default_has_30s_timeout() {
        let target = Target::default();
        assert_eq!(target.timeout, Duration::from_secs(30));
    }

    #[test]
    fn target_default_uses_local_platform() {
        let target = Target::default();
        assert_eq!(target.platform, Platform::Local);
    }

    #[test]
    fn metric_value_new_creates_empty_evidence() {
        let mv = MetricValue::new(42.0, "ms");
        assert!(mv.evidence.is_empty());
        assert_eq!(mv.value, 42.0);
        assert_eq!(mv.unit, "ms");
    }
}
