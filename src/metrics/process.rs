#![allow(dead_code)]

use std::fs::File;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use super::{Platform, Target};

#[derive(Debug)]
pub struct ProcessOutput {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub wall_time: Duration,
    pub stdout_path: Option<PathBuf>,
    pub stderr_path: Option<PathBuf>,
}

/// Run `target.binary` (optionally wrapped by `command_override`) and capture
/// stdout/stderr to evidence files in `target.working_dir`.
///
/// When `command_override` is set the override binary is spawned with
/// `extra_args` followed by `target.binary` as its final argument, e.g.
/// `/usr/bin/time -v /path/to/binary`.
pub fn run_process(
    target: &Target,
    command_override: Option<&str>,
    extra_args: &[&str],
    evidence_prefix: &str,
) -> Result<ProcessOutput> {
    if let Platform::Remote(_) = &target.platform {
        bail!("remote targets are not supported by local collectors");
    }

    let stdout_path = target.working_dir.join(format!("{evidence_prefix}_stdout.txt"));
    let stderr_path = target.working_dir.join(format!("{evidence_prefix}_stderr.txt"));

    let stdout_file = File::create(&stdout_path)?;
    let stderr_file = File::create(&stderr_path)?;

    let mut cmd = if let Some(override_cmd) = command_override {
        let mut c = Command::new(override_cmd);
        c.args(extra_args);
        c.arg(&target.binary);
        c
    } else {
        let mut c = Command::new(&target.binary);
        c.args(extra_args);
        c
    };

    cmd.stdout(stdout_file).stderr(stderr_file).current_dir(&target.working_dir);

    if let Some(input_path) = &target.input {
        cmd.stdin(File::open(input_path)?);
    } else {
        cmd.stdin(Stdio::null());
    }

    let mut child = cmd.spawn()?;
    let start = Instant::now();

    let exit_code = loop {
        if let Some(status) = child.try_wait()? {
            break status.code().unwrap_or(-1);
        } else if start.elapsed() >= target.timeout {
            child.kill()?;
            child.wait()?;
            bail!("process timed out after {:?}", target.timeout);
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    let wall_time = start.elapsed();
    let stdout = std::fs::read(&stdout_path)?;
    let stderr = std::fs::read(&stderr_path)?;

    Ok(ProcessOutput {
        exit_code,
        stdout,
        stderr,
        wall_time,
        stdout_path: Some(stdout_path),
        stderr_path: Some(stderr_path),
    })
}

/// Check whether a tool is available on PATH by trying to spawn `tool --version`.
/// Returns `Err` with a clear message if the binary is not found.
pub fn check_tool(name: &str) -> Result<()> {
    match Command::new(name)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            let _ = child.wait();
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            bail!("required tool '{name}' not found in PATH")
        }
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn local_target(dir: &std::path::Path) -> Target {
        Target {
            binary: PathBuf::from("/bin/true"),
            platform: Platform::Local,
            input: None,
            working_dir: dir.to_path_buf(),
            timeout: Duration::from_secs(10),
        }
    }

    #[test]
    fn run_true_returns_zero() {
        let dir = tempdir().unwrap();
        let target = local_target(dir.path());
        let output = run_process(&target, None, &[], "test").unwrap();
        assert_eq!(output.exit_code, 0);
    }

    #[test]
    fn run_false_returns_nonzero() {
        let dir = tempdir().unwrap();
        let mut target = local_target(dir.path());
        target.binary = PathBuf::from("/bin/false");
        let output = run_process(&target, None, &[], "test").unwrap();
        assert_ne!(output.exit_code, 0);
    }

    #[test]
    fn stdout_captured() {
        let dir = tempdir().unwrap();
        let mut target = local_target(dir.path());
        target.binary = PathBuf::from("/bin/echo");
        let output = run_process(&target, None, &["hello"], "test").unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).contains("hello"));
    }

    #[test]
    fn evidence_files_created() {
        let dir = tempdir().unwrap();
        let target = local_target(dir.path());
        run_process(&target, None, &[], "myprefix").unwrap();
        assert!(dir.path().join("myprefix_stdout.txt").exists());
        assert!(dir.path().join("myprefix_stderr.txt").exists());
    }

    #[test]
    fn timeout_kills_long_process() {
        let dir = tempdir().unwrap();
        let mut target = local_target(dir.path());
        target.binary = PathBuf::from("/bin/sleep");
        target.timeout = Duration::from_secs(1);
        let result = run_process(&target, None, &["60"], "test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }

    #[test]
    fn stdin_from_input_file() {
        let dir = tempdir().unwrap();
        let input_file = dir.path().join("input.txt");
        std::fs::write(&input_file, b"hello from stdin").unwrap();
        let mut target = local_target(dir.path());
        target.binary = PathBuf::from("/bin/cat");
        target.input = Some(input_file);
        let output = run_process(&target, None, &[], "test").unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).contains("hello from stdin"));
    }

    #[test]
    fn remote_target_returns_error() {
        let dir = tempdir().unwrap();
        let mut target = local_target(dir.path());
        target.platform = Platform::Remote("ssh://host".into());
        let result = run_process(&target, None, &[], "test");
        assert!(result.is_err());
    }
}
