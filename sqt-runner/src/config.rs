use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// How input files are passed to the binary under test.
#[derive(Debug, Clone, Default)]
pub enum InputMode {
    /// Pipe input file contents to stdin (default).
    #[default]
    Stdin,
    /// Pass input file path as the last CLI argument.
    Arg,
    /// Set an environment variable to the input file path.
    Env(String),
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

    /// `SonarQube` configuration (optional; enables `sonar.*` metrics).
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
    /// `SonarQube` server URL.
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

/// Configuration for the `GET /api/v1/docs` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct DocsConfig {
    /// Directory containing qualification documents pre-placed by the ML-L Provider.
    ///
    /// The runner scans this directory (non-recursively) at request time:
    /// - `*.sdoc` files → `documents[]` (served as raw UTF-8 text)
    /// - all other files → `attachments[]` (served as base64-encoded blobs)
    pub directory: PathBuf,
}

/// Hardware identity configuration for the `GET /api/v1/version` endpoint.
///
/// Distinct from `sqt_types::HardwareInfo` (which describes hardware for
/// `JobStatusResponse`). This section carries identity and toolchain metadata
/// that the runner reports via the version endpoint.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HardwareConfig {
    /// Short identifier for the target hardware, e.g. `"gr740-leon4"`.
    /// Used in `VersionResponse.target_id`.
    #[serde(default)]
    pub target_id: String,

    /// Versions of key tools/runtimes available on the runner,
    /// e.g. `{"onnxruntime": "1.17"}`. Used in `VersionResponse.toolchain_versions`.
    #[serde(default)]
    pub toolchain_versions: std::collections::HashMap<String, String>,
}

/// Which execution backend the runner drives.
///
/// Selected by the top-level `backend:` key. Backend-specific settings live in
/// a section named after the variant (currently only `docker:`). Adding a new
/// backend is a new variant here plus a match arm in `main.rs`'s factory.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// Run workloads in Docker containers (the default).
    #[default]
    Docker,
}

/// Settings specific to the Docker execution backend (the `docker:` section).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DockerBackendConfig {
    /// Per-tool Docker image configuration.
    #[serde(default)]
    pub tools: ToolsConfig,

    /// Build configuration for compiling the main program source.
    ///
    /// When set, the runner compiles the source before each job and links it
    /// against the ML-L engine shared library. When absent, `binary.path`
    /// must point to a pre-built executable.
    #[serde(default)]
    pub build: Option<BuildConfig>,
}

/// Top-level server configuration loaded from a YAML file.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Which execution backend to use.
    #[serde(default)]
    pub backend: Backend,

    /// Binary under test configuration.
    pub binary: BinaryConfig,

    /// Workspace directory for evidence and tool output.
    #[serde(default = "default_workspace")]
    pub workspace: PathBuf,

    /// Docker backend settings (tools, build step).
    #[serde(default)]
    pub docker: DockerBackendConfig,

    /// Description of the target hardware.
    ///
    /// Returned verbatim in `JobStatusResponse.hardware` so the reporter can
    /// record which device produced each result.
    #[serde(default)]
    pub hardware: sqt_types::HardwareInfo,

    /// Runner identity and toolchain metadata for `GET /api/v1/version`.
    #[serde(default)]
    pub runner_hardware: HardwareConfig,

    /// Qualification documents served via `GET /api/v1/docs`.
    ///
    /// When absent the endpoint returns 404. Configure this to point to the
    /// zip file pre-placed by the ML-L Provider before a qualification session.
    #[serde(default)]
    pub docs: Option<DocsConfig>,
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

        // Resolve docs directory path to absolute.
        if let Some(ref mut docs) = config.docs
            && docs.directory.is_relative()
        {
            docs.directory = cwd.join(&docs.directory);
        }

        // Resolve build paths to absolute (Docker bind mounts require absolute paths).
        if let Some(ref mut build) = config.docker.build {
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
    pub fn from_tools(tools: &ToolsConfig) -> Self {
        let sonar_token =
            tools.sonarqube.as_ref().and_then(|s| std::env::var(&s.token_env).ok());

        Self {
            run_image: tools.run.as_ref().map(|r| r.image.clone()),
            valgrind_image: tools.valgrind.as_ref().map(|v| v.image.clone()),
            coverage_image: tools.coverage.as_ref().map(|c| c.image.clone()),
            sonar_url: tools.sonarqube.as_ref().map(|s| s.url.clone()),
            sonar_token,
            static_image: tools.static_analysis.as_ref().map(|s| s.image.clone()),
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
docker:
  tools:
    run:
      image: sqt-run:latest
"#;
        let config: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.binary.path, "target/debug/my-app");
        assert!(config.binary.args.is_empty());
        assert!(config.binary.env.is_empty());
        assert!(matches!(config.binary.input_mode, InputMode::Stdin));
        assert!(matches!(config.backend, Backend::Docker));
        assert_eq!(config.workspace, PathBuf::from("/tmp/sqt-dev-server"));
        assert_eq!(config.docker.tools.run.as_ref().unwrap().image, "sqt-run:latest");
        assert!(config.docker.tools.valgrind.is_none());
    }

    #[test]
    fn parse_full_config() {
        let yaml = r#"
backend: docker

binary:
  path: target/debug/my-app
  args: ["--model", "resnet50"]
  env:
    LD_LIBRARY_PATH: /opt/libs
    MODEL_DIR: /data/models
  input_mode: arg

workspace: /var/sqt/workspace

docker:
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
        assert!(config.docker.tools.valgrind.is_some());
        assert!(config.docker.tools.coverage.is_some());
        assert_eq!(config.docker.tools.sonarqube.as_ref().unwrap().token_env, "MY_SONAR_TOKEN");
    }

    #[test]
    fn backend_defaults_to_docker_when_absent() {
        let yaml = "binary:\n  path: app\n";
        let config: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(config.backend, Backend::Docker));
    }

    #[test]
    fn backend_unknown_value_is_error() {
        let yaml = "backend: labgrid\nbinary:\n  path: app\n";
        let result: Result<ServerConfig, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err(), "unknown backend should fail to parse until a variant exists");
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
    fn tool_config_from_tools() {
        let yaml = r#"
binary:
  path: app
docker:
  tools:
    run:
      image: run:1
    valgrind:
      image: valgrind:1
"#;
        let config: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        let tc = ToolConfig::from_tools(&config.docker.tools);
        assert_eq!(tc.run_image.as_deref(), Some("run:1"));
        assert_eq!(tc.valgrind_image.as_deref(), Some("valgrind:1"));
        assert!(tc.coverage_image.is_none());
        assert!(tc.sonar_url.is_none());
        assert!(tc.static_image.is_none());
    }

    #[test]
    fn tool_config_reads_sonar_token_from_env() {
        let yaml = r#"
binary:
  path: app
docker:
  tools:
    run:
      image: run:1
    sonarqube:
      url: http://sonar:9000
      token_env: MY_SONAR_TOKEN
"#;
        // SAFETY: test-only, single-threaded context
        unsafe { std::env::set_var("MY_SONAR_TOKEN", "secret-token") };
        let config: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        let tc = ToolConfig::from_tools(&config.docker.tools);
        assert_eq!(tc.sonar_url.as_deref(), Some("http://sonar:9000"));
        assert_eq!(tc.sonar_token.as_deref(), Some("secret-token"));
        unsafe { std::env::remove_var("MY_SONAR_TOKEN") };
    }

    #[test]
    fn tool_config_sonar_token_absent_when_env_unset() {
        let yaml = r#"
binary:
  path: app
docker:
  tools:
    run:
      image: run:1
    sonarqube:
      url: http://sonar:9000
"#;
        // SAFETY: test-only, single-threaded context
        unsafe { std::env::remove_var("SONAR_TOKEN") };
        let config: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        let tc = ToolConfig::from_tools(&config.docker.tools);
        assert!(tc.sonar_token.is_none());
    }

    #[test]
    fn parse_env_input_mode_empty_var_name_is_error() {
        let yaml = r#"
binary:
  path: app
  input_mode: "env:"
"#;
        let result: Result<ServerConfig, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn server_config_load_resolves_relative_paths() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("sqt-runner.yaml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
binary:
  path: artifacts/ml_l_inference
workspace: workspace
docker:
  tools:
    run:
      image: sqt-run:latest
"#
        )
        .unwrap();

        let config = ServerConfig::load(&config_path).unwrap();

        assert!(config.binary.path.starts_with('/'), "binary path must be absolute");
        assert!(config.workspace.is_absolute(), "workspace must be absolute");
    }

    #[test]
    fn server_config_load_resolves_build_paths() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("sqt-runner.yaml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
binary:
  path: /bin/app
docker:
  tools:
    run:
      image: sqt-run:latest
  build:
    image: gcc:14
    source: src/main.cpp
    engine_lib: lib/libengine.so
    include_dirs:
      - include/
"#
        )
        .unwrap();

        let config = ServerConfig::load(&config_path).unwrap();
        let build = config.docker.build.unwrap();

        assert!(PathBuf::from(&build.source).is_absolute(), "build.source must be absolute");
        assert!(
            PathBuf::from(&build.engine_lib).is_absolute(),
            "build.engine_lib must be absolute"
        );
        assert!(
            PathBuf::from(&build.include_dirs[0]).is_absolute(),
            "include_dirs[0] must be absolute"
        );
    }

    #[test]
    fn server_config_load_resolves_docs_directory() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("sqt-runner.yaml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
binary:
  path: /bin/app
docker:
  tools:
    run:
      image: sqt-run:latest
docs:
  directory: docs/
"#
        )
        .unwrap();

        let config = ServerConfig::load(&config_path).unwrap();
        let docs = config.docs.unwrap();

        assert!(docs.directory.is_absolute(), "docs.directory must be absolute");
    }

    #[test]
    fn server_config_load_keeps_absolute_paths_unchanged() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("sqt-runner.yaml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
binary:
  path: /absolute/path/app
workspace: /absolute/workspace
docker:
  tools:
    run:
      image: sqt-run:latest
"#
        )
        .unwrap();

        let config = ServerConfig::load(&config_path).unwrap();

        assert_eq!(config.binary.path, "/absolute/path/app");
        assert_eq!(config.workspace, PathBuf::from("/absolute/workspace"));
    }
}
