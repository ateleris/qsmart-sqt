use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::config::BinaryConfig;
use crate::docker::{self, DockerRun};

/// Run valgrind memcheck on the binary and return the total number of
/// leaked bytes.
///
/// Respects `BinaryConfig.args` so the binary is invoked the same way as
/// in a normal run.
pub fn collect(
    image: &str,
    binary_config: &BinaryConfig,
    workspace: &Path,
    timeout: Duration,
) -> Result<f64> {
    let binary = &binary_config.path;
    let xml_path = workspace.join("valgrind.xml");

    let binary_path = Path::new(binary);
    let binary_dir = binary_path.parent().unwrap_or(Path::new("/"));

    let mut command = vec![
        "valgrind".to_string(),
        "--tool=memcheck".to_string(),
        "--xml=yes".to_string(),
        format!("--xml-file={}", xml_path.display()),
        binary.clone(),
    ];
    // Append configured binary arguments
    command.extend(binary_config.args.iter().cloned());

    // Collect environment variables
    let env: Vec<(String, String)> = binary_config
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let spec = DockerRun {
        image: image.to_string(),
        command,
        volumes: vec![
            docker::identity_mount(binary_dir),
            docker::identity_mount(workspace),
        ],
        working_dir: Some(binary_dir.display().to_string()),
        timeout,
        env,
    };

    let _output = docker::run_container(&spec).context("valgrind: docker execution failed")?;

    // Parse XML output for leaked bytes
    let xml = std::fs::read_to_string(&xml_path)
        .context("valgrind: failed to read XML output")?;

    let leaks = parse_leaked_bytes(&xml);
    Ok(leaks)
}

/// Parse total leaked bytes from valgrind XML output.
///
/// Looks for `<leakedbytes>N</leakedbytes>` or sums `<loss_record>` entries.
/// Falls back to 0 if no leaks section found (meaning no leaks).
fn parse_leaked_bytes(xml: &str) -> f64 {
    // Simple approach: sum all <leakedbytes>N</leakedbytes> values
    let mut total: f64 = 0.0;
    for segment in xml.split("<leakedbytes>").skip(1) {
        if let Some(end) = segment.find("</leakedbytes>")
            && let Ok(val) = segment[..end].trim().parse::<f64>()
        {
            total += val;
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_leaked_bytes_sums_entries() {
        let xml = r#"<?xml version="1.0"?>
<valgrindoutput>
  <error><leakedbytes>100</leakedbytes></error>
  <error><leakedbytes>200</leakedbytes></error>
</valgrindoutput>"#;
        assert_eq!(parse_leaked_bytes(xml), 300.0);
    }

    #[test]
    fn parse_leaked_bytes_no_leaks() {
        let xml = r#"<?xml version="1.0"?>
<valgrindoutput></valgrindoutput>"#;
        assert_eq!(parse_leaked_bytes(xml), 0.0);
    }

    #[test]
    fn parse_leaked_bytes_single_entry() {
        let xml = "<error><leakedbytes>512</leakedbytes></error>";
        assert_eq!(parse_leaked_bytes(xml), 512.0);
    }

    #[test]
    fn parse_leaked_bytes_ignores_malformed_values() {
        let xml = "<leakedbytes>not-a-number</leakedbytes><leakedbytes>100</leakedbytes>";
        assert_eq!(parse_leaked_bytes(xml), 100.0);
    }

    #[test]
    fn parse_leaked_bytes_empty_string() {
        assert_eq!(parse_leaked_bytes(""), 0.0);
    }
}
