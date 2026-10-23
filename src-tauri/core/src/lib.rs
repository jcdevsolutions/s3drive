//! Pure logic for S3Dive.
//!
//! Everything that can be decided without touching the OS lives here so it can be
//! unit-tested without a webview: the persisted model, validation, the exact rclone
//! argument vectors, the rclone config file format, and platform dependency probes.

pub mod model;
pub mod platform;
pub mod rclone;
pub mod rclone_conf;
pub mod remotes;
pub mod store;
pub mod validate;

pub use model::{
    new_id, now_ms, AppState, Direction, Job, JobKind, Mount, Remote, RunOutcome, RunSummary,
    S3Provider, Settings, VfsCacheMode, STATE_VERSION,
};
pub use platform::{fuse_status, Os};
pub use rclone::{split_flag_line, RemoteEntry, TransferOptions};
pub use rclone_conf::remote_pairs;
pub use remotes::{apply_remote_update, plan_remote_update, PlannedRemote};
pub use store::{load_state, save_state, StoreError};
pub use validate::extra_flags;

pub const APP_NAME: &str = "S3Dive";
