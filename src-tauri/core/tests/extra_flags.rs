//! The public API the Settings screen depends on: an extra flag must reach rclone
//! as separate arguments, and the config flag must be refused.

use s3dive_core::rclone::{split_flag_line, transfer_args, TransferOptions};
use s3dive_core::validate::extra_flags;
use s3dive_core::{Job, JobKind, Settings};

const CONFIG: &str = "/home/me/.config/s3dive/rclone.conf";

#[test]
fn a_flag_line_with_a_value_splits_into_two_arguments() {
    let job = Job::new("Docs", JobKind::Copy, "media", "docs", "/home/me/docs");
    let settings = Settings {
        default_flags: vec!["--transfers 8".into()],
        ..Settings::default()
    };

    let args = transfer_args(&job, &settings, CONFIG, TransferOptions::default());

    assert!(args.contains(&"--transfers".to_string()), "{args:?}");
    assert!(args.contains(&"8".to_string()), "{args:?}");
    assert!(!args.contains(&"--transfers 8".to_string()), "{args:?}");
}

#[test]
fn the_tokenizer_splits_what_the_user_typed() {
    assert_eq!(split_flag_line("--transfers 8"), vec!["--transfers", "8"]);
    assert_eq!(split_flag_line("--transfers=8"), vec!["--transfers=8"]);
    assert_eq!(
        split_flag_line("--log-file \"my file.log\""),
        vec!["--log-file", "my file.log"]
    );
    assert!(split_flag_line("   ").is_empty());
    assert_eq!(split_flag_line("--transfers\t8"), vec!["--transfers", "8"]);
}

#[test]
fn a_valid_extra_flag_is_accepted() {
    assert!(extra_flags(&["--transfers 8".into()]).is_ok());
    assert!(extra_flags(&[]).is_ok());
}

#[test]
fn a_line_that_is_not_a_flag_is_refused() {
    let error = extra_flags(&["transfers 8".into()]).unwrap_err();
    assert_eq!(
        error,
        "'transfers 8' is not an rclone flag. Extra flags must start with '-' (for example --transfers 8)."
    );
}

#[test]
fn the_config_flag_is_refused() {
    assert!(extra_flags(&["--config /etc/rclone.conf".into()]).is_err());
    assert!(extra_flags(&["--config=/etc/rclone.conf".into()]).is_err());
}
