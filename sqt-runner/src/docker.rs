// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use uuid::Uuid;

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
/// Enforces the timeout by polling with `try_wait` and sending `docker kill`
/// followed by `SIGKILL` to the CLI process when the deadline is exceeded.
pub fn run_container(spec: &DockerRun) -> Result<DockerOutput> {
    // Stable name so we can kill the container by name on timeout.
    let container_name = format!("sqt-{}", Uuid::new_v4().simple());

    let mut cmd = Command::new("docker");
    cmd.arg("run").arg("--rm").arg("--name").arg(&container_name);

    for (host_path, container_path) in &spec.volumes {
        cmd.arg("-v").arg(format!("{}:{}", host_path.display(), container_path));
    }
    for (key, val) in &spec.env {
        cmd.arg("-e").arg(format!("{key}={val}"));
    }
    if let Some(ref wd) = spec.working_dir {
        cmd.arg("-w").arg(wd);
    }
    cmd.arg(&spec.image);
    for arg in &spec.command {
        cmd.arg(arg);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    tracing::info!(
        image = %spec.image,
        container = %container_name,
        cmd = ?spec.command,
        env = ?spec.env,
        volumes = ?spec.volumes.iter().map(|(h, c)| format!("{}:{}", h.display(), c)).collect::<Vec<_>>(),
        "starting docker container"
    );

    let start = Instant::now();
    let mut child = cmd
        .spawn()
        .context("failed to execute docker command — is Docker installed and running?")?;

    // Drain stdout/stderr on separate threads. Pipe buffers are finite:
    // if we don't read, the child blocks on write and try_wait never fires.
    let mut out_pipe = child.stdout.take().expect("stdout was piped");
    let mut err_pipe = child.stderr.take().expect("stderr was piped");
    let out_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out_pipe.read_to_end(&mut buf);
        buf
    });
    let err_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err_pipe.read_to_end(&mut buf);
        buf
    });

    // Poll for exit, enforcing the hard timeout.
    let exit_code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => {}
            Err(e) => {
                kill_container(&container_name, &mut child);
                let _ = out_thread.join();
                let _ = err_thread.join();
                return Err(e.into());
            }
        }
        if start.elapsed() >= spec.timeout {
            tracing::warn!(
                container = %container_name,
                timeout_secs = spec.timeout.as_secs_f64(),
                "container exceeded timeout, killing"
            );
            kill_container(&container_name, &mut child);
            let _ = out_thread.join();
            let _ = err_thread.join();
            bail!(
                "docker container '{}' timed out after {:.1}s",
                container_name,
                spec.timeout.as_secs_f64()
            );
        }
        thread::sleep(Duration::from_millis(100));
    };
    let wall_time = start.elapsed();

    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();

    let stdout_text = String::from_utf8_lossy(&stdout);
    let stderr_text = String::from_utf8_lossy(&stderr);

    if exit_code != 0 {
        tracing::warn!(
            image = %spec.image,
            container = %container_name,
            exit_code,
            wall_ms = wall_time.as_secs_f64() * 1000.0,
            stdout = %stdout_text,
            stderr = %stderr_text,
            "docker container exited with non-zero status"
        );
    } else {
        tracing::info!(
            image = %spec.image,
            container = %container_name,
            exit_code,
            wall_ms = wall_time.as_secs_f64() * 1000.0,
            stdout = %stdout_text,
            stderr = %stderr_text,
            "docker container finished"
        );
    }

    Ok(DockerOutput { exit_code, stdout, stderr, wall_time })
}

/// Send `docker kill` to stop the container, then kill the CLI process.
fn kill_container(container_name: &str, child: &mut std::process::Child) {
    let _ = Command::new("docker")
        .args(["kill", container_name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output();
    child.kill().ok();
    child.wait().ok();
}

/// Single-quote a string for safe use as one `sh` word.
///
/// Wrapping in single quotes disables all shell interpretation (word splitting,
/// globbing, command substitution, redirection); an embedded single quote is
/// closed, escaped as `\'`, and reopened. This is the canonical POSIX-safe
/// quoting used for every argument interpolated into a `sh -c` command,
/// including untrusted, client-supplied values (SRS security, F-C1).
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
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
        .context("failed to run `docker info` - is Docker installed?")?;

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
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .context("failed to run `docker image inspect`")?;

    if !output.status.success() {
        bail!("Docker image '{image}' not found locally - build or pull it first");
    }
    Ok(())
}

/// Check that a binary exists on the host and is executable inside the given
/// Docker image.
///
/// The binary must exist on the host filesystem - this is the binary under test,
/// not a system utility. Its parent directory is mounted into the container at
/// the same path, then `test -x` verifies it's executable inside the container.
pub fn check_binary_in_image(image: &str, binary: &str) -> Result<()> {
    let binary_path = Path::new(binary);

    if !binary_path.exists() {
        bail!(
            "binary '{binary}' does not exist on the host \
             - check the 'binary.path' in your config"
        );
    }

    let binary_dir = binary_path.parent().unwrap_or(Path::new("/"));
    let volume_mount = format!("{}:{}", binary_dir.display(), binary_dir.display());

    tracing::info!(
        "preflight binary check: docker run --rm -v {} {} test -x {}",
        volume_mount,
        image,
        binary
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
             - check that the file has execute permissions. \
             stderr: {}",
            stderr.trim()
        );
    }
    Ok(())
}
