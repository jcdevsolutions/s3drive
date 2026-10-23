import { invoke } from "@tauri-apps/api/core";
import type {
  AppState,
  Bootstrap,
  FuseStatus,
  Job,
  Mount,
  MountRuntime,
  RcloneInfo,
  Remote,
  RemoteEntry,
  RemoteTest,
  RunSummary,
  Settings,
} from "./types";

/**
 * Typed wrappers for the Rust command surface. Tauri maps JS camelCase argument
 * keys onto the snake_case Rust parameters.
 */
export const api = {
  bootstrap: () => invoke<Bootstrap>("bootstrap"),
  refreshRclone: () => invoke<RcloneInfo>("refresh_rclone"),

  saveSettings: (settings: Settings) => invoke<AppState>("save_settings", { settings }),

  saveRemote: (remote: Remote, secretAccessKey: string | null) =>
    invoke<AppState>("save_remote", { remote, secretAccessKey }),
  deleteRemote: (id: string) => invoke<AppState>("delete_remote", { id }),
  testRemote: (id: string) => invoke<RemoteTest>("test_remote", { id }),
  browseRemote: (remoteId: string, path: string) =>
    invoke<RemoteEntry[]>("browse_remote", { remoteId, path }),

  saveJob: (job: Job) => invoke<AppState>("save_job", { job }),
  deleteJob: (id: string) => invoke<AppState>("delete_job", { id }),
  runJob: (jobId: string, resync: boolean) => invoke<RunSummary>("run_job", { jobId, resync }),
  cancelJob: (runId: string) => invoke<void>("cancel_job", { runId }),

  saveMount: (mount: Mount) => invoke<AppState>("save_mount", { mount }),
  deleteMount: (id: string) => invoke<AppState>("delete_mount", { id }),
  startMount: (id: string) => invoke<MountRuntime>("start_mount", { id }),
  stopMount: (id: string) => invoke<void>("stop_mount", { id }),
  mountStates: () => invoke<MountRuntime[]>("mount_states"),
  fuseStatus: () => invoke<FuseStatus>("fuse_status"),

  getAutostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
};

/** Commands reject with the Rust `Err(String)`, so render that directly. */
export function describeError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
