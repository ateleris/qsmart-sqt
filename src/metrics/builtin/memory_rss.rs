use anyhow::{Result, bail};

use crate::metrics::process::{check_tool, run_process};
use crate::metrics::{CollectorFactory, MetricCollector, MetricValue, Target};

pub struct MemoryPeakRssCollector;

impl MetricCollector for MemoryPeakRssCollector {
    fn name(&self) -> &'static str {
        "memory.peak_rss_kb"
    }

    fn collect(&self, target: &Target, _config: &serde_yaml::Value) -> Result<MetricValue> {
        check_tool("/usr/bin/time")?;
        let output = run_process(target, Some("/usr/bin/time"), &["-v"], "memory_peak_rss")?;
        let stderr = std::str::from_utf8(&output.stderr)?;
        let kb = parse_max_rss(stderr)?;
        Ok(MetricValue::new(kb, "KB"))
    }
}

#[allow(dead_code)]
fn parse_max_rss(stderr: &str) -> Result<f64> {
    for line in stderr.lines() {
        if line.contains("Maximum resident set size")
            && let Some((_, val_str)) = line.split_once(':')
        {
            let kb: f64 = val_str.trim().parse()?;
            return Ok(kb);
        }
    }
    bail!("could not find 'Maximum resident set size' in /usr/bin/time output")
}

fn create() -> Box<dyn MetricCollector> {
    Box::new(MemoryPeakRssCollector)
}

inventory::submit! {
    CollectorFactory { name: "memory.peak_rss_kb", create }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_memory_peak_rss_kb() {
        assert_eq!(MemoryPeakRssCollector.name(), "memory.peak_rss_kb");
    }

    #[test]
    fn parse_max_rss_valid_output() {
        let stderr = "\
\tCommand being timed: \"/bin/true\"\n\
\tUser time (seconds): 0.00\n\
\tMaximum resident set size (kbytes): 1536\n\
\tExit status: 0\n";
        assert_eq!(parse_max_rss(stderr).unwrap(), 1536.0);
    }

    #[test]
    fn parse_max_rss_missing_line() {
        let stderr = "some other output without the expected line\n";
        assert!(parse_max_rss(stderr).is_err());
    }

    #[test]
    fn parse_max_rss_invalid_number() {
        let stderr = "\tMaximum resident set size (kbytes): not_a_number\n";
        assert!(parse_max_rss(stderr).is_err());
    }
}
