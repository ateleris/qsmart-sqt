use anyhow::{Result, bail};

use crate::metrics::process::{check_tool, run_process};
use crate::metrics::{CollectorFactory, MetricCollector, MetricValue, Target};

pub struct CoverageLineCollector;

impl MetricCollector for CoverageLineCollector {
    fn name(&self) -> &'static str {
        "coverage.line_pct"
    }

    fn collect(&self, target: &Target, config: &serde_yaml::Value) -> Result<MetricValue> {
        let info_file = config["info_file"].as_str().ok_or_else(|| {
            anyhow::anyhow!("'info_file' path not specified in constraint config")
        })?;

        check_tool("lcov")?;

        // lcov --summary writes its output to stderr; capture both.
        let output = run_process(target, Some("lcov"), &["--summary", info_file], "coverage_line")?;

        let combined = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let percent = parse_line_coverage(&combined)?;
        Ok(MetricValue::new(percent, "%"))
    }
}

#[allow(dead_code)]
fn parse_line_coverage(output: &str) -> Result<f64> {
    for line in output.lines() {
        if line.trim_start().starts_with("lines......:")
            && let Some((_, after_colon)) = line.split_once(':')
        {
            let pct_str = after_colon.trim().split('%').next().unwrap_or("").trim();
            let pct: f64 = pct_str.parse()?;
            return Ok(pct);
        }
    }
    bail!("could not find line coverage percentage in lcov output")
}

fn create() -> Box<dyn MetricCollector> {
    Box::new(CoverageLineCollector)
}

inventory::submit! {
    CollectorFactory { name: "coverage.line_pct", create }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_coverage_line_pct() {
        assert_eq!(CoverageLineCollector.name(), "coverage.line_pct");
    }

    #[test]
    fn parse_line_coverage_valid() {
        let output = "Summary coverage rate:\n  lines......: 85.5% (171 of 200 lines)\n";
        assert_eq!(parse_line_coverage(output).unwrap(), 85.5);
    }

    #[test]
    fn parse_line_coverage_100_percent() {
        let output = "  lines......: 100.0% (200 of 200 lines)\n";
        assert_eq!(parse_line_coverage(output).unwrap(), 100.0);
    }

    #[test]
    fn parse_line_coverage_no_match() {
        let output = "Reading tracefile ...\nNo summary available.\n";
        assert!(parse_line_coverage(output).is_err());
    }

    #[test]
    fn missing_info_file_config_returns_error() {
        use crate::metrics::Platform;
        use std::path::PathBuf;
        use std::time::Duration;
        let target = crate::metrics::Target {
            binary: PathBuf::from("/bin/true"),
            platform: Platform::Local,
            input: None,
            working_dir: PathBuf::from("."),
            timeout: Duration::from_secs(10),
        };
        let result = CoverageLineCollector.collect(&target, &serde_yaml::Value::Null);
        assert!(result.is_err());
    }
}
