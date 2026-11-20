// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Execution backends: how the Runner runs one job and describes its target.

use std::{collections::HashMap, path::Path, time::Duration};

use anyhow::{Context, Result};
use sqt_types::{HardwareInfo, JobMetrics, JobOutput, JobRequest};

pub mod docker;

/// Result of executing one job: the collected metrics plus the binary's
/// captured inference output (if any).
pub struct ExecutionOutcome {
    /// Metrics collected for the job.
    pub metrics: JobMetrics,
    /// The binary's inference output, if it produced any.
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

    /// Remove a finished job's workspace directory (backlog F-M5).
    ///
    /// A directory that no longer exists counts as removed. Backends whose
    /// tools write files the Runner may not delete override this.
    fn remove_workspace(&self, workspace: &Path) -> Result<()>;
}

/// Remove `workspace` with `std::fs::remove_dir_all`. If that is refused
/// (`PermissionDenied`), let `make_removable` clear what the Runner may not
/// delete, then remove the directory again. A directory that does not exist
/// counts as removed. Pure apart from the file system, so the decision is
/// tested without Docker.
///
/// # Errors
///
/// Returns an error if the directory cannot be removed, or if
/// `make_removable` fails.
pub fn remove_workspace_with(
    workspace: &Path,
    make_removable: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    use std::io::ErrorKind;
    match std::fs::remove_dir_all(workspace) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) if e.kind() == ErrorKind::PermissionDenied => {
            make_removable(workspace).with_context(|| {
                format!("{} could not be removed ({e}) nor made removable", workspace.display())
            })?;
            match std::fs::remove_dir_all(workspace) {
                Err(e) if e.kind() != ErrorKind::NotFound => Err(e)
                    .with_context(|| format!("{} is still not removable", workspace.display())),
                _ => Ok(()),
            }
        }
        Err(e) => Err(e).with_context(|| format!("removing {}", workspace.display())),
    }
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

    /// See [`ExecutionBackend::remove_workspace`]. Defaults to a plain removal.
    fn remove_workspace(&self, workspace: &Path) -> Result<()> {
        remove_workspace_with(workspace, |_| {
            anyhow::bail!("this backend cannot remove files the Runner may not delete")
        })
    }
}

impl<T: LifecycleBackend> ExecutionBackend for T {
    fn preflight(&self) -> Result<()> {
        LifecycleBackend::preflight(self)
    }

    fn describe(&self) -> BackendDescriptor {
        LifecycleBackend::describe(self)
    }

    fn remove_workspace(&self, workspace: &Path) -> Result<()> {
        LifecycleBackend::remove_workspace(self, workspace)
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

#[cfg(test)]
mod tests {
    use std::{cell::Cell, os::unix::fs::PermissionsExt};

    use super::*;

    /// A workspace holding a directory the current user may not delete from,
    /// as root-owned tool output is for the Runner. Returns `None` when the
    /// test runs as root, which may delete anything.
    fn locked_workspace(root: &Path) -> Option<std::path::PathBuf> {
        let ws = root.join("job");
        let locked = ws.join("tool-output");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("valgrind.xml"), "x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
        let probe = locked.join("probe");
        if std::fs::write(&probe, "").is_ok() {
            std::fs::remove_file(probe).unwrap();
            return None;
        }
        Some(ws)
    }

    fn unlock(ws: &Path) {
        std::fs::set_permissions(ws.join("tool-output"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn a_missing_or_ordinary_workspace_is_removed_without_the_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let called = Cell::new(false);
        let fallback = |_: &Path| {
            called.set(true);
            Ok(())
        };
        remove_workspace_with(&dir.path().join("never-created"), fallback).unwrap();
        let ws = dir.path().join("job");
        std::fs::create_dir_all(ws.join("sub")).unwrap();
        std::fs::write(ws.join("sub/file"), "x").unwrap();
        remove_workspace_with(&ws, fallback).unwrap();
        assert!(!ws.exists());
        assert!(!called.get(), "no fallback for a directory the Runner may delete");
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn a_refused_removal_falls_back_and_then_removes_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let Some(ws) = locked_workspace(dir.path()) else {
            return; // running as root: nothing is refused
        };
        let called = Cell::new(false);
        remove_workspace_with(&ws, |w| {
            called.set(true);
            unlock(w); // what the clean-up container achieves
            Ok(())
        })
        .unwrap();
        assert!(called.get(), "a refused removal must use the fallback");
        assert!(!ws.exists(), "the workspace must be gone afterwards");
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn a_failing_fallback_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let Some(ws) = locked_workspace(dir.path()) else {
            return; // running as root: nothing is refused
        };
        let err = remove_workspace_with(&ws, |_| anyhow::bail!("docker unavailable"))
            .expect_err("a workspace that stays must be reported");
        assert!(format!("{err:#}").contains("docker unavailable"), "{err:#}");
        unlock(&ws);
    }
}
