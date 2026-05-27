use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use sqt_types::{HardwareInfo, Instrumentation, JobMetrics, JobRequest};

use crate::config::{BinaryConfig, BuildConfig, ToolConfig};
use crate::docker::{self, DockerRun};
use crate::tools::{binary_size, coverage, run_binary, sonarqube, static_analysis, valgrind};

use super::JobExecutor;

/// Default job executor: compiles (when configured) and runs workloads in
/// Docker containers, then collects metrics via the configured tool chain.
///
/// # Customisation
///
/// Fork `sqt-runner` and replace `DockerExecutor` with your own
/// [`JobExecutor`] implementation in `main.rs` to integrate a different ML-L
/// framework, compiler, or target architecture.
pub struct DockerExecutor {
    binary_config: BinaryConfig,
    build_config: Option<BuildConfig>,
    tool_config: ToolConfig,
    hardware: HardwareInfo,
}

impl DockerExecutor {
    pub fn new(
        binary_config: BinaryConfig,
        build_config: Option<BuildConfig>,
        tool_config: ToolConfig,
        hardware: HardwareInfo,
    ) -> Self {
        Self { binary_config, build_config, tool_config, hardware }
    }

    /// Compile the main program source against the ML-L engine shared library.
    ///
    /// Produces `<workspace>/binary` and copies the engine `.so` into
    /// `<workspace>/` so that `LD_LIBRARY_PATH=<workspace>` is sufficient for
    /// the binary to find it at runtime.
    fn compile(&self, request: &JobRequest, workspace: &Path) -> Result<PathBuf> {
        let build = self
            .build_config
            .as_ref()
            .expect("compile called without build_config");

        let source_path = Path::new(&build.source);
        let source_dir = source_path.parent().unwrap_or(Path::new("/"));
        let engine_lib_path = Path::new(&build.engine_lib);
        let engine_lib_dir = engine_lib_path.parent().unwrap_or(Path::new("/"));
        let output = workspace.join("binary");

        let flags = instrumentation_flags(&request.instrumentation);

        // Build the compiler invocation - every argument is individually
        // shell-quoted so paths or flags containing spaces or special
        // characters are handled safely.
        let mut compile_args: Vec<String> = vec![shell_quote(&build.compiler)];
        compile_args.extend(flags.iter().map(|s| shell_quote(s)));
        compile_args.extend(build.extra_flags.iter().map(|s| shell_quote(s)));
        for inc in &build.include_dirs {
            compile_args.push(format!("-I{}", shell_quote(inc)));
        }
        compile_args.extend([
            "-o".to_string(),
            shell_quote(&output.display().to_string()),
            shell_quote(&build.source),
            shell_quote(&build.engine_lib),
        ]);

        // Compile then copy engine lib into workspace so it is reachable at
        // runtime via LD_LIBRARY_PATH=<workspace>.
        let shell_cmd = format!(
            "{} && cp {} {}",
            compile_args.join(" "),
            shell_quote(&build.engine_lib),
            shell_quote(&workspace.display().to_string()),
        );

        let compile_timeout =
            Duration::from_secs(build.compile_timeout_secs.unwrap_or(300));

        // Collect unique volume mounts
        let mut volumes: Vec<(PathBuf, String)> = vec![
            docker::identity_mount(source_dir),
            docker::identity_mount(engine_lib_dir),
            docker::identity_mount(workspace),
        ];
        for inc in &build.include_dirs {
            let inc_path = Path::new(inc);
            volumes.push(docker::identity_mount(inc_path));
        }
        dedup_volumes(&mut volumes);

        let spec = DockerRun {
            image: build.image.clone(),
            command: vec!["sh".into(), "-c".into(), shell_cmd],
            volumes,
            working_dir: Some(source_dir.display().to_string()),
            timeout: compile_timeout,
            env: vec![],
        };

        let out = docker::run_container(&spec).context("compile step: docker execution failed")?;
        if out.exit_code != 0 {
            anyhow::bail!(
                "compilation failed (exit {}): {}",
                out.exit_code,
                String::from_utf8_lossy(&out.stderr)
            );
        }

        Ok(output)
    }
}

impl JobExecutor for DockerExecutor {
    fn execute(&self, request: &JobRequest, workspace: &Path) -> Result<JobMetrics> {
        let timeout = Duration::from_secs(request.timeout_secs.unwrap_or(60));

        // Compile if build config is present; otherwise use the pre-built binary.
        let binary_config = if self.build_config.is_some() {
            let binary_path = self.compile(request, workspace)?;
            let mut bc = self.binary_config.clone();
            bc.path = binary_path.display().to_string();
            // Ensure the engine lib (copied to workspace) is findable at runtime.
            bc.env
                .entry("LD_LIBRARY_PATH".to_string())
                .or_insert_with(|| workspace.display().to_string());
            bc
        } else {
            self.binary_config.clone()
        };

        let input = request
            .input
            .as_ref()
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let mut metrics = JobMetrics::default();
        let config = &self.tool_config;

        // --- Execution metrics -----------------------------------------------

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
        let run_out =
            run_binary::collect(run_image, &binary_config, input.as_deref(), workspace, timeout)?;

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

        // --- Optional: valgrind ----------------------------------------------

        if let Some(ref image) = config.valgrind_image {
            tracing::info!(image = %image, "running valgrind");
            match valgrind::collect(image, &binary_config, workspace, timeout) {
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

        // --- Optional: coverage ----------------------------------------------

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

        // --- Binary size (no Docker needed) ----------------------------------

        tracing::info!(binary = %binary_config.path, "collecting binary size");
        match binary_size::collect(&binary_config.path) {
            Ok(size_kb) => {
                tracing::info!(size_kb, "binary_size complete");
                metrics.binary_size_kb = Some(size_kb);
            }
            Err(e) => tracing::warn!("binary_size failed: {e:#}"),
        }

        // --- Optional: SonarQube ---------------------------------------------

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

        // --- Optional: static analysis ---------------------------------------

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

    fn hardware_info(&self) -> HardwareInfo {
        self.hardware.clone()
    }
}

/// Return compiler flags for the requested instrumentation mode.
fn instrumentation_flags(instrumentation: &Instrumentation) -> &'static [&'static str] {
    match instrumentation {
        Instrumentation::None => &["-O2"],
        Instrumentation::Asan => &["-fsanitize=address", "-g", "-O1"],
        Instrumentation::Valgrind => &["-g", "-O1"],
        Instrumentation::Gprof => &["-pg", "-g", "-O2"],
    }
}

/// Single-quote a shell argument to prevent word splitting and glob expansion.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Remove duplicate volume mounts (same host path), keeping the first entry.
fn dedup_volumes(volumes: &mut Vec<(PathBuf, String)>) {
    let mut seen = std::collections::HashSet::new();
    volumes.retain(|(host, _)| seen.insert(host.clone()));
}
