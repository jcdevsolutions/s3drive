//! Locating and invoking the user's rclone binary.

use s3dive_core::platform::{fuse_status, FuseStatus, Os};
use s3dive_core::rclone;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RcloneInfo {
    pub available: bool,
    pub program: Option<String>,
    pub version: Option<String>,
    /// "configured", "path" or "missing".
    pub source: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Rclone {
    pub program: Option<PathBuf>,
    pub source: String,
}

fn executable_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["rclone.exe", "rclone"]
    } else {
        &["rclone"]
    }
}

/// Resolve a program on PATH without shelling out. On Unix a candidate must be
/// executable, so a stray non-executable file cannot shadow a real rclone.
pub fn find_in_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(program);
        if candidate.is_file() && is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        true
    }
}

fn find_rclone_on_path() -> Option<PathBuf> {
    executable_names()
        .iter()
        .find_map(|name| find_in_path(name))
}

impl Rclone {
    /// Resolve rclone from the explicit setting first, then PATH.
    pub fn locate(configured: Option<&str>) -> Self {
        if let Some(configured) = configured.map(str::trim).filter(|value| !value.is_empty()) {
            let path = PathBuf::from(configured);
            if path.is_file() {
                return Self {
                    program: Some(path),
                    source: "configured".to_string(),
                };
            }
            // Fall back to PATH but remember that the setting is broken.
            if let Some(found) = find_rclone_on_path() {
                return Self {
                    program: Some(found),
                    source: "path".to_string(),
                };
            }
            return Self {
                program: None,
                source: "missing".to_string(),
            };
        }

        match find_rclone_on_path() {
            Some(path) => Self {
                program: Some(path),
                source: "path".to_string(),
            },
            None => Self {
                program: None,
                source: "missing".to_string(),
            },
        }
    }

    pub fn program(&self) -> Result<&Path, String> {
        self.program.as_deref().ok_or_else(|| {
            "rclone was not found. Install rclone and, if it is not on PATH, set its location in Settings."
                .to_string()
        })
    }

    /// Run rclone, capture stdout, and turn failures into actionable messages.
    ///
    /// Bounded by a timeout: an unreachable S3 endpoint makes rclone retry for minutes
    /// with no output, which would otherwise leave a button spinning forever.
    pub async fn captured(&self, args: &[String]) -> Result<String, String> {
        const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

        let program = self.program()?;
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let child = command.spawn().map_err(|error| {
            format!(
                "Cannot run rclone at {}: {error}",
                program.to_string_lossy()
            )
        })?;
        let pid = child.id();

        let output = match tokio::time::timeout(TIMEOUT, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => return Err(format!("rclone did not finish: {error}")),
            Err(_) => {
                if let Some(pid) = pid {
                    kill_process(pid);
                }
                return Err(format!(
                    "rclone did not answer within {} seconds. Check the endpoint URL, the region, your network and the secret key.",
                    TIMEOUT.as_secs()
                ));
            }
        };

        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).to_string());
        }

        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let detail = if stderr.trim().is_empty() {
            stdout
        } else {
            stderr
        };
        Err(rclone::friendly_error(&detail))
    }

    pub async fn info(&self) -> RcloneInfo {
        let Some(program) = self.program.clone() else {
            return RcloneInfo {
                available: false,
                program: None,
                version: None,
                source: self.source.clone(),
                message: Some(
                    "rclone was not found. Install it from https://rclone.org/downloads/ and reopen S3Dive, or set its path in Settings."
                        .to_string(),
                ),
            };
        };

        match self.captured(&rclone::version_args()).await {
            Ok(stdout) => RcloneInfo {
                available: true,
                program: Some(program.to_string_lossy().to_string()),
                version: rclone::parse_version(&stdout),
                source: self.source.clone(),
                message: None,
            },
            Err(message) => RcloneInfo {
                available: false,
                program: Some(program.to_string_lossy().to_string()),
                version: None,
                source: self.source.clone(),
                message: Some(message),
            },
        }
    }
}

/// Probe the platform's mount prerequisite (FUSE / macFUSE / WinFsp).
pub fn fuse_status_now(os: &Os) -> FuseStatus {
    let exists = |path: &str| Path::new(path).exists();
    let in_path = |program: &str| find_in_path(program).is_some();
    fuse_status(os, &exists, &in_path)
}

/// Terminate a child by pid.
///
/// Used where the process handle is owned by an awaited `wait()` future and cannot be
/// borrowed again (cancellation inside `tokio::select!`), and on app exit, where the
/// async runtime is shutting down.
pub fn kill_process(pid: u32) {
    terminate(pid, false);
    // Escalate when the child ignores SIGTERM: a stuck mount or mirror must still die.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(3));
        if is_running(pid) {
            terminate(pid, true);
        }
    });
}

#[cfg(unix)]
fn terminate(pid: u32, force: bool) {
    let _ = std::process::Command::new("kill")
        .arg(if force { "-KILL" } else { "-TERM" })
        .arg(pid.to_string())
        .status();
}

#[cfg(unix)]
fn is_running(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn terminate(pid: u32, _force: bool) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .status();
}

#[cfg(windows)]
fn is_running(_pid: u32) -> bool {
    false
}

#[cfg(not(any(unix, windows)))]
fn terminate(pid: u32, _force: bool) {
    let _ = pid;
}

#[cfg(not(any(unix, windows)))]
fn is_running(_pid: u32) -> bool {
    false
}
