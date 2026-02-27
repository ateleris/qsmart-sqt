use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// How input files are passed to the binary under test.
#[derive(Debug, Clone)]
pub enum InputMode {
    /// Pipe input file contents to stdin (default).
    Stdin,
    /// Pass input file path as the last CLI argument.
    Arg,
    /// Set an environment variable to the input file path.
    Env(String),
}

impl Default for InputMode {
    fn default() -> Self {
        Self::Stdin
    }
}

impl<'de> Deserialize<'de> for InputMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "stdin" => Ok(Self::Stdin),
            "arg" => Ok(Self::Arg),
            other if other.starts_with("env:") => {
                let var = other.strip_prefix("env:").unwrap().to_string();
                if var.is_empty() {
                    return Err(serde::de::Error::custom(
                        "env: requires a variable name (e.g. env:INPUT_FILE)",
                    ));
                }
                Ok(Self::Env(var))
            }
            _ => Err(serde::de::Error::custom(format!(
                "unknown input_mode: {s} (expected: stdin, arg, env:VAR_NAME)"
            ))),
        }
    }
}

/// Configuration for the binary under test.
#[derive(Debug, Clone, Deserialize)]
pub struct BinaryConfig {
    /// Path to the binary.
    pub path: String,

    /// Extra arguments always passed to the binary.
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables set for every run.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// How input files are delivered to the binary.
    #[serde(default)]
    pub input_mode: InputMode,
}

/// Top-level tool configuration section.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ToolsConfig {
    /// Docker image for running the binary (required for run jobs).
    pub run: Option<RunToolConfig>,

    /// Valgrind configuration (optional; enables `memory.leaks`).
    pub valgrind: Option<ValgrindToolConfig>,

    /// Coverage configuration (optional; enables `coverage.line_pct`).
    pub coverage: Option<CoverageToolConfig>,

    /// SonarQube configuration (optional; enables `sonar.*` metrics).
    pub sonarqube: Option<SonarQubeToolConfig>,

    /// Static analysis configuration (optional; enables `static.warnings`).
    pub static_analysis: Option<StaticAnalysisToolConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunToolConfig {
    pub image: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ValgrindToolConfig {
    pub image: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CoverageToolConfig {
    pub image: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SonarQubeToolConfig {
    /// SonarQube server URL.
    pub url: String,

    /// Name of the environment variable holding the authentication token.
    #[serde(default = "default_sonar_token_env")]
    pub token_env: String,
}

fn default_sonar_token_env() -> String {
    "SONAR_TOKEN".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct StaticAnalysisToolConfig {
    pub image: String,
}

/// Top-level server configuration loaded from a YAML file.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Binary under test configuration.
    pub binary: BinaryConfig,

    /// Workspace directory for evidence and tool output.
    #[serde(default = "default_workspace")]
    pub workspace: PathBuf,

    /// Per-tool configuration.
    #[serde(default)]
    pub tools: ToolsConfig,
}

fn default_workspace() -> PathBuf {
    PathBuf::from("/tmp/sqt-dev-server")
}

impl ServerConfig {
    /// Load configuration from a YAML file.
    pub fn load(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config file: {}", path.display()))?;
        let config: Self = serde_yaml::from_str(&contents)
            .with_context(|| format!("failed to parse config file: {}", path.display()))?;
        Ok(config)
    }
}

/// Flattened tool configuration used by job executors.
///
/// Built from `ServerConfig` at startup — this is the internal representation
/// that `run_job` and `analysis_job` consume.
#[derive(Clone)]
pub struct ToolConfig {
    pub run_image: Option<String>,
    pub valgrind_image: Option<String>,
    pub coverage_image: Option<String>,
    pub sonar_url: Option<String>,
    pub sonar_token: Option<String>,
    pub static_image: Option<String>,
}

impl ToolConfig {
    pub fn from_server_config(config: &ServerConfig) -> Self {
        let sonar_token = config
            .tools
            .sonarqube
            .as_ref()
            .and_then(|s| std::env::var(&s.token_env).ok());

        Self {
            run_image: config.tools.run.as_ref().map(|r| r.image.clone()),
            valgrind_image: config.tools.valgrind.as_ref().map(|v| v.image.clone()),
            coverage_image: config.tools.coverage.as_ref().map(|c| c.image.clone()),
            sonar_url: config.tools.sonarqube.as_ref().map(|s| s.url.clone()),
            sonar_token,
            static_image: config
                .tools
                .static_analysis
                .as_ref()
                .map(|s| s.image.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config() {
        let yaml = r#"
binary:
  path: target/debug/my-app
tools:
  run:
    image: sqt-run:latest
"#;
        let config: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.binary.path, "target/debug/my-app");
        assert!(config.binary.args.is_empty());
        assert!(config.binary.env.is_empty());
        assert!(matches!(config.binary.input_mode, InputMode::Stdin));
        assert_eq!(config.workspace, PathBuf::from("/tmp/sqt-dev-server"));
        assert_eq!(config.tools.run.as_ref().unwrap().image, "sqt-run:latest");
        assert!(config.tools.valgrind.is_none());
    }

    #[test]
    fn parse_full_config() {
        let yaml = r#"
binary:
  path: target/debug/my-app
  args: ["--model", "resnet50"]
  env:
    LD_LIBRARY_PATH: /opt/libs
    MODEL_DIR: /data/models
  input_mode: arg

workspace: /var/sqt/workspace

tools:
  run:
    image: sqt-run:latest
  valgrind:
    image: sqt-valgrind:latest
  coverage:
    image: sqt-coverage:latest
  sonarqube:
    url: http://sonar.local:9000
    token_env: MY_SONAR_TOKEN
  static_analysis:
    image: sqt-static:latest
"#;
        let config: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.binary.path, "target/debug/my-app");
        assert_eq!(config.binary.args, vec!["--model", "resnet50"]);
        assert_eq!(config.binary.env.get("LD_LIBRARY_PATH").unwrap(), "/opt/libs");
        assert!(matches!(config.binary.input_mode, InputMode::Arg));
        assert_eq!(config.workspace, PathBuf::from("/var/sqt/workspace"));
        assert!(config.tools.valgrind.is_some());
        assert!(config.tools.coverage.is_some());
        assert_eq!(
            config.tools.sonarqube.as_ref().unwrap().token_env,
            "MY_SONAR_TOKEN"
        );
    }

    #[test]
    fn parse_input_mode_stdin() {
        let yaml = r#"
binary:
  path: app
  input_mode: stdin
"#;
        let config: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(config.binary.input_mode, InputMode::Stdin));
    }

    #[test]
    fn parse_input_mode_env() {
        let yaml = r#"
binary:
  path: app
  input_mode: "env:INPUT_FILE"
"#;
        let config: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        match config.binary.input_mode {
            InputMode::Env(ref var) => assert_eq!(var, "INPUT_FILE"),
            other => panic!("expected Env, got {other:?}"),
        }
    }

    #[test]
    fn parse_input_mode_invalid() {
        let yaml = r#"
binary:
  path: app
  input_mode: "foobar"
"#;
        let result: Result<ServerConfig, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn tool_config_from_server_config() {
        let yaml = r#"
binary:
  path: app
tools:
  run:
    image: run:1
  valgrind:
    image: valgrind:1
"#;
        let config: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        let tc = ToolConfig::from_server_config(&config);
        assert_eq!(tc.run_image.as_deref(), Some("run:1"));
        assert_eq!(tc.valgrind_image.as_deref(), Some("valgrind:1"));
        assert!(tc.coverage_image.is_none());
        assert!(tc.sonar_url.is_none());
        assert!(tc.static_image.is_none());
    }
}
