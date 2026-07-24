use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use clap::Parser;
use tokio::sync::Semaphore;

mod auth;
mod config;
mod docker;
mod executor;
mod handlers;
mod store;
mod tools;

use config::{
    AuthConfig, Backend, BinaryConfig, DockerBackendConfig, DocsConfig, HardwareConfig,
    ServerConfig, ToolConfig,
};
use executor::ExecutionBackend;
use executor::docker::DockerExecutor;
use handlers::get_version;
use sqt_types::HardwareInfo;

#[derive(Parser)]
#[command(
    name = "sqt-runner",
    version,
    about = "Local job runner for SQT development (executes tools in Docker containers)"
)]
struct Cli {
    /// Path to the YAML configuration file
    #[arg(short, long)]
    config: PathBuf,

    /// Port to listen on (overrides config)
    #[arg(short, long, default_value = "3000")]
    port: u16,

    /// Host to bind to (overrides config)
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Run startup readiness checks: the execution backend's own preflight (Docker
/// daemon, images, binary) plus the core's docs-directory check, which is
/// backend-independent.
fn run_preflight_checks(executor: &dyn ExecutionBackend, docs: Option<&DocsConfig>) {
    tracing::info!("running preflight checks...");

    executor.preflight().expect("preflight: backend check failed");

    if let Some(docs) = docs {
        assert!(
            docs.directory.is_dir(),
            "preflight: docs directory not found or not a directory: {} — \
             ensure the ML-L Provider has pre-placed documents there",
            docs.directory.display()
        );
        let n = std::fs::read_dir(&docs.directory)
            .expect("preflight: cannot read docs directory")
            .filter_map(std::result::Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .count();
        tracing::info!("docs directory OK: {} ({n} file(s))", docs.directory.display());
    }

    tracing::info!("preflight checks passed");
}

/// Construct the execution backend selected by `backend:`. Adding a backend is
/// a new match arm here (plus a `Backend` variant and its config section).
fn build_executor(
    backend: Backend,
    binary: BinaryConfig,
    docker: DockerBackendConfig,
    hardware: HardwareInfo,
    runner_hardware: HardwareConfig,
) -> Arc<dyn ExecutionBackend> {
    match backend {
        Backend::Docker => {
            let tool_config = ToolConfig::from_tools(&docker.tools);
            Arc::new(DockerExecutor::new(
                binary,
                docker.build,
                tool_config,
                hardware,
                runner_hardware.target_id,
                runner_hardware.toolchain_versions,
            ))
        }
    }
}

/// Resolve the expected bearer token from the auth config (SRS-IF-006).
///
/// Returns `None` when authentication is disabled (no `auth:` section), logging
/// a prominent warning. Aborts startup if auth is configured but the named
/// environment variable is unset or empty — a misconfiguration that would
/// otherwise silently leave every endpoint open.
fn resolve_auth_token(auth: Option<&AuthConfig>) -> Option<String> {
    let Some(cfg) = auth else {
        tracing::warn!(
            "bearer authentication DISABLED — no `auth:` section in config; \
             all endpoints except /health are unauthenticated (SRS-IF-006)"
        );
        return None;
    };

    let token = std::env::var(&cfg.token_env).ok().filter(|t| !t.is_empty()).unwrap_or_else(|| {
        panic!(
            "auth is configured (auth.token_env = {}) but that environment \
                 variable is unset or empty — set it or remove the auth section",
            cfg.token_env
        )
    });
    tracing::info!("bearer authentication enabled (token from ${})", cfg.token_env);
    Some(token)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    let server_config = ServerConfig::load(&cli.config).expect("failed to load configuration file");

    tracing::info!("backend: {:?}", server_config.backend);
    tracing::info!("binary under test: {}", server_config.binary.path);
    if !server_config.binary.args.is_empty() {
        tracing::info!("binary args: {:?}", server_config.binary.args);
    }
    if !server_config.binary.env.is_empty() {
        tracing::info!("binary env: {:?}", server_config.binary.env);
    }
    tracing::info!("input mode: {:?}", server_config.binary.input_mode);

    if let Some(ref build) = server_config.docker.build {
        tracing::info!("build image: {}", build.image);
        tracing::info!("build source: {}", build.source);
        tracing::info!("build engine_lib: {}", build.engine_lib);
    }

    if let Some(ref docs) = server_config.docs {
        tracing::info!("docs directory: {}", docs.directory.display());
    } else {
        tracing::info!("docs directory: not configured (GET /api/v1/docs will return 404)");
    }

    let tools = &server_config.docker.tools;
    if let Some(ref t) = tools.run {
        tracing::info!("run image: {}", t.image);
    }
    if let Some(ref t) = tools.valgrind {
        tracing::info!("valgrind image: {}", t.image);
    }
    if let Some(ref t) = tools.coverage {
        tracing::info!("coverage image: {}", t.image);
    }
    if let Some(ref t) = tools.sonarqube {
        tracing::info!("sonarqube URL: {}", t.url);
    }
    if let Some(ref t) = tools.static_analysis {
        tracing::info!("static analysis image: {}", t.image);
    }

    let auth_token = resolve_auth_token(server_config.auth.as_ref());

    let executor = build_executor(
        server_config.backend,
        server_config.binary,
        server_config.docker,
        server_config.hardware,
        server_config.runner_hardware,
    );

    run_preflight_checks(executor.as_ref(), server_config.docs.as_ref());

    // Create workspace root directory (per-job subdirs are created at job start)
    std::fs::create_dir_all(&server_config.workspace)
        .expect("failed to create workspace directory");

    let state = handlers::AppState {
        store: store::new_store(),
        workspace: server_config.workspace,
        executor,
        job_semaphore: Arc::new(Semaphore::new(1)),
        docs_dir: server_config.docs.map(|d| d.directory),
        runner_version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
    };

    // All routes require bearer auth (SRS-IF-006) except /health, the
    // unauthenticated liveness probe. When no token is configured the auth
    // layer is omitted (a warning was already logged by resolve_auth_token).
    let mut protected = Router::new()
        .route("/api/v1/jobs", post(handlers::create_job))
        .route("/api/v1/jobs/{job_id}", get(handlers::get_job))
        .route("/api/v1/docs", get(handlers::get_docs))
        .route("/api/v1/version", get(get_version));
    if let Some(token) = auth_token {
        protected = protected.route_layer(axum::middleware::from_fn_with_state(
            Arc::new(token),
            auth::require_bearer,
        ));
    }

    let app = Router::new()
        .route("/api/v1/health", get(handlers::get_health))
        .merge(protected)
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", cli.host, cli.port).parse().expect("invalid host:port");

    tracing::info!("sqt-runner listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
