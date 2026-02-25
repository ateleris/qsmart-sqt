use anyhow::Result;

use crate::metrics::process::{check_tool, run_process};
use crate::metrics::{CollectorFactory, MetricCollector, MetricValue, Target};

pub struct MemoryLeaksCollector;

impl MetricCollector for MemoryLeaksCollector {
    fn name(&self) -> &'static str {
        "memory.leaks"
    }

    fn collect(&self, target: &Target, _config: &serde_yaml::Value) -> Result<MetricValue> {
        check_tool("valgrind")?;
        let xml_path = target.working_dir.join("valgrind_output.xml");
        let xml_arg = format!("--xml-file={}", xml_path.display());
        let _output = run_process(
            target,
            Some("valgrind"),
            &["--tool=memcheck", "--leak-check=full", "--xml=yes", &xml_arg],
            "memory_leaks",
        )?;
        let xml = std::fs::read_to_string(&xml_path)?;
        let total_bytes = parse_valgrind_leaks(&xml)?;
        Ok(MetricValue::new(total_bytes, "bytes"))
    }
}

#[allow(dead_code)]
fn parse_valgrind_leaks(xml: &str) -> Result<f64> {
    let mut total = 0.0_f64;
    for line in xml.lines() {
        let trimmed = line.trim();
        if let Some(inner) =
            trimmed.strip_prefix("<leakedbytes>").and_then(|s| s.strip_suffix("</leakedbytes>"))
        {
            let bytes: f64 = inner.parse()?;
            total += bytes;
        }
    }
    Ok(total)
}

fn create() -> Box<dyn MetricCollector> {
    Box::new(MemoryLeaksCollector)
}

inventory::submit! {
    CollectorFactory { name: "memory.leaks", create }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALGRIND_XML_WITH_LEAKS: &str = r#"<?xml version="1.0"?>
<valgrindoutput>
  <error>
    <kind>LeakDefinitelyLost</kind>
    <leakedbytes>40</leakedbytes>
    <leakedblocks>1</leakedblocks>
  </error>
  <error>
    <kind>LeakIndirectlyLost</kind>
    <leakedbytes>24</leakedbytes>
    <leakedblocks>1</leakedblocks>
  </error>
</valgrindoutput>"#;

    const VALGRIND_XML_NO_LEAKS: &str = r#"<?xml version="1.0"?>
<valgrindoutput>
  <status><state>FINISHED</state></status>
</valgrindoutput>"#;

    #[test]
    fn name_is_memory_leaks() {
        assert_eq!(MemoryLeaksCollector.name(), "memory.leaks");
    }

    #[test]
    fn parse_valgrind_leaks_with_leaks() {
        let total = parse_valgrind_leaks(VALGRIND_XML_WITH_LEAKS).unwrap();
        assert_eq!(total, 64.0); // 40 + 24
    }

    #[test]
    fn parse_valgrind_leaks_no_leaks() {
        let total = parse_valgrind_leaks(VALGRIND_XML_NO_LEAKS).unwrap();
        assert_eq!(total, 0.0);
    }

    #[test]
    fn parse_valgrind_leaks_empty() {
        let total = parse_valgrind_leaks("").unwrap();
        assert_eq!(total, 0.0);
    }
}
