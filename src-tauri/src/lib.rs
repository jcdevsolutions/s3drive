mod commands;
mod context;
mod jobs;
mod mounts;
mod paths;
mod runner;

use context::AppContext;
use paths::Paths;
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::MacosLauncher;

/// WebKitGTK's DMA-BUF renderer misrenders on some Linux driver stacks (seen on
/// nouveau with an NVIDIA card, X11, WebKitGTK 2.52): the window comes up as
/// corrupted horizontal bands instead of the UI. Turning that renderer off is the
/// documented workaround and costs nothing noticeable for this UI, so it is the
/// default on Linux. Set `WEBKIT_DISABLE_DMABUF_RENDERER=0` to opt back in.
/// Must run before the webview is created.
#[cfg(target_os = "linux")]
fn apply_linux_webkit_defaults() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

#[cfg(not(target_os = "linux"))]
fn apply_linux_webkit_defaults() {}

pub fn run() {
    apply_linux_webkit_defaults();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let config_dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from(".s3dive"));
            // Create it up front so "reveal config folder" works before the first save.
            let _ = std::fs::create_dir_all(&config_dir);
            let context = AppContext::load(Paths::new(config_dir));
            let warning = context.load_warning.clone();
            app.manage(context);

            if let Some(warning) = warning {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let _ = handle.emit("startup:warning", warning);
                });
            }

            // Mounts flagged `autoMount` come up whenever S3Dive starts, which is
            // what "auto mount on reboot" reduces to once the app is registered at login.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                mounts::start_auto_mounts(handle).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::refresh_rclone,
            commands::save_settings,
            commands::save_remote,
            commands::delete_remote,
            commands::test_remote,
            commands::browse_remote,
            commands::save_job,
            commands::delete_job,
            commands::run_job,
            commands::cancel_job,
            commands::save_mount,
            commands::delete_mount,
            commands::start_mount,
            commands::stop_mount,
            commands::mount_states,
            commands::fuse_status,
            commands::get_autostart,
            commands::set_autostart,
        ])
        .build(tauri::generate_context!())
        .expect("error while building S3Dive");

    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            // Both kinds of child are children of this process: a destructive mirror or a
            // FUSE mount must never outlive the window. kill_process escalates to SIGKILL.
            jobs::kill_all(handle);
            mounts::kill_all(handle);
        }
    });
}
