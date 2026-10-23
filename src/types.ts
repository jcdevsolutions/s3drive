/**
 * Mirrors the Rust model. JSON is camelCase on both sides (serde `rename_all`).
 */

export type S3Provider = "aws" | "other";

export interface Remote {
  id: string;
  name: string;
  provider: S3Provider;
  accessKeyId: string;
  region: string;
  endpoint: string | null;
  forcePathStyle: boolean;
  createdAtMs: number;
}

export type JobKind = "sync" | "copy" | "bisync";
export type Direction = "push" | "pull";
export type RunOutcome = "running" | "success" | "failed" | "cancelled";

export interface RunSummary {
  runId: string;
  startedAtMs: number;
  finishedAtMs: number | null;
  outcome: RunOutcome;
  exitCode: number | null;
  message: string | null;
}

export interface Job {
  id: string;
  name: string;
  kind: JobKind;
  direction: Direction;
  remote: string;
  remotePath: string;
  localPath: string;
  dryRun: boolean;
  include: string[];
  exclude: string[];
  createdAtMs: number;
  lastRun: RunSummary | null;
}

export type VfsCacheMode = "off" | "minimal" | "writes" | "full";

export interface Mount {
  id: string;
  name: string;
  remote: string;
  remotePath: string;
  mountPoint: string;
  autoMount: boolean;
  readOnly: boolean;
  allowOther: boolean;
  vfsCacheMode: VfsCacheMode;
  createdAtMs: number;
}

export interface Settings {
  rclonePath: string | null;
  defaultFlags: string[];
  bandwidthLimit: string | null;
}

export interface AppState {
  schemaVersion: number;
  settings: Settings;
  remotes: Remote[];
  jobs: Job[];
  mounts: Mount[];
}

export interface RcloneInfo {
  available: boolean;
  program: string | null;
  version: string | null;
  source: "configured" | "path" | "missing" | string;
  message: string | null;
}

export interface FuseStatus {
  available: boolean;
  summary: string;
  detail: string;
  hint: string | null;
  installUrl: string | null;
}

export interface MountRuntime {
  mountId: string;
  running: boolean;
  pid: number | null;
  detail: string;
  startedAtMs: number | null;
}

export interface Bootstrap {
  state: AppState;
  rclone: RcloneInfo;
  fuse: FuseStatus;
  autostart: boolean;
  platform: string;
  configDir: string;
  rcloneConf: string;
  stateFile: string;
  runningMounts: MountRuntime[];
  warning: string | null;
}

export interface RemoteTest {
  ok: boolean;
  message: string;
  entries: string[];
}

export interface RemoteEntry {
  name: string;
  path: string;
  isDir: boolean;
}

export interface JobLine {
  runId: string;
  jobId: string;
  stream: "stdout" | "stderr";
  line: string;
}

export interface JobFinished {
  jobId: string;
  summary: RunSummary;
}

export interface MountChanged {
  mountId: string;
  runtime: MountRuntime;
}

export interface MountLine {
  mountId: string;
  line: string;
}
