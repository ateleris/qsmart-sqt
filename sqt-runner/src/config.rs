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

    /// Valgrind configuration (optional; enables `memory.leaks_bytes`).
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

/// Build configuration for compiling the main program source against the ML-L
/// engine shared library.
///
/// When present, the runner compiles the source file before each job run.
/// Omit entirely when using a pre-built binary (the common case during
/// development).
#[derive(Debug, Clone, Deserialize)]
pub struct BuildConfig {
    /// Docker image that provides the compiler and toolchain (e.g.
    /// `gcc:14-bookworm`).
    pub image: String,

    /// Absolute path to the main program source file (.cpp / .c).
    pub source: String,

    /// Absolute path to the ML-L engine shared library (.so) to link against.
    /// The library is also copied into the workspace so the binary can find it
    /// at runtime via `LD_LIBRARY_PATH`.
    pub engine_lib: String,

    /// Compiler executable (default: `g++`).
    #[serde(default = "default_compiler")]
    pub compiler: String,

    /// Extra compiler/linker flags appended after instrumentation flags.
    #[serde(default)]
    pub extra_flags: Vec<String>,

    /// Additional directories added as `-I` include paths.
    #[serde(default)]
    pub include_dirs: Vec<String>,

    /// Maximum time allowed for the compile step in seconds (default: 300).
    #[serde(default)]
    pub compile_timeout_secs: Option<u64>,
}

fn default_compiler() -> String {
    "g++".into()
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

    /// Build configuration for compiling the main program source.
    ///
    /// When set, the runner compiles the source before each job and links it
    /// against the ML-L engine shared library. When absent, `binary.path`
    /// must point to a pre-built executable.
    #[serde(default)]
    pub build: Option<BuildConfig>,

    /// Description of the target hardware.
    ///
    /// Returned verbatim in `JobStatusResponse.hardware` so the reporter can
    /// record which device produced each result.
    #[serde(default)]
    pub hardware: sqt_api_types::HardwareInfo,
}

fn default_workspace() -> PathBuf {
    PathBuf::from("/tmp/sqt-dev-server")
}

impl ServerConfig {
    /// Load configuration from a YAML file.
    ///
    /// Relative paths in `binary.path`, `workspace`, and any `build.*` paths
    /// are resolved against the current working directory. Docker requires
    /// absolute paths for all volume mounts and working directories.
    pub fn load(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config file: {}", path.display()))?;
        let mut config: Self = serde_yaml::from_str(&contents)
            .with_context(|| format!("failed to parse config file: {}", path.display()))?;

        let cwd = std::env::current_dir().context("failed to determine current directory")?;

        // Resolve binary path to absolute.
        let binary_path = PathBuf::from(&config.binary.path);
        if binary_path.is_relative() {
            config.binary.path = cwd.join(&binary_path).display().to_string();
        }

        // Resolve workspace to absolute.
        if config.workspace.is_relative() {
            config.workspace = cwd.join(&config.workspace);
        }

        // Resolve build paths to absolute (Docker bind mounts require absolute paths).
        if let Some(ref mut build) = config.build {
            let source = PathBuf::from(&build.source);
            if source.is_relative() {
                build.source = cwd.join(&source).display().to_string();
            }

            let engine_lib = PathBuf::from(&build.engine_lib);
            if engine_lib.is_relative() {
                build.engine_lib = cwd.join(&engine_lib).display().to_string();
            }

            for inc in &mut build.include_dirs {
                let p = PathBuf::from(&*inc);
                if p.is_relative() {
                    *inc = cwd.join(&p).display().to_string();
                }
            }
        }

        Ok(config)
    }
}

/// Flattened tool configuration used by job executors.
///
/// Built from `ServerConfig` at startup - this is the internal representation
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
