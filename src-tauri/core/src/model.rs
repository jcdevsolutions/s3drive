//! Persisted domain model.
//!
//! JSON is serialized as camelCase because the only consumer is the TypeScript UI.
//! Secrets are deliberately absent: S3 keys live in the app-owned `rclone.conf`.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Version of the on-disk application state.
///
/// v2 replaces the remote name in a job or a mount with the id of the remote.
pub const STATE_VERSION: u32 = 2;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_millis() as u64)
        .unwrap_or(0)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn default_region() -> String {
    "us-east-1".to_string()
}

/// Which S3 dialect the remote speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum S3Provider {
    #[default]
    Aws,
    Other,
}

impl S3Provider {
    pub fn rclone_value(self) -> &'static str {
        match self {
            S3Provider::Aws => "AWS",
            S3Provider::Other => "Other",
        }
    }
}

/// An S3 remote. Never holds the secret key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Remote {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub provider: S3Provider,
    pub access_key_id: String,
    #[serde(default = "default_region")]
    pub region: String,
    /// Custom endpoint for S3-compatible services (MinIO, R2, Wasabi...).
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Path-style addressing, required by most non-AWS endpoints.
    #[serde(default)]
    pub force_path_style: bool,
    pub created_at_ms: u64,
}

impl Remote {
    pub fn new(name: impl Into<String>, access_key_id: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            name: name.into(),
            provider: S3Provider::Aws,
            access_key_id: access_key_id.into(),
            region: default_region(),
            endpoint: None,
            force_path_style: false,
            created_at_ms: now_ms(),
        }
    }

    /// `remote:bucket/prefix` for this remote.
    pub fn spec(&self, path: &str) -> String {
        crate::rclone::remote_spec(&self.name, path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// Mirror: makes the destination identical, deleting extras there.
    Sync,
    /// Additive: copies without deleting anything at the destination.
    Copy,
    /// Two-way, conflict-detecting.
    Bisync,
}

impl JobKind {
    pub fn transfer_command(self) -> &'static str {
        match self {
            JobKind::Sync => "sync",
            JobKind::Copy => "copy",
            JobKind::Bisync => "bisync",
        }
    }

    /// True when the command can delete data at the destination.
    pub fn deletes_destination(self) -> bool {
        matches!(self, JobKind::Sync)
    }

    pub fn requires_direction(self) -> bool {
        !matches!(self, JobKind::Bisync)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Local folder to bucket.
    #[default]
    Push,
    /// Bucket to local folder.
    Pull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    #[default]
    Running,
    Success,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub run_id: String,
    pub started_at_ms: u64,
    #[serde(default)]
    pub finished_at_ms: Option<u64>,
    #[serde(default)]
    pub outcome: RunOutcome,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub name: String,
    pub kind: JobKind,
    #[serde(default)]
    pub direction: Direction,
    /// The id of the remote this job transfers to or from.
    ///
    /// The id, not the name: a name can be changed, and a job that follows a
    /// renamed bucket would move to a different one without the user asking.
    #[serde(default)]
    pub remote_id: String,
    /// The remote name used before schema v2. It is read to migrate an old state
    /// file and is never written back.
    #[serde(default, rename = "remote", skip_serializing)]
    pub legacy_remote: String,
    /// Bucket and optional prefix, without a leading slash.
    #[serde(default)]
    pub remote_path: String,
    pub local_path: String,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    pub created_at_ms: u64,
    #[serde(default)]
    pub last_run: Option<RunSummary>,
}

impl Job {
    pub fn new(
        name: impl Into<String>,
        kind: JobKind,
        remote_id: impl Into<String>,
        remote_path: impl Into<String>,
        local_path: impl Into<String>,
    ) -> Self {
        Self {
            id: new_id(),
            name: name.into(),
            kind,
            direction: Direction::default(),
            remote_id: remote_id.into(),
            legacy_remote: String::new(),
            remote_path: remote_path.into(),
            local_path: local_path.into(),
            dry_run: false,
            include: Vec::new(),
            exclude: Vec::new(),
            created_at_ms: now_ms(),
            last_run: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VfsCacheMode {
    Off,
    Minimal,
    #[default]
    Writes,
    Full,
}

impl VfsCacheMode {
    pub fn rclone_value(self) -> &'static str {
        match self {
            VfsCacheMode::Off => "off",
            VfsCacheMode::Minimal => "minimal",
            VfsCacheMode::Writes => "writes",
            VfsCacheMode::Full => "full",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mount {
    pub id: String,
    pub name: String,
    /// The id of the remote this mount exposes.
    #[serde(default)]
    pub remote_id: String,
    /// The remote name used before schema v2. It is read to migrate an old state
    /// file and is never written back.
    #[serde(default, rename = "remote", skip_serializing)]
    pub legacy_remote: String,
    /// Bucket and optional prefix, without a leading slash.
    #[serde(default)]
    pub remote_path: String,
    pub mount_point: String,
    /// Start this mount when S3Dive launches at login.
    #[serde(default)]
    pub auto_mount: bool,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub allow_other: bool,
    #[serde(default)]
    pub vfs_cache_mode: VfsCacheMode,
    pub created_at_ms: u64,
}

impl Mount {
    pub fn new(
        name: impl Into<String>,
        remote_id: impl Into<String>,
        remote_path: impl Into<String>,
        mount_point: impl Into<String>,
    ) -> Self {
        Self {
            id: new_id(),
            name: name.into(),
            remote_id: remote_id.into(),
            legacy_remote: String::new(),
            remote_path: remote_path.into(),
            mount_point: mount_point.into(),
            auto_mount: false,
            read_only: false,
            allow_other: false,
            vfs_cache_mode: VfsCacheMode::default(),
            created_at_ms: now_ms(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Explicit rclone binary. `None` means "find it on PATH".
    #[serde(default)]
    pub rclone_path: Option<String>,
    /// Free-form extra flags appended to every rclone invocation.
    #[serde(default)]
    pub default_flags: Vec<String>,
    /// `--bwlimit` value, e.g. "8M".
    #[serde(default)]
    pub bandwidth_limit: Option<String>,
}

/// Everything S3Dive persists, in one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub remotes: Vec<Remote>,
    #[serde(default)]
    pub jobs: Vec<Job>,
    #[serde(default)]
    pub mounts: Vec<Mount>,
}

fn default_schema_version() -> u32 {
    STATE_VERSION
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            schema_version: STATE_VERSION,
            settings: Settings::default(),
            remotes: Vec::new(),
            jobs: Vec::new(),
            mounts: Vec::new(),
        }
    }
}

impl AppState {
    /// Give every record an identity.
    ///
    /// A state file written by an older build could hold an empty id. Every
    /// lookup by id then pointed at the same record, so two buckets could not
    /// coexist. Repairing the ids on load keeps such a file usable.
    pub fn ensure_ids(&mut self) {
        for remote in self.remotes.iter_mut() {
            if remote.id.trim().is_empty() {
                remote.id = new_id();
            }
        }
        for job in self.jobs.iter_mut() {
            if job.id.trim().is_empty() {
                job.id = new_id();
            }
        }
        for mount in self.mounts.iter_mut() {
            if mount.id.trim().is_empty() {
                mount.id = new_id();
            }
        }
    }

    pub fn find_remote(&self, name: &str) -> Option<&Remote> {
        self.remotes.iter().find(|remote| remote.name == name)
    }

    /// The remote a job or a mount points at.
    pub fn find_remote_by_id(&self, id: &str) -> Option<&Remote> {
        self.remotes.iter().find(|remote| remote.id == id)
    }

    pub fn find_job(&self, id: &str) -> Option<&Job> {
        self.jobs.iter().find(|job| job.id == id)
    }

    pub fn find_mount(&self, id: &str) -> Option<&Mount> {
        self.mounts.iter().find(|mount| mount.id == id)
    }

    /// Remote names double as rclone config section names, so they must be unique.
    pub fn remote_name_taken(&self, name: &str, except_id: Option<&str>) -> bool {
        self.remotes.iter().any(|remote| {
            remote.name.eq_ignore_ascii_case(name) && Some(remote.id.as_str()) != except_id
        })
    }

    /// Every remote a job or mount points at must still exist.
    pub fn references_remote(&self, id: &str) -> bool {
        self.jobs.iter().any(|job| job.remote_id == id)
            || self.mounts.iter().any(|mount| mount.remote_id == id)
    }

    /// Jobs and mounts whose remote is gone.
    ///
    /// They are reported instead of being re-pointed at another bucket: a
    /// transfer that silently changes its source is worse than one that stops.
    pub fn dangling_references(&self) -> Vec<String> {
        let mut dangling = Vec::new();
        for job in &self.jobs {
            if self.find_remote_by_id(&job.remote_id).is_none() {
                dangling.push(format!("Job '{}' has no bucket.", job.name));
            }
        }
        for mount in &self.mounts {
            if self.find_remote_by_id(&mount.remote_id).is_none() {
                dangling.push(format!("Mount '{}' has no bucket.", mount.name));
            }
        }
        dangling
    }

    /// Resolve the pre-v2 remote names into remote ids.
    ///
    /// Before schema v2 a job and a mount stored the name of their remote, and
    /// the name was a mutable key: renaming a bucket moved every job that used
    /// it. The name is resolved once, here, and the id takes over from then on.
    /// A reference whose remote is gone keeps an empty id and is reported by
    /// `dangling_references`.
    pub fn migrate_references(&mut self) {
        let by_name: std::collections::HashMap<&str, &str> = self
            .remotes
            .iter()
            .map(|remote| (remote.name.as_str(), remote.id.as_str()))
            .collect();

        for job in self.jobs.iter_mut() {
            if job.remote_id.is_empty() && !job.legacy_remote.is_empty() {
                job.remote_id = by_name
                    .get(job.legacy_remote.as_str())
                    .map(|id| id.to_string())
                    .unwrap_or_default();
            }
        }
        for mount in self.mounts.iter_mut() {
            if mount.remote_id.is_empty() && !mount.legacy_remote.is_empty() {
                mount.remote_id = by_name
                    .get(mount.legacy_remote.as_str())
                    .map(|id| id.to_string())
                    .unwrap_or_default();
            }
        }

        self.schema_version = STATE_VERSION;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_kind_maps_to_rclone_commands() {
        assert_eq!(JobKind::Sync.transfer_command(), "sync");
        assert_eq!(JobKind::Copy.transfer_command(), "copy");
        assert_eq!(JobKind::Bisync.transfer_command(), "bisync");
    }

    #[test]
    fn only_sync_deletes_at_destination() {
        assert!(JobKind::Sync.deletes_destination());
        assert!(!JobKind::Copy.deletes_destination());
        assert!(!JobKind::Bisync.deletes_destination());
        assert!(!JobKind::Bisync.requires_direction());
        assert!(JobKind::Sync.requires_direction());
    }

    #[test]
    fn state_serializes_with_camel_case_keys() {
        let mut state = AppState::default();
        state.remotes.push(Remote::new("media", "AKIAEXAMPLE"));
        state.jobs.push(Job::new(
            "Photos",
            JobKind::Bisync,
            "media",
            "photos/raw",
            "/home/me/Pictures",
        ));

        let json = serde_json::to_string(&state).expect("serialize");
        assert!(json.contains("\"schemaVersion\""), "json: {json}");
        assert!(json.contains("\"accessKeyId\""), "json: {json}");
        assert!(json.contains("\"localPath\""), "json: {json}");
        assert!(json.contains("\"remotePath\""), "json: {json}");

        let round_trip: AppState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(round_trip, state);
    }

    #[test]
    fn missing_optional_fields_deserialize_with_defaults() {
        let json = r#"{
            "remotes": [{
                "id": "r1",
                "name": "media",
                "accessKeyId": "AKIA",
                "createdAtMs": 1
            }],
            "jobs": [],
            "mounts": []
        }"#;
        let state: AppState = serde_json::from_str(json).expect("deserialize legacy state");
        assert_eq!(state.schema_version, STATE_VERSION);
        assert_eq!(state.remotes[0].region, "us-east-1");
        assert_eq!(state.remotes[0].provider, S3Provider::Aws);
        assert_eq!(state.jobs.len(), 0);
    }

    #[test]
    fn secrets_are_never_part_of_the_persisted_model() {
        let mut remote = Remote::new("media", "AKIAEXAMPLE");
        remote.endpoint = Some("https://minio.local:9000".into());
        let json = serde_json::to_string(&remote).expect("serialize");
        assert!(!json.to_lowercase().contains("secret"));
    }

    #[test]
    fn remote_names_are_unique_case_insensitively() {
        let mut state = AppState::default();
        state.remotes.push(Remote::new("Media", "AKIA"));
        assert!(state.remote_name_taken("media", None));
        assert!(!state.remote_name_taken("media", Some(&state.remotes[0].id)));
    }

    #[test]
    fn vfs_cache_modes_map_to_rclone_values() {
        assert_eq!(VfsCacheMode::Off.rclone_value(), "off");
        assert_eq!(VfsCacheMode::Minimal.rclone_value(), "minimal");
        assert_eq!(VfsCacheMode::Writes.rclone_value(), "writes");
        assert_eq!(VfsCacheMode::Full.rclone_value(), "full");
    }
}
