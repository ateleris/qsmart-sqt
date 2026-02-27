use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::docker::{self, DockerRun};

/// Run static analysis (cppcheck) on the binary's source directory and
/// count the number of warning-level issues.
pub fn collect(
    image: &str,
    binary: &str,
    workspace: &Path,
    timeout: Duration,
) -> Result<i64> {
    let binary_path = Path::new(binary);
    let binary_dir = binary_path.parent().unwrap_or(Path::new("/"));

    let spec = DockerRun {
        image: image.to_string(),
        command: vec![
            "sh".to_string(),
            "-c".to_string(),
            // cppcheck writes results to stderr; we redirect to stdout for capture
            format!(
                "cppcheck --enable=warning --quiet {} 2>&1",
                binary_dir.display()
            ),
        ],
        volumes: vec![
            docker::identity_mount(binary_dir),
            docker::identity_mount(workspace),
        ],
        working_dir: Some(binary_dir.display().to_string()),
        timeout,
    };

    let output = docker::run_container(&spec).context("static_analysis: docker execution failed")?;
    let text = String::from_utf8_lossy(&output.stdout);
    let stderr_text = String::from_utf8_lossy(&output.stderr);

    Ok(count_warnings(&format!("{text}\n{stderr_text}")))
}

/// Count lines containing "warning" (case-insensitive) in the tool output.
fn count_warnings(text: &str) -> i64 {
    text.lines()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("warning") || lower.contains("[warning]")
        })
        .count() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_warnings_basic() {
        let text = "file.c:10: warning: unused variable 'x'\n\
                     file.c:20: note: something\n\
                     file.c:30: warning: implicit declaration\n";
        assert_eq!(count_warnings(text), 2);
    }

    #[test]
    fn count_warnings_empty() {
        assert_eq!(count_warnings(""), 0);
    }
}
