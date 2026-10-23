//! On-disk persistence: one JSON state file and one app-owned `rclone.conf`.
//!
//! Both contain credentials-adjacent material (access key IDs, obscured secrets), so
//! both are written atomically with mode 0600 on Unix.

use crate::model::{AppState, STATE_VERSION};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum StoreError {
    Io { path: String, detail: String },
    Corrupt { path: String, detail: String },
    UnsupportedVersion { found: u32, supported: u32 },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io { path, detail } => write!(formatter, "Cannot access {path}: {detail}"),
            StoreError::Corrupt { path, detail } => write!(
                formatter,
                "{path} is not readable S3Dive state ({detail}). Move it aside to start fresh."
            ),
            StoreError::UnsupportedVersion { found, supported } => write!(
                formatter,
                "State file was written by a newer S3Dive (schema {found}, this build supports {supported})."
            ),
        }
    }
}

impl std::error::Error for StoreError {}

fn io_error(path: &Path, error: std::io::Error) -> StoreError {
    StoreError::Io {
        path: path.display().to_string(),
        detail: error.to_string(),
    }
}

/// Read the state file. A missing file is an empty state, not an error.
pub fn load_state(path: &Path) -> Result<AppState, StoreError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(AppState::default()),
        Err(error) => return Err(io_error(path, error)),
    };

    if contents.trim().is_empty() {
        return Ok(AppState::default());
    }

    let state: AppState = serde_json::from_str(&contents).map_err(|error| StoreError::Corrupt {
        path: path.display().to_string(),
        detail: error.to_string(),
    })?;

    if state.schema_version > STATE_VERSION {
        return Err(StoreError::UnsupportedVersion {
            found: state.schema_version,
            supported: STATE_VERSION,
        });
    }

    Ok(state)
}

/// Write the state file atomically.
pub fn save_state(path: &Path, state: &AppState) -> Result<(), StoreError> {
    let json = serde_json::to_string_pretty(state).expect("app state is always serializable");
    write_private_file(path, &json)
}

/// Write a file that may hold credentials: temp file in the same directory,
/// mode 0600, then an atomic rename.
pub fn write_private_file(path: &Path, contents: &str) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    }

    let temp = temp_path(path);
    let mut file = open_private(&temp).map_err(|error| io_error(&temp, error))?;
    {
        use std::io::Write;
        file.write_all(contents.as_bytes())
            .map_err(|error| io_error(&temp, error))?;
        file.flush().map_err(|error| io_error(&temp, error))?;
    }
    drop(file);

    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        io_error(path, error)
    })
}

/// Create the file already at 0600, so a crash mid-write cannot leave a
/// world-readable temp file behind.
fn open_private(path: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Move an unreadable state file aside so the next save cannot destroy it.
/// Returns the backup path when a file was actually moved.
pub fn quarantine_state(path: &Path) -> Result<Option<PathBuf>, StoreError> {
    if !path.exists() {
        return Ok(None);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|delta| delta.as_secs())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "state.json".to_string());
    let backup = path.with_file_name(format!("{file_name}.corrupt-{stamp}"));
    fs::rename(path, &backup).map_err(|error| io_error(path, error))?;
    Ok(Some(backup))
}

fn temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "s3dive".to_string());
    path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Job, JobKind, Mount, Remote};

    fn sample_state() -> AppState {
        let mut state = AppState::default();
        state.remotes.push(Remote::new("media", "AKIAEXAMPLE"));
        state.jobs.push(Job::new(
            "Photos",
            JobKind::Sync,
            "media",
            "photos",
            "/home/me/Pictures",
        ));
        state
            .mounts
            .push(Mount::new("Media", "media", "photos", "/mnt/media"));
        state
    }

    #[test]
    fn state_round_trips_through_the_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.json");
        let state = sample_state();

        save_state(&path, &state).expect("save");
        let loaded = load_state(&path).expect("load");
        assert_eq!(loaded, state);
    }

    #[test]
    fn a_missing_file_is_an_empty_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested/dir/state.json");
        let loaded = load_state(&path).expect("load");
        assert_eq!(loaded, AppState::default());
    }

    #[test]
    fn an_empty_file_is_an_empty_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.json");
        fs::write(&path, "  \n").expect("write");
        assert_eq!(load_state(&path).expect("load"), AppState::default());
    }

    #[test]
    fn corrupt_state_is_reported_with_the_path_and_a_way_out() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.json");
        fs::write(&path, "{ not json").expect("write");

        let error = load_state(&path).expect_err("corrupt state must not be silently replaced");
        let message = error.to_string();
        assert!(message.contains("state.json"), "{message}");
        assert!(message.contains("Move it aside"), "{message}");
    }

    #[test]
    fn state_from_a_newer_schema_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.json");
        fs::write(
            &path,
            r#"{"schemaVersion": 99, "remotes": [], "jobs": [], "mounts": []}"#,
        )
        .expect("write");

        let error = load_state(&path).expect_err("newer schema must be refused");
        assert!(matches!(
            error,
            StoreError::UnsupportedVersion {
                found: 99,
                supported: STATE_VERSION
            }
        ));
    }

    #[test]
    fn files_are_written_owner_only_and_leave_no_temp_trailing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("rclone.conf");
        write_private_file(&path, "[media]\ntype = s3\n").expect("write");

        assert_eq!(
            fs::read_to_string(&path).expect("read"),
            "[media]\ntype = s3\n"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("metadata").permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "mode was {:o}", mode & 0o777);
        }

        let leftovers: Vec<String> = fs::read_dir(dir.path())
            .expect("read_dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");
    }

    #[test]
    fn private_files_are_created_with_restricted_mode_not_fixed_up_later() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("secret.conf");
        let file = {
            let mut options = fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(&path).expect("create")
        };
        drop(file);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("metadata").permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "created with {:o}", mode & 0o777);
        }
    }

    #[test]
    fn a_corrupt_state_file_is_moved_aside_instead_of_overwritten() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.json");
        fs::write(&path, "{ not json").expect("write");

        assert!(load_state(&path).is_err());
        let backup = quarantine_state(&path)
            .expect("quarantine")
            .expect("a file was moved");
        assert!(backup.exists());
        assert_eq!(fs::read_to_string(&backup).expect("read"), "{ not json");
        assert!(!path.exists());

        assert_eq!(quarantine_state(&path).expect("no-op"), None);
    }

    #[test]
    fn saving_overwrites_previous_contents() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.json");

        let mut state = sample_state();
        save_state(&path, &state).expect("save");
        state.remotes.clear();
        save_state(&path, &state).expect("save again");

        let loaded = load_state(&path).expect("load");
        assert!(loaded.remotes.is_empty());
    }
}
