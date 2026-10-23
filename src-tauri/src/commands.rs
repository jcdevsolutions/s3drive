//! The Tauri command surface. Every mutation returns the new `AppState` so the UI has
//! exactly one source of truth.

use crate::context::{AppContext, LockExt, MountRuntime};
use crate::runner::RcloneInfo;
use crate::{jobs, mounts};
use s3dive_core::model::{now_ms, AppState, Job, Mount, Remote, RunSummary, Settings};
use s3dive_core::platform::FuseStatus;
use s3dive_core::{rclone, rclone_conf, remotes, store, validate};
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub state: AppState,
    pub rclone: RcloneInfo,
    pub fuse: FuseStatus,
    pub autostart: bool,
    pub platform: String,
    pub config_dir: String,
    pub rclone_conf: String,
    pub state_file: String,
    pub running_mounts: Vec<MountRuntime>,
    /// Non-fatal startup problem, e.g. an unreadable state file.
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTest {
    pub ok: bool,
    pub message: String,
    /// First bucket names seen at the remote root, for an at-a-glance check.
    pub entries: Vec<String>,
}

fn read_conf(context: &AppContext) -> Result<String, String> {
    let path = context.paths.rclone_conf();
    match std::fs::read_to_string(&path) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(format!(
            "Cannot read {}: {error}. Fix its permissions first: S3Dive will not overwrite a config it cannot read.",
            path.display()
        )),
    }
}

fn autostart_enabled(app: &AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub async fn bootstrap(app: AppHandle) -> Result<Bootstrap, String> {
    let context = app.state::<AppContext>();
    let state = context.state();
    let rclone = context.rclone().info().await;
    let fuse = context.fuse();
    let autostart = autostart_enabled(&app);
    let running_mounts = mounts::states(&app);

    Ok(Bootstrap {
        state,
        rclone,
        fuse,
        autostart,
        platform: format!("{:?}", context.os),
        config_dir: context.paths.config_dir.to_string_lossy().to_string(),
        rclone_conf: context.paths.rclone_conf().to_string_lossy().to_string(),
        state_file: context.paths.state_file().to_string_lossy().to_string(),
        running_mounts,
        warning: context.load_warning.clone(),
    })
}

#[tauri::command]
pub async fn refresh_rclone(app: AppHandle) -> Result<RcloneInfo, String> {
    let context = app.state::<AppContext>();
    Ok(context.rclone().info().await)
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, mut settings: Settings) -> Result<AppState, String> {
    let context = app.state::<AppContext>();

    settings.rclone_path = settings
        .rclone_path
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if let Some(configured) = settings.rclone_path.as_deref() {
        let looks_like_path = configured.contains('/') || configured.contains('\\');
        if looks_like_path && !std::path::Path::new(configured).is_file() {
            return Err(format!("No rclone binary found at {configured}."));
        }
    }

    settings.bandwidth_limit = settings
        .bandwidth_limit
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    settings.default_flags = settings
        .default_flags
        .into_iter()
        .map(|flag| flag.trim().to_string())
        .filter(|flag| !flag.is_empty())
        .collect();

    validate::extra_flags(&settings.default_flags)?;

    let (_, snapshot) = context.mutate(|state| {
        state.settings = settings;
        Ok(())
    })?;
    Ok(snapshot)
}

#[tauri::command]
pub async fn save_remote(
    app: AppHandle,
    remote: Remote,
    secret_access_key: Option<String>,
) -> Result<AppState, String> {
    let context = app.state::<AppContext>();
    save_remote_inner(&context, remote, secret_access_key)
}

/// Save a remote: its credentials go to the app-owned `rclone.conf` and its
/// definition to the state file.
///
/// The state transition itself lives in `s3dive_core::remotes`, so it can be
/// tested without a webview; this function only wires it to the filesystem.
fn save_remote_inner(
    context: &AppContext,
    remote: Remote,
    secret_access_key: Option<String>,
) -> Result<AppState, String> {
    let state = context.state();

    let mut record = remote;
    record.name = record.name.trim().to_string();
    record.access_key_id = record.access_key_id.trim().to_string();
    record.region = record.region.trim().to_string();
    record.endpoint = record
        .endpoint
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    validate::validate_remote(&record)?;

    // Plan first: the record gets its identity before anything looks it up, so a
    // new bucket can never be mistaken for an existing one.
    let planned = remotes::plan_remote_update(&state, record)?;
    let record = &planned.record;

    // The name is the rclone config section name, so it must stay unique.
    if state.remote_name_taken(&record.name, Some(record.id.as_str())) {
        return Err(format!("A bucket named '{}' already exists.", record.name));
    }

    // The credentials live in the section named after the remote, so a rename
    // reads them from the section the remote had before.
    let section_name = planned
        .previous_name
        .as_deref()
        .unwrap_or(record.name.as_str());
    let current_conf = read_conf(context)?;
    let secret = secret_access_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    // The secret is written verbatim. rclone's s3 backend reads `secret_access_key` as
    // plaintext and never reveals an obscured value, so an obscured secret makes every
    // request fail with SignatureDoesNotMatch. The file is private and mode 0600.
    let secret = match secret {
        Some(secret) => secret.to_string(),
        None => rclone_conf::get_section(&current_conf, section_name)
            .and_then(|pairs| {
                pairs
                    .into_iter()
                    .find(|(key, _)| key == rclone_conf::SECTION_SECRET_ACCESS_KEY)
                    .map(|(_, value)| value)
            })
            .ok_or_else(|| {
                format!(
                    "Enter the secret access key for '{}'. It is written to S3Dive's rclone.conf.",
                    record.name
                )
            })?,
    };

    let mut updated_conf = current_conf.clone();
    if let Some(old_name) = planned.previous_name.as_deref() {
        if old_name != record.name {
            updated_conf = rclone_conf::remove_section(&updated_conf, old_name);
        }
    }
    updated_conf = rclone_conf::set_section(
        &updated_conf,
        &record.name,
        &rclone_conf::remote_pairs(record, &secret),
    );

    // The state transition is applied last and re-checks the name, so the config
    // file and the state file cannot end up disagreeing about which buckets exist.
    commit_remote_change(context, &current_conf, &updated_conf, move |state| {
        remotes::apply_remote_update(state, &planned)?;
        Ok(())
    })
}

/// Write the config file and the state together.
///
/// The config is written first: a bucket without credentials is worse than a
/// credential without a bucket. When the state write fails, the previous config
/// content is written back, so the two files never disagree.
fn commit_remote_change(
    context: &AppContext,
    previous_conf: &str,
    updated_conf: &str,
    change: impl FnOnce(&mut AppState) -> Result<(), String>,
) -> Result<AppState, String> {
    let conf_path = context.paths.rclone_conf();
    store::write_private_file(&conf_path, updated_conf).map_err(|error| error.to_string())?;

    match context.mutate(change) {
        Ok((_, snapshot)) => Ok(snapshot),
        Err(error) => {
            if let Err(rollback) = store::write_private_file(&conf_path, previous_conf) {
                return Err(format!(
                    "{error}. The rclone config could not be restored: {rollback}"
                ));
            }
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn delete_remote(app: AppHandle, id: String) -> Result<AppState, String> {
    let context = app.state::<AppContext>();
    delete_remote_inner(&context, id)
}

/// Remove a remote and the credentials stored for it.
fn delete_remote_inner(context: &AppContext, id: String) -> Result<AppState, String> {
    let state = context.state();
    let remote = state
        .remotes
        .iter()
        .find(|candidate| candidate.id == id)
        .cloned()
        .ok_or_else(|| "That bucket no longer exists.".to_string())?;

    if state.references_remote(&remote.name) {
        return Err(format!(
            "'{}' is still used by a job or a mount. Remove those first.",
            remote.name
        ));
    }

    let current_conf = read_conf(context)?;
    let updated_conf = rclone_conf::remove_section(&current_conf, &remote.name);

    commit_remote_change(context, &current_conf, &updated_conf, move |state| {
        state.remotes.retain(|candidate| candidate.id != remote.id);
        Ok(())
    })
}

#[tauri::command]
pub async fn test_remote(app: AppHandle, id: String) -> Result<RemoteTest, String> {
    let context = app.state::<AppContext>();
    let state = context.state();
    let remote = state
        .remotes
        .iter()
        .find(|candidate| candidate.id == id)
        .cloned()
        .ok_or_else(|| "That bucket no longer exists.".to_string())?;

    let args = rclone::test_remote_args(&remote.name, &context.paths.conf_arg());
    match context.rclone().captured(&args).await {
        Ok(stdout) => {
            let entries: Vec<String> = stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .take(200)
                .map(str::to_string)
                .collect();
            let message = if entries.is_empty() {
                "Connected. The remote root is empty.".to_string()
            } else {
                format!(
                    "Connected. {} bucket(s) visible at the remote root.",
                    entries.len()
                )
            };
            Ok(RemoteTest {
                ok: true,
                message,
                entries,
            })
        }
        Err(message) => Ok(RemoteTest {
            ok: false,
            message,
            entries: Vec::new(),
        }),
    }
}

#[tauri::command]
pub async fn browse_remote(
    app: AppHandle,
    remote_id: String,
    path: String,
) -> Result<Vec<rclone::RemoteEntry>, String> {
    let context = app.state::<AppContext>();
    let state = context.state();
    let remote = state
        .remotes
        .iter()
        .find(|candidate| candidate.id == remote_id)
        .cloned()
        .ok_or_else(|| "That bucket no longer exists.".to_string())?;
    validate::remote_path(&path)?;

    let args = rclone::browse_args(&remote.name, &path, &context.paths.conf_arg());
    let stdout = context.rclone().captured(&args).await?;
    rclone::parse_lsjson(&stdout)
}

#[tauri::command]
pub fn save_job(app: AppHandle, mut job: Job) -> Result<AppState, String> {
    let context = app.state::<AppContext>();
    let state = context.state();

    if job.id.trim().is_empty() {
        job.id = s3dive_core::new_id();
    }
    if job.created_at_ms == 0 {
        job.created_at_ms = now_ms();
    }
    job.name = job.name.trim().to_string();
    job.remote_path = job.remote_path.trim().to_string();
    job.local_path = job.local_path.trim().to_string();
    job.include = job
        .include
        .into_iter()
        .map(|pattern| pattern.trim().to_string())
        .filter(|pattern| !pattern.is_empty())
        .collect();
    job.exclude = job
        .exclude
        .into_iter()
        .map(|pattern| pattern.trim().to_string())
        .filter(|pattern| !pattern.is_empty())
        .collect();

    if let Some(existing) = state.jobs.iter().find(|candidate| candidate.id == job.id) {
        job.created_at_ms = existing.created_at_ms;
        // The UI never owns run history.
        job.last_run = existing.last_run.clone();
    }

    // Editing a running job would leave the child on the old config while the UI shows the
    // new one, so it is refused exactly like editing a mounted mount.
    if context.active_jobs.lock_recover().contains(&job.id) {
        return Err(format!(
            "'{}' is running right now. Wait for it to finish before changing it.",
            job.name
        ));
    }

    validate::validate_job(&state, &job)?;

    let (_, snapshot) = context.mutate(|state| {
        if let Some(index) = state
            .jobs
            .iter()
            .position(|candidate| candidate.id == job.id)
        {
            state.jobs[index] = job;
        } else {
            state.jobs.push(job);
        }
        Ok(())
    })?;
    Ok(snapshot)
}

#[tauri::command]
pub fn delete_job(app: AppHandle, id: String) -> Result<AppState, String> {
    let context = app.state::<AppContext>();
    let running: Vec<String> = context
        .runs
        .lock_recover()
        .iter()
        .filter(|(_, entry)| entry.job_id == id)
        .map(|(run_id, _)| run_id.clone())
        .collect();
    for run_id in running {
        let _ = jobs::cancel(app.clone(), &run_id);
    }

    let (_, snapshot) = context.mutate(|state| {
        state.jobs.retain(|candidate| candidate.id != id);
        Ok(())
    })?;
    Ok(snapshot)
}

#[tauri::command]
pub async fn run_job(
    app: AppHandle,
    job_id: String,
    resync: Option<bool>,
) -> Result<RunSummary, String> {
    jobs::run(app, job_id, resync.unwrap_or(false)).await
}

#[tauri::command]
pub fn cancel_job(app: AppHandle, run_id: String) -> Result<(), String> {
    jobs::cancel(app, &run_id)
}

#[tauri::command]
pub fn save_mount(app: AppHandle, mut mount: Mount) -> Result<AppState, String> {
    let context = app.state::<AppContext>();
    let state = context.state();

    if mount.id.trim().is_empty() {
        mount.id = s3dive_core::new_id();
    }
    if mount.created_at_ms == 0 {
        mount.created_at_ms = now_ms();
    }
    mount.name = mount.name.trim().to_string();
    mount.remote_path = mount.remote_path.trim().to_string();
    mount.mount_point = mount.mount_point.trim().to_string();

    if let Some(existing) = state
        .mounts
        .iter()
        .find(|candidate| candidate.id == mount.id)
    {
        mount.created_at_ms = existing.created_at_ms;
    }

    validate::validate_mount(&state, &mount)?;

    // Editing a live mount would leave the child pointing at the old settings.
    if context.mounts.lock_recover().contains_key(&mount.id) {
        return Err(format!(
            "'{}' is mounted right now. Unmount it before changing its settings.",
            mount.name
        ));
    }

    let (_, snapshot) = context.mutate(|state| {
        if let Some(index) = state
            .mounts
            .iter()
            .position(|candidate| candidate.id == mount.id)
        {
            state.mounts[index] = mount;
        } else {
            state.mounts.push(mount);
        }
        Ok(())
    })?;
    Ok(snapshot)
}

#[tauri::command]
pub fn delete_mount(app: AppHandle, id: String) -> Result<AppState, String> {
    let context = app.state::<AppContext>();
    let _ = mounts::stop(app.clone(), &id);

    let (_, snapshot) = context.mutate(|state| {
        state.mounts.retain(|candidate| candidate.id != id);
        Ok(())
    })?;
    Ok(snapshot)
}

#[tauri::command]
pub async fn start_mount(app: AppHandle, id: String) -> Result<MountRuntime, String> {
    mounts::start(app, id).await
}

#[tauri::command]
pub fn stop_mount(app: AppHandle, id: String) -> Result<(), String> {
    mounts::stop(app, &id)
}

#[tauri::command]
pub fn mount_states(app: AppHandle) -> Vec<MountRuntime> {
    mounts::states(&app)
}

#[tauri::command]
pub fn fuse_status(app: AppHandle) -> FuseStatus {
    let context = app.state::<AppContext>();
    context.fuse()
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|error| error.to_string())?;
    } else {
        manager.disable().map_err(|error| error.to_string())?;
    }
    manager.is_enabled().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;
    use s3dive_core::model::{Job, JobKind, Remote};

    fn context_in(dir: &std::path::Path) -> AppContext {
        AppContext::load(Paths::new(dir.to_path_buf()))
    }

    #[test]
    fn saving_two_buckets_keeps_both() {
        let dir = tempfile::tempdir().expect("tempdir");
        let context = context_in(dir.path());

        // The UI sends an empty id for a bucket it has never saved before.
        let mut first = Remote::new("media", "AKIAONE");
        first.id = String::new();
        let state =
            save_remote_inner(&context, first, Some("secret-one".into())).expect("save the first");
        assert_eq!(state.remotes.len(), 1);

        let mut second = Remote::new("backup", "AKIATWO");
        second.id = String::new();
        let state = save_remote_inner(&context, second, Some("secret-two".into()))
            .expect("save the second");

        assert_eq!(
            state.remotes.len(),
            2,
            "the second bucket replaced the first"
        );
        assert_ne!(
            state.remotes[0].id, state.remotes[1].id,
            "both buckets share the same id"
        );
        assert!(!state.remotes[0].id.is_empty(), "a bucket kept an empty id");
    }

    #[test]
    fn a_new_bucket_does_not_repoint_an_existing_job() {
        let dir = tempfile::tempdir().expect("tempdir");
        let context = context_in(dir.path());

        let mut first = Remote::new("media", "AKIAONE");
        first.id = String::new();
        save_remote_inner(&context, first, Some("secret-one".into())).expect("save the first");

        context
            .mutate(|state| {
                state.jobs.push(Job::new(
                    "Photos",
                    JobKind::Sync,
                    "media",
                    "photos",
                    "/home/me/Pictures",
                ));
                Ok(())
            })
            .expect("add the job");

        let mut second = Remote::new("backup", "AKIATWO");
        second.id = String::new();
        let state = save_remote_inner(&context, second, Some("secret-two".into()))
            .expect("save the second");

        assert_eq!(
            state.jobs[0].remote, "media",
            "the job was silently repointed at another bucket"
        );
    }

    #[test]
    fn deleting_a_bucket_removes_only_that_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let context = context_in(dir.path());

        let mut first = Remote::new("media", "AKIAONE");
        first.id = String::new();
        save_remote_inner(&context, first, Some("secret-one".into())).expect("save the first");

        let mut second = Remote::new("backup", "AKIATWO");
        second.id = String::new();
        let state = save_remote_inner(&context, second, Some("secret-two".into()))
            .expect("save the second");
        let backup_id = state
            .remotes
            .iter()
            .find(|remote| remote.name == "backup")
            .expect("the backup bucket")
            .id
            .clone();

        let state = delete_remote_inner(&context, backup_id).expect("delete the backup bucket");

        assert_eq!(state.remotes.len(), 1);
        assert_eq!(state.remotes[0].name, "media");
    }

    #[test]
    fn a_failed_state_write_restores_the_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let context = context_in(dir.path());

        let mut first = Remote::new("media", "AKIAONE");
        first.id = String::new();
        save_remote_inner(&context, first, Some("secret-one".into())).expect("save the first");

        let conf_before =
            std::fs::read_to_string(context.paths.rclone_conf()).expect("read the config");

        // Make the state write fail: the state file path is a directory, so the
        // atomic rename cannot land.
        std::fs::remove_file(context.paths.state_file()).expect("remove the state file");
        std::fs::create_dir_all(context.paths.state_file()).expect("block the state file");

        let mut second = Remote::new("backup", "AKIATWO");
        second.id = String::new();
        let error = save_remote_inner(&context, second, Some("secret-two".into()))
            .expect_err("the write fails");

        assert!(
            error.contains("Cannot access"),
            "the error does not name the failed file: {error}"
        );
        let conf_after =
            std::fs::read_to_string(context.paths.rclone_conf()).expect("read the config");
        assert_eq!(conf_after, conf_before, "the config was not restored");
        assert_eq!(context.state().remotes.len(), 1, "the state changed anyway");
    }
}
