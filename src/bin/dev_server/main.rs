use std::net::SocketAddr;
use std::path::PathBuf;

use axum::Router;
use axum::routing::{get, post};
use clap::Parser;

mod analysis_job;
mod docker;
mod handlers;
mod run_job;
mod store;
mod tools;

#[derive(Parser)]
#[command(
    name = "sqt-dev-server",
    version,
    about = "Local job runner for SQT development (executes tools in Docker containers)"
)]
struct Cli {
    /// Port to listen on
    #[arg(short, long, default_value = "3000")]
    port: u16,

    /// Host to bind to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Workspace directory for evidence and tool output
    #[arg(long, default_value = "/tmp/sqt-dev-server")]
    workspace: PathBuf,

    /// Docker image for binary execution (required for run jobs)
    #[arg(long)]
    run_image: String,

    /// Docker image with valgrind installed (optional; enables memory.leaks)
    #[arg(long)]
    valgrind_image: Option<String>,

    /// Docker image with lcov/gcov installed (optional; enables coverage.line_pct)
    #[arg(long)]
    coverage_image: Option<String>,

    /// SonarQube server URL (optional; enables sonar.* metrics)
    #[arg(long)]
    sonar_url: Option<String>,

    /// Name of the environment variable holding the SonarQube token
    #[arg(long, default_value = "SONAR_TOKEN")]
    sonar_token_env: String,

    /// Docker image with cppcheck/clang-tidy (optional; enables static.warnings)
    #[arg(long)]
    static_image: Option<String>,
}

/// Configuration for which tool images are available.
#[derive(Clone)]
pub struct ToolConfig {
    pub run_image: String,
    pub valgrind_image: Option<String>,
    pub coverage_image: Option<String>,
    pub sonar_url: Option<String>,
    pub sonar_token: Option<String>,
    pub static_image: Option<String>,
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
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

    // Resolve SonarQube token from environment
    let sonar_token = cli
        .sonar_url
        .as_ref()
        .and_then(|_| std::env::var(&cli.sonar_token_env).ok());

    let tool_config = ToolConfig {
        run_image: cli.run_image.clone(),
        valgrind_image: cli.valgrind_image.clone(),
        coverage_image: cli.coverage_image.clone(),
        sonar_url: cli.sonar_url.clone(),
        sonar_token,
        static_image: cli.static_image.clone(),
    };

    // Log available tools
    tracing::info!("run image: {}", tool_config.run_image);
    if let Some(ref img) = tool_config.valgrind_image {
        tracing::info!("valgrind image: {img}");
    }
    if let Some(ref img) = tool_config.coverage_image {
        tracing::info!("coverage image: {img}");
    }
    if let Some(ref url) = tool_config.sonar_url {
        tracing::info!("sonarqube URL: {url}");
    }
    if let Some(ref img) = tool_config.static_image {
        tracing::info!("static analysis image: {img}");
    }

    // Create workspace directory
    std::fs::create_dir_all(&cli.workspace).expect("failed to create workspace directory");

    let state = handlers::AppState {
        store: store::new_store(),
        workspace: cli.workspace,
        tool_config: std::sync::Arc::new(tool_config),
    };

    let app = Router::new()
        .route("/api/v1/runs", post(handlers::create_run))
        .route("/api/v1/runs/{run_id}", get(handlers::get_run))
        .route("/api/v1/analysis", post(handlers::create_analysis))
        .route(
            "/api/v1/analysis/{analysis_id}",
            get(handlers::get_analysis),
        )
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", cli.host, cli.port)
        .parse()
        .expect("invalid host:port");

    tracing::info!("sqt-dev-server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
