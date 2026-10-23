//! Platform capability probes.
//!
//! Mounting needs FUSE on macOS/Linux and WinFsp on Windows. S3Dive detects what is
//! missing and guides the user instead of running an installer with elevated rights.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Os {
    Macos,
    Windows,
    Linux,
    Other(String),
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Os::Macos
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            Os::Other(std::env::consts::OS.to_string())
        }
    }

    pub fn is_unix(&self) -> bool {
        matches!(self, Os::Macos | Os::Linux | Os::Other(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuseStatus {
    pub available: bool,
    pub summary: String,
    pub detail: String,
    #[serde(default)]
    pub hint: Option<String>,
    #[serde(default)]
    pub install_url: Option<String>,
}

impl FuseStatus {
    fn ready(summary: &str, detail: &str) -> Self {
        Self {
            available: true,
            summary: summary.to_string(),
            detail: detail.to_string(),
            hint: None,
            install_url: None,
        }
    }

    fn missing(summary: &str, detail: &str, hint: &str, install_url: &str) -> Self {
        Self {
            available: false,
            summary: summary.to_string(),
            detail: detail.to_string(),
            hint: Some(hint.to_string()),
            install_url: Some(install_url.to_string()),
        }
    }
}

const FUSE_DOC: &str = "https://rclone.org/commands/rclone_mount/#fuse";

/// `exists` answers "does this path exist?" and `in_path` answers "is this program on PATH?".
/// Both are injected so every platform branch is unit-testable on any host.
pub fn fuse_status(
    os: &Os,
    exists: &dyn Fn(&str) -> bool,
    in_path: &dyn Fn(&str) -> bool,
) -> FuseStatus {
    match os {
        Os::Linux => {
            if !exists("/dev/fuse") {
                return FuseStatus::missing(
                    "FUSE is not available",
                    "/dev/fuse does not exist. Mounting needs the fuse kernel module and fuse3.",
                    "Install fuse3 with your package manager (for example `sudo apt install fuse3`) and reboot if the module is not loaded.",
                    FUSE_DOC,
                );
            }
            if !(in_path("fusermount3") || in_path("fusermount")) {
                return FuseStatus::missing(
                    "FUSE utilities are missing",
                    "fusermount3 was not found on PATH, so rclone cannot create or release mounts.",
                    "Install the fuse3 package: `sudo apt install fuse3` (Debian/Ubuntu) or `sudo dnf install fuse3` (Fedora).",
                    FUSE_DOC,
                );
            }
            FuseStatus::ready(
                "Mounting is available",
                "/dev/fuse and fusermount were found.",
            )
        }
        Os::Macos => {
            if exists("/Library/Filesystems/macfuse.fs") {
                FuseStatus::ready("Mounting is available", "macFUSE is installed.")
            } else {
                FuseStatus::missing(
                    "macFUSE is not installed",
                    "/Library/Filesystems/macfuse.fs was not found.",
                    "Install macFUSE, allow it in System Settings > Privacy & Security, then restart S3Dive.",
                    "https://macfuse.io/",
                )
            }
        }
        Os::Windows => {
            const CANDIDATES: [&str; 3] = [
                r"C:\Program Files\WinFsp\bin\winfsp-x64.dll",
                r"C:\Program Files (x86)\WinFsp\bin\winfsp-x64.dll",
                r"C:\Program Files\WinFsp\bin\winfsp-a64.dll",
            ];
            if CANDIDATES.iter().any(|candidate| exists(candidate)) || in_path("winfsp-x64.dll") {
                FuseStatus::ready("Mounting is available", "WinFsp is installed.")
            } else {
                FuseStatus::missing(
                    "WinFsp is not installed",
                    "No WinFsp installation was found in Program Files.",
                    "Install WinFsp with `winget install WinFsp.WinFsp` or from the WinFsp releases page, then restart S3Dive.",
                    "https://winfsp.dev/rel/",
                )
            }
        }
        Os::Other(name) => FuseStatus::missing(
            "Unsupported platform for mounting",
            &format!("rclone mount support on '{name}' has not been verified by S3Dive."),
            "Sync jobs still work on this platform; only mounting is affected.",
            FUSE_DOC,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never(_: &str) -> bool {
        false
    }

    fn always(_: &str) -> bool {
        true
    }

    #[test]
    fn linux_requires_the_device_and_fusermount() {
        let no_device = fuse_status(&Os::Linux, &never, &always);
        assert!(!no_device.available);
        assert!(
            no_device.detail.contains("/dev/fuse"),
            "{}",
            no_device.detail
        );
        assert!(no_device.hint.unwrap().contains("fuse3"));

        let no_utils = fuse_status(&Os::Linux, &always, &never);
        assert!(!no_utils.available);
        assert!(
            no_utils.summary.contains("utilities"),
            "{}",
            no_utils.summary
        );

        let ready = fuse_status(&Os::Linux, &always, &|program| program == "fusermount3");
        assert!(ready.available);
        assert!(ready.hint.is_none());
    }

    #[test]
    fn macos_checks_the_macfuse_filesystem() {
        let missing = fuse_status(&Os::Macos, &never, &never);
        assert!(!missing.available);
        assert_eq!(missing.install_url.as_deref(), Some("https://macfuse.io/"));
        assert!(missing.detail.contains("/Library/Filesystems/macfuse.fs"));

        let present = fuse_status(
            &Os::Macos,
            &|path| path == "/Library/Filesystems/macfuse.fs",
            &never,
        );
        assert!(present.available);
    }

    #[test]
    fn windows_accepts_any_known_winfsp_location() {
        assert!(!fuse_status(&Os::Windows, &never, &never).available);

        let x86 = fuse_status(
            &Os::Windows,
            &|path| path == r"C:\Program Files (x86)\WinFsp\bin\winfsp-x64.dll",
            &never,
        );
        assert!(x86.available);

        let arm = fuse_status(
            &Os::Windows,
            &|path| path == r"C:\Program Files\WinFsp\bin\winfsp-a64.dll",
            &never,
        );
        assert!(arm.available);

        let on_path = fuse_status(&Os::Windows, &never, &|name| name == "winfsp-x64.dll");
        assert!(on_path.available);
    }

    #[test]
    fn unknown_platforms_keep_sync_working_but_not_mounting() {
        let status = fuse_status(&Os::Other("freebsd".into()), &always, &always);
        assert!(!status.available);
        assert!(status.summary.contains("Unsupported"));
        assert!(status.hint.unwrap().contains("Sync jobs still work"));
    }

    #[test]
    fn unix_platforms_are_recognised_for_flag_selection() {
        assert!(Os::Linux.is_unix());
        assert!(Os::Macos.is_unix());
        assert!(Os::Other("freebsd".into()).is_unix());
        assert!(!Os::Windows.is_unix());
    }
}
