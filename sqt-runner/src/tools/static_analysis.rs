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
/// container failed, cppcheck exited non-zero, or the directory holds no
/// source file cppcheck checks. The source files are counted in the container
/// before cppcheck runs, so the check does not depend on cppcheck's wording.
pub fn collect(image: &str, binary: &str, workspace: &Path, timeout: Duration) -> Result<i64> {
    let binary_path = Path::new(binary);
    let binary_dir = binary_path.parent().unwrap_or(Path::new("/"));

    let spec = DockerRun {
        image: image.to_string(),
        command: vec![
            "sh".to_string(),
            "-c".to_string(),
            // Count the files cppcheck checks (it skips headers unless they
            // are included), then run it; it writes results to stderr, which
            // is redirected to stdout for capture.
            format!(
                "n=$(find {dir} -type f \\( {SOURCE_PATTERNS} \\) | wc -l); echo {MARKER}$n; \
                 cppcheck --enable=warning --quiet {dir} 2>&1",
                dir = docker::shell_quote(&binary_dir.display().to_string()),
            ),
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

/// Prefix of the line in which the container reports how many source files
/// cppcheck is given.
const MARKER: &str = "SQT_SOURCE_FILES=";

/// `find` name patterns of the files cppcheck checks on its own.
const SOURCE_PATTERNS: &str = "-name '*.c' -o -name '*.cpp' -o -name '*.cc' -o -name '*.cxx' \
     -o -name '*.c++' -o -name '*.tpp' -o -name '*.txx'";

/// The warning count of a cppcheck run that actually checked source code.
/// cppcheck exits 0 whether or not it finds issues, so a non-zero exit code
/// means it could not check; and with no source file it checks nothing. Either
/// way there is no measurement, which must not read as zero warnings. The
/// number of source files comes from the container's marker line, not from
/// cppcheck's messages.
fn warnings_of_completed_check(exit_code: i32, text: &str) -> Result<i64> {
    if exit_code != 0 {
        bail!("static_analysis: cppcheck exited with code {exit_code}: {}", text.trim());
    }
    let files = text
        .lines()
        .find_map(|l| l.trim().strip_prefix(MARKER))
        .and_then(|n| n.trim().parse::<u64>().ok())
        .context("static_analysis: the container did not report its source file count")?;
    if files == 0 {
        bail!("static_analysis: no source file to check");
    }
    let report: Vec<&str> = text.lines().filter(|l| !l.trim().starts_with(MARKER)).collect();
    Ok(count_warnings(&report.join("\n")))
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
        let text = "SQT_SOURCE_FILES=2\nsrc/a.c:3: warning: x\n";
        assert_eq!(warnings_of_completed_check(0, text).unwrap(), 1);
        assert_eq!(warnings_of_completed_check(0, "SQT_SOURCE_FILES=1\n").unwrap(), 0);
    }

    /// Verifies: SRS-REL-002
    #[test]
    fn a_check_that_did_not_run_is_an_error_not_zero() {
        assert!(warnings_of_completed_check(127, "sh: cppcheck: not found").is_err());
        assert!(warnings_of_completed_check(1, "cppcheck: error: unrecognized").is_err());
        // No source file, whatever cppcheck says about it, in any language.
        assert!(warnings_of_completed_check(0, "SQT_SOURCE_FILES=0\n").is_err());
        assert!(
            warnings_of_completed_check(0, "SQT_SOURCE_FILES=0\nKeine Quelldateien gefunden.")
                .is_err()
        );
        // Without the marker the file count is unknown.
        assert!(warnings_of_completed_check(0, "src/a.c:3: warning: x").is_err());
    }
}
