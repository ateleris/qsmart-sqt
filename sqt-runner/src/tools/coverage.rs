// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::docker::{self, DockerRun};

/// Run lcov to collect line coverage percentage.
///
/// Expects the binary to have been built with `--coverage` (gcov) and the
/// coverage data files (`.gcda`, `.gcno`) to exist alongside the binary.
pub fn collect(image: &str, binary: &str, workspace: &Path, timeout: Duration) -> Result<f64> {
    let binary_path = Path::new(binary);
    let binary_dir = binary_path.parent().unwrap_or(Path::new("/"));
    let lcov_output = workspace.join("coverage.info");

    // Step 1: capture coverage data
    let spec = DockerRun {
        image: image.to_string(),
        command: vec![
            "sh".to_string(),
            "-c".to_string(),
            format!(
                "lcov --capture --directory {} --output-file {} --quiet 2>&1 && \
                 lcov --summary {} 2>&1",
                binary_dir.display(),
                lcov_output.display(),
                lcov_output.display(),
            ),
        ],
        volumes: vec![docker::identity_mount(binary_dir), docker::identity_mount(workspace)],
        working_dir: Some(binary_dir.display().to_string()),
        timeout,
        env: vec![],
    };

    let output = docker::run_container(&spec).context("coverage: docker execution failed")?;
    let stdout_text = String::from_utf8_lossy(&output.stdout);
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout_text}\n{stderr_text}");

    parse_line_coverage(&combined)
}

/// Parse line coverage percentage from `lcov --summary` output.
///
/// Expected format: `  lines......: 85.5% (171 of 200 lines)`
fn parse_line_coverage(text: &str) -> Result<f64> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("lines") {
            // lines......: 85.5% (171 of 200 lines)
            if let Some(pct_start) = trimmed.find(':') {
                let after_colon = trimmed[pct_start + 1..].trim();
                if let Some(pct_end) = after_colon.find('%')
                    && let Ok(val) = after_colon[..pct_end].trim().parse::<f64>()
                {
                    return Ok(val);
                }
            }
        }
    }
    bail!("coverage: could not parse line coverage from lcov output")
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
    fn parse_line_coverage_extracts_percentage() {
        let text = "Reading tracefile coverage.info\n\
                     Summary coverage rate:\n\
                       lines......: 85.5% (171 of 200 lines)\n\
                       functions..: 90.0% (9 of 10 functions)\n";
        assert_eq!(parse_line_coverage(text).unwrap(), 85.5);
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn parse_line_coverage_missing_fails() {
        assert!(parse_line_coverage("no coverage here").is_err());
    }
}
