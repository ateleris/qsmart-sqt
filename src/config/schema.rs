use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SqtConfig {
    pub project: ProjectConfig,
    pub build_dir: PathBuf,
    pub api: ApiConfig,
    pub report: ReportConfig,
}

#[derive(Debug, Deserialize)]
pub struct ProjectConfig {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiConfig {
    pub url: String,
    pub token_env: Option<String>,
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
    /// Returns an error if required fields are empty or `api.url` has an invalid scheme.
    pub fn validate(&mut self, config_dir: &Path) -> Result<()> {
        if self.project.name.is_empty() {
            bail!("project.name must not be empty");
        }
        if self.project.version.is_empty() {
            bail!("project.version must not be empty");
        }
        let url_lower = self.api.url.to_ascii_lowercase();
        if !url_lower.starts_with("http://") && !url_lower.starts_with("https://") {
            bail!("api.url must start with http:// or https://");
        }
        if self.build_dir.is_relative() {
            self.build_dir = config_dir.join(&self.build_dir);
        }
        if self.report.output_dir.is_relative() {
            self.report.output_dir = config_dir.join(&self.report.output_dir);
        }
        Ok(())
    }

    pub fn print_summary(&self) {
        tracing::info!("Project: {} v{}", self.project.name, self.project.version);
        tracing::info!("Build dir: {}", self.build_dir.display());
        tracing::info!("API URL: {}", self.api.url);
        if let Some(env) = &self.api.token_env {
            tracing::info!("API token env: {env}");
        } else {
            tracing::info!("API token env: (none - unauthenticated)");
        }
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

    #[test]
    fn valid_config_parses() {
        let cfg = load(
            r#"
project:
  name: "Test Project"
  version: "1.0.0"
build_dir: "./build"
api:
  url: "https://example.com"
  token_env: "MY_TOKEN"
report:
  output_dir: "./reports"
"#,
        )
        .unwrap();
        assert_eq!(cfg.project.name, "Test Project");
        assert_eq!(cfg.project.version, "1.0.0");
        assert_eq!(cfg.api.url, "https://example.com");
        assert_eq!(cfg.api.token_env.as_deref(), Some("MY_TOKEN"));
    }

    #[test]
    fn missing_required_field_rejected() {
        // Missing 'api' section
        let result = load(
            r#"
project:
  name: "Test"
  version: "1.0"
build_dir: "./build"
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
build_dir: "./build"
api:
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
build_dir: "./build"
api:
  url: "ftp://example.com"
report:
  output_dir: "./reports"
"#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("api.url"));
    }

    #[test]
    fn optional_token_env_can_be_omitted() {
        let cfg = load(
            r#"
project:
  name: "Test"
  version: "1.0.0"
build_dir: "./build"
api:
  url: "http://localhost:8080"
report:
  output_dir: "./reports"
"#,
        )
        .unwrap();
        assert!(cfg.api.token_env.is_none());
    }

    #[test]
    fn extra_fields_ignored() {
        let cfg = load(
            r#"
project:
  name: "Test"
  version: "1.0.0"
  extra_field: "ignored"
build_dir: "./build"
api:
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
build_dir: "./build"
api:
  url: "https://example.com"
report:
  output_dir: "./reports"
"#,
        );
        let config_dir = f.path().parent().unwrap().to_path_buf();
        let cfg = super::super::load_config(f.path()).unwrap();
        assert!(cfg.build_dir.is_absolute());
        assert!(cfg.report.output_dir.is_absolute());
        assert!(cfg.build_dir.starts_with(&config_dir));
        assert!(cfg.report.output_dir.starts_with(&config_dir));
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
build_dir: "./build"
api:
  url: "HTTPS://example.com"
report:
  output_dir: "./reports"
"#,
        );
        assert!(cfg.is_ok(), "uppercase scheme must be accepted");
    }
}
