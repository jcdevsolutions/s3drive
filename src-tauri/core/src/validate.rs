//! Validation of user input before it reaches rclone or the filesystem.

use crate::model::{AppState, Job, Mount, Remote};
use crate::rclone::{split_flag_line, CONFIG_FLAG};
use std::path::{Component, Path, PathBuf};

const MAX_NAME: usize = 64;

/// rclone remote names become config section names, so keep them boring and portable.
pub fn remote_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Remote name cannot be empty.".into());
    }
    if trimmed.chars().count() > MAX_NAME {
        return Err(format!(
            "Remote name must be at most {MAX_NAME} characters."
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
    {
        return Err(
            "Remote name may only contain letters, digits, and the characters - _ . +".into(),
        );
    }
    if trimmed.starts_with('-') {
        return Err("Remote name cannot start with a dash.".into());
    }
    Ok(())
}

/// A path inside the bucket: `bucket` or `bucket/prefix`, never absolute.
pub fn remote_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.starts_with('/') {
        return Err("Remote path is relative to the remote: do not start it with a slash.".into());
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err("Remote path cannot contain control characters.".into());
    }
    if trimmed.contains('\\') {
        return Err("Remote path must use forward slashes.".into());
    }
    if trimmed.split('/').any(|segment| segment == "..") {
        return Err("Remote path cannot contain '..' segments.".into());
    }
    Ok(())
}

/// A local filesystem path. Accepts POSIX absolute paths and Windows drive/UNC paths.
pub fn local_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Folder path cannot be empty.".into());
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err("Folder path cannot contain control characters.".into());
    }
    if !is_absolute(trimmed) {
        return Err(format!(
            "'{trimmed}' is not an absolute path. Pick a folder with the browse button."
        ));
    }
    Ok(())
}

pub fn is_absolute(path: &str) -> bool {
    let path = path.trim();
    if path.starts_with('/') {
        return true;
    }
    // Windows drive letter, e.g. C:\Users or C:/Users
    let bytes = path.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    // UNC path, e.g. \\server\share
    path.starts_with("\\\\")
}

pub fn mount_point(path: &str) -> Result<(), String> {
    local_path(path)?;
    let normalized = normalize(Path::new(path.trim()));
    if normalized.parent().is_none() {
        return Err("Refusing to mount over the filesystem root.".into());
    }
    Ok(())
}

pub fn filter_pattern(pattern: &str) -> Result<(), String> {
    if pattern.trim().is_empty() {
        return Err("Filter patterns cannot be empty.".into());
    }
    if pattern.chars().any(|c| c.is_control()) {
        return Err("Filter patterns cannot contain control characters.".into());
    }
    Ok(())
}

/// True when one path is the same as, inside, or a parent of the other.
///
/// Mount points that overlap a sync folder cause recursive copies, so the caller
/// uses this to refuse them. `..` segments are resolved lexically and symlinks are
/// resolved when the paths exist, so `/mnt/../data` cannot sneak past a check against
/// `/data`.
pub fn paths_overlap(a: &Path, b: &Path) -> bool {
    let a = comparable(a);
    let b = comparable(b);
    a.starts_with(&b) || b.starts_with(&a)
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                // A real directory component can be cancelled.
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                // `/..` stays at the root, it does not climb above it.
                Some(Component::RootDir) | Some(Component::Prefix(_)) => {}
                // Relative `../..` keeps its leading parents.
                _ => out.push(Component::ParentDir),
            },
            other => out.push(other),
        }
    }
    out
}

/// Normalized path, canonicalized when it exists so symlinks cannot hide an overlap.
fn comparable(path: &Path) -> PathBuf {
    let normalized = normalize(path);
    std::fs::canonicalize(&normalized).unwrap_or(normalized)
}

/// Every extra flag is appended to every rclone call, so each line must be a flag.
///
/// S3Dive owns the rclone config file, so `--config` is refused: the user would
/// silently point rclone at a different config than the one the app manages.
pub fn extra_flags(flags: &[String]) -> Result<(), String> {
    for line in flags {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let tokens = split_flag_line(trimmed);
        let first = tokens.first().map(String::as_str).unwrap_or_default();
        if !first.starts_with('-') {
            return Err(format!(
                "'{trimmed}' is not an rclone flag. Extra flags must start with '-' (for example --transfers 8)."
            ));
        }
        if first == CONFIG_FLAG || first.starts_with("--config=") {
            return Err(format!(
                "'{first}' cannot be an extra flag: S3Dive chooses the rclone config file."
            ));
        }
    }
    Ok(())
}

pub fn validate_remote(remote: &Remote) -> Result<(), String> {
    remote_name(&remote.name)?;
    if remote.access_key_id.trim().is_empty() {
        return Err("Access key ID cannot be empty.".into());
    }
    if remote.access_key_id.chars().any(|c| c.is_control()) {
        return Err("Access key ID contains invalid characters.".into());
    }
    if remote.region.trim().is_empty() {
        return Err("Region cannot be empty.".into());
    }
    if remote.region.chars().any(|c| c.is_control()) {
        return Err("Region contains invalid characters.".into());
    }
    if let Some(endpoint) = remote.endpoint.as_deref() {
        let endpoint = endpoint.trim();
        if !endpoint.is_empty() {
            if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
                return Err("Endpoint must start with http:// or https://.".into());
            }
            if endpoint.chars().any(|c| c.is_whitespace()) {
                return Err("Endpoint cannot contain spaces.".into());
            }
        }
    }
    Ok(())
}

pub fn validate_job(state: &AppState, job: &Job) -> Result<(), String> {
    if job.name.trim().is_empty() {
        return Err("Job name cannot be empty.".into());
    }
    if state.find_remote_by_id(&job.remote_id).is_none() {
        return Err("The bucket this job uses no longer exists.".into());
    }
    remote_path(&job.remote_path)?;
    local_path(&job.local_path)?;
    for pattern in job.include.iter().chain(job.exclude.iter()) {
        filter_pattern(pattern)?;
    }

    // The mirror of the mount-side check: a job inside a mount point would make rclone
    // read its own output, so refuse it at creation time too.
    let local = Path::new(job.local_path.trim());
    for mount in &state.mounts {
        if paths_overlap(local, Path::new(mount.mount_point.trim())) {
            return Err(format!(
                "The folder {} overlaps the mount point of '{}'. Pick a folder outside it.",
                job.local_path, mount.name
            ));
        }
    }

    Ok(())
}

pub fn validate_mount(state: &AppState, mount: &Mount) -> Result<(), String> {
    if mount.name.trim().is_empty() {
        return Err("Mount name cannot be empty.".into());
    }
    if state.find_remote_by_id(&mount.remote_id).is_none() {
        return Err("The bucket this mount uses no longer exists.".into());
    }
    remote_path(&mount.remote_path)?;
    mount_point(&mount.mount_point)?;

    let candidate = Path::new(mount.mount_point.trim());
    for other in &state.mounts {
        if other.id == mount.id {
            continue;
        }
        if paths_overlap(candidate, Path::new(other.mount_point.trim())) {
            return Err(format!(
                "Mount point {} overlaps or nests with the mount point of '{}'.",
                mount.mount_point, other.name
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{JobKind, VfsCacheMode};

    #[test]
    fn remote_names_reject_rclone_breaking_input() {
        assert!(remote_name("media").is_ok());
        assert!(remote_name("work_bucket-2024.v2+x").is_ok());

        let empty = remote_name("   ").unwrap_err();
        assert!(empty.contains("cannot be empty"), "{empty}");

        let colon = remote_name("media:prod").unwrap_err();
        assert!(colon.contains("may only contain"), "{colon}");

        let slash = remote_name("a/b").unwrap_err();
        assert!(slash.contains("may only contain"), "{slash}");

        let dash = remote_name("-media").unwrap_err();
        assert!(dash.contains("start with a dash"), "{dash}");

        let long = remote_name(&"a".repeat(65)).unwrap_err();
        assert!(long.contains("at most 64"), "{long}");
    }

    #[test]
    fn remote_paths_must_be_relative_and_normal() {
        assert!(remote_path("").is_ok());
        assert!(remote_path("photos/raw").is_ok());
        assert!(remote_path("/photos").is_err());
        assert!(remote_path("photos/../etc").is_err());
        assert!(remote_path("photos\\raw").is_err());
        assert!(remote_path("bad\npath").is_err());
    }

    #[test]
    fn absolute_path_detection_handles_both_platforms() {
        assert!(is_absolute("/home/me/data"));
        assert!(is_absolute(r"C:\Users\me\data"));
        assert!(is_absolute("C:/Users/me/data"));
        assert!(is_absolute(r"\\server\share"));
        assert!(!is_absolute("relative/dir"));
        assert!(!is_absolute("~/data"));
        assert!(!is_absolute("C:"));
    }

    #[test]
    fn local_paths_require_absolute_directories() {
        assert!(local_path("/home/me/data").is_ok());
        assert!(local_path(r"D:\backup").is_ok());
        assert!(local_path("backup").is_err());
        assert!(local_path("  ").is_err());
    }

    #[test]
    fn mount_points_refuse_the_root() {
        assert!(mount_point("/mnt/s3").is_ok());
        let err = mount_point("/").unwrap_err();
        assert!(err.contains("filesystem root"), "{err}");
    }

    #[test]
    fn overlapping_paths_are_detected_both_ways() {
        assert!(paths_overlap(
            Path::new("/mnt/s3"),
            Path::new("/mnt/s3/media")
        ));
        assert!(paths_overlap(
            Path::new("/mnt/s3/media/"),
            Path::new("/mnt/s3")
        ));
        assert!(paths_overlap(Path::new("/mnt/s3"), Path::new("/mnt/s3")));
        assert!(!paths_overlap(Path::new("/mnt/s3"), Path::new("/mnt/s4")));
        assert!(!paths_overlap(Path::new("/mnt/s3"), Path::new("/mnt/s30")));
    }

    #[test]
    fn validate_job_reports_the_first_problem_clearly() {
        let mut state = AppState::default();
        let remote = Remote::new("media", "AKIA");
        state.remotes.push(remote.clone());

        let mut job = Job::new(
            "Photos",
            JobKind::Sync,
            remote.id.clone(),
            "photos",
            "/home/me/Pictures",
        );
        assert!(validate_job(&state, &job).is_ok());

        job.remote_id = "gone".into();
        let err = validate_job(&state, &job).unwrap_err();
        assert!(err.contains("no longer exists"), "{err}");

        job.remote_id = remote.id.clone();
        job.local_path = "relative".into();
        let err = validate_job(&state, &job).unwrap_err();
        assert!(err.contains("absolute path"), "{err}");

        job.local_path = "/home/me/Pictures".into();
        job.exclude = vec!["  ".into()];
        let err = validate_job(&state, &job).unwrap_err();
        assert!(err.contains("cannot be empty"), "{err}");
    }

    #[test]
    fn a_job_cannot_point_inside_a_mount_point() {
        let mut state = AppState::default();
        let remote = Remote::new("media", "AKIA");
        state.remotes.push(remote.clone());
        state.mounts.push(Mount::new(
            "Media",
            remote.id.clone(),
            "photos",
            "/mnt/media",
        ));

        let mut job = Job::new(
            "Photos",
            JobKind::Copy,
            remote.id.clone(),
            "photos",
            "/mnt/media/photos",
        );
        let error = validate_job(&state, &job).unwrap_err();
        assert!(error.contains("overlaps the mount point"), "{error}");

        job.local_path = "/mnt/media".into();
        assert!(validate_job(&state, &job).is_err());

        job.local_path = "/home/me/Pictures".into();
        assert!(validate_job(&state, &job).is_ok());
    }

    #[test]
    fn validate_mount_checks_remote_path_and_mount_point() {
        let mut state = AppState::default();
        let remote = Remote::new("media", "AKIA");
        state.remotes.push(remote.clone());

        let mut mount = Mount::new("Media", remote.id.clone(), "photos", "/mnt/media");
        mount.vfs_cache_mode = VfsCacheMode::Full;
        assert!(validate_mount(&state, &mount).is_ok());

        mount.mount_point = "mnt/media".into();
        assert!(validate_mount(&state, &mount).is_err());

        mount.mount_point = "/mnt/media".into();
        mount.remote_path = "/photos".into();
        assert!(validate_mount(&state, &mount).is_err());
    }

    #[test]
    fn overlap_resolves_dotdot_before_comparing() {
        assert!(paths_overlap(
            Path::new("/home/me/mnt/../Pictures/sub"),
            Path::new("/home/me/Pictures")
        ));
        assert!(paths_overlap(
            Path::new("/srv/sync"),
            Path::new("/srv/sync/../sync")
        ));
        assert!(!paths_overlap(
            Path::new("/srv/sync/../other"),
            Path::new("/srv/sync")
        ));
    }

    #[test]
    fn duplicate_mount_points_are_caught_after_normalization() {
        let mut state = AppState::default();
        let remote = Remote::new("media", "AKIA");
        state.remotes.push(remote.clone());
        state
            .mounts
            .push(Mount::new("A", remote.id.clone(), "a", "/mnt/shared"));

        let mut second = Mount::new("B", remote.id.clone(), "b", "/mnt/shared/../shared");
        let error = validate_mount(&state, &second).unwrap_err();
        assert!(error.contains("overlaps or nests"), "{error}");

        second.mount_point = "/mnt/shared/nested".into();
        assert!(validate_mount(&state, &second).is_err());

        second.mount_point = "/mnt/other".into();
        assert!(validate_mount(&state, &second).is_ok());
    }

    #[test]
    fn mount_point_rejects_root_spellings() {
        for spelling in ["/", "/./", "/..", "//", "/../"] {
            let error = mount_point(spelling).unwrap_err();
            assert!(error.contains("filesystem root"), "{spelling}: {error}");
        }
        assert!(mount_point("/mnt/media").is_ok());
    }

    #[test]
    fn extra_flags_must_start_with_a_dash() {
        assert!(extra_flags(&[]).is_ok());
        assert!(extra_flags(&["".into()]).is_ok());
        assert!(extra_flags(&["   ".into()]).is_ok());
        assert!(extra_flags(&["--transfers".into()]).is_ok());
        assert!(extra_flags(&["--transfers 8".into()]).is_ok());

        let error = extra_flags(&["transfers 8".into()]).unwrap_err();
        assert_eq!(
            error,
            "'transfers 8' is not an rclone flag. Extra flags must start with '-' (for example --transfers 8)."
        );
    }

    #[test]
    fn extra_flags_cannot_replace_the_config_file() {
        let error = extra_flags(&["--config /etc/rclone.conf".into()]).unwrap_err();
        assert!(error.contains("--config"), "{error}");
        assert!(error.contains("config file"), "{error}");

        let error = extra_flags(&["--config=/etc/rclone.conf".into()]).unwrap_err();
        assert!(error.contains("--config"), "{error}");
        assert!(error.contains("config file"), "{error}");
    }

    #[test]
    fn endpoints_must_be_http_urls() {
        let mut remote = Remote::new("media", "AKIA");
        remote.endpoint = Some("minio.local:9000".into());
        let err = validate_remote(&remote).unwrap_err();
        assert!(err.contains("must start with http"), "{err}");

        remote.endpoint = Some("https://minio.local:9000".into());
        assert!(validate_remote(&remote).is_ok());

        remote.endpoint = Some("   ".into());
        assert!(validate_remote(&remote).is_ok());
    }
}
