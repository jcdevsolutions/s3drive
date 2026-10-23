//! The Tauri command surface. Every mutation returns the new `AppState` so the UI has
//! exactly one source of truth.

use crate::context::{AppContext, LockExt, MountRuntime};
use crate::runner::RcloneInfo;
use crate::{jobs, mounts};
use s3dive_core::model::{now_ms, AppState, Job, Mount, Remote, RunSummary, Settings};
use s3dive_core::platform::FuseStatus;
use s3dive_core::{rclone, rclone_conf, store, validate};
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

    for flag in &settings.default_flags {
        if !flag.starts_with('-') {
            return Err(format!(
                "'{flag}' is not an rclone flag. Extra flags must start with '-' (for example --transfers 8)."
            ));
        }
    }

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
    let state = context.state();
    let existing = state
        .remotes
        .iter()
        .find(|candidate| candidate.id == remote.id)
        .cloned();

    let mut record = remote;
    record.name = record.name.trim().to_string();
    record.access_key_id = record.access_key_id.trim().to_string();
    record.region = record.region.trim().to_string();
    record.endpoint = record
        .endpoint
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    validate::validate_remote(&record)?;

    let except = existing.as_ref().map(|candidate| candidate.id.as_str());
    if state.remote_name_taken(&record.name, except) {
        return Err(format!("A bucket named '{}' already exists.", record.name));
    }

    let previous_name = existing
        .as_ref()
        .map(|candidate| candidate.name.clone())
        .unwrap_or_else(|| record.name.clone());
    let conf_path = context.paths.rclone_conf();
    let current_conf = read_conf(&context)?;
    let secret = secret_access_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    // rclone obscures the secret, so plaintext never reaches disk and never appears
    // in a log line of this app.
    let obscured = match secret {
        Some(secret) => context.rclone().obscure(secret).await?,
        None => rclone_conf::get_section(&current_conf, &previous_name)
            .and_then(|pairs| {
                pairs
                    .into_iter()
                    .find(|(key, _)| key == rclone_conf::SECTION_SECRET_ACCESS_KEY)
                    .map(|(_, value)| value)
            })
            .ok_or_else(|| {
                format!(
                    "Enter the secret access key for '{}'. It is stored obscured in rclone.conf.",
                    record.name
                )
            })?,
    };

    let mut updated_conf = current_conf;
    if let Some(existing) = &existing {
        if existing.name != record.name {
            updated_conf = rclone_conf::remove_section(&updated_conf, &existing.name);
        }
    }
    updated_conf = rclone_conf::set_section(
        &updated_conf,
        &record.name,
        &rclone_conf::remote_pairs(&record, &obscured),
    );
    store::write_private_file(&conf_path, &updated_conf).map_err(|error| error.to_string())?;

    if let Some(existing) = existing {
        record.created_at_ms = existing.created_at_ms;
    }

    // A rename must keep jobs and mounts pointing at the same bucket.
    let renamed_from = state
        .remotes
        .iter()
        .find(|candidate| candidate.id == record.id)
        .map(|candidate| candidate.name.clone())
        .filter(|old_name| *old_name != record.name);
    let new_name = record.name.clone();

    let (_, snapshot) = context.mutate(move |state| {
        if let Some(old_name) = renamed_from {
            for job in state.jobs.iter_mut() {
                if job.remote == old_name {
                    job.remote = new_name.clone();
                }
            }
            for mount in state.mounts.iter_mut() {
                if mount.remote == old_name {
                    mount.remote = new_name.clone();
                }
            }
        }
        if let Some(index) = state
            .remotes
            .iter()
            .position(|candidate| candidate.id == record.id)
        {
            state.remotes[index] = record;
        } else {
            state.remotes.push(record);
        }
        Ok(())
    })?;
    Ok(snapshot)
}

#[tauri::command]
pub async fn delete_remote(app: AppHandle, id: String) -> Result<AppState, String> {
    let context = app.state::<AppContext>();
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

    let conf_path = context.paths.rclone_conf();
    let current_conf = read_conf(&context)?;
    let updated_conf = rclone_conf::remove_section(&current_conf, &remote.name);
    store::write_private_file(&conf_path, &updated_conf).map_err(|error| error.to_string())?;

    let (_, snapshot) = context.mutate(|state| {
        state.remotes.retain(|candidate| candidate.id != remote.id);
        Ok(())
    })?;
    Ok(snapshot)
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
    match context.rclone().captured(&args, None).await {
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
    let stdout = context.rclone().captured(&args, None).await?;
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
