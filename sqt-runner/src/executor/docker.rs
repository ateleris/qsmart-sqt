// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use sqt_types::{HardwareInfo, Instrumentation, JobMetrics, JobRequest};

use super::{BackendDescriptor, ExecutionOutcome, JobContext, LifecycleBackend};
use crate::{
    config::{BinaryConfig, BuildConfig, ToolConfig},
    docker::{self, DockerRun, shell_quote},
    tools::{
        binary_size, coverage, output_capture, run_binary, sonarqube, static_analysis, valgrind,
    },
};

/// Default job executor: compiles (when configured) and runs workloads in
/// Docker containers, then collects metrics via the configured tool chain.
///
/// # Customisation
///
/// Fork `sqt-runner` and replace `DockerExecutor` with your own
/// [`LifecycleBackend`] implementation in `main.rs` to integrate a different
/// ML-L framework, compiler, or target architecture. Implementing
/// `LifecycleBackend` yields an [`super::ExecutionBackend`] via the blanket
/// impl, so `main.rs` can store it as `Arc<dyn ExecutionBackend>` unchanged.
pub struct DockerExecutor {
    binary_config: BinaryConfig,
    build_config: Option<BuildConfig>,
    tool_config: ToolConfig,
    hardware: HardwareInfo,
    target_id: String,
    toolchain_versions: std::collections::HashMap<String, String>,
}

/// Per-job state produced by `prepare` and consumed by the `run`/`teardown`
/// phases. Exposed as `<DockerExecutor as LifecycleBackend>::Prepared`.
pub struct Prepared {
    /// Binary config with `path` resolved to the actual executable (the
    /// pre-built binary, or the freshly compiled one in build mode).
    binary_config: BinaryConfig,
    /// Resolved input path for this job, if any.
    input: Option<String>,
}

/// Raw measurements gathered by `run`, mapped into `JobMetrics` by `collect`.
/// Keeping the IO (`run`) separate from the pure mapping (`collect`) makes the
/// latter trivially testable. Exposed as
/// `<DockerExecutor as LifecycleBackend>::RawRun`.
pub struct RawRun {
    primary: run_binary::RunBinaryOutput,
    leaks_bytes: Option<i64>,
    coverage_line_pct: Option<f64>,
    binary_size_kb: Option<f64>,
    /// `(critical, major)` issue counts from `SonarQube`, if queried.
    sonar_issues: Option<(i64, i64)>,
    static_warnings: Option<i64>,
}

impl DockerExecutor {
    pub fn new(
        binary_config: BinaryConfig,
        build_config: Option<BuildConfig>,
        tool_config: ToolConfig,
        hardware: HardwareInfo,
        target_id: String,
        toolchain_versions: std::collections::HashMap<String, String>,
    ) -> Self {
        Self { binary_config, build_config, tool_config, hardware, target_id, toolchain_versions }
    }

    /// Compile the main program source against the ML-L engine shared library.
    ///
    /// Produces `<workspace>/binary` and copies the engine `.so` into
    /// `<workspace>/` so that `LD_LIBRARY_PATH=<workspace>` is sufficient for
    /// the binary to find it at runtime.
    fn compile(&self, request: &JobRequest, workspace: &Path) -> Result<PathBuf> {
        let build = self.build_config.as_ref().expect("compile called without build_config");

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

        let compile_timeout = Duration::from_secs(build.compile_timeout_secs.unwrap_or(300));

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

impl LifecycleBackend for DockerExecutor {
    type Prepared = Prepared;
    type RawRun = RawRun;

    fn preflight(&self) -> Result<()> {
        tracing::info!("running Docker backend preflight checks...");

        docker::check_docker().context("Docker check failed")?;

        let images: Vec<(&str, &str)> = [
            (self.tool_config.run_image.as_deref(), "run"),
            (self.tool_config.valgrind_image.as_deref(), "valgrind"),
            (self.tool_config.coverage_image.as_deref(), "coverage"),
            (self.tool_config.static_image.as_deref(), "static_analysis"),
            (self.build_config.as_ref().map(|b| b.image.as_str()), "build"),
        ]
        .into_iter()
        .filter_map(|(img, name)| img.map(|i| (i, name)))
        .collect();

        for (image, name) in &images {
            docker::check_image(image).with_context(|| format!("{name} image check failed"))?;
            tracing::info!("{name} image OK: {image}");
        }

        // When building per-job there is no pre-built binary to probe; the
        // compile step validates the source instead.
        if self.build_config.is_none()
            && let Some(ref run_image) = self.tool_config.run_image
        {
            docker::check_binary_in_image(run_image, &self.binary_config.path)
                .context("binary check failed")?;
            tracing::info!("binary OK: {} (in {})", self.binary_config.path, run_image);
        }

        tracing::info!("Docker backend preflight passed");
        Ok(())
    }

    fn describe(&self) -> BackendDescriptor {
        BackendDescriptor {
            hardware: self.hardware.clone(),
            target_id: self.target_id.clone(),
            toolchain_versions: self.toolchain_versions.clone(),
        }
    }

    /// Phase 1: resolve the binary to run (compile in build mode, else use the
    /// pre-built one) and the input path for this job.
    fn prepare(&self, ctx: &JobContext) -> Result<Self::Prepared> {
        let binary_config = if self.build_config.is_some() {
            let binary_path = self.compile(ctx.request, ctx.workspace)?;
            let mut bc = self.binary_config.clone();
            bc.path = binary_path.display().to_string();
            // Ensure the engine lib (copied to workspace) is findable at runtime.
            bc.env
                .entry("LD_LIBRARY_PATH".to_string())
                .or_insert_with(|| ctx.workspace.display().to_string());
            bc
        } else {
            self.binary_config.clone()
        };

        let input = ctx.request.input.as_ref().and_then(|v| v.as_str()).map(str::to_string);

        Ok(Prepared { binary_config, input })
    }

    /// Phase 2: execute the binary and run every configured analysis tool,
    /// gathering their raw results. Optional tools fail soft (warn + `None`).
    //
    // A flat sequence of independent tool invocations; splitting it would only
    // scatter the soft-fail handling without aiding readability.
    #[expect(
        clippy::too_many_lines,
        reason = "one step per metric collector, run in sequence against the same container set-up"
    )]
    fn run(&self, ctx: &JobContext, prepared: &Prepared) -> Result<RawRun> {
        let config = &self.tool_config;
        let binary_config = &prepared.binary_config;
        let input = prepared.input.as_deref();
        let workspace = ctx.workspace;
        let timeout = ctx.timeout;

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
        let primary = run_binary::collect(run_image, binary_config, input, workspace, timeout)?;
        tracing::info!(
            return_code = ?primary.return_code,
            time_ms = ?primary.time_execution_ms,
            peak_rss_kb = ?primary.memory_peak_rss_kb,
            cpu_pct = ?primary.cpu_pct,
            "run_binary complete"
        );

        // --- Optional: valgrind ----------------------------------------------
        let leaks_bytes = if let Some(ref image) = config.valgrind_image {
            tracing::info!(image = %image, "running valgrind");
            match valgrind::collect(image, binary_config, workspace, timeout) {
                Ok(leaks) => {
                    tracing::info!(leaked_bytes = leaks, "valgrind complete");
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "leaked bytes are a whole number parsed from valgrind output"
                    )]
                    Some(leaks as i64)
                }
                Err(e) => {
                    tracing::warn!("valgrind failed, skipping memory.leaks_bytes: {e:#}");
                    None
                }
            }
        } else {
            None
        };

        // --- Optional: coverage ----------------------------------------------
        let coverage_line_pct = if let Some(ref image) = config.coverage_image {
            tracing::info!(image = %image, "running lcov coverage");
            match coverage::collect(image, &binary_config.path, workspace, timeout) {
                Ok(pct) => {
                    tracing::info!(line_pct = pct, "coverage complete");
                    Some(pct)
                }
                Err(e) => {
                    tracing::warn!("lcov failed, skipping coverage.line_pct: {e:#}");
                    None
                }
            }
        } else {
            None
        };

        // --- Binary size (no Docker needed) ----------------------------------
        tracing::info!(binary = %binary_config.path, "collecting binary size");
        let binary_size_kb = match binary_size::collect(&binary_config.path) {
            Ok(size_kb) => {
                tracing::info!(size_kb, "binary_size complete");
                Some(size_kb)
            }
            Err(e) => {
                tracing::warn!("binary_size failed: {e:#}");
                None
            }
        };

        // --- Optional: SonarQube ---------------------------------------------
        let sonar_issues =
            if let (Some(url), Some(token)) = (&config.sonar_url, &config.sonar_token) {
                let project_key = Path::new(&binary_config.path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&binary_config.path);

                tracing::info!(url = %url, project_key = %project_key, "querying sonarqube");
                match sonarqube::collect(url, token, project_key) {
                    Ok((critical, major)) => {
                        tracing::info!(critical, major, "sonarqube complete");
                        Some((critical, major))
                    }
                    Err(e) => {
                        tracing::warn!("sonarqube failed, skipping static metrics: {e:#}");
                        None
                    }
                }
            } else {
                None
            };

        // --- Optional: static analysis ---------------------------------------
        let static_warnings = if let Some(ref image) = config.static_image {
            tracing::info!(image = %image, "running static analysis");
            match static_analysis::collect(image, &binary_config.path, workspace, timeout) {
                Ok(warnings) => {
                    tracing::info!(warnings, "static_analysis complete");
                    Some(warnings)
                }
                Err(e) => {
                    tracing::warn!("static analysis failed: {e:#}");
                    None
                }
            }
        } else {
            None
        };

        Ok(RawRun {
            primary,
            leaks_bytes,
            coverage_line_pct,
            binary_size_kb,
            sonar_issues,
            static_warnings,
        })
    }

    /// Phase 3: map raw measurements into the `JobMetrics` DTO and capture the
    /// binary's stdout as the job's [`sqt_types::JobOutput`]. Pure, no IO.
    /// Consumes `RawRun` by value as the terminal owner of the run's output.
    fn collect(&self, ctx: &JobContext, raw: Self::RawRun) -> Result<ExecutionOutcome> {
        let (static_critical_issues, static_major_issues) = match raw.sonar_issues {
            Some((critical, major)) => (Some(critical), Some(major)),
            None => (None, None),
        };

        // Label the output with the workload's wire name (e.g. "regression").
        let output_type = serde_json::to_value(&ctx.request.use_case)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "output".to_string());
        let output = output_capture::build_job_output(&output_type, &raw.primary.stdout);

        let metrics = JobMetrics {
            return_code: raw.primary.return_code,
            time_avg_ms: raw.primary.time_execution_ms,
            memory_peak_rss_kb: raw.primary.memory_peak_rss_kb,
            memory_avg_rss_kb: raw.primary.memory_avg_rss_kb,
            cpu_avg_normalized_pct: raw.primary.cpu_pct,
            memory_leaks_bytes: raw.leaks_bytes,
            coverage_line_pct: raw.coverage_line_pct,
            binary_size_kb: raw.binary_size_kb,
            harness_docker_overhead_ms: raw.primary.docker_overhead_ms,
            static_critical_issues,
            static_major_issues,
            static_warnings: raw.static_warnings,
            ..Default::default()
        };

        Ok(ExecutionOutcome { metrics, output })
    }

    // Phase 4 (teardown): the default no-op suffices — `docker run --rm` removes
    // containers and the workspace is owned by the caller. Other backends
    // override it to release boards, power, etc.
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

/// Remove duplicate volume mounts (same host path), keeping the first entry.
fn dedup_volumes(volumes: &mut Vec<(PathBuf, String)>) {
    let mut seen = std::collections::HashSet::new();
    volumes.retain(|(host, _)| seen.insert(host.clone()));
}
