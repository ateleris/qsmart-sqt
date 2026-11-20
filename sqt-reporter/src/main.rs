// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! The `sqt` command: runs a qualification session or its pre-flight check
//! against a Runner. See `sqt --help` and [`sqt_reporter::cli`].

use anyhow::Result;
use clap::Parser;
use sqt_reporter::cli::{Cli, Commands};
use sqt_reporter::{api, config, evidence, orchestrator, outputs, preflight, report, specs};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // RUST_LOG (or info), raised by -v / -vv, or warn with -q (SRS-OPS-002).
    let rust_log = std::env::var("RUST_LOG").ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(cli.log_filter(rust_log.as_deref())))
        .init();

    match cli.command {
        Commands::Preflight { config } => {
            let cfg = config::load_config(&config)?;
            cfg.print_summary();

            let client = build_client(&cfg)?;

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
            let (token, abort) = spawn_signal_handler();

            let client = build_client(&cfg)?;

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

            let run =
                orchestrator::run_with_abort(&specs, &client, &cfg, token, abort, &mut ev_store)
                    .await;
            let (mut result, provenance, stopped) = match run {
                Ok((result, provenance)) => (result, provenance, None),
                Err(stopped) => {
                    // The run stopped before any specification was evaluated.
                    // The report and the summary still list every specification,
                    // as blocked (SRS-OPS-003, SRS-FUNC-009).
                    let reason = stopped.to_string();
                    let (result, provenance) = orchestrator::blocked_run(
                        &specs,
                        &cfg,
                        &reason,
                        stopped.version.as_ref(),
                        stopped.interrupted,
                    );
                    (result, provenance, Some(reason))
                }
            };
            orchestrator::persist_runner_responses(&client, &mut ev_store);

            let (rep, written) = outputs::write_run_outputs(
                &mut result,
                provenance,
                &mut ev_store,
                output.as_deref(),
            )?;
            report::print_terminal(&rep, &written.run_dir);
            if let Some(reason) = &stopped {
                println!("Run stopped before evaluation: {reason}");
                // A run a termination signal stopped exits like any interrupted
                // run (SRS-OPS-008).
                std::process::exit(if result.interrupted { 2 } else { 1 });
            }
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

/// The Runner client for `cfg`: the bearer token from the environment variable
/// the configuration names (already checked by validation) and the configured
/// CA certificate (SRS-IF-006, SRS-IF-010).
fn build_client(cfg: &config::SqtConfig) -> Result<api::ApiClient> {
    let token = cfg.runner.token_env.as_ref().and_then(|var| std::env::var(var).ok());
    let ca_cert = cfg.runner.ca_cert_pem()?;
    api::ApiClient::with_ca_cert(
        &cfg.runner.url,
        token.as_deref(),
        (&cfg.runner).into(),
        ca_cert.as_deref(),
    )
}

/// Handle Ctrl-C and SIGTERM (SRS-OPS-008): the first signal cancels the first
/// token, which stops submitting and lets the jobs already on the Runner
/// finish; a second signal cancels the second, which stops waiting for them.
fn spawn_signal_handler() -> (CancellationToken, CancellationToken) {
    let (shutdown, abort) = (CancellationToken::new(), CancellationToken::new());
    let (s, a) = (shutdown.clone(), abort.clone());
    tokio::spawn(async move {
        wait_for_signal().await;
        s.cancel();
        tracing::warn!(
            "Shutdown signal received -- no further jobs are submitted; waiting for the jobs \
             already running to finish (signal again to stop now)"
        );
        wait_for_signal().await;
        a.cancel();
        tracing::warn!("Second shutdown signal -- no longer waiting for running jobs");
    });
    (shutdown, abort)
}

/// Returns on Ctrl-C or SIGTERM.
async fn wait_for_signal() {
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        () = wait_for_sigterm() => {},
    }
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
