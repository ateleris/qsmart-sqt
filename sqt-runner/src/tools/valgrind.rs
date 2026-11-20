// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! `memory.leaks_bytes`: bytes leaked according to Valgrind memcheck.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::config::BinaryConfig;
use crate::docker::{self, DockerRun};

/// Run valgrind memcheck on the binary and return the total number of
/// leaked bytes.
///
/// Respects `BinaryConfig.args` so the binary is invoked the same way as
/// in a normal run.
///
/// # Errors
///
/// Returns an error, so the metric is reported as `null` instead of `0`
/// (backlog F-M1, SRS-REL-002), when the container cannot run valgrind or when
/// its XML report is missing or does not show a finished run.
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
        // Leak records appear in the XML report only with a full leak check.
        "--leak-check=full".to_string(),
        "--xml=yes".to_string(),
        format!("--xml-file={}", xml_path.display()),
        binary.clone(),
    ];
    // Append configured binary arguments
    command.extend(binary_config.args.iter().cloned());

    // Collect environment variables
    let env: Vec<(String, String)> =
        binary_config.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

    let spec = DockerRun {
        image: image.to_string(),
        command,
        volumes: vec![docker::identity_mount(binary_dir), docker::identity_mount(workspace)],
        working_dir: Some(binary_dir.display().to_string()),
        timeout,
        env,
    };

    let output = docker::run_container(&spec).context("valgrind: docker execution failed")?;
    // Valgrind passes on the exit code of the binary, which may be anything.
    // 125 to 127 come from Docker or the shell: valgrind itself did not start.
    if (125..=127).contains(&output.exit_code) {
        bail!("valgrind: the container could not run valgrind (exit code {})", output.exit_code);
    }

    let xml = std::fs::read_to_string(&xml_path).context("valgrind: failed to read XML output")?;
    leaked_bytes_of_finished_run(&xml)
}

/// Total leaked bytes of a valgrind XML report, if the report shows a run
/// that finished. A report without a finished run says nothing about leaks,
/// so it is an error rather than zero (backlog F-M1).
fn leaked_bytes_of_finished_run(xml: &str) -> Result<f64> {
    if !xml.contains("<valgrindoutput") {
        bail!("valgrind: the XML output is not a valgrind report");
    }
    if !xml.contains("<state>FINISHED</state>") {
        bail!("valgrind: the report shows no finished run of the binary");
    }
    Ok(parse_leaked_bytes(xml))
}

/// Sum of all `<leakedbytes>N</leakedbytes>` values in a valgrind XML report;
/// zero when it lists no leak.
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
#[expect(
    clippy::float_cmp,
    reason = "values are parsed or converted exactly; tests assert exact results"
)]
mod tests {
    use super::*;

    /// Verifies: SRS-INST-001
    #[test]
    fn parse_leaked_bytes_sums_entries() {
        let xml = r#"<?xml version="1.0"?>
<valgrindoutput>
  <error><leakedbytes>100</leakedbytes></error>
  <error><leakedbytes>200</leakedbytes></error>
</valgrindoutput>"#;
        assert_eq!(parse_leaked_bytes(xml), 300.0);
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn parse_leaked_bytes_no_leaks() {
        let xml = r#"<?xml version="1.0"?>
<valgrindoutput></valgrindoutput>"#;
        assert_eq!(parse_leaked_bytes(xml), 0.0);
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn parse_leaked_bytes_single_entry() {
        let xml = "<error><leakedbytes>512</leakedbytes></error>";
        assert_eq!(parse_leaked_bytes(xml), 512.0);
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn parse_leaked_bytes_ignores_malformed_values() {
        let xml = "<leakedbytes>not-a-number</leakedbytes><leakedbytes>100</leakedbytes>";
        assert_eq!(parse_leaked_bytes(xml), 100.0);
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn parse_leaked_bytes_empty_string() {
        assert_eq!(parse_leaked_bytes(""), 0.0);
    }

    /// Verifies: SRS-REL-002, SRS-INST-001
    #[test]
    fn finished_run_without_leaks_is_zero() {
        let xml = "<valgrindoutput><status><state>RUNNING</state></status>\
                   <status><state>FINISHED</state></status></valgrindoutput>";
        assert_eq!(leaked_bytes_of_finished_run(xml).unwrap(), 0.0);
    }

    /// Verifies: SRS-REL-002, SRS-INST-001
    #[test]
    fn finished_run_sums_its_leaks() {
        let xml = "<valgrindoutput><status><state>FINISHED</state></status>\
                   <error><kind>Leak_DefinitelyLost</kind><xwhat><leakedbytes>64</leakedbytes></xwhat></error>\
                   </valgrindoutput>";
        assert_eq!(leaked_bytes_of_finished_run(xml).unwrap(), 64.0);
    }

    /// Verifies: SRS-REL-002
    #[test]
    fn report_without_a_finished_run_is_an_error_not_zero() {
        // Valgrind started but the binary never ran to the end.
        let unfinished = "<valgrindoutput><status><state>RUNNING</state></status></valgrindoutput>";
        assert!(leaked_bytes_of_finished_run(unfinished).is_err());
        // Not a valgrind report at all.
        assert!(leaked_bytes_of_finished_run("").is_err());
        assert!(leaked_bytes_of_finished_run("valgrind: failed to start tool").is_err());
    }
}
