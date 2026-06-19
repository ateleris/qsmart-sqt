use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use sqt_reporter::{api, config, orchestrator, preflight, report, specs};

#[derive(Parser)]
#[command(name = "sqt", version, about = "Software Qualification Toolkit")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a full qualification session against the SQT Runner.
    Run {
        #[arg(short, long, env = "SQT_CONFIG")]
        config: PathBuf,
        #[arg(short, long, help = "Write JSON report to this path")]
        output: Option<PathBuf>,
        #[arg(
            long,
            help = "Validate config and Runner connectivity; exit without submitting jobs"
        )]
        preflight: bool,
    },
    /// Validate config, check Runner connectivity and auth, load spec files —
    /// without submitting any jobs (SRS-OPS-007).
    ///
    /// Equivalent to `sqt run --preflight` but available as a first-class
    /// subcommand for CI pipelines that want a dedicated pre-flight step.
    Preflight {
        #[arg(short, long, env = "SQT_CONFIG", help = "Path to sqt-reporter.yaml")]
        config: PathBuf,
    },
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Preflight { config } => {
            let cfg = config::load_config(&config)?;
            cfg.print_summary();

            let token_val = cfg.runner.token_env.as_ref().and_then(|var| std::env::var(var).ok());
            let client = api::ApiClient::new(
                &cfg.runner.url,
                token_val.as_deref(),
                cfg.runner.retry_max,
                cfg.runner.retry_backoff_initial_secs,
            )?;

            match preflight::preflight(&cfg, &client).await {
                Ok(()) => {
                    tracing::info!("Pre-flight OK");
                }
                Err(e) => {
                    eprintln!("Pre-flight FAILED: {e}");
                    std::process::exit(1);
                }
            }
        }

        Commands::Run { config, output, preflight: do_preflight } => {
            let cfg = config::load_config(&config)?;
            cfg.print_summary();

            let specs =
                specs::loader::load_specs(&cfg.test_specs.files, cfg.runner.job_timeout_secs)?;
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
            let token_val = cfg.runner.token_env.as_ref().and_then(|var| std::env::var(var).ok());
            let client = api::ApiClient::new(
                &cfg.runner.url,
                token_val.as_deref(),
                cfg.runner.retry_max,
                cfg.runner.retry_backoff_initial_secs,
            )?;

            if do_preflight {
                match preflight::preflight(&cfg, &client).await {
                    Ok(()) => {
                        tracing::info!("Pre-flight OK");
                        return Ok(());
                    }
                    Err(e) => {
                        eprintln!("Pre-flight FAILED: {e}");
                        std::process::exit(1);
                    }
                }
            }

            let evidence_dir = cfg.report.output_dir.join("evidence");
            let mut ev_store = sqt_reporter::evidence::EvidenceStore::new(evidence_dir.clone())?;

            let (result, provenance) =
                orchestrator::run(&specs, &client, &cfg, token, &mut ev_store).await?;

            let rep = report::Report::from_run_result(&result, provenance);
            report::print_terminal(&rep);

            if let Some(ref path) = output {
                report::write_json(&rep, path)?;
                tracing::info!("Report written to {}", path.display());
            }

            // Traceability matrix (if baseline was available)
            if let Some(ref req_set) = result.requirement_set {
                let matrix = sqt_reporter::traceability::build(req_set, &result, &rep.timestamp);
                let t_json = cfg.report.output_dir.join("traceability.json");
                let t_txt = cfg.report.output_dir.join("traceability.txt");
                sqt_reporter::traceability::write_json(&matrix, &t_json)?;
                sqt_reporter::traceability::write_text(&matrix, &t_txt)?;
                tracing::info!(
                    "Traceability matrix written to {}",
                    cfg.report.output_dir.display()
                );
            }

            // Bundle all artefacts into qdp.zip (T-055).
            let qdp_path = report::write_qdp(
                &cfg.report.output_dir,
                output.as_deref(),
                Some(&cfg.report.output_dir.join("traceability.json")),
                Some(&cfg.report.output_dir.join("traceability.txt")),
                Some(&evidence_dir),
            )?;
            tracing::info!("QDP bundle: {}", qdp_path.display());

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
        let mut sig = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        sig.recv().await;
    }
    #[cfg(not(unix))]
    {
        std::future::pending::<()>().await;
    }
}
