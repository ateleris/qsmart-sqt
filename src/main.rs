use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod config;

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
        }
    }

    Ok(())
}
