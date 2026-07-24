use std::{path::Path, time::Duration};

use anyhow::{Context, Result};

use crate::config::{BinaryConfig, InputMode};

/// Execution metrics produced by running the binary in a container.
pub struct RunBinaryOutput {
    pub return_code: Option<i64>,
    /// The binary's own execution time. When GNU time is available inside the
    /// container this is its "Elapsed (wall clock) time"; otherwise it falls
    /// back to the outer `docker run` wall clock.
    pub time_execution_ms: Option<f64>,
    pub memory_peak_rss_kb: Option<f64>,
    pub memory_avg_rss_kb: Option<f64>,
    pub cpu_pct: Option<f64>,
    /// Harness/Docker overhead = outer `docker run` wall clock − inner elapsed
    /// time. `None` when GNU time was unavailable (no inner measurement to
    /// subtract). Diagnostic only.
    pub docker_overhead_ms: Option<f64>,
}
use crate::docker::{self, DockerRun};

/// Run the binary inside a Docker container and collect execution metrics.
///
/// If GNU time (`/usr/bin/time`) is available in the container, it is used to
/// collect peak RSS and CPU%.  Otherwise, those metrics are returned as `None`
/// and only return code and wall time (measured on the host side) are reported.
///
/// Respects `BinaryConfig` for extra arguments, environment variables, and
/// input delivery mode.
pub fn collect(
    image: &str,
    binary_config: &BinaryConfig,
    input: Option<&str>,
    workspace: &Path,
    timeout: Duration,
) -> Result<RunBinaryOutput> {
    let binary = &binary_config.path;

    // Build the invocation fragment (binary + args + input handling).
    let binary_with_args = build_binary_invocation(binary_config, input);

    // Use GNU time if available, otherwise fall back to plain execution.
    // The shell snippet checks for /usr/bin/time at runtime so we work with
    // any container image.
    let shell_cmd = format!(
        "if [ -x /usr/bin/time ]; then /usr/bin/time -v {binary_with_args} 2>&1; \
         else {binary_with_args}; fi"
    );
    let command = vec!["sh".to_string(), "-c".to_string(), shell_cmd];

    let binary_path = Path::new(binary);
    let binary_dir = binary_path.parent().unwrap_or(Path::new("/"));

    let mut volumes = vec![docker::identity_mount(binary_dir)];

    // Mount input file directory if needed
    if let Some(inp) = input {
        let inp_dir = Path::new(inp).parent().unwrap_or(Path::new("/"));
        if inp_dir != binary_dir {
            volumes.push(docker::identity_mount(inp_dir));
        }
    }

    // Mount workspace for any output
    volumes.push(docker::identity_mount(workspace));

    // Collect environment variables
    let mut env: Vec<(String, String)> =
        binary_config.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

    // If input_mode is Env, set the variable to the input path
    if let InputMode::Env(ref var) = binary_config.input_mode
        && let Some(inp) = input
    {
        env.push((var.clone(), inp.to_string()));
    }

    let spec = DockerRun {
        image: image.to_string(),
        command,
        volumes,
        working_dir: Some(binary_dir.display().to_string()),
        timeout,
        env,
    };

    let output = docker::run_container(&spec).context("run_binary: docker execution failed")?;

    // When GNU time is present, its output ends up in stdout via 2>&1.
    // When it's absent, stdout is just the binary's own output.
    let combined = String::from_utf8_lossy(&output.stdout);

    let memory_peak_rss = parse_time_field(&combined, "Maximum resident set size (kbytes):");
    let cpu_pct = parse_time_field(&combined, "Percent of CPU this job got:");

    if memory_peak_rss.is_none() {
        tracing::info!("/usr/bin/time not found in container - peak RSS and CPU% unavailable");
    }

    // The outer wall clock covers the whole `docker run`, including container
    // create/start/teardown. The primary latency metric must reflect the
    // binary's own execution (SRS-FUNC-005), so prefer GNU time's inner
    // "Elapsed (wall clock) time" when present and record the difference as
    // harness/Docker overhead. Without GNU time there is no inner measurement,
    // so fall back to the outer wall clock and report no overhead.
    let outer_wall_ms = output.wall_time.as_secs_f64() * 1000.0;
    let (time_execution_ms, docker_overhead_ms) =
        if let Some(inner_ms) = parse_elapsed_ms(&combined) {
            (Some(inner_ms), Some((outer_wall_ms - inner_ms).max(0.0)))
        } else {
            tracing::info!(
                "/usr/bin/time elapsed line unavailable - reporting outer docker wall clock \
                 as time.avg_ms (includes container overhead)"
            );
            (Some(outer_wall_ms), None)
        };

    Ok(RunBinaryOutput {
        return_code: Some(i64::from(output.exit_code)),
        time_execution_ms,
        memory_peak_rss_kb: memory_peak_rss,
        memory_avg_rss_kb: None,
        cpu_pct,
        docker_overhead_ms,
    })
}

/// Build the shell command fragment for invoking the binary with its
/// configured arguments and input delivery mode.
///
/// Returns a string like: `./app --flag1 val1 < input.bin`
/// or: `./app --flag1 val1 input.bin` (for `input_mode: arg`)
fn build_binary_invocation(config: &BinaryConfig, input: Option<&str>) -> String {
    let mut parts = vec![config.path.clone()];
    parts.extend(config.args.iter().cloned());

    match (&config.input_mode, input) {
        (InputMode::Stdin, Some(inp)) => {
            format!("{} < {inp}", shell_join(&parts))
        }
        (InputMode::Arg, Some(inp)) => {
            parts.push(inp.to_string());
            shell_join(&parts)
        }
        // InputMode::Env is handled via Docker -e flag, no command change needed
        _ => shell_join(&parts),
    }
}

/// Join parts with spaces, quoting any that contain special characters.
fn shell_join(parts: &[String]) -> String {
    parts
        .iter()
        .map(|p| {
            if p.contains(' ') || p.contains('\'') || p.contains('"') {
                format!("'{}'", p.replace('\'', "'\\''"))
            } else {
                p.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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

/// Parse GNU time `-v`'s "Elapsed (wall clock) time" line into milliseconds.
///
/// The value is formatted as `h:mm:ss`, `m:ss`, or `m:ss.ss` (e.g. `1:02:03`
/// or `0:01.23`). Returns `None` when the line is absent or unparseable.
fn parse_elapsed_ms(text: &str) -> Option<f64> {
    let line = text.lines().find(|l| l.trim().starts_with("Elapsed (wall clock) time"))?;
    let value = line.trim().rsplit_once(": ")?.1.trim();
    parse_hms_to_ms(value)
}

/// Parse an `h:mm:ss` / `m:ss` / `ss[.ss]` duration into milliseconds.
fn parse_hms_to_ms(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [s] => (0.0, 0.0, s.parse::<f64>().ok()?),
        [m, s] => (0.0, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        [h, m, s] => (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        _ => return None,
    };
    Some((h * 3600.0 + m * 60.0 + sec) * 1000.0)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn make_config(path: &str, args: &[&str], input_mode: InputMode) -> BinaryConfig {
        BinaryConfig {
            path: path.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            env: HashMap::new(),
            input_mode,
        }
    }

    #[test]
    fn parse_time_field_extracts_rss() {
        let text = "\tCommand being timed: \"./app\"\n\
                     \tMaximum resident set size (kbytes): 15432\n\
                     \tPercent of CPU this job got: 98%\n";
        assert_eq!(parse_time_field(text, "Maximum resident set size (kbytes):"), Some(15432.0));
        assert_eq!(parse_time_field(text, "Percent of CPU this job got:"), Some(98.0));
    }

    #[test]
    fn parse_time_field_missing_returns_none() {
        assert_eq!(parse_time_field("no such line", "Maximum resident set size (kbytes):"), None);
    }

    #[test]
    fn invocation_no_args_no_input() {
        let config = make_config("./app", &[], InputMode::Stdin);
        assert_eq!(build_binary_invocation(&config, None), "./app");
    }

    #[test]
    fn invocation_with_args_no_input() {
        let config = make_config("./app", &["--model", "resnet50"], InputMode::Stdin);
        assert_eq!(build_binary_invocation(&config, None), "./app --model resnet50");
    }

    #[test]
    fn invocation_stdin_with_input() {
        let config = make_config("./app", &["--flag"], InputMode::Stdin);
        assert_eq!(
            build_binary_invocation(&config, Some("/data/input.bin")),
            "./app --flag < /data/input.bin"
        );
    }

    #[test]
    fn invocation_arg_with_input() {
        let config = make_config("./app", &["--flag"], InputMode::Arg);
        assert_eq!(
            build_binary_invocation(&config, Some("/data/input.bin")),
            "./app --flag /data/input.bin"
        );
    }

    #[test]
    fn invocation_env_with_input() {
        let config = make_config("./app", &[], InputMode::Env("INPUT_FILE".into()));
        // Env mode doesn't modify the command - it's handled via Docker -e
        assert_eq!(build_binary_invocation(&config, Some("/data/input.bin")), "./app");
    }

    #[test]
    fn invocation_stdin_no_input() {
        let config = make_config("./app", &["--flag"], InputMode::Stdin);
        assert_eq!(build_binary_invocation(&config, None), "./app --flag");
    }

    #[test]
    fn shell_join_quotes_spaces() {
        let parts = vec!["/usr/bin/app".to_string(), "arg with space".to_string()];
        let result = shell_join(&parts);
        assert_eq!(result, "/usr/bin/app 'arg with space'");
    }

    #[test]
    fn shell_join_escapes_single_quotes() {
        let parts = vec!["./app".to_string(), "it's".to_string()];
        let result = shell_join(&parts);
        assert_eq!(result, "./app 'it'\\''s'");
    }

    #[test]
    fn shell_join_plain_args_unquoted() {
        let parts = vec!["./app".to_string(), "--model".to_string(), "resnet50".to_string()];
        assert_eq!(shell_join(&parts), "./app --model resnet50");
    }

    #[test]
    fn parse_time_field_strips_percent() {
        let text = "\tPercent of CPU this job got: 75%\n";
        assert_eq!(parse_time_field(text, "Percent of CPU this job got:"), Some(75.0));
    }

    #[test]
    fn parse_time_field_returns_none_for_non_numeric() {
        let text = "\tSome field: not-a-number\n";
        assert_eq!(parse_time_field(text, "Some field:"), None);
    }

    // --- Inner elapsed time (F-C3, SRS-FUNC-005) -----------------------------

    #[test]
    fn parse_elapsed_ms_m_ss_format() {
        let text = "\tElapsed (wall clock) time (h:mm:ss or m:ss): 0:01.23\n";
        assert_eq!(parse_elapsed_ms(text), Some(1230.0));
    }

    #[test]
    fn parse_elapsed_ms_h_mm_ss_format() {
        let text = "\tElapsed (wall clock) time (h:mm:ss or m:ss): 1:02:03\n";
        assert_eq!(parse_elapsed_ms(text), Some(3_723_000.0));
    }

    #[test]
    fn parse_elapsed_ms_seconds_only() {
        let text = "\tElapsed (wall clock) time (h:mm:ss or m:ss): 12.50\n";
        assert_eq!(parse_elapsed_ms(text), Some(12_500.0));
    }

    #[test]
    fn parse_elapsed_ms_absent_returns_none() {
        assert_eq!(parse_elapsed_ms("no time output here"), None);
    }
}
