//! Regression tests for odd/tasks/plaintext-s3-secret.md (S1, S2, S3).
//!
//! S2 requires the secret to reach the app-owned `rclone.conf` verbatim in a 0600 file,
//! and S3 requires it to stay out of every rclone argument vector and out of state.json.

use s3dive_core::model::{
    AppState, Job, JobKind, Mount, Remote, S3Provider, Settings, STATE_VERSION,
};
use s3dive_core::{rclone, rclone_conf, store, Os};
use std::fs;
use std::path::Path;

/// A realistic B2 application key: 31 chars, base64-ish, contains '/'.
const SECRET: &str = "K0055aa7b8c9d0e1f2a3b4c5d6S/7890";

fn b2_remote() -> Remote {
    let mut r = Remote::new("probe", "0055aa7b8c9d0e1f2a3b4c5d6e7f8");
    r.provider = S3Provider::Other;
    r.region = "us-east-005".into();
    r.endpoint = Some("https://s3.us-east-005.backblazeb2.com".into());
    r
}

fn secret_of(conf: &str, section: &str) -> String {
    rclone_conf::get_section(conf, section)
        .expect("section")
        .into_iter()
        .find(|(k, _)| k == rclone_conf::SECTION_SECRET_ACCESS_KEY)
        .map(|(_, v)| v)
        .expect("secret present")
}

/// S2's file mode requirement only exists on Unix; Windows has no 0600.
#[cfg(unix)]
fn assert_private_mode(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "mode was {mode:o}");
}

#[cfg(not(unix))]
fn assert_private_mode(_path: &Path) {}

/// Write one remote section the way `save_remote` does and return the file contents.
fn save_remote(conf: &str, remote: &Remote, secret: &str) -> String {
    rclone_conf::set_section(
        conf,
        &remote.name,
        &rclone_conf::remote_pairs(remote, secret),
    )
}

// S2: verbatim on disk, file is 0600.
#[test]
fn s2_secret_is_byte_identical_on_disk_and_file_is_0600() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("rclone.conf");

    let conf = save_remote("", &b2_remote(), SECRET);
    store::write_private_file(&path, &conf).expect("write conf");

    let on_disk = fs::read_to_string(&path).expect("read conf");
    assert!(
        on_disk.contains(&format!("secret_access_key = {SECRET}\n")),
        "conf did not carry the secret on its own line: {on_disk}"
    );
    assert_eq!(secret_of(&on_disk, "probe"), SECRET);
    assert_private_mode(&path);
}

// S2: no transformation mangles a real key.
#[test]
fn s2_special_characters_survive_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("rclone.conf");

    for secret in ["a+b/c=d-e_f", "AbCdEf/0123456789+/abcdefgh==", SECRET] {
        let conf = save_remote("", &b2_remote(), secret);
        store::write_private_file(&path, &conf).expect("write conf");

        let on_disk = fs::read_to_string(&path).expect("read conf");
        assert_eq!(secret_of(&on_disk, "probe"), secret, "conf was: {on_disk}");
        assert_private_mode(&path);
    }
}

// S2: overwriting a pre-existing world-readable conf still yields 0600 + the new secret.
#[test]
fn s2_overwrite_keeps_mode_0600_and_the_new_secret() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("rclone.conf");

    // A config left behind by the previous version: obscured secret, 0644.
    let stale = "[probe]\ntype = s3\nsecret_access_key = obscured-old-value\n";
    fs::write(&path, stale).expect("seed conf");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod seed");
        let seeded = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(seeded, 0o644, "seed file must start world-readable");
    }

    let current = fs::read_to_string(&path).expect("read conf");
    let updated = save_remote(&current, &b2_remote(), SECRET);
    store::write_private_file(&path, &updated).expect("rewrite conf");

    let on_disk = fs::read_to_string(&path).expect("read conf");
    assert_eq!(secret_of(&on_disk, "probe"), SECRET, "{on_disk}");
    assert!(
        !on_disk.contains("obscured-old-value"),
        "the obscured value must be gone: {on_disk}"
    );
    assert_private_mode(&path);
}

// S2 interaction: empty field keeps the stored secret across a rename and a resave.
#[test]
fn s2_kept_secret_survives_a_rename_and_a_second_save() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("rclone.conf");

    // First save: the user types the secret once.
    let mut remote = b2_remote();
    let conf = save_remote("", &remote, SECRET);
    store::write_private_file(&path, &conf).expect("first save");

    // Rename with the secret field left empty: the stored value for the old section
    // name is reused, then the old section is dropped (save_remote's keep path).
    let current = fs::read_to_string(&path).expect("read conf");
    let kept = secret_of(&current, &remote.name);
    assert_eq!(kept, SECRET);
    let mut renamed = rclone_conf::remove_section(&current, &remote.name);
    remote.name = "probe-renamed".into();
    renamed = save_remote(&renamed, &remote, &kept);
    store::write_private_file(&path, &renamed).expect("save after rename");

    let after_rename = fs::read_to_string(&path).expect("read conf");
    assert_eq!(
        secret_of(&after_rename, "probe-renamed"),
        SECRET,
        "{after_rename}"
    );
    assert!(
        rclone_conf::get_section(&after_rename, "probe").is_none(),
        "the old section must be gone: {after_rename}"
    );
    assert_private_mode(&path);

    // A second save with the field still empty keeps the value again.
    let kept_again = secret_of(&after_rename, "probe-renamed");
    assert_eq!(kept_again, SECRET);
    let resaved = save_remote(&after_rename, &remote, &kept_again);
    store::write_private_file(&path, &resaved).expect("second save");

    let after_resave = fs::read_to_string(&path).expect("read conf");
    assert_eq!(
        secret_of(&after_resave, "probe-renamed"),
        SECRET,
        "{after_resave}"
    );
    assert_private_mode(&path);
}

// S3: no argument vector S3Dive builds can carry the secret.
#[test]
fn s3_no_rclone_argument_vector_carries_the_secret() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = dir.path().join("rclone.conf").to_string_lossy().to_string();

    let remote = b2_remote();
    let job = Job::new(
        "Probe job",
        JobKind::Copy,
        remote.name.as_str(),
        "bucket/prefix",
        "/home/me/Pictures",
    );
    let mount = Mount::new("Probe mount", remote.name.as_str(), "", "/mnt/probe");
    let settings = Settings::default();

    let vectors = [
        (
            "test_remote_args",
            rclone::test_remote_args(&remote.name, &config),
        ),
        (
            "browse_args",
            rclone::browse_args(&remote.name, "bucket/prefix", &config),
        ),
        (
            "transfer_args",
            rclone::transfer_args(
                &job,
                remote.name.as_str(),
                &settings,
                &config,
                rclone::TransferOptions::default(),
            ),
        ),
        (
            "mount_args",
            rclone::mount_args(&mount, remote.name.as_str(), &settings, &config, &Os::Linux),
        ),
        ("version_args", rclone::version_args()),
    ];

    for (label, args) in vectors {
        assert!(!args.is_empty(), "{label} produced no arguments");
        for arg in &args {
            assert!(!arg.contains(SECRET), "{label} carries the secret: {arg}");
        }
        let joined = args.join(" ");
        assert!(
            !joined.contains(SECRET),
            "{label} carries the secret: {joined}"
        );
        // `secret_access_key` may only ever appear as a config *key* inside the file.
        assert!(
            !joined.contains("secret_access_key"),
            "{label} mentions secret_access_key: {joined}"
        );
    }
}

// S3: persisted state carries no secret value and no secret field.
#[test]
fn s3_state_json_never_contains_the_secret() {
    let mut state = AppState::default();
    assert_eq!(state.schema_version, STATE_VERSION);
    let remote = b2_remote();
    state.jobs.push(Job::new(
        "Probe job",
        JobKind::Copy,
        remote.name.as_str(),
        "bucket/prefix",
        "/home/me/Pictures",
    ));
    state.mounts.push(Mount::new(
        "Probe mount",
        remote.name.as_str(),
        "",
        "/mnt/probe",
    ));
    state.remotes.push(remote);

    let json = serde_json::to_string(&state).expect("serialize");
    assert!(
        !json.contains(SECRET),
        "state json carries the secret: {json}"
    );
    assert!(
        !json.to_lowercase().contains("secret"),
        "state json has a secret field: {json}"
    );

    // The persisted file is the same document, still without the secret.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("state.json");
    store::save_state(&path, &state).expect("save state");
    let on_disk = fs::read_to_string(&path).expect("read state");
    assert!(
        !on_disk.contains(SECRET),
        "state file carries the secret: {on_disk}"
    );
    assert!(
        !on_disk.to_lowercase().contains("secret"),
        "state file has a secret field: {on_disk}"
    );
}
