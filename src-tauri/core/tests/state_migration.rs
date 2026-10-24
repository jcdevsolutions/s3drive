//! The state migration from schema v1 to v2.
//!
//! Before v2 a job and a mount named their remote, and the name was a mutable
//! key: renaming a bucket moved every job that used it. The migration resolves
//! the names into ids once. A reference whose bucket is gone keeps an empty id
//! and is reported, never re-pointed at another bucket.

use s3dive_core::model::STATE_VERSION;
use s3dive_core::remotes::{apply_remote_update, plan_remote_update};
use s3dive_core::store::{load_state, save_state};
use std::path::Path;

fn write_state(dir: &Path, name: &str, contents: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write the state file");
    path
}

const V1_STATE: &str = r#"{
    "schemaVersion": 1,
    "remotes": [
        {"id": "r1", "name": "media", "accessKeyId": "AKIA", "createdAtMs": 1},
        {"id": "r2", "name": "backup", "accessKeyId": "AKIA2", "createdAtMs": 1}
    ],
    "jobs": [{
        "id": "j1", "name": "Photos", "kind": "sync", "direction": "push",
        "remote": "media", "remotePath": "photos",
        "localPath": "/home/me/Pictures", "createdAtMs": 1
    }],
    "mounts": [{
        "id": "m1", "name": "Media", "remote": "media", "remotePath": "photos",
        "mountPoint": "/mnt/media", "createdAtMs": 1
    }]
}"#;

#[test]
fn a_v1_file_resolves_its_remote_names_into_ids() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_state(dir.path(), "state.json", V1_STATE);

    let loaded = load_state(&path).expect("load the v1 file");

    assert_eq!(loaded.schema_version, STATE_VERSION);
    assert_eq!(loaded.jobs[0].remote_id, "r1", "the job lost its bucket");
    assert_eq!(
        loaded.mounts[0].remote_id, "r1",
        "the mount lost its bucket"
    );
    assert!(
        loaded.dangling_references().is_empty(),
        "{:?}",
        loaded.dangling_references()
    );
}

#[test]
fn a_v1_file_with_a_missing_remote_is_reported_not_repointed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_state(
        dir.path(),
        "state.json",
        r#"{
            "schemaVersion": 1,
            "remotes": [
                {"id": "r1", "name": "media", "accessKeyId": "AKIA", "createdAtMs": 1},
                {"id": "r2", "name": "backup", "accessKeyId": "AKIA2", "createdAtMs": 1}
            ],
            "jobs": [{
                "id": "j1", "name": "Photos", "kind": "sync", "direction": "push",
                "remote": "gone", "remotePath": "photos",
                "localPath": "/home/me/Pictures", "createdAtMs": 1
            }],
            "mounts": []
        }"#,
    );

    let loaded = load_state(&path).expect("load the v1 file");

    assert_eq!(loaded.jobs[0].remote_id, "", "the job was re-pointed");
    let dangling = loaded.dangling_references();
    assert_eq!(dangling.len(), 1, "{dangling:?}");
    assert!(dangling[0].contains("Photos"), "{dangling:?}");
}

#[test]
fn a_v1_remote_without_an_id_is_repaired_before_the_names_are_resolved() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_state(
        dir.path(),
        "state.json",
        r#"{
            "schemaVersion": 1,
            "remotes": [
                {"id": "", "name": "media", "accessKeyId": "AKIA", "createdAtMs": 1}
            ],
            "jobs": [{
                "id": "j1", "name": "Photos", "kind": "sync", "direction": "push",
                "remote": "media", "remotePath": "photos",
                "localPath": "/home/me/Pictures", "createdAtMs": 1
            }],
            "mounts": []
        }"#,
    );

    let loaded = load_state(&path).expect("load the v1 file");

    let repaired = &loaded.remotes[0].id;
    assert!(!repaired.is_empty(), "the empty id survived");
    assert_eq!(loaded.jobs[0].remote_id, *repaired);
    assert!(loaded.dangling_references().is_empty());
}

#[test]
fn a_name_that_does_not_match_exactly_is_reported_not_guessed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_state(
        dir.path(),
        "state.json",
        r#"{
            "schemaVersion": 1,
            "remotes": [
                {"id": "r1", "name": "Media", "accessKeyId": "AKIA", "createdAtMs": 1}
            ],
            "jobs": [{
                "id": "j1", "name": "Photos", "kind": "sync", "direction": "push",
                "remote": "media", "remotePath": "photos",
                "localPath": "/home/me/Pictures", "createdAtMs": 1
            }],
            "mounts": []
        }"#,
    );

    let loaded = load_state(&path).expect("load the v1 file");

    assert_eq!(loaded.jobs[0].remote_id, "", "the name was guessed");
    assert_eq!(loaded.dangling_references().len(), 1);
}

#[test]
fn a_rename_keeps_the_job_on_the_same_bucket() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_state(dir.path(), "state.json", V1_STATE);
    let mut state = load_state(&path).expect("load the v1 file");

    let renamed = {
        let mut remote = state.remotes[0].clone();
        remote.name = "pictures".into();
        remote
    };
    let planned = plan_remote_update(&state, renamed).expect("plan the rename");
    apply_remote_update(&mut state, &planned).expect("apply the rename");

    assert_eq!(state.remotes[0].name, "pictures");
    assert_eq!(
        state.jobs[0].remote_id, "r1",
        "the job moved to another bucket"
    );
    assert_eq!(state.mounts[0].remote_id, "r1");
    let resolved = state
        .find_remote_by_id(&state.jobs[0].remote_id)
        .expect("the bucket the job uses");
    assert_eq!(
        resolved.name, "pictures",
        "the job did not follow the rename"
    );
}

#[test]
fn the_saved_file_carries_the_id_and_not_the_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_state(dir.path(), "state.json", V1_STATE);
    let state = load_state(&path).expect("load the v1 file");

    save_state(&path, &state).expect("save");

    let json = std::fs::read_to_string(&path).expect("read");
    assert!(json.contains("\"remoteId\""), "{json}");
    assert!(
        !json.contains("\"remote\":"),
        "the old name field survives: {json}"
    );
}
