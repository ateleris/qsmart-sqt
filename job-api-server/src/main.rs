mod handlers;
mod state;
mod types;

use axum::{Router, routing::{get, post}};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let store = state::new_store();

    let app = Router::new()
        .route("/api/v1/docs",           get(handlers::get_docs))
        .route("/api/v1/jobs",           post(handlers::submit_job))
        .route("/api/v1/jobs/{job_id}",  get(handlers::get_job))
        .with_state(store);

    let addr = "0.0.0.0:8080";
    tracing::info!("sqt-job-api-server listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
