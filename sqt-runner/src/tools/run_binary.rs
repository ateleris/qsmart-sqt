use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::config::{BinaryConfig, InputMode};

/// Execution metrics produced by running the binary in a container.
pub struct RunBinaryOutput {
    pub return_code: Option<i64>,
    pub time_execution_ms: Option<f64>,
    pub memory_peak_rss_kb: Option<f64>,
    pub memory_avg_rss_kb: Option<f64>,
    pub cpu_pct: Option<f64>,
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

    // Wall time is always measured on the host side, independent of GNU time.
    let time_execution = output.wall_time.as_secs_f64() * 1000.0; // ms

    Ok(RunBinaryOutput {
        return_code: Some(i64::from(output.exit_code)),
        time_execution_ms: Some(time_execution),
        memory_peak_rss_kb: memory_peak_rss,
        memory_avg_rss_kb: None,
        cpu_pct,
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
            // `inp` is the untrusted client-supplied path; quote it so shell
            // metacharacters (`;`, `|`, `$`, backticks, …) cannot break out of
            // the redirection into a second command (F-C1).
            format!("{} < {}", shell_join(&parts), docker::shell_quote(inp))
        }
        (InputMode::Arg, Some(inp)) => {
            parts.push(inp.to_string());
            shell_join(&parts)
        }
        // InputMode::Env is handled via Docker -e flag, no command change needed
        _ => shell_join(&parts),
    }
}

/// Join parts into a single `sh` command line, single-quoting every part so
/// that no element — trusted binary/args or untrusted input — can be
/// interpreted as shell syntax.
fn shell_join(parts: &[String]) -> String {
    parts.iter().map(|p| docker::shell_quote(p)).collect::<Vec<_>>().join(" ")
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
    use std::collections::HashMap;

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

    // Every element is single-quoted (safe by construction); quoted trusted
    // args are still passed verbatim to the program by the shell.

    #[test]
    fn invocation_no_args_no_input() {
        let config = make_config("./app", &[], InputMode::Stdin);
        assert_eq!(build_binary_invocation(&config, None), "'./app'");
    }

    #[test]
    fn invocation_with_args_no_input() {
        let config = make_config("./app", &["--model", "resnet50"], InputMode::Stdin);
        assert_eq!(build_binary_invocation(&config, None), "'./app' '--model' 'resnet50'");
    }

    #[test]
    fn invocation_stdin_with_input() {
        let config = make_config("./app", &["--flag"], InputMode::Stdin);
        assert_eq!(
            build_binary_invocation(&config, Some("/data/input.bin")),
            "'./app' '--flag' < '/data/input.bin'"
        );
    }

    #[test]
    fn invocation_arg_with_input() {
        let config = make_config("./app", &["--flag"], InputMode::Arg);
        assert_eq!(
            build_binary_invocation(&config, Some("/data/input.bin")),
            "'./app' '--flag' '/data/input.bin'"
        );
    }

    #[test]
    fn invocation_env_with_input() {
        let config = make_config("./app", &[], InputMode::Env("INPUT_FILE".into()));
        // Env mode doesn't modify the command - it's handled via Docker -e
        assert_eq!(build_binary_invocation(&config, Some("/data/input.bin")), "'./app'");
    }

    #[test]
    fn invocation_stdin_no_input() {
        let config = make_config("./app", &["--flag"], InputMode::Stdin);
        assert_eq!(build_binary_invocation(&config, None), "'./app' '--flag'");
    }

    // --- Security: client-supplied `input` must never break out of the shell ---
    // (F-C1) `input` originates from the untrusted `JobRequest.input`.

    #[test]
    fn invocation_stdin_input_with_metachars_is_neutralised() {
        let config = make_config("./app", &[], InputMode::Stdin);
        let inv = build_binary_invocation(&config, Some("/tmp/x; touch /tmp/pwned"));
        // The malicious payload must be a single quoted redirection target, not
        // a second command.
        assert_eq!(inv, "'./app' < '/tmp/x; touch /tmp/pwned'");
    }

    #[test]
    fn invocation_arg_input_with_metachars_is_neutralised() {
        let config = make_config("./app", &[], InputMode::Arg);
        let inv = build_binary_invocation(&config, Some("$(reboot)"));
        assert_eq!(inv, "'./app' '$(reboot)'");
    }

    #[test]
    fn invocation_input_with_single_quote_is_escaped() {
        let config = make_config("./app", &[], InputMode::Stdin);
        let inv = build_binary_invocation(&config, Some("a'b"));
        assert_eq!(inv, "'./app' < 'a'\\''b'");
    }

    #[test]
    fn shell_join_quotes_spaces() {
        let parts = vec!["/usr/bin/app".to_string(), "arg with space".to_string()];
        let result = shell_join(&parts);
        assert_eq!(result, "'/usr/bin/app' 'arg with space'");
    }

    #[test]
    fn shell_join_escapes_single_quotes() {
        let parts = vec!["./app".to_string(), "it's".to_string()];
        let result = shell_join(&parts);
        assert_eq!(result, "'./app' 'it'\\''s'");
    }

    #[test]
    fn shell_join_quotes_every_part() {
        let parts = vec!["./app".to_string(), "--model".to_string(), "resnet50".to_string()];
        assert_eq!(shell_join(&parts), "'./app' '--model' 'resnet50'");
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
}
