// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! `static.warnings`: warnings from cppcheck.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::docker::{self, DockerRun};

/// Run static analysis (cppcheck) on the binary's source directory and
/// count the number of warning-level issues.
///
/// # Errors
///
/// Returns an error, so the metric is reported as `null` instead of `0`
/// (backlog F-M1, SRS-REL-002), when cppcheck did not analyse anything: the
/// container failed, cppcheck exited non-zero, or it found no source file.
pub fn collect(image: &str, binary: &str, workspace: &Path, timeout: Duration) -> Result<i64> {
    let binary_path = Path::new(binary);
    let binary_dir = binary_path.parent().unwrap_or(Path::new("/"));

    let spec = DockerRun {
        image: image.to_string(),
        command: vec![
            "sh".to_string(),
            "-c".to_string(),
            // cppcheck writes results to stderr; we redirect to stdout for capture
            format!("cppcheck --enable=warning --quiet {} 2>&1", binary_dir.display()),
        ],
        volumes: vec![docker::identity_mount(binary_dir), docker::identity_mount(workspace)],
        working_dir: Some(binary_dir.display().to_string()),
        timeout,
        env: vec![],
    };

    let output =
        docker::run_container(&spec).context("static_analysis: docker execution failed")?;
    let text = String::from_utf8_lossy(&output.stdout);
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    warnings_of_completed_check(output.exit_code, &format!("{text}\n{stderr_text}"))
}

/// The warning count of a cppcheck run that actually checked source code.
/// cppcheck exits 0 whether or not it finds issues, so a non-zero exit code
/// means it could not check; and without source files it checks nothing.
/// Either way there is no measurement, which must not read as zero warnings.
fn warnings_of_completed_check(exit_code: i32, text: &str) -> Result<i64> {
    if exit_code != 0 {
        bail!("static_analysis: cppcheck exited with code {exit_code}: {}", text.trim());
    }
    let lower = text.to_lowercase();
    if lower.contains("no c or c++ source files found")
        || lower.contains("could not find or open any of the paths")
    {
        bail!("static_analysis: cppcheck found no source files to check");
    }
    Ok(count_warnings(text))
}

/// Count lines containing "warning" (case-insensitive) in the tool output.
fn count_warnings(text: &str) -> i64 {
    text.lines()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("warning") || lower.contains("[warning]")
        })
        .count()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies: SRS-INST-001
    #[test]
    fn count_warnings_basic() {
        let text = "file.c:10: warning: unused variable 'x'\n\
                     file.c:20: note: something\n\
                     file.c:30: warning: implicit declaration\n";
        assert_eq!(count_warnings(text), 2);
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn count_warnings_empty() {
        assert_eq!(count_warnings(""), 0);
    }

    /// Verifies: SRS-REL-002, SRS-INST-001
    #[test]
    fn completed_check_counts_its_warnings() {
        let text = "src/a.c:3: warning: x\n";
        assert_eq!(warnings_of_completed_check(0, text).unwrap(), 1);
        assert_eq!(warnings_of_completed_check(0, "").unwrap(), 0);
    }

    /// Verifies: SRS-REL-002
    #[test]
    fn a_check_that_did_not_run_is_an_error_not_zero() {
        assert!(warnings_of_completed_check(127, "sh: cppcheck: not found").is_err());
        assert!(warnings_of_completed_check(1, "cppcheck: error: unrecognized").is_err());
        assert!(
            warnings_of_completed_check(
                0,
                "cppcheck: error: could not find or open any of the paths given."
            )
            .is_err()
        );
        assert!(warnings_of_completed_check(0, "No C or C++ source files found.").is_err());
    }
}
