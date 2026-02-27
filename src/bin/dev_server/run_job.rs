use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use sqt::api::types::RunMetrics;

use crate::ToolConfig;
use crate::tools::{coverage, run_binary, valgrind};

/// Execute a run job: run the binary and collect all configured metrics.
///
/// Individual tool failures set that metric to `None` rather than failing
/// the entire job.
pub fn execute(
    binary: &str,
    input: Option<&str>,
    timeout_secs: u64,
    workspace: &Path,
    config: &ToolConfig,
) -> Result<RunMetrics> {
    let timeout = Duration::from_secs(timeout_secs);

    // Always run the binary to get basic metrics
    let mut metrics = run_binary::collect(
        &config.run_image,
        binary,
        input,
        workspace,
        timeout,
    )?;

    // Optional: valgrind for memory leak detection
    if let Some(ref image) = config.valgrind_image {
        match valgrind::collect(image, binary, workspace, timeout) {
            Ok(leaks) => metrics.memory_leaks = Some(leaks),
            Err(e) => tracing::warn!("valgrind failed, skipping memory.leaks: {e:#}"),
        }
    }

    // Optional: lcov for line coverage
    if let Some(ref image) = config.coverage_image {
        match coverage::collect(image, binary, workspace, timeout) {
            Ok(pct) => metrics.coverage_line_pct = Some(pct),
            Err(e) => tracing::warn!("lcov failed, skipping coverage.line_pct: {e:#}"),
        }
    }

    Ok(metrics)
}
