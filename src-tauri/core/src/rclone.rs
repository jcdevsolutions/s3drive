//! The exact rclone command lines S3Dive builds, plus parsers for its output.
//!
//! These are pure functions on purpose: they are the part of the app most likely to
//! break silently, so every flag order is pinned by a unit test.

use crate::model::{Direction, Job, JobKind, Mount, Settings};
use crate::platform::Os;
use serde::{Deserialize, Serialize};

pub const CONFIG_FLAG: &str = "--config";

/// `remote:bucket/prefix`, tolerating stray slashes from user input.
pub fn remote_spec(remote: &str, path: &str) -> String {
    let trimmed = path.trim().trim_matches('/');
    if trimmed.is_empty() {
        format!("{remote}:")
    } else {
        format!("{remote}:{trimmed}")
    }
}

pub fn version_args() -> Vec<String> {
    vec!["version".to_string()]
}

/// Split one user-entered flag line into the argv elements it stands for.
///
/// rclone reads one element per argument, so `--transfers 8` has to reach it as two
/// elements, never as one. Quotes group a value so a path with spaces survives, and a
/// token that already carries `=` stays a single element.
pub fn split_flag_line(line: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;

    for ch in line.chars() {
        match quote {
            Some(open) => {
                if ch == open {
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                    started = true;
                } else if ch.is_whitespace() {
                    if started {
                        tokens.push(std::mem::take(&mut current));
                        started = false;
                    }
                } else {
                    current.push(ch);
                    started = true;
                }
            }
        }
    }
    if started {
        tokens.push(current);
    }
    tokens
}

fn base_args(config_path: &str, settings: &Settings) -> Vec<String> {
    let mut args = vec![CONFIG_FLAG.to_string(), config_path.to_string()];
    if let Some(limit) = settings
        .bandwidth_limit
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        args.push("--bwlimit".to_string());
        args.push(limit.to_string());
    }
    for flag in &settings.default_flags {
        let mut tokens = split_flag_line(flag).into_iter().peekable();
        while let Some(token) = tokens.next() {
            // S3Dive owns the config path. `save_settings` already refuses a config
            // flag, but a state file edited by hand must not redirect rclone to a
            // different config either. A bare `--config` takes the next token as
            // its value, so both are dropped.
            if token == CONFIG_FLAG {
                tokens.next();
                continue;
            }
            if token.starts_with("--config=") {
                continue;
            }
            args.push(token);
        }
    }
    args
}

/// Options that only apply to a single run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransferOptions {
    /// First `bisync` run must establish the baseline with `--resync`.
    pub resync: bool,
}

/// Full argument vector for a sync, copy or bisync job.
pub fn transfer_args(
    job: &Job,
    settings: &Settings,
    config_path: &str,
    options: TransferOptions,
) -> Vec<String> {
    let mut args = base_args(config_path, settings);
    let remote = remote_spec(&job.remote, &job.remote_path);
    args.push(job.kind.transfer_command().to_string());

    match (job.kind, job.direction) {
        (JobKind::Bisync, _) => {
            args.push(job.local_path.clone());
            args.push(remote);
        }
        (_, Direction::Push) => {
            args.push(job.local_path.clone());
            args.push(remote);
        }
        (_, Direction::Pull) => {
            args.push(remote);
            args.push(job.local_path.clone());
        }
    }

    args.push("--log-level".to_string());
    args.push("INFO".to_string());
    args.push("--stats".to_string());
    args.push("5s".to_string());
    args.push("--stats-one-line".to_string());

    if job.dry_run {
        args.push("--dry-run".to_string());
    }
    for pattern in &job.include {
        args.push("--include".to_string());
        args.push(pattern.clone());
    }
    for pattern in &job.exclude {
        args.push("--exclude".to_string());
        args.push(pattern.clone());
    }
    // `--resync` only exists for bisync; passing it to sync/copy is an rclone error.
    if options.resync && job.kind == JobKind::Bisync {
        args.push("--resync".to_string());
    }

    args
}

/// Full argument vector for `rclone mount`. S3Dive supervises the child process,
/// so `--daemon` is never used.
pub fn mount_args(mount: &Mount, settings: &Settings, config_path: &str, os: &Os) -> Vec<String> {
    let mut args = base_args(config_path, settings);
    args.push("mount".to_string());
    args.push(remote_spec(&mount.remote, &mount.remote_path));
    args.push(mount.mount_point.clone());

    args.push("--vfs-cache-mode".to_string());
    args.push(mount.vfs_cache_mode.rclone_value().to_string());
    args.push("--dir-cache-time".to_string());
    args.push("5m".to_string());
    args.push("--log-level".to_string());
    args.push("INFO".to_string());

    if mount.read_only {
        args.push("--read-only".to_string());
    }
    // WinFsp has no equivalent of FUSE's allow_other, rclone rejects it on Windows.
    if mount.allow_other && os.is_unix() {
        args.push("--allow-other".to_string());
    }
    args
}

/// List folders inside a remote path, used by the bucket/prefix picker.
pub fn browse_args(remote: &str, path: &str, config_path: &str) -> Vec<String> {
    vec![
        CONFIG_FLAG.to_string(),
        config_path.to_string(),
        "lsjson".to_string(),
        remote_spec(remote, path),
        "--dirs-only".to_string(),
        "--max-depth".to_string(),
        "1".to_string(),
        "--no-modtime".to_string(),
    ]
}

/// Cheap credential check: listing the remote root proves the keys are accepted.
pub fn test_remote_args(remote: &str, config_path: &str) -> Vec<String> {
    vec![
        CONFIG_FLAG.to_string(),
        config_path.to_string(),
        "lsf".to_string(),
        format!("{remote}:"),
        "--max-depth".to_string(),
        "1".to_string(),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

/// Parse `rclone lsjson --dirs-only` output into folder entries, sorted by name.
pub fn parse_lsjson(input: &str) -> Result<Vec<RemoteEntry>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let value: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|error| format!("rclone returned unreadable JSON: {error}"))?;
    let items = value
        .as_array()
        .ok_or_else(|| "rclone returned JSON that is not a list of entries.".to_string())?;

    let mut entries = Vec::with_capacity(items.len());
    for item in items {
        let name = item
            .get("Name")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let path = item
            .get("Path")
            .and_then(|value| value.as_str())
            .unwrap_or(&name)
            .to_string();
        let is_dir = item
            .get("IsDir")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        entries.push(RemoteEntry { name, path, is_dir });
    }
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    Ok(entries)
}

/// Extract `v1.75.2` from the first line of `rclone version`.
pub fn parse_version(stdout: &str) -> Option<String> {
    let first_line = stdout.lines().next()?;
    first_line
        .split_whitespace()
        .find(|token| {
            token
                .strip_prefix('v')
                .and_then(|rest| rest.chars().next())
                .is_some_and(|first| first.is_ascii_digit())
        })
        .map(str::to_string)
}

fn last_error_line(stderr: &str) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or("rclone failed without output");
    if line.chars().count() > 300 {
        let clipped: String = line.chars().take(300).collect();
        format!("{clipped}…")
    } else {
        line.to_string()
    }
}

/// Turn rclone's stderr into something a user can act on.
pub fn friendly_error(stderr: &str) -> String {
    let lower = stderr.to_lowercase();
    let detail = last_error_line(stderr);

    let hint = if lower.contains("signaturedoesnotmatch") {
        "Signature mismatch: the secret access key does not match the access key ID."
    } else if lower.contains("invalidaccesskeyid") {
        "Unknown access key ID: check it for typos and make sure the key is active."
    } else if lower.contains("access denied") || lower.contains("forbidden") {
        "Access denied: the credentials work but lack permission for this bucket."
    } else if lower.contains("nosuchbucket") || lower.contains("bucket does not exist") {
        "That bucket does not exist on this remote, or the credentials cannot see it."
    } else if lower.contains("couldn't find remote")
        || (lower.contains("didn't find section") && lower.contains("config"))
    {
        "The remote is missing from the rclone config. Re-save the bucket to recreate it."
    } else if lower.contains("directory not empty") || lower.contains("mount point is not empty") {
        "The mount point must be an existing empty directory."
    } else if lower.contains("winfsp") {
        "WinFsp is required on Windows: install it from https://winfsp.dev."
    } else if lower.contains("fusermount") || lower.contains("fuse") {
        "FUSE is not available: install fuse3 (Linux) or macFUSE (macOS), then retry."
    } else if lower.contains("no such host")
        || lower.contains("connection refused")
        || lower.contains("i/o timeout")
        || lower.contains("tls handshake")
    {
        "Cannot reach the endpoint: check the endpoint URL, the region and your network."
    } else if lower.contains("is a file") || lower.contains("not a directory") {
        "The local folder does not exist or is not a directory."
    } else {
        return detail;
    };

    format!("{hint} rclone said: {detail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::VfsCacheMode;

    const CONFIG: &str = "/home/me/.config/s3dive/rclone.conf";

    fn settings() -> Settings {
        Settings::default()
    }

    #[test]
    fn remote_spec_normalizes_slashes() {
        assert_eq!(remote_spec("media", ""), "media:");
        assert_eq!(remote_spec("media", "/"), "media:");
        assert_eq!(remote_spec("media", "photos"), "media:photos");
        assert_eq!(remote_spec("media", "/photos/2024/"), "media:photos/2024");
    }

    #[test]
    fn push_job_sends_local_folder_to_remote() {
        let mut job = Job::new(
            "Photos",
            JobKind::Sync,
            "media",
            "photos",
            "/home/me/Pictures",
        );
        job.direction = Direction::Push;
        job.dry_run = true;
        job.include = vec!["*.jpg".into()];
        job.exclude = vec!["*.tmp".into()];

        let args = transfer_args(&job, &settings(), CONFIG, TransferOptions::default());

        assert_eq!(
            args,
            vec![
                "--config",
                CONFIG,
                "sync",
                "/home/me/Pictures",
                "media:photos",
                "--log-level",
                "INFO",
                "--stats",
                "5s",
                "--stats-one-line",
                "--dry-run",
                "--include",
                "*.jpg",
                "--exclude",
                "*.tmp",
            ]
        );
    }

    #[test]
    fn pull_job_reverses_source_and_destination() {
        let mut job = Job::new(
            "Restore",
            JobKind::Copy,
            "media",
            "backups/2024",
            "/srv/restore",
        );
        job.direction = Direction::Pull;

        let args = transfer_args(&job, &settings(), CONFIG, TransferOptions::default());
        assert_eq!(args[2], "copy");
        assert_eq!(args[3], "media:backups/2024");
        assert_eq!(args[4], "/srv/restore");
    }

    #[test]
    fn bisync_ignores_direction_and_adds_resync_on_first_run() {
        let mut job = Job::new(
            "Two way",
            JobKind::Bisync,
            "media",
            "notes",
            "/home/me/notes",
        );
        job.direction = Direction::Pull;

        let first = transfer_args(&job, &settings(), CONFIG, TransferOptions { resync: true });
        assert_eq!(first[2], "bisync");
        assert_eq!(first[3], "/home/me/notes");
        assert_eq!(first[4], "media:notes");
        assert!(first.contains(&"--resync".to_string()));

        let later = transfer_args(&job, &settings(), CONFIG, TransferOptions::default());
        assert!(!later.contains(&"--resync".to_string()));
    }

    #[test]
    fn resync_is_never_passed_to_sync_or_copy() {
        let sync = Job::new(
            "Mirror",
            JobKind::Sync,
            "media",
            "photos",
            "/home/me/Pictures",
        );
        let args = transfer_args(&sync, &settings(), CONFIG, TransferOptions { resync: true });
        assert!(!args.contains(&"--resync".to_string()));

        let copy = Job::new(
            "Backup",
            JobKind::Copy,
            "media",
            "photos",
            "/home/me/Pictures",
        );
        let args = transfer_args(&copy, &settings(), CONFIG, TransferOptions { resync: true });
        assert!(!args.contains(&"--resync".to_string()));
    }

    #[test]
    fn bandwidth_limit_and_global_flags_apply_to_every_run() {
        let job = Job::new("Docs", JobKind::Copy, "media", "docs", "/home/me/docs");
        let settings = Settings {
            rclone_path: None,
            default_flags: vec![" --transfers 8 ".into(), "  ".into()],
            bandwidth_limit: Some("8M".into()),
        };

        let args = transfer_args(&job, &settings, CONFIG, TransferOptions::default());
        assert_eq!(
            &args[0..7],
            [
                "--config",
                CONFIG,
                "--bwlimit",
                "8M",
                "--transfers",
                "8",
                "copy"
            ]
        );
    }

    #[test]
    fn extra_flags_reach_rclone_as_separate_arguments() {
        let job = Job::new("Docs", JobKind::Copy, "media", "docs", "/home/me/docs");
        let settings = Settings {
            rclone_path: None,
            default_flags: vec!["--transfers 8".into()],
            bandwidth_limit: None,
        };

        let args = transfer_args(&job, &settings, CONFIG, TransferOptions::default());

        assert!(args.contains(&"--transfers".to_string()), "{args:?}");
        assert!(args.contains(&"8".to_string()), "{args:?}");
        assert!(!args.contains(&"--transfers 8".to_string()), "{args:?}");
    }

    #[test]
    fn an_extra_flag_cannot_redirect_the_config_path() {
        let job = Job::new("Docs", JobKind::Copy, "media", "docs", "/home/me/docs");
        let settings = Settings {
            default_flags: vec![
                "--config=/etc/rclone.conf".into(),
                "--config /etc/rclone.conf".into(),
                "--transfers".into(),
            ],
            ..Settings::default()
        };

        let args = transfer_args(&job, &settings, CONFIG, TransferOptions::default());

        assert_eq!(args[0], "--config");
        assert_eq!(args[1], CONFIG, "the app config path was replaced");
        assert!(
            !args.iter().any(|arg| arg.starts_with("--config=")),
            "{args:?}"
        );
        assert!(
            !args.iter().any(|arg| arg == "/etc/rclone.conf"),
            "the value of a refused --config survived as a positional argument: {args:?}"
        );
    }

    #[test]
    fn empty_bandwidth_limit_is_ignored() {
        let job = Job::new("Docs", JobKind::Copy, "media", "docs", "/home/me/docs");
        let settings = Settings {
            bandwidth_limit: Some("   ".into()),
            ..Settings::default()
        };
        let args = transfer_args(&job, &settings, CONFIG, TransferOptions::default());
        assert!(!args.contains(&"--bwlimit".to_string()));
    }

    #[test]
    fn mount_args_are_supervised_and_never_daemonize() {
        let mut mount = Mount::new("Media", "media", "photos", "/mnt/media");
        mount.read_only = true;
        mount.vfs_cache_mode = VfsCacheMode::Full;

        let args = mount_args(&mount, &settings(), CONFIG, &Os::Linux);
        assert_eq!(
            args,
            vec![
                "--config",
                CONFIG,
                "mount",
                "media:photos",
                "/mnt/media",
                "--vfs-cache-mode",
                "full",
                "--dir-cache-time",
                "5m",
                "--log-level",
                "INFO",
                "--read-only",
            ]
        );
        assert!(!args.contains(&"--daemon".to_string()));
    }

    #[test]
    fn allow_other_is_unix_only() {
        let mut mount = Mount::new("Media", "media", "", "/mnt/media");
        mount.allow_other = true;

        assert!(mount_args(&mount, &settings(), CONFIG, &Os::Linux)
            .contains(&"--allow-other".to_string()));
        assert!(mount_args(&mount, &settings(), CONFIG, &Os::Macos)
            .contains(&"--allow-other".to_string()));
        assert!(!mount_args(&mount, &settings(), CONFIG, &Os::Windows)
            .contains(&"--allow-other".to_string()));
    }

    #[test]
    fn browse_and_test_args_list_folders_and_root() {
        let browse = browse_args("media", "/photos", CONFIG);
        assert_eq!(browse[2], "lsjson");
        assert_eq!(browse[3], "media:photos");
        assert!(browse.contains(&"--dirs-only".to_string()));

        let test = test_remote_args("media", CONFIG);
        assert_eq!(test[2], "lsf");
        assert_eq!(test[3], "media:");
    }

    #[test]
    fn lsjson_parsing_sorts_and_defaults_the_path() {
        let json = r#"[
            {"Name": "zeta", "Path": "zeta", "IsDir": true},
            {"Name": "Alpha", "Path": "photos/Alpha", "IsDir": true}
        ]"#;
        let entries = parse_lsjson(json).expect("parse");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "Alpha");
        assert_eq!(entries[0].path, "photos/Alpha");
        assert_eq!(entries[1].name, "zeta");
        assert!(entries[0].is_dir);
    }

    #[test]
    fn lsjson_parsing_handles_empty_and_invalid_payloads() {
        assert_eq!(parse_lsjson("").unwrap(), Vec::new());
        assert_eq!(parse_lsjson("  \n ").unwrap(), Vec::new());
        assert!(parse_lsjson("not json")
            .unwrap_err()
            .contains("unreadable JSON"));
        assert!(parse_lsjson("{}").unwrap_err().contains("not a list"));
    }

    #[test]
    fn version_parsing_extracts_the_semver_token() {
        let stdout = "rclone v1.75.2\n- os/version: linuxmint 22.3 (64 bit)\n";
        assert_eq!(parse_version(stdout).as_deref(), Some("v1.75.2"));
        assert_eq!(parse_version("garbage"), None);
    }

    #[test]
    fn errors_become_actionable_hints() {
        let denied =
            friendly_error("ERROR : Failed to ls: AccessDenied: Access Denied\nstatus code: 403");
        assert!(denied.contains("Access denied"), "{denied}");
        assert!(denied.contains("status code: 403"), "{denied}");

        let signature = friendly_error("SignatureDoesNotMatch");
        assert!(signature.contains("Signature mismatch"), "{signature}");

        let fuse = friendly_error("fusermount3: executable file not found in $PATH");
        assert!(fuse.contains("FUSE is not available"), "{fuse}");

        let unknown = friendly_error("something odd happened");
        assert_eq!(unknown, "something odd happened");
    }

    #[test]
    fn error_hints_are_truncated() {
        let long = "x".repeat(500);
        let message = friendly_error(&long);
        assert!(
            message.chars().count() <= 301,
            "{}",
            message.chars().count()
        );
        assert!(message.ends_with('…'));
    }

    #[test]
    fn version_probe_uses_the_version_subcommand() {
        assert_eq!(version_args(), vec!["version"]);
    }

    #[test]
    fn a_flag_line_splits_into_the_arguments_it_stands_for() {
        assert_eq!(split_flag_line("--transfers 8"), vec!["--transfers", "8"]);
        assert_eq!(split_flag_line("--transfers=8"), vec!["--transfers=8"]);
        assert_eq!(
            split_flag_line("--log-file \"my file.log\""),
            vec!["--log-file", "my file.log"]
        );
        assert_eq!(
            split_flag_line("--log-file 'my file.log'"),
            vec!["--log-file", "my file.log"]
        );
        assert!(split_flag_line("   ").is_empty());
        assert_eq!(split_flag_line("--transfers\t8"), vec!["--transfers", "8"]);
    }
}
