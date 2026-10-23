//! Where S3Dive keeps its files.
//!
//! Deliberately separate from rclone's own config: S3Dive always passes
//! `--config <app>/rclone.conf`, so a user's hand-written rclone setup is never touched.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
}

impl Paths {
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    pub fn state_file(&self) -> PathBuf {
        self.config_dir.join("state.json")
    }

    pub fn rclone_conf(&self) -> PathBuf {
        self.config_dir.join("rclone.conf")
    }

    /// The value passed to every rclone invocation as `--config`.
    pub fn conf_arg(&self) -> String {
        self.rclone_conf().to_string_lossy().to_string()
    }
}
