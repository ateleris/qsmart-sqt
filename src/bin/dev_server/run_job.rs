use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use sqt::api::types::JobMetrics;

use crate::config::{BinaryConfig, ToolConfig};
use crate::tools::{binary_size, coverage, run_binary, sonarqube, static_analysis, valgrind};

/// Execute a job: run the binary and collect all configured metrics.
///
/// Combines what was previously `run_job` and `analysis_job` — the single
/// `/api/v1/jobs` endpoint covers both execution and analysis metrics.
///
/// `model_zip` is the base64-encoded customer model ZIP.  The dev-server stub
/// logs its presence but does not unpack it; real inference engines handle
/// unpacking server-side.
///
/// Individual tool failures set that metric to `None` rather than aborting
/// the entire job.
pub fn execute(
    binary_config: &BinaryConfig,
    input: Option<&str>,
    model_zip: Option<&str>,
    timeout_secs: u64,
    workspace: &Path,
    config: &ToolConfig,
) -> Result<JobMetrics> {
    let timeout = Duration::from_secs(timeout_secs);
    let mut metrics = JobMetrics::default();

    if let Some(zip) = model_zip {
        tracing::info!(zip_len = zip.len(), "model_zip received (stub: not applied)");
    }

    // --- Execution metrics ---------------------------------------------------

    let run_image = config
        .run_image
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no run image configured (tools.run.image)"))?;

    tracing::info!(
        binary = %binary_config.path,
        input = ?input,
        image = %run_image,
        "running binary"
    );
    let run_out = run_binary::collect(run_image, binary_config, input, workspace, timeout)?;

    metrics.return_code = run_out.return_code;
    metrics.time_avg_ms = run_out.time_execution_ms;
    metrics.memory_peak_rss_kb = run_out.memory_peak_rss_kb;
    metrics.memory_avg_rss_kb = run_out.memory_avg_rss_kb;
    metrics.cpu_avg_normalized_pct = run_out.cpu_pct;

    tracing::info!(
        return_code = ?metrics.return_code,
        time_ms = ?metrics.time_avg_ms,
        peak_rss_kb = ?metrics.memory_peak_rss_kb,
        cpu_pct = ?metrics.cpu_avg_normalized_pct,
        "run_binary complete"
    );

    // --- Optional: valgrind --------------------------------------------------

    if let Some(ref image) = config.valgrind_image {
        tracing::info!(image = %image, "running valgrind");
        match valgrind::collect(image, binary_config, workspace, timeout) {
            Ok(leaks) => {
                tracing::info!(leaked_bytes = leaks, "valgrind complete");
                #[allow(clippy::cast_possible_truncation)]
                {
                    metrics.memory_leaks_bytes = Some(leaks as i64);
                }
            }
            Err(e) => tracing::warn!("valgrind failed, skipping memory.leaks_bytes: {e:#}"),
        }
    }

    // --- Optional: coverage --------------------------------------------------

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

    // --- Binary size (no Docker needed) --------------------------------------

    tracing::info!(binary = %binary_config.path, "collecting binary size");
    match binary_size::collect(&binary_config.path) {
        Ok(size_kb) => {
            tracing::info!(size_kb, "binary_size complete");
            metrics.binary_size_kb = Some(size_kb);
        }
        Err(e) => tracing::warn!("binary_size failed: {e:#}"),
    }

    // --- Optional: SonarQube -------------------------------------------------

    if let (Some(url), Some(token)) = (&config.sonar_url, &config.sonar_token) {
        let project_key = Path::new(&binary_config.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&binary_config.path);

        tracing::info!(url = %url, project_key = %project_key, "querying sonarqube");
        match sonarqube::collect(url, token, project_key) {
            Ok((critical, major)) => {
                tracing::info!(critical, major, "sonarqube complete");
                metrics.static_critical_issues = Some(critical);
                metrics.static_major_issues = Some(major);
            }
            Err(e) => tracing::warn!("sonarqube failed, skipping static metrics: {e:#}"),
        }
    }

    // --- Optional: static analysis -------------------------------------------

    if let Some(ref image) = config.static_image {
        tracing::info!(image = %image, "running static analysis");
        match static_analysis::collect(image, &binary_config.path, workspace, timeout) {
            Ok(warnings) => {
                tracing::info!(warnings, "static_analysis complete");
                metrics.static_warnings = Some(warnings);
            }
            Err(e) => tracing::warn!("static analysis failed: {e:#}"),
        }
    }

    Ok(metrics)
}
