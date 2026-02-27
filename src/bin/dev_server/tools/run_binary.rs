use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use sqt::api::types::RunMetrics;

use crate::config::{BinaryConfig, InputMode};
use crate::docker::{self, DockerRun};

/// Run the binary inside a Docker container using `/usr/bin/time -v` to
/// collect execution metrics: return code, wall time, peak/avg RSS, CPU%.
///
/// Respects `BinaryConfig` for extra arguments, environment variables, and
/// input delivery mode.
pub fn collect(
    image: &str,
    binary_config: &BinaryConfig,
    input: Option<&str>,
    workspace: &Path,
    timeout: Duration,
) -> Result<RunMetrics> {
    let binary = &binary_config.path;

    // Build the command that will be wrapped by /usr/bin/time.
    // We always go through `sh -c` so we can handle stdin redirection and
    // env vars uniformly.
    let binary_with_args = build_binary_invocation(binary_config, input);
    let shell_cmd = format!("/usr/bin/time -v {binary_with_args} 2>&1");
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
    let mut env: Vec<(String, String)> = binary_config
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // If input_mode is Env, set the variable to the input path
    if let InputMode::Env(ref var) = binary_config.input_mode {
        if let Some(inp) = input {
            env.push((var.clone(), inp.to_string()));
        }
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

    // When using sh -c with 2>&1, time output goes to stdout
    let combined = String::from_utf8_lossy(&output.stdout);

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
        assert_eq!(
            parse_time_field("no such line", "Maximum resident set size (kbytes):"),
            None
        );
    }

    #[test]
    fn invocation_no_args_no_input() {
        let config = make_config("./app", &[], InputMode::Stdin);
        assert_eq!(build_binary_invocation(&config, None), "./app");
    }

    #[test]
    fn invocation_with_args_no_input() {
        let config = make_config("./app", &["--model", "resnet50"], InputMode::Stdin);
        assert_eq!(
            build_binary_invocation(&config, None),
            "./app --model resnet50"
        );
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
        // Env mode doesn't modify the command — it's handled via Docker -e
        assert_eq!(
            build_binary_invocation(&config, Some("/data/input.bin")),
            "./app"
        );
    }
}
