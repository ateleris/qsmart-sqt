// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use sqt_reporter::{api, config, evidence, orchestrator, outputs, preflight, report, specs};

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
        #[arg(
            short,
            long,
            help = "Also copy the JSON report to this path (it is always written into the run directory)"
        )]
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
            let client =
                api::ApiClient::new(&cfg.runner.url, token_val.as_deref(), (&cfg.runner).into())?;

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
            let client =
                api::ApiClient::new(&cfg.runner.url, token_val.as_deref(), (&cfg.runner).into())?;

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

            // Everything this run writes goes into its own directory (SRS-FUNC-008).
            let run_dir = evidence::RunDir::create(&cfg.report.output_dir)?;
            let mut ev_store = evidence::EvidenceStore::new(run_dir)?;

            let (mut result, provenance) =
                match orchestrator::run(&specs, &client, &cfg, token, &mut ev_store).await {
                    Ok(run) => run,
                    Err(e) => {
                        // Keep what the Runner sent before the run stopped.
                        orchestrator::persist_runner_responses(&client, &mut ev_store);
                        if let Err(me) = ev_store.write_manifest() {
                            tracing::warn!("cannot write evidence manifest: {me}");
                        }
                        eprintln!("Run failed; evidence in {}", ev_store.run().path.display());
                        return Err(e);
                    }
                };
            orchestrator::persist_runner_responses(&client, &mut ev_store);

            let (rep, written) = outputs::write_run_outputs(
                &mut result,
                provenance,
                &mut ev_store,
                output.as_deref(),
            )?;
            report::print_terminal(&rep);
            println!(
                "Run {} — report and evidence in {}",
                ev_store.run().id,
                written.run_dir.display()
            );
            tracing::info!("Report: {}", written.report_json.display());
            if let Some(ref path) = output {
                tracing::info!("Report copied to {}", path.display());
            }
            tracing::info!("QDP bundle: {}", written.qdp.display());

            // Exit code 2 for interrupted, 1 for any failed or infra-errored
            // spec (SRS-OPS-001/006), 0 only when everything passed.
            if result.interrupted {
                std::process::exit(2);
            }
            if rep.summary.is_gate_failure() {
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
