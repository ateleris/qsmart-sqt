use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use clap::Parser;
use tokio::sync::Semaphore;

mod config;
mod docker;
mod executor;
mod handlers;
mod store;
mod tools;

use config::{ServerConfig, ToolConfig};
use executor::docker::DockerExecutor;

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

    if let Some(ref build) = server_config.build {
        tracing::info!("build image: {}", build.image);
        tracing::info!("build source: {}", build.source);
        tracing::info!("build engine_lib: {}", build.engine_lib);
    }

    if let Some(ref docs) = server_config.docs {
        tracing::info!("docs directory: {}", docs.directory.display());
    } else {
        tracing::info!("docs directory: not configured (GET /api/v1/docs will return 404)");
    }

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

    // --- Preflight checks (fail fast) ----------------------------------------
    tracing::info!("running preflight checks...");

    docker::check_docker().expect("preflight: Docker check failed");

    // Check all configured images exist
    let images: Vec<(&str, &str)> = [
        (tool_config.run_image.as_deref(), "run"),
        (tool_config.valgrind_image.as_deref(), "valgrind"),
        (tool_config.coverage_image.as_deref(), "coverage"),
        (tool_config.static_image.as_deref(), "static_analysis"),
        (server_config.build.as_ref().map(|b| b.image.as_str()), "build"),
    ]
    .into_iter()
    .filter_map(|(img, name)| img.map(|i| (i, name)))
    .collect();

    for (image, name) in &images {
        docker::check_image(image)
            .unwrap_or_else(|e| panic!("preflight: {name} image check failed: {e}"));
        tracing::info!("{name} image OK: {image}");
    }

    // Check binary is executable inside the run image (only for pre-built binaries)
    if server_config.build.is_none() {
        if let Some(ref run_image) = tool_config.run_image {
            docker::check_binary_in_image(run_image, &server_config.binary.path)
                .expect("preflight: binary check failed");
            tracing::info!(
                "binary OK: {} (in {})",
                server_config.binary.path,
                run_image
            );
        }
    }

    // Check docs directory exists and is readable if configured
    if let Some(ref docs) = server_config.docs {
        if !docs.directory.is_dir() {
            panic!(
                "preflight: docs directory not found or not a directory: {} — \
                 ensure the ML-L Provider has pre-placed documents there",
                docs.directory.display()
            );
        }
        let n = std::fs::read_dir(&docs.directory)
            .expect("preflight: cannot read docs directory")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .count();
        tracing::info!("docs directory OK: {} ({n} file(s))", docs.directory.display());
    }

    tracing::info!("preflight checks passed");

    // Create workspace root directory (per-job subdirs are created at job start)
    std::fs::create_dir_all(&server_config.workspace)
        .expect("failed to create workspace directory");

    let executor = Arc::new(DockerExecutor::new(
        server_config.binary,
        server_config.build,
        tool_config,
        server_config.hardware,
    ));

    let state = handlers::AppState {
        store: store::new_store(),
        workspace: server_config.workspace,
        executor,
        job_semaphore: Arc::new(Semaphore::new(1)),
        docs_dir: server_config.docs.map(|d| d.directory),
    };

    let app = Router::new()
        .route("/api/v1/jobs", post(handlers::create_job))
        .route("/api/v1/jobs/{job_id}", get(handlers::get_job))
        .route("/api/v1/docs", get(handlers::get_docs))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", cli.host, cli.port)
        .parse()
        .expect("invalid host:port");

    tracing::info!("sqt-dev-server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
