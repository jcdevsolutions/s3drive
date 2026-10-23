//! The remote lifecycle: identity, uniqueness and reference rewrites.
//!
//! Pure state transitions, so they are testable without a GUI, a display server or
//! an rclone binary.

use crate::model::{new_id, now_ms, AppState, Remote};

/// A remote save, prepared but not applied.
///
/// The prepared record already carries its identity, so nothing downstream can
/// mistake a new remote for an existing one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRemote {
    /// The record as it will be stored.
    pub record: Remote,
    /// The name the remote had before this save, when it already existed.
    pub previous_name: Option<String>,
}

/// Prepare a remote save: give it an identity and find what it replaces.
///
/// A record without an id is a new remote and gets a fresh one; a record with a
/// known id is an edit of that remote and keeps its creation time.
pub fn plan_remote_update(state: &AppState, incoming: Remote) -> Result<PlannedRemote, String> {
    let mut record = incoming;
    if record.id.trim().is_empty() {
        record.id = new_id();
    }

    let existing = state.remotes.iter().find(|c| c.id == record.id).cloned();
    match &existing {
        Some(previous) => record.created_at_ms = previous.created_at_ms,
        None => {
            if record.created_at_ms == 0 {
                record.created_at_ms = now_ms();
            }
        }
    }

    Ok(PlannedRemote {
        record,
        previous_name: existing.map(|previous| previous.name),
    })
}

/// Apply a prepared remote save to the state.
///
/// The name is the rclone config section name, so it must stay unique. A rename
/// changes the name only: the jobs and mounts that use this remote keep pointing
/// at its id, so they follow the bucket the user renamed.
pub fn apply_remote_update(state: &mut AppState, planned: &PlannedRemote) -> Result<(), String> {
    let record = &planned.record;

    if state.remote_name_taken(&record.name, Some(record.id.as_str())) {
        return Err(format!("A bucket named '{}' already exists.", record.name));
    }

    match state.remotes.iter().position(|c| c.id == record.id) {
        Some(index) => state.remotes[index] = record.clone(),
        None => state.remotes.push(record.clone()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Job, JobKind, Mount};

    fn remote_with_id(id: &str, name: &str) -> Remote {
        let mut remote = Remote::new(name, "AKIAEXAMPLE");
        remote.id = id.to_string();
        remote
    }

    #[test]
    fn a_new_remote_gets_an_identity() {
        let state = AppState::default();
        let mut incoming = Remote::new("media", "AKIA");
        incoming.id = String::new();

        let planned = plan_remote_update(&state, incoming).expect("plan");

        assert!(!planned.record.id.is_empty(), "the record kept an empty id");
        assert!(planned.previous_name.is_none());
        assert_ne!(planned.record.created_at_ms, 0);
    }

    #[test]
    fn a_second_bucket_does_not_replace_the_first() {
        let mut state = AppState::default();
        state.remotes.push(remote_with_id("r1", "media"));

        let mut second = Remote::new("backup", "AKIA2");
        second.id = String::new();
        let planned = plan_remote_update(&state, second).expect("plan");
        apply_remote_update(&mut state, &planned).expect("apply");

        assert_eq!(state.remotes.len(), 2, "the first bucket was replaced");
        assert_eq!(state.remotes[0].name, "media");
        assert_eq!(state.remotes[1].name, "backup");
        assert_ne!(state.remotes[0].id, state.remotes[1].id);
    }

    #[test]
    fn saving_the_same_remote_twice_keeps_one_record() {
        let mut state = AppState::default();
        state.remotes.push(remote_with_id("r1", "media"));

        let mut edit = remote_with_id("r1", "media");
        edit.region = "eu-west-1".into();
        let planned = plan_remote_update(&state, edit).expect("plan");
        apply_remote_update(&mut state, &planned).expect("apply");

        assert_eq!(state.remotes.len(), 1);
        assert_eq!(state.remotes[0].region, "eu-west-1");
    }

    #[test]
    fn editing_a_remote_keeps_its_creation_time() {
        let mut state = AppState::default();
        let mut original = remote_with_id("r1", "media");
        original.created_at_ms = 1234;
        state.remotes.push(original);

        let mut edit = remote_with_id("r1", "media");
        edit.created_at_ms = 0;
        let planned = plan_remote_update(&state, edit).expect("plan");
        apply_remote_update(&mut state, &planned).expect("apply");

        assert_eq!(state.remotes[0].created_at_ms, 1234);
    }

    #[test]
    fn a_duplicate_name_is_refused() {
        let mut state = AppState::default();
        state.remotes.push(remote_with_id("r1", "media"));

        let mut clash = Remote::new("Media", "AKIA2");
        clash.id = String::new();
        let planned = plan_remote_update(&state, clash).expect("plan");
        let error = apply_remote_update(&mut state, &planned).expect_err("duplicate name");

        assert!(
            error.contains("A bucket named 'Media' already exists."),
            "{error}"
        );
        assert_eq!(
            state.remotes.len(),
            1,
            "the state changed despite the error"
        );
    }

    #[test]
    fn renaming_a_bucket_keeps_the_jobs_pointing_at_the_same_bucket() {
        let mut state = AppState::default();
        state.remotes.push(remote_with_id("r1", "media"));
        state.jobs.push(Job::new(
            "Photos",
            JobKind::Sync,
            "r1",
            "photos",
            "/home/me/Pictures",
        ));
        state
            .mounts
            .push(Mount::new("Media", "r1", "photos", "/mnt/media"));

        let renamed = remote_with_id("r1", "pictures");
        let planned = plan_remote_update(&state, renamed).expect("plan");
        apply_remote_update(&mut state, &planned).expect("apply");

        assert_eq!(state.remotes[0].name, "pictures");
        // The jobs keep the id, so they follow the bucket the user renamed.
        assert_eq!(state.jobs[0].remote_id, "r1");
        assert_eq!(state.mounts[0].remote_id, "r1");
        let remote = state
            .find_remote_by_id(&state.jobs[0].remote_id)
            .expect("the remote the job uses");
        assert_eq!(remote.name, "pictures");
    }

    #[test]
    fn a_second_bucket_does_not_repoint_an_existing_job() {
        let mut state = AppState::default();
        state.remotes.push(remote_with_id("r1", "media"));
        state.jobs.push(Job::new(
            "Photos",
            JobKind::Sync,
            "r1",
            "photos",
            "/home/me/Pictures",
        ));

        let mut second = Remote::new("backup", "AKIA2");
        second.id = String::new();
        let planned = plan_remote_update(&state, second).expect("plan");
        apply_remote_update(&mut state, &planned).expect("apply");

        assert_eq!(
            state.jobs[0].remote_id, "r1",
            "the job was repointed at another bucket"
        );
    }
}
