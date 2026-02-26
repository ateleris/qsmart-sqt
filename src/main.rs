use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod api;
mod config;
mod constraint;
mod metrics;
mod orchestrator;
mod specs;

#[derive(Parser)]
#[command(name = "sqt", version, about = "Software Qualification Toolkit")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(short, long, env = "SQT_CONFIG")]
        config: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run { config } => {
            let cfg = config::load_config(&config)?;
            cfg.print_summary();

            let specs = specs::all_specs();
            if specs.is_empty() {
                tracing::warn!("No specs defined — nothing to run");
                return Ok(());
            }

            // Read API token from the env var named in config.
            let token = cfg
                .api
                .token_env
                .as_ref()
                .and_then(|var| std::env::var(var).ok());
            let client = api::JobApiClient::new(&cfg.api.url, token.as_deref())?;

            let result = orchestrator::run(&specs, &client, &cfg).await?;

            let total = result.results.len();
            let passed = result.results.iter().filter(|r| r.passed).count();
            let failed = total - passed;
            tracing::info!("{passed}/{total} specs passed, {failed} failed");

            if failed > 0 {
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
