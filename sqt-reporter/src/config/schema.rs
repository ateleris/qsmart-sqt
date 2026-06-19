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

#[derive(Debug, Deserialize)]
pub struct ReportConfig {
    pub output_dir: PathBuf,
}

impl SqtConfig {
    /// Validate the configuration and resolve relative paths against `config_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if required fields are empty, `runner.url` has an invalid scheme,
    /// `test_specs.files` is empty, or the bearer token env var is missing/empty.
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
        // Validate bearer token env var if configured.
        if let Some(ref var_name) = self.runner.token_env {
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
}
