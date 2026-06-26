use std::path::Path;

use anyhow::Result;
use sqt_types::{HardwareInfo, JobMetrics, JobRequest};

pub mod docker;

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

    /// Run a single job and return the collected metrics.
    ///
    /// `workspace` is a per-job scratch directory owned by the caller. The
    /// executor may write intermediate files there (Valgrind XML, coverage
    /// data, ...). The caller creates and owns the directory.
    fn execute(&self, request: &JobRequest, workspace: &Path) -> Result<JobMetrics>;

    /// Describe the hardware this executor targets.
    ///
    /// Returned verbatim in `JobStatusResponse.hardware` so reporters can
    /// correlate metrics with the underlying hardware without an extra call.
    fn hardware_info(&self) -> HardwareInfo;
}
