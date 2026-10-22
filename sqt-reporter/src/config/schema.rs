use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SqtConfig {
    pub project: ProjectConfig,
    pub test_specs: TestSpecsConfig,
    pub runner: RunnerConfig,
    pub report: ReportConfig,
}

#[derive(Debug, Deserialize)]
pub struct ProjectConfig {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TestSpecsConfig {
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct RunnerConfig {
    pub url: String,
    pub token_env: Option<String>,
    #[serde(default = "default_max_concurrent_jobs")]
    pub max_concurrent_jobs: u32,
    #[serde(default = "default_job_timeout_secs")]
    pub job_timeout_secs: u64,
    #[serde(default = "default_retry_max")]
    pub retry_max: u32,
    #[serde(default = "default_retry_backoff_initial_secs")]
    pub retry_backoff_initial_secs: u64,
    #[serde(default = "default_poll_interval_initial_secs")]
    pub poll_interval_initial_secs: u64,
    #[serde(default = "default_poll_interval_max_secs")]
    pub poll_interval_max_secs: u64,
    /// Upper bound on one HTTP request to the Runner, body included.
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
}

fn default_max_concurrent_jobs() -> u32 {
    8
}
fn default_job_timeout_secs() -> u64 {
    300
}
fn default_retry_max() -> u32 {
    3
}
fn default_retry_backoff_initial_secs() -> u64 {
    1
}
fn default_poll_interval_initial_secs() -> u64 {
    1
}
fn default_poll_interval_max_secs() -> u64 {
    30
}
fn default_request_timeout_secs() -> u64 {
    30
}

/// Upper bound on `runner.retry_max`: bounded retry (SRS-FUNC-020).
const RETRY_MAX_LIMIT: u32 = 10;
/// Upper bound on `runner.retry_backoff_initial_secs`.
const RETRY_BACKOFF_INITIAL_LIMIT_SECS: u64 = 60;
/// Upper bound on `runner.request_timeout_secs`.
const REQUEST_TIMEOUT_LIMIT_SECS: u64 = 600;

impl RunnerConfig {
    fn validate_ranges(&self) -> Result<()> {
        if self.max_concurrent_jobs == 0 {
            bail!("runner.max_concurrent_jobs must be at least 1");
        }
        if self.job_timeout_secs == 0 {
            bail!("runner.job_timeout_secs must be at least 1");
        }
        if self.poll_interval_initial_secs == 0 {
            bail!("runner.poll_interval_initial_secs must be at least 1");
        }
        if self.poll_interval_max_secs < self.poll_interval_initial_secs {
            bail!(
                "runner.poll_interval_max_secs ({}) must not be less than runner.poll_interval_initial_secs ({})",
                self.poll_interval_max_secs,
                self.poll_interval_initial_secs
            );
        }
        if !(1..=REQUEST_TIMEOUT_LIMIT_SECS).contains(&self.request_timeout_secs) {
            bail!(
                "runner.request_timeout_secs must be between 1 and {REQUEST_TIMEOUT_LIMIT_SECS}, got {}",
                self.request_timeout_secs
            );
        }
        if self.retry_max > RETRY_MAX_LIMIT {
            bail!("runner.retry_max must be at most {RETRY_MAX_LIMIT}, got {}", self.retry_max);
        }
        if self.retry_backoff_initial_secs > RETRY_BACKOFF_INITIAL_LIMIT_SECS {
            bail!(
                "runner.retry_backoff_initial_secs must be at most {RETRY_BACKOFF_INITIAL_LIMIT_SECS}, got {}",
                self.retry_backoff_initial_secs
            );
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct ReportConfig {
    pub output_dir: PathBuf,
}

impl SqtConfig {
    /// Validate the configuration and resolve relative paths against `config_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error naming the offending field (SRS-FUNC-002) if a required field is
    /// empty, `runner.url` has an invalid scheme, `test_specs.files` is empty, a runner
    /// setting is out of range, or the bearer token env var is missing/empty.
    pub fn validate(&mut self, config_dir: &Path) -> Result<()> {
        if self.project.name.is_empty() {
            bail!("project.name must not be empty");
        }
        if self.project.version.is_empty() {
            bail!("project.version must not be empty");
        }
        let url_lower = self.runner.url.to_ascii_lowercase();
        if !url_lower.starts_with("http://") && !url_lower.starts_with("https://") {
            bail!("runner.url must start with http:// or https://");
        }
        if self.test_specs.files.is_empty() {
            bail!("test_specs.files must not be empty");
        }
        if self.report.output_dir.as_os_str().is_empty() {
            bail!("report.output_dir must not be empty");
        }
        self.runner.validate_ranges()?;
        // Validate bearer token env var if configured.
        if let Some(ref var_name) = self.runner.token_env {
            if var_name.is_empty() {
                bail!("runner.token_env must not be empty; omit it to run unauthenticated");
            }
            match std::env::var(var_name) {
                Ok(val) if !val.is_empty() => {}
                _ => {
                    bail!(
                        "bearer token env var '{var_name}' is set but empty or unset — check your environment"
                    );
                }
            }
        }
        // Resolve relative paths.
        for path in &mut self.test_specs.files {
            if path.is_relative() {
                *path = config_dir.join(&*path);
            }
        }
        if self.report.output_dir.is_relative() {
            self.report.output_dir = config_dir.join(&self.report.output_dir);
        }
        Ok(())
    }

    pub fn print_summary(&self) {
        tracing::info!("Project: {} v{}", self.project.name, self.project.version);
        tracing::info!("Runner URL: {}", self.runner.url);
        if let Some(env) = &self.runner.token_env {
            tracing::info!("Runner token env: {env}");
        } else {
            tracing::info!("Runner token env: (none - unauthenticated)");
        }
        tracing::info!(
            "Max concurrent jobs: {}, job timeout: {}s",
            self.runner.max_concurrent_jobs,
            self.runner.job_timeout_secs
        );
        tracing::info!("Report output dir: {}", self.report.output_dir.display());
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    fn write_config(yaml: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{yaml}").unwrap();
        f
    }

    fn load(yaml: &str) -> anyhow::Result<SqtConfig> {
        let f = write_config(yaml);
        super::super::load_config(f.path())
    }

    const MINIMAL_YAML: &str = r#"
project:
  name: "Test Project"
  version: "1.0.0"
test_specs:
  files:
    - "./specs/example.yaml"
runner:
  url: "https://example.com"
report:
  output_dir: "./reports"
"#;

    /// Verifies: SRS-FUNC-001
    #[test]
    fn valid_config_parses() {
        // Set the token env var so validation passes.
        // SAFETY: single-threaded test; no other thread reads this var.
        unsafe { std::env::set_var("SQT_TEST_MY_TOKEN_ZZZZZ", "secret") };
        let cfg = load(
            r#"
project:
  name: "Test Project"
  version: "1.0.0"
test_specs:
  files:
    - "./specs/example.yaml"
runner:
  url: "https://example.com"
  token_env: "SQT_TEST_MY_TOKEN_ZZZZZ"
report:
  output_dir: "./reports"
"#,
        )
        .unwrap();
        unsafe { std::env::remove_var("SQT_TEST_MY_TOKEN_ZZZZZ") };
        assert_eq!(cfg.project.name, "Test Project");
        assert_eq!(cfg.project.version, "1.0.0");
        assert_eq!(cfg.runner.url, "https://example.com");
        assert_eq!(cfg.runner.token_env.as_deref(), Some("SQT_TEST_MY_TOKEN_ZZZZZ"));
    }

    /// Verifies: SRS-FUNC-002
    #[test]
    fn missing_required_field_rejected() {
        // Missing 'runner' section
        let result = load(
            r#"
project:
  name: "Test"
  version: "1.0"
test_specs:
  files:
    - "./specs/example.yaml"
report:
  output_dir: "./reports"
"#,
        );
        assert!(result.is_err());
    }

    /// Verifies: SRS-FUNC-002
    #[test]
    fn empty_project_name_rejected() {
        let result = load(
            r#"
project:
  name: ""
  version: "1.0.0"
test_specs:
  files:
    - "./specs/example.yaml"
runner:
  url: "https://example.com"
report:
  output_dir: "./reports"
"#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("project.name"));
    }

    /// Verifies: SRS-FUNC-002
    #[test]
    fn invalid_url_scheme_rejected() {
        let result = load(
            r#"
project:
  name: "Test"
  version: "1.0.0"
test_specs:
  files:
    - "./specs/example.yaml"
runner:
  url: "ftp://example.com"
report:
  output_dir: "./reports"
"#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("runner.url"));
    }

    /// Verifies: SRS-IF-006
    #[test]
    fn optional_token_env_can_be_omitted() {
        let cfg = load(
            r#"
project:
  name: "Test"
  version: "1.0.0"
test_specs:
  files:
    - "./specs/example.yaml"
runner:
  url: "http://localhost:8080"
report:
  output_dir: "./reports"
"#,
        )
        .unwrap();
        assert!(cfg.runner.token_env.is_none());
    }

    /// Verifies: SRS-FUNC-001
    #[test]
    fn extra_fields_ignored() {
        let cfg = load(
            r#"
project:
  name: "Test"
  version: "1.0.0"
  extra_field: "ignored"
test_specs:
  files:
    - "./specs/example.yaml"
runner:
  url: "https://example.com"
  unknown_future_field: true
report:
  output_dir: "./reports"
unknown_top_level: 42
"#,
        )
        .unwrap();
        assert_eq!(cfg.project.name, "Test");
    }

    /// Verifies: SRS-RES-001
    #[test]
    fn relative_paths_resolved_against_config_dir() {
        let f = write_config(
            r#"
project:
  name: "Test"
  version: "1.0.0"
test_specs:
  files:
    - "./specs/example.yaml"
runner:
  url: "https://example.com"
report:
  output_dir: "./reports"
"#,
        );
        let config_dir = f.path().parent().unwrap().to_path_buf();
        let cfg = super::super::load_config(f.path()).unwrap();
        assert!(cfg.report.output_dir.is_absolute());
        assert!(cfg.report.output_dir.starts_with(&config_dir));
        // test_specs paths should also be resolved
        for p in &cfg.test_specs.files {
            assert!(p.is_absolute(), "test_specs path should be absolute: {}", p.display());
            assert!(p.starts_with(&config_dir));
        }
    }

    /// Verifies: SRS-FUNC-002
    #[test]
    fn url_scheme_check_is_case_insensitive() {
        // RFC 3986: URI schemes are case-insensitive. HTTPS:// must not be
        // rejected as an unsupported scheme.
        let cfg = load(
            r#"
project:
  name: "Test"
  version: "1.0.0"
test_specs:
  files:
    - "./specs/example.yaml"
runner:
  url: "HTTPS://example.com"
report:
  output_dir: "./reports"
"#,
        );
        assert!(cfg.is_ok(), "uppercase scheme must be accepted");
    }

    /// Verifies: SRS-FUNC-002
    #[test]
    fn empty_test_specs_files_rejected() {
        let result = load(
            r#"
project:
  name: "Test"
  version: "1.0.0"
test_specs:
  files: []
runner:
  url: "https://example.com"
report:
  output_dir: "./reports"
"#,
        );
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("test_specs.files"),
            "error should mention test_specs.files"
        );
    }

    /// Verifies: SRS-FUNC-001, SRS-PERF-002, SRS-PERF-003
    #[test]
    fn runner_operational_defaults_applied() {
        let cfg = load(MINIMAL_YAML).unwrap();
        assert_eq!(cfg.runner.max_concurrent_jobs, 8);
        assert_eq!(cfg.runner.job_timeout_secs, 300);
        assert_eq!(cfg.runner.retry_max, 3);
        assert_eq!(cfg.runner.retry_backoff_initial_secs, 1);
        assert_eq!(cfg.runner.poll_interval_initial_secs, 1);
        assert_eq!(cfg.runner.poll_interval_max_secs, 30);
    }

    /// Verifies: SRS-FUNC-001, SRS-PERF-002, SRS-PERF-003
    #[test]
    fn runner_operational_fields_can_be_overridden() {
        let cfg = load(
            r#"
project:
  name: "Test"
  version: "1.0.0"
test_specs:
  files:
    - "./specs/example.yaml"
runner:
  url: "https://example.com"
  max_concurrent_jobs: 4
  job_timeout_secs: 600
  retry_max: 5
  retry_backoff_initial_secs: 2
  poll_interval_initial_secs: 3
  poll_interval_max_secs: 60
report:
  output_dir: "./reports"
"#,
        )
        .unwrap();
        assert_eq!(cfg.runner.max_concurrent_jobs, 4);
        assert_eq!(cfg.runner.job_timeout_secs, 600);
        assert_eq!(cfg.runner.retry_max, 5);
        assert_eq!(cfg.runner.retry_backoff_initial_secs, 2);
        assert_eq!(cfg.runner.poll_interval_initial_secs, 3);
        assert_eq!(cfg.runner.poll_interval_max_secs, 60);
    }

    /// Verifies: SRS-IF-006, SRS-FUNC-002
    #[test]
    fn token_env_validation_fires_when_var_missing() {
        // Use an env var name that is guaranteed not to be set.
        let var_name = "SQT_TEST_ABSENT_TOKEN_VAR_ZZZZZ";
        // Ensure it really is absent.
        // SAFETY: single-threaded test; no other thread reads this var.
        unsafe { std::env::remove_var(var_name) };
        let yaml = format!(
            r#"
project:
  name: "Test"
  version: "1.0.0"
test_specs:
  files:
    - "./specs/example.yaml"
runner:
  url: "https://example.com"
  token_env: "{var_name}"
report:
  output_dir: "./reports"
"#
        );
        let result = load(&yaml);
        assert!(result.is_err(), "should fail when token env var is absent");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("bearer token env var"), "error should mention bearer token: {msg}");
        assert!(msg.contains(var_name), "error should mention the var name: {msg}");
    }

    /// Verifies: SRS-IF-006, SRS-FUNC-002
    #[test]
    fn token_env_validation_fires_when_var_empty() {
        let var_name = "SQT_TEST_EMPTY_TOKEN_VAR_ZZZZZ";
        // Set it to an empty string.
        // SAFETY: single-threaded test; no other thread reads this var.
        unsafe { std::env::set_var(var_name, "") };
        let yaml = format!(
            r#"
project:
  name: "Test"
  version: "1.0.0"
test_specs:
  files:
    - "./specs/example.yaml"
runner:
  url: "https://example.com"
  token_env: "{var_name}"
report:
  output_dir: "./reports"
"#
        );
        let result = load(&yaml);
        // Clean up regardless.
        // SAFETY: single-threaded test; no other thread reads this var.
        unsafe { std::env::remove_var(var_name) };
        assert!(result.is_err(), "should fail when token env var is empty");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("bearer token env var"), "error should mention bearer token: {msg}");
    }

    /// `MINIMAL_YAML` with extra lines under `runner:` and the given `output_dir`.
    fn config_with(runner_extra: &str, output_dir: &str) -> String {
        format!(
            r#"
project:
  name: "Test"
  version: "1.0.0"
test_specs:
  files:
    - "./specs/example.yaml"
runner:
  url: "https://example.com"
{runner_extra}
report:
  output_dir: "{output_dir}"
"#
        )
    }

    /// Verifies: SRS-FUNC-002
    #[test]
    fn out_of_range_runner_settings_are_rejected_naming_the_field() {
        let cases = [
            ("  max_concurrent_jobs: 0", "runner.max_concurrent_jobs"),
            ("  job_timeout_secs: 0", "runner.job_timeout_secs"),
            ("  poll_interval_initial_secs: 0", "runner.poll_interval_initial_secs"),
            (
                "  poll_interval_initial_secs: 10\n  poll_interval_max_secs: 5",
                "runner.poll_interval_max_secs",
            ),
            ("  retry_max: 11", "runner.retry_max"),
            ("  request_timeout_secs: 0", "runner.request_timeout_secs"),
            ("  request_timeout_secs: 601", "runner.request_timeout_secs"),
            ("  retry_backoff_initial_secs: 61", "runner.retry_backoff_initial_secs"),
            ("  token_env: \"\"", "runner.token_env"),
        ];
        for (extra, field) in cases {
            let err = load(&config_with(extra, "./reports")).expect_err(extra);
            assert!(err.to_string().contains(field), "{extra:?}: error must name {field}: {err}");
        }
    }

    /// Verifies: SRS-FUNC-002
    #[test]
    fn boundary_runner_settings_are_accepted() {
        let extra = "  max_concurrent_jobs: 1\n  job_timeout_secs: 1\n  retry_max: 10\n  \
                     retry_backoff_initial_secs: 60\n  poll_interval_initial_secs: 5\n  \
                     poll_interval_max_secs: 5";
        let cfg = load(&config_with(extra, "./reports")).unwrap();
        assert_eq!(cfg.runner.max_concurrent_jobs, 1);
        assert_eq!(cfg.runner.retry_max, 10);
    }

    /// Verifies: SRS-FUNC-002, SRS-RES-001
    #[test]
    fn empty_output_dir_rejected() {
        let err = load(&config_with("", "")).unwrap_err();
        assert!(err.to_string().contains("report.output_dir"), "{err}");
    }
}
