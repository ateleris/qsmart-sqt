use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use sqt::api;

mod config;
mod constraint;
mod metrics;
mod orchestrator;
mod report;
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
        #[arg(short, long, help = "Write JSON report to this path")]
        output: Option<PathBuf>,
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
        Commands::Run { config, output } => {
            let cfg = config::load_config(&config)?;
            cfg.print_summary();

            let specs = specs::all_specs();
            if specs.is_empty() {
                tracing::warn!("No specs defined - nothing to run");
                return Ok(());
            }

            // Read API token from the env var named in config.
            let token = cfg
                .api
                .token_env
                .as_ref()
                .and_then(|var| std::env::var(var).ok());
            let client = api::ApiClient::new(&cfg.api.url, token.as_deref())?;

            let result = orchestrator::run(&specs, &client, &cfg).await?;

            let rep = report::Report::from_run_result(&result);
            report::print_terminal(&rep);

            if let Some(path) = output {
                report::write_json(&rep, &path)?;
                tracing::info!("Report written to {}", path.display());
            }

            if rep.summary.failed > 0 {
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
