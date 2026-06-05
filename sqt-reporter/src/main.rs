use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use sqt_reporter::{api, config, orchestrator, report, specs};

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

            // ---------------------------------------------------------------
            // Graceful shutdown: SIGTERM + Ctrl-C (SRS-OPS-008)
            // ---------------------------------------------------------------
            let token = tokio_util::sync::CancellationToken::new();
            let token_clone = token.clone();
            tokio::spawn(async move {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {},
                    () = wait_for_sigterm() => {},
                }
                token_clone.cancel();
                tracing::warn!("Shutdown signal received -- draining in-flight jobs");
            });

            // Read API token from the env var named in config.
            // Note: validate() already checked the var is non-empty if token_env is set.
            let token_val = cfg
                .runner
                .token_env
                .as_ref()
                .and_then(|var| std::env::var(var).ok());
            let client = api::ApiClient::new(
                &cfg.runner.url,
                token_val.as_deref(),
                cfg.runner.retry_max,
                cfg.runner.retry_backoff_initial_secs,
            )?;

            let (result, provenance) = orchestrator::run(&specs, &client, &cfg, token).await?;

            let rep = report::Report::from_run_result(&result, provenance);
            report::print_terminal(&rep);

            if let Some(path) = output {
                report::write_json(&rep, &path)?;
                tracing::info!("Report written to {}", path.display());
            }

            // Exit code 2 for interrupted, 1 for any failure, 0 for all-pass.
            if result.interrupted {
                std::process::exit(2);
            }
            if rep.summary.failed > 0 {
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

/// Returns when SIGTERM is received (Unix only).  On non-Unix targets this
/// future never resolves, so the `tokio::select!` in the signal handler
/// simply ignores this branch.
async fn wait_for_sigterm() {
    #[cfg(unix)]
    {
        let mut sig = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        )
        .expect("failed to install SIGTERM handler");
        sig.recv().await;
    }
    #[cfg(not(unix))]
    {
        std::future::pending::<()>().await;
    }
}
