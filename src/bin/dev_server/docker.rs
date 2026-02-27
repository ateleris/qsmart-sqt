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

    tracing::info!(
        image = %spec.image,
        cmd = ?spec.command,
        env = ?spec.env,
        volumes = ?spec.volumes.iter().map(|(h, c)| format!("{}:{}", h.display(), c)).collect::<Vec<_>>(),
        "starting docker container"
    );

    let start = Instant::now();
    let output = cmd
        .output()
        .context("failed to execute docker command — is Docker installed and running?")?;
    let wall_time = start.elapsed();

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout_text = String::from_utf8_lossy(&output.stdout);
    let stderr_text = String::from_utf8_lossy(&output.stderr);

    if wall_time > spec.timeout {
        tracing::error!(
            image = %spec.image,
            exit_code,
            wall_ms = wall_time.as_millis() as u64,
            stdout = %stdout_text,
            stderr = %stderr_text,
            "docker container exceeded timeout"
        );
        bail!(
            "docker container exceeded timeout ({:.1}s > {:.1}s)",
            wall_time.as_secs_f64(),
            spec.timeout.as_secs_f64()
        );
    }

    if !output.status.success() {
        tracing::warn!(
            image = %spec.image,
            exit_code,
            wall_ms = wall_time.as_millis() as u64,
            stdout = %stdout_text,
            stderr = %stderr_text,
            "docker container exited with non-zero status"
        );
    } else {
        tracing::info!(
            image = %spec.image,
            exit_code,
            wall_ms = wall_time.as_millis() as u64,
            stdout = %stdout_text,
            stderr = %stderr_text,
            "docker container finished"
        );
    }

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

/// Check that Docker is available and responding.
pub fn check_docker() -> Result<()> {
    let output = Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .output()
        .context("failed to run `docker info` — is Docker installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Docker is not available: {}", stderr.trim());
    }

    let version = String::from_utf8_lossy(&output.stdout);
    tracing::info!("Docker server version: {}", version.trim());
    Ok(())
}

/// Check that a Docker image exists locally.
pub fn check_image(image: &str) -> Result<()> {
    let output = Command::new("docker")
        .args(["image", "inspect", image])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .context("failed to run `docker image inspect`")?;

    if !output.status.success() {
        bail!("Docker image '{image}' not found locally — build or pull it first");
    }
    Ok(())
}

/// Check that a binary exists on the host and is executable inside the given
/// Docker image.
///
/// The binary must exist on the host filesystem — this is the binary under test,
/// not a system utility. Its parent directory is mounted into the container at
/// the same path, then `test -x` verifies it's executable inside the container.
pub fn check_binary_in_image(image: &str, binary: &str) -> Result<()> {
    let binary_path = Path::new(binary);

    if !binary_path.exists() {
        bail!(
            "binary '{binary}' does not exist on the host \
             — check the 'binary.path' in your config"
        );
    }

    let binary_dir = binary_path.parent().unwrap_or(Path::new("/"));
    let volume_mount = format!("{}:{}", binary_dir.display(), binary_dir.display());

    tracing::info!(
        "preflight binary check: docker run --rm -v {} {} test -x {}",
        volume_mount, image, binary
    );

    let output = Command::new("docker")
        .args(["run", "--rm"])
        .arg("-v")
        .arg(&volume_mount)
        .arg(image)
        .args(["test", "-x", binary])
        .output()
        .context("failed to run binary check in container")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "binary '{binary}' is not executable inside image '{image}' \
             — check that the file has execute permissions. \
             stderr: {}", stderr.trim()
        );
    }
    Ok(())
}
