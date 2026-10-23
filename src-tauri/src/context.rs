//! Shared in-process state: the persisted document plus the live process registry.

use crate::paths::Paths;
use crate::runner::{fuse_status_now, Rclone};
use s3dive_core::model::AppState;
use s3dive_core::platform::{FuseStatus, Os};
use s3dive_core::store::{load_state, quarantine_state, save_state};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use tokio::sync::oneshot;

/// A running job: cancelling it asks the supervising task to kill the child.
pub struct RunEntry {
    pub job_id: String,
    /// Set once the child exists, so quitting the app can terminate it.
    pub pid: Option<u32>,
    /// Taken when the run is cancelled. The entry stays in the registry until the
    /// child is really dead, so its pid can still be killed on app exit.
    pub cancel: Option<oneshot::Sender<()>>,
}

/// A live mount child process plus what the UI needs to know about it.
pub struct MountEntry {
    /// Taken when the mount is stopped. The entry stays in the registry until the
    /// child is really dead, so its pid can still be killed on app exit.
    pub cancel: Option<oneshot::Sender<()>>,
    pub runtime: MountRuntime,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountRuntime {
    pub mount_id: String,
    pub running: bool,
    pub pid: Option<u32>,
    pub detail: String,
    pub started_at_ms: Option<u64>,
}

/// A poisoned mutex still holds valid data. Recovering keeps the app usable instead of
/// turning one panicking task into a permanently broken UI on every later command.
pub trait LockExt<T> {
    fn lock_recover(&self) -> std::sync::MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn lock_recover(&self) -> std::sync::MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub struct AppContext {
    pub paths: Paths,
    pub os: Os,
    /// Populated when the state file could not be read at startup.
    pub load_warning: Option<String>,
    state: Mutex<AppState>,
    pub runs: Mutex<HashMap<String, RunEntry>>,
    /// One entry per job that currently has a live child, claimed before spawning.
    pub active_jobs: Mutex<HashSet<String>>,
    pub mounts: Mutex<HashMap<String, MountEntry>>,
}

impl AppContext {
    pub fn load(paths: Paths) -> Self {
        let (state, load_warning) = match load_state(&paths.state_file()) {
            Ok(state) => (state, None),
            Err(error) => {
                // Never let the first save destroy a file we could not read: keep a copy
                // next to it and say where it went.
                let warning = match crate::context::quarantine(&paths) {
                    Some(backup) => format!(
                        "{error} The unreadable file was kept as {}.",
                        backup.display()
                    ),
                    None => error.to_string(),
                };
                (AppState::default(), Some(warning))
            }
        };

        Self {
            paths,
            os: Os::current(),
            load_warning,
            state: Mutex::new(state),
            runs: Mutex::new(HashMap::new()),
            active_jobs: Mutex::new(HashSet::new()),
            mounts: Mutex::new(HashMap::new()),
        }
    }

    pub fn state(&self) -> AppState {
        self.state.lock_recover().clone()
    }

    /// Apply a change and persist it. On failure nothing changes, in memory or on
    /// disk, so the UI never shows state the disk does not have.
    pub fn mutate<R>(
        &self,
        change: impl FnOnce(&mut AppState) -> Result<R, String>,
    ) -> Result<(R, AppState), String> {
        let mut guard = self.state.lock_recover();
        let backup = guard.clone();

        let value = match change(&mut guard) {
            Ok(value) => value,
            Err(error) => {
                *guard = backup;
                return Err(error);
            }
        };

        if let Err(error) = save_state(&self.paths.state_file(), &guard) {
            *guard = backup;
            return Err(error.to_string());
        }

        Ok((value, guard.clone()))
    }

    pub fn rclone(&self) -> Rclone {
        let configured = self.state().settings.rclone_path;
        Rclone::locate(configured.as_deref())
    }

    /// Pids of every live job child, for teardown on exit.
    pub fn running_job_pids(&self) -> Vec<u32> {
        self.runs
            .lock_recover()
            .values()
            .filter_map(|entry| entry.pid)
            .collect()
    }

    pub fn fuse(&self) -> FuseStatus {
        fuse_status_now(&self.os)
    }
}

/// Keep a copy of a state file we could not read, so the next save does not erase it.
fn quarantine(paths: &Paths) -> Option<std::path::PathBuf> {
    quarantine_state(&paths.state_file()).ok().flatten()
}
