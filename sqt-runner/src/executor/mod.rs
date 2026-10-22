// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

use std::{collections::HashMap, path::Path, time::Duration};

use anyhow::Result;
use sqt_types::{HardwareInfo, JobMetrics, JobOutput, JobRequest};

pub mod docker;

/// Result of executing one job: the collected metrics plus the binary's
/// captured inference output (if any).
pub struct ExecutionOutcome {
    pub metrics: JobMetrics,
    pub output: Option<JobOutput>,
}

/// Backend-agnostic bundle of everything a backend needs to run one job.
///
/// Constructed by the caller (the runner core) and passed to every per-job
/// lifecycle phase, so backends never re-derive timeout policy or reach for
/// the workspace path on their own.
pub struct JobContext<'a> {
    /// The submitted job request.
    pub request: &'a JobRequest,
    /// Per-job scratch directory, already created by the caller.
    pub workspace: &'a Path,
    /// Wall-clock budget for the job's execution.
    pub timeout: Duration,
}

/// Static description of an execution backend, surfaced on `GET /api/v1/version`
/// and in `JobStatusResponse.hardware`.
///
/// Sourcing this from the backend (rather than from scattered config fields)
/// keeps the version/hardware reporting correct for whichever backend is wired
/// in, without the HTTP layer knowing which one it is.
#[derive(Clone)]
pub struct BackendDescriptor {
    /// Hardware descriptor returned verbatim in `JobStatusResponse.hardware`.
    pub hardware: HardwareInfo,
    /// Short identifier for the target, e.g. `"gr740-leon4"`.
    pub target_id: String,
    /// Versions of key tools/runtimes available on this backend.
    pub toolchain_versions: HashMap<String, String>,
}

/// Pluggable job execution backend.
///
/// The default implementation is [`docker::DockerExecutor`], which compiles
/// (when `BuildConfig` is present) and executes workloads inside Docker
/// containers. Customers who need to target specific hardware or ML-L
/// frameworks implement this trait in their own fork and wire it into
/// `main.rs`.
///
/// # Execution model
///
/// A job runs in up to two phases:
///
/// 1. **Compile** - triggered when the runner is configured with a
///    [`crate::config::BuildConfig`]. The executor compiles the
///    customer-supplied main program source file and links it against the
///    ML-L engine shared library. Instrumentation flags are chosen per
///    `JobRequest.instrumentation`:
///    - `none`     → `-O2`
///    - `asan`     → `-fsanitize=address -g -O1`
///    - `valgrind` → `-g -O1`
///    - `gprof`    → `-pg -g -O2`
///
/// 2. **Execute** - the binary (pre-built or freshly compiled) is invoked and
///    metrics are collected through the configured tool chain (Valgrind,
///    lcov, cppcheck, `SonarQube`, ...).
///
/// The trait is intentionally backend-agnostic: the HTTP/queue/store layer
/// only ever calls these methods, so a non-Docker backend (e.g. an on-target
/// GRMON or `LabGrid` runner) can be dropped in by implementing this trait. The
/// startup readiness check is part of the contract via [`preflight`], so the
/// core never reaches around the trait to probe Docker directly.
///
/// [`preflight`]: ExecutionBackend::preflight
pub trait ExecutionBackend: Send + Sync {
    /// One-time readiness check, run once at startup before the server binds.
    ///
    /// Implementations validate everything they need to execute jobs (for the
    /// Docker backend: the daemon is up, configured images exist, the binary
    /// is executable inside the run image). Returning `Err` aborts startup.
    fn preflight(&self) -> Result<()>;

    /// Run a single job and return the collected metrics plus captured output.
    ///
    /// `workspace` is a per-job scratch directory owned by the caller. The
    /// executor may write intermediate files there (Valgrind XML, coverage
    /// data, ...). The caller creates and owns the directory.
    fn execute(&self, request: &JobRequest, workspace: &Path) -> Result<ExecutionOutcome>;

    /// Describe the backend: target hardware, identity, and toolchain versions.
    ///
    /// `hardware` is returned verbatim in `JobStatusResponse.hardware`;
    /// `target_id` and `toolchain_versions` feed `GET /api/v1/version`.
    fn describe(&self) -> BackendDescriptor;
}

/// Ergonomic, lifecycle-oriented backend trait — **implement this**, not
/// [`ExecutionBackend`], when adding a backend.
///
/// A blanket impl turns every `LifecycleBackend` into an `ExecutionBackend`,
/// supplying the `execute` orchestration (prepare → run → collect, with
/// `teardown` always run afterwards) for free. The only types that cross the
/// boundary are [`JobContext`] in and `JobMetrics` / [`BackendDescriptor`]
/// out; `Prepared` and `RawRun` stay private to the implementing backend.
///
/// The trait is *not* object-safe (it has associated types), which is fine:
/// the object-safe [`ExecutionBackend`] is what the HTTP layer stores as
/// `Arc<dyn ExecutionBackend>`.
pub trait LifecycleBackend: Send + Sync {
    /// Backend-private handle produced by `prepare` (e.g. compiled binary path,
    /// container spec, board session).
    type Prepared;
    /// Backend-private raw output produced by `run`, mapped to metrics by
    /// `collect`.
    type RawRun;

    /// See [`ExecutionBackend::preflight`].
    fn preflight(&self) -> Result<()>;

    /// See [`ExecutionBackend::describe`].
    fn describe(&self) -> BackendDescriptor;

    /// Phase 1: provision artifacts for the job (compile, flash, stage input).
    fn prepare(&self, ctx: &JobContext) -> Result<Self::Prepared>;

    /// Phase 2: run the workload under measurement, gathering raw output.
    fn run(&self, ctx: &JobContext, prepared: &Self::Prepared) -> Result<Self::RawRun>;

    /// Phase 3: map raw output into the `JobMetrics` DTO plus the captured
    /// [`JobOutput`].
    fn collect(&self, ctx: &JobContext, raw: Self::RawRun) -> Result<ExecutionOutcome>;

    /// Phase 4: release per-job resources. Runs even when an earlier phase
    /// failed. Defaults to dropping `Prepared` with no extra work.
    fn teardown(&self, _ctx: &JobContext, _prepared: Self::Prepared) -> Result<()> {
        Ok(())
    }
}

impl<T: LifecycleBackend> ExecutionBackend for T {
    fn preflight(&self) -> Result<()> {
        LifecycleBackend::preflight(self)
    }

    fn describe(&self) -> BackendDescriptor {
        LifecycleBackend::describe(self)
    }

    fn execute(&self, request: &JobRequest, workspace: &Path) -> Result<ExecutionOutcome> {
        let timeout = Duration::from_secs(request.timeout_secs.unwrap_or(60));
        let ctx = JobContext { request, workspace, timeout };

        let prepared = self.prepare(&ctx)?;
        // run → collect; teardown runs unconditionally afterwards.
        let outcome = self.run(&ctx, &prepared).and_then(|raw| self.collect(&ctx, raw));
        if let Err(e) = self.teardown(&ctx, prepared) {
            tracing::warn!(error = format!("{e:#}"), "teardown failed");
        }
        outcome
    }
}
