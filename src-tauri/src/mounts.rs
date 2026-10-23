//! Mount supervision: one long-lived rclone child per mount, plus auto-mount at launch.

use crate::context::{AppContext, LockExt, MountEntry, MountRuntime};
use s3dive_core::model::{now_ms, Mount};
use s3dive_core::rclone;
use s3dive_core::validate;
use serde::Serialize;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountLine {
    pub mount_id: String,
    pub line: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountChanged {
    pub mount_id: String,
    pub runtime: MountRuntime,
}

fn emit_change(app: &AppHandle, runtime: &MountRuntime) {
    let _ = app.emit(
        "mount:changed",
        MountChanged {
            mount_id: runtime.mount_id.clone(),
            runtime: runtime.clone(),
        },
    );
}

/// Start one mount. Returns the runtime immediately; the child is supervised in the
/// background so a crashed mount is reflected in the UI.
pub async fn start(app: AppHandle, mount_id: String) -> Result<MountRuntime, String> {
    let (mount, program, args) = {
        let context = app.state::<AppContext>();
        let state = context.state();
        let mount = state
            .find_mount(&mount_id)
            .cloned()
            .ok_or_else(|| format!("Mount '{mount_id}' no longer exists."))?;
        validate::validate_mount(&state, &mount)?;

        let fuse = context.fuse();
        if !fuse.available {
            let mut message = format!("{}: {}", fuse.summary, fuse.detail);
            if let Some(hint) = fuse.hint {
                message.push_str(&format!(" {hint}"));
            }
            if let Some(url) = fuse.install_url {
                message.push_str(&format!(" ({url})"));
            }
            return Err(message);
        }

        let mount_point = Path::new(&mount.mount_point);
        if !mount_point.exists() {
            return Err(format!(
                "The mount point {} does not exist. Create the folder first.",
                mount.mount_point
            ));
        }
        if !mount_point.is_dir() {
            return Err(format!(
                "The mount point {} is not a directory.",
                mount.mount_point
            ));
        }
        match std::fs::read_dir(mount_point) {
            Ok(mut entries) => {
                if entries.next().is_some() {
                    return Err(format!(
                        "The mount point {} must be empty before mounting.",
                        mount.mount_point
                    ));
                }
            }
            Err(error) => {
                return Err(format!(
                    "Cannot read the mount point {}: {error}",
                    mount.mount_point
                ));
            }
        }

        // A mount point inside a synced folder would make rclone read its own output.
        for job in &state.jobs {
            if validate::paths_overlap(mount_point, Path::new(&job.local_path)) {
                return Err(format!(
                    "The mount point {} overlaps the folder of job '{}'. Choose a separate directory.",
                    mount.mount_point, job.name
                ));
            }
        }

        for other in &state.mounts {
            if other.id != mount.id
                && validate::paths_overlap(
                    Path::new(&other.mount_point),
                    Path::new(&mount.mount_point),
                )
            {
                return Err(format!(
                    "Mount point {} overlaps or nests with '{}' at {}.",
                    mount.mount_point, other.name, other.mount_point
                ));
            }
        }

        let rclone = context.rclone();
        let program = rclone.program()?.to_path_buf();
        let args = rclone::mount_args(
            &mount,
            &state.settings,
            &context.paths.conf_arg(),
            &context.os,
        );
        (mount, program, args)
    };

    // rclone mounts a missing bucket without complaint and then fails every write,
    // so check that the bucket (first path segment) exists. Network errors fall through.
    if let Some(bucket) = mount
        .remote_path
        .trim()
        .trim_matches('/')
        .split('/')
        .next()
        .filter(|b| !b.is_empty())
    {
        let context = app.state::<AppContext>();
        let probe = rclone::browse_args(&mount.remote, bucket, &context.paths.conf_arg());
        if let Err(error) = context.rclone().captured(&probe).await {
            if error.to_lowercase().contains("directory not found") {
                return Err(format!(
                    "Bucket '{bucket}' does not exist on '{}'. The folder path must start with the bucket name, e.g. my-bucket/photos.",
                    mount.remote
                ));
            }
        }
    }

    // Claim the mount before spawning: check and insert under one lock guard, so two
    // concurrent starts cannot both create a child for the same mount point.
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let starting = MountRuntime {
        mount_id: mount.id.clone(),
        running: true,
        pid: None,
        detail: "Starting rclone mount…".to_string(),
        started_at_ms: Some(now_ms()),
    };
    {
        let context = app.state::<AppContext>();
        let mut live = context.mounts.lock_recover();
        if live.contains_key(&mount.id) {
            return Err(format!("'{}' is already mounted.", mount.name));
        }
        live.insert(
            mount.id.clone(),
            MountEntry {
                cancel: Some(cancel_tx),
                runtime: starting,
            },
        );
    }

    let mut child = match Command::new(program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            let context = app.state::<AppContext>();
            context.mounts.lock_recover().remove(&mount.id);
            return Err(format!("Cannot start rclone mount: {error}"));
        }
    };

    let pid = child.id();
    let stderr = child.stderr.take();
    let last_line = Arc::new(StdMutex::new(String::new()));

    let runtime = MountRuntime {
        mount_id: mount.id.clone(),
        running: true,
        pid,
        detail: "Starting rclone mount…".to_string(),
        started_at_ms: Some(now_ms()),
    };

    // The claim can disappear between the spawn and this update when the user cancels a
    // mount that was still starting. That child would then be owned by nobody, so kill it.
    let still_claimed = {
        let context = app.state::<AppContext>();
        let mut live = context.mounts.lock_recover();
        match live.get_mut(&mount.id) {
            Some(entry) => {
                entry.runtime = runtime.clone();
                true
            }
            None => false,
        }
    };
    if !still_claimed {
        if let Some(pid) = pid {
            crate::runner::kill_process(pid);
        }
        return Err(format!("'{}' was cancelled while starting.", mount.name));
    }
    emit_change(&app, &runtime);

    if let Some(stderr) = stderr {
        let emit_app = app.clone();
        let emit_id = mount.id.clone();
        let shared = last_line.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(mut guard) = shared.lock() {
                    *guard = line.clone();
                }
                let _ = emit_app.emit(
                    "mount:line",
                    MountLine {
                        mount_id: emit_id.clone(),
                        line,
                    },
                );
            }
        });
    }

    let watch_app = app.clone();
    let watch_id = mount.id.clone();
    tauri::async_runtime::spawn(async move {
        // As in the job runner: `wait()` holds the child borrow, so cancellation goes
        // through the pid and the handle is awaited only after the select resolves.
        let exited = tokio::select! {
            status = child.wait() => {
                Some(match status {
                    Ok(status) if status.success() => "rclone mount exited.".to_string(),
                    Ok(status) => match status.code() {
                        Some(code) => format!("rclone mount exited with status {code}."),
                        None => "rclone mount was terminated by a signal.".to_string(),
                    },
                    Err(error) => format!("Cannot wait for rclone mount: {error}"),
                })
            }
            _ = cancel_rx => {
                if let Some(pid) = pid {
                    crate::runner::kill_process(pid);
                }
                None
            }
        };

        let reason = match exited {
            Some(reason) => reason,
            None => {
                let _ = child.wait().await;
                "Unmounted by the user.".to_string()
            }
        };

        let stray = last_line
            .lock()
            .map(|guard| guard.trim().to_string())
            .unwrap_or_default();
        let detail = if stray.is_empty() {
            reason
        } else {
            format!("{reason} Last message: {stray}")
        };

        let context = watch_app.state::<AppContext>();
        context.mounts.lock_recover().remove(&watch_id);

        let runtime = MountRuntime {
            mount_id: watch_id.clone(),
            running: false,
            pid: None,
            detail,
            started_at_ms: None,
        };
        emit_change(&watch_app, &runtime);
    });

    Ok(runtime)
}

/// Ask a mount to unmount.
///
/// The entry stays in the registry until the supervising task sees the child exit,
/// so the pid stays killable: quitting the app right after an unmount still kills the
/// rclone child instead of leaving a stale mount behind.
pub fn stop(app: AppHandle, mount_id: &str) -> Result<(), String> {
    let context = app.state::<AppContext>();
    stop_mount(&context, mount_id)
}

fn stop_mount(context: &AppContext, mount_id: &str) -> Result<(), String> {
    let sender = {
        let mut live = context.mounts.lock_recover();
        let entry = live
            .get_mut(mount_id)
            .ok_or_else(|| "That mount is not running.".to_string())?;
        entry.cancel.take()
    };

    match sender {
        Some(sender) => sender
            .send(())
            .map_err(|_| "That mount had already stopped.".to_string()),
        None => Err("That mount is already unmounting.".to_string()),
    }
}

/// Best-effort synchronous teardown used on process exit, where the async runtime
/// is already shutting down and a cancel message would never be delivered.
pub fn kill_all(app: &AppHandle) {
    let context = app.state::<AppContext>();
    let pids: Vec<u32> = context
        .mounts
        .lock_recover()
        .values()
        .filter_map(|entry| entry.runtime.pid)
        .collect();

    for pid in pids {
        crate::runner::kill_process(pid);
    }
    context.mounts.lock_recover().clear();
}

pub fn states(app: &AppHandle) -> Vec<MountRuntime> {
    let context = app.state::<AppContext>();
    let live: Vec<MountRuntime> = context
        .mounts
        .lock_recover()
        .values()
        .map(|entry| entry.runtime.clone())
        .collect();
    live
}

/// Bring up every mount flagged `autoMount`. Failures are surfaced as mount events
/// instead of aborting startup, so one broken mount cannot block the others.
pub async fn start_auto_mounts(app: AppHandle) {
    let flagged: Vec<Mount> = {
        let context = app.state::<AppContext>();
        context
            .state()
            .mounts
            .into_iter()
            .filter(|mount| mount.auto_mount)
            .collect()
    };

    for mount in flagged {
        if let Err(error) = start(app.clone(), mount.id.clone()).await {
            let runtime = MountRuntime {
                mount_id: mount.id.clone(),
                running: false,
                pid: None,
                detail: format!("Auto-mount failed: {error}"),
                started_at_ms: None,
            };
            emit_change(&app, &runtime);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    #[test]
    fn a_stopped_mount_stays_killable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let context = AppContext::load(Paths::new(dir.path().to_path_buf()));

        let (sender, _receiver) = tokio::sync::oneshot::channel();
        context.mounts.lock_recover().insert(
            "mount-1".to_string(),
            MountEntry {
                cancel: Some(sender),
                runtime: MountRuntime {
                    mount_id: "mount-1".to_string(),
                    running: true,
                    pid: Some(4343),
                    detail: "running".to_string(),
                    started_at_ms: None,
                },
            },
        );

        stop_mount(&context, "mount-1").expect("stop the mount");

        let pids: Vec<u32> = context
            .mounts
            .lock_recover()
            .values()
            .filter_map(|entry| entry.runtime.pid)
            .collect();
        assert_eq!(
            pids,
            vec![4343],
            "the pid left the registry before the child died"
        );
    }

    #[test]
    fn stopping_an_unknown_mount_says_it_is_not_running() {
        let dir = tempfile::tempdir().expect("tempdir");
        let context = AppContext::load(Paths::new(dir.path().to_path_buf()));

        let error = stop_mount(&context, "missing").expect_err("there is no such mount");
        assert!(error.contains("not running"), "{error}");
    }
}
