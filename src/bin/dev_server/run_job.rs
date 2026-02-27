use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use sqt::api::types::RunMetrics;

use crate::config::{BinaryConfig, ToolConfig};
use crate::tools::{coverage, run_binary, valgrind};

/// Execute a run job: run the binary and collect all configured metrics.
///
/// Individual tool failures set that metric to `None` rather than failing
/// the entire job.
pub fn execute(
    binary_config: &BinaryConfig,
    input: Option<&str>,
    timeout_secs: u64,
    workspace: &Path,
    config: &ToolConfig,
) -> Result<RunMetrics> {
    let timeout = Duration::from_secs(timeout_secs);

    let run_image = config
        .run_image
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no run image configured (tools.run.image)"))?;

    // Always run the binary to get basic metrics
    tracing::info!(
        binary = %binary_config.path,
        input = ?input,
        image = %run_image,
        "running binary"
    );
    let mut metrics = run_binary::collect(
        run_image,
        binary_config,
        input,
        workspace,
        timeout,
    )?;
    tracing::info!(
        return_code = ?metrics.return_code,
        time_ms = ?metrics.time_execution,
        peak_rss_kb = ?metrics.memory_peak_rss,
        cpu_pct = ?metrics.cpu_max_pct,
        "run_binary complete"
    );

    // Optional: valgrind for memory leak detection
    if let Some(ref image) = config.valgrind_image {
        tracing::info!(image = %image, "running valgrind");
        match valgrind::collect(image, binary_config, workspace, timeout) {
            Ok(leaks) => {
                tracing::info!(leaked_bytes = leaks, "valgrind complete");
                metrics.memory_leaks = Some(leaks);
            }
            Err(e) => tracing::warn!("valgrind failed, skipping memory.leaks: {e:#}"),
        }
    }

    // Optional: lcov for line coverage
    if let Some(ref image) = config.coverage_image {
        tracing::info!(image = %image, "running lcov coverage");
        match coverage::collect(image, &binary_config.path, workspace, timeout) {
            Ok(pct) => {
                tracing::info!(line_pct = pct, "coverage complete");
                metrics.coverage_line_pct = Some(pct);
            }
            Err(e) => tracing::warn!("lcov failed, skipping coverage.line_pct: {e:#}"),
        }
    }

    Ok(metrics)
}
