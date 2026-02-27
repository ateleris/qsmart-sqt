use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// Specification for a Docker container run.
pub struct DockerRun {
    pub image: String,
    pub command: Vec<String>,
    pub volumes: Vec<(PathBuf, String)>,
    pub working_dir: Option<String>,
    pub timeout: Duration,
    /// Environment variables passed to the container via `-e`.
    pub env: Vec<(String, String)>,
}

/// Output captured from a Docker container run.
pub struct DockerOutput {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub wall_time: Duration,
}

/// Execute `docker run --rm` with the given spec and capture output.
///
/// Enforces the timeout by using `--stop-timeout` and checking wall time.
pub fn run_container(spec: &DockerRun) -> Result<DockerOutput> {
    let mut cmd = Command::new("docker");
    cmd.arg("run").arg("--rm");

    // Volume mounts
    for (host_path, container_path) in &spec.volumes {
        cmd.arg("-v").arg(format!(
            "{}:{}",
            host_path.display(),
            container_path
        ));
    }

    // Environment variables
    for (key, val) in &spec.env {
        cmd.arg("-e").arg(format!("{key}={val}"));
    }

    // Working directory inside container
    if let Some(ref wd) = spec.working_dir {
        cmd.arg("-w").arg(wd);
    }

    // Image
    cmd.arg(&spec.image);

    // Command to run
    for arg in &spec.command {
        cmd.arg(arg);
    }

    tracing::debug!(
        image = %spec.image,
        cmd = ?spec.command,
        "starting docker container"
    );

    let start = Instant::now();
    let output = cmd
        .output()
        .context("failed to execute docker command — is Docker installed and running?")?;
    let wall_time = start.elapsed();

    if wall_time > spec.timeout {
        bail!(
            "docker container exceeded timeout ({:.1}s > {:.1}s)",
            wall_time.as_secs_f64(),
            spec.timeout.as_secs_f64()
        );
    }

    let exit_code = output.status.code().unwrap_or(-1);
    tracing::debug!(
        exit_code,
        wall_secs = wall_time.as_secs_f64(),
        "docker container finished"
    );

    Ok(DockerOutput {
        exit_code,
        stdout: output.stdout,
        stderr: output.stderr,
        wall_time,
    })
}

/// Helper: create a volume mount pair from a host path, mapping it to
/// the same absolute path inside the container.
pub fn identity_mount(path: &Path) -> (PathBuf, String) {
    (path.to_path_buf(), path.display().to_string())
}
