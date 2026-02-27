use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use sqt::api::types::AnalysisMetrics;

use crate::config::ToolConfig;
use crate::tools::{binary_size, sonarqube, static_analysis};

/// Execute an analysis job: collect all configured analysis metrics.
///
/// Individual tool failures set that metric to `None` rather than failing
/// the entire job.
pub fn execute(
    binary: &str,
    timeout_secs: u64,
    workspace: &Path,
    config: &ToolConfig,
) -> Result<AnalysisMetrics> {
    let timeout = Duration::from_secs(timeout_secs);
    let mut metrics = AnalysisMetrics::default();

    // Always collect binary size (no Docker needed)
    tracing::info!(binary = %binary, "collecting binary size");
    match binary_size::collect(binary) {
        Ok(size_kb) => {
            tracing::info!(size_kb, "binary_size complete");
            metrics.binary_size_kb = Some(size_kb);
        }
        Err(e) => tracing::warn!("binary_size failed: {e:#}"),
    }

    // Optional: SonarQube issues
    if let (Some(url), Some(token)) = (&config.sonar_url, &config.sonar_token) {
        let project_key = Path::new(binary)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(binary);

        tracing::info!(url = %url, project_key = %project_key, "querying sonarqube");
        match sonarqube::collect(url, token, project_key) {
            Ok((critical, major)) => {
                tracing::info!(critical, major, "sonarqube complete");
                metrics.sonar_critical_issues = Some(critical);
                metrics.sonar_major_issues = Some(major);
            }
            Err(e) => tracing::warn!("sonarqube failed, skipping sonar metrics: {e:#}"),
        }
    }

    // Optional: static analysis warnings
    if let Some(ref image) = config.static_image {
        tracing::info!(image = %image, "running static analysis");
        match static_analysis::collect(image, binary, workspace, timeout) {
            Ok(warnings) => {
                tracing::info!(warnings, "static_analysis complete");
                metrics.static_warnings = Some(warnings);
            }
            Err(e) => tracing::warn!("static analysis failed: {e:#}"),
        }
    }

    Ok(metrics)
}
