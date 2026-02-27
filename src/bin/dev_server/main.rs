use std::net::SocketAddr;
use std::path::PathBuf;

use axum::Router;
use axum::routing::{get, post};
use clap::Parser;

mod analysis_job;
mod config;
mod docker;
mod handlers;
mod run_job;
mod store;
mod tools;

use config::{ServerConfig, ToolConfig};

#[derive(Parser)]
#[command(
    name = "sqt-dev-server",
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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    let server_config =
        ServerConfig::load(&cli.config).expect("failed to load configuration file");

    let tool_config = ToolConfig::from_server_config(&server_config);

    // Log binary and available tools
    tracing::info!("binary under test: {}", server_config.binary.path);
    if !server_config.binary.args.is_empty() {
        tracing::info!("binary args: {:?}", server_config.binary.args);
    }
    if !server_config.binary.env.is_empty() {
        tracing::info!("binary env: {:?}", server_config.binary.env);
    }
    tracing::info!("input mode: {:?}", server_config.binary.input_mode);

    if let Some(ref img) = tool_config.run_image {
        tracing::info!("run image: {img}");
    }
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
    std::fs::create_dir_all(&server_config.workspace)
        .expect("failed to create workspace directory");

    let state = handlers::AppState {
        store: store::new_store(),
        workspace: server_config.workspace.clone(),
        binary_config: std::sync::Arc::new(server_config.binary),
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
