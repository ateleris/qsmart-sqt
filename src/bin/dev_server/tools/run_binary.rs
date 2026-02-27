use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use sqt::api::types::RunMetrics;

use crate::docker::{self, DockerRun};

/// Run the binary inside a Docker container using `/usr/bin/time -v` to
/// collect execution metrics: return code, wall time, peak/avg RSS, CPU%.
pub fn collect(
    image: &str,
    binary: &str,
    input: Option<&str>,
    workspace: &Path,
    timeout: Duration,
) -> Result<RunMetrics> {
    // We use GNU time in verbose mode to capture resource usage.
    // Format: /usr/bin/time -v <binary> [< input]
    let mut command = vec![
        "/usr/bin/time".to_string(),
        "-v".to_string(),
        binary.to_string(),
    ];

    // If there is an input file, redirect it via shell
    if let Some(inp) = input {
        command = vec![
            "sh".to_string(),
            "-c".to_string(),
            format!("/usr/bin/time -v {binary} < {inp} 2>&1"),
        ];
    }

    let binary_path = Path::new(binary);
    let binary_dir = binary_path.parent().unwrap_or(Path::new("/"));

    let mut volumes = vec![docker::identity_mount(binary_dir)];

    // Mount input file directory if different
    if let Some(inp) = input {
        let inp_dir = Path::new(inp).parent().unwrap_or(Path::new("/"));
        if inp_dir != binary_dir {
            volumes.push(docker::identity_mount(inp_dir));
        }
    }

    // Mount workspace for any output
    volumes.push(docker::identity_mount(workspace));

    let spec = DockerRun {
        image: image.to_string(),
        command,
        volumes,
        working_dir: Some(binary_dir.display().to_string()),
        timeout,
    };

    let output = docker::run_container(&spec).context("run_binary: docker execution failed")?;

    // Parse GNU time -v output from stderr (or combined stdout if redirected).
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let combined = if input.is_some() {
        // When using shell redirect, time output goes to stdout via 2>&1
        String::from_utf8_lossy(&output.stdout).into_owned()
    } else {
        stderr_text.to_string()
    };

    let time_execution = output.wall_time.as_secs_f64() * 1000.0; // ms
    let memory_peak_rss = parse_time_field(&combined, "Maximum resident set size (kbytes):");
    let cpu_pct = parse_time_field(&combined, "Percent of CPU this job got:");

    Ok(RunMetrics {
        return_code: Some(i64::from(output.exit_code)),
        time_execution: Some(time_execution),
        memory_peak_rss,
        memory_avg_rss: None, // GNU time doesn't provide avg RSS
        cpu_max_pct: cpu_pct,
        cpu_avg_pct: cpu_pct, // single sample — max == avg
        memory_leaks: None,
        coverage_line_pct: None,
        ml_accuracy: None,
        ml_f1_score: None,
        ml_recall: None,
        ml_precision: None,
    })
}

/// Parse a numeric value from GNU time `-v` output.
///
/// Lines look like: `\tMaximum resident set size (kbytes): 12345`
fn parse_time_field(text: &str, label: &str) -> Option<f64> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(label) {
            let val_str = rest.trim().trim_end_matches('%');
            return val_str.parse::<f64>().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_time_field_extracts_rss() {
        let text = "\tCommand being timed: \"./app\"\n\
                     \tMaximum resident set size (kbytes): 15432\n\
                     \tPercent of CPU this job got: 98%\n";
        assert_eq!(
            parse_time_field(text, "Maximum resident set size (kbytes):"),
            Some(15432.0)
        );
        assert_eq!(
            parse_time_field(text, "Percent of CPU this job got:"),
            Some(98.0)
        );
    }

    #[test]
    fn parse_time_field_missing_returns_none() {
        assert_eq!(parse_time_field("no such line", "Maximum resident set size (kbytes):"), None);
    }
}
