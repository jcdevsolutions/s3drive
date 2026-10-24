import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, describeError } from "./api";
import type {
  AppState,
  FuseStatus,
  Platform,
  JobFinished,
  JobLine,
  MountChanged,
  MountLine,
  MountRuntime,
  RcloneInfo,
} from "./types";

const MAX_LOG_LINES = 1500;

export interface Notice {
  kind: "info" | "success" | "error";
  text: string;
}

export interface RunHandle {
  runId: string;
  jobId: string;
}

interface AppContextValue {
  state: AppState;
  rclone: RcloneInfo;
  fuse: FuseStatus;
  autostart: boolean;
  platform: Platform;
  /** The release version of the app. */
  version: string;
  configDir: string;
  rcloneConf: string;
  stateFile: string;
  logs: Record<string, string[]>;
  runs: Record<string, RunHandle>;
  runtimes: Record<string, MountRuntime>;
  /** Jobs and mounts whose bucket is gone. */
  dangling: string[];
  notice: Notice | null;
  setNotice: (notice: Notice | null) => void;
  replaceState: (state: AppState) => void;
  replaceRclone: (info: RcloneInfo) => void;
  replaceAutostart: (enabled: boolean) => void;
  replaceFuse: (status: FuseStatus) => void;
  trackRun: (handle: RunHandle) => void;
  clearLog: (key: string) => void;
}

const AppContext = createContext<AppContextValue | null>(null);

export function useApp(): AppContextValue {
  const value = useContext(AppContext);
  if (!value) {
    throw new Error("useApp must be used inside <S3DiveProvider>");
  }
  return value;
}

function appendLine(
  log: Record<string, string[]>,
  key: string,
  line: string,
): Record<string, string[]> {
  const existing = log[key] ?? [];
  const next = existing.length >= MAX_LOG_LINES ? existing.slice(1) : existing.slice();
  next.push(line);
  return { ...log, [key]: next };
}

export function S3DiveProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AppState | null>(null);
  const [rclone, setRclone] = useState<RcloneInfo | null>(null);
  const [fuse, setFuse] = useState<FuseStatus | null>(null);
  const [autostart, setAutostart] = useState(false);
  const [platform, setPlatform] = useState<Platform | null>(null);
  const [version, setVersion] = useState("");
  const [paths, setPaths] = useState({ configDir: "", rcloneConf: "", stateFile: "" });
  const [logs, setLogs] = useState<Record<string, string[]>>({});
  const [runs, setRuns] = useState<Record<string, RunHandle>>({});
  const [runtimes, setRuntimes] = useState<Record<string, MountRuntime>>({});
  const [dangling, setDangling] = useState<Array<string>>([]);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [fatal, setFatal] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const unlisteners: Array<() => void> = [];

    // Registering after an await means the cleanup can run first (React strict mode
    // mounts, unmounts and mounts again), so unlisten immediately when that happens.
    const register = async <T,>(event: string, handler: (payload: T) => void) => {
      const off = await listen<T>(event, ({ payload }) => handler(payload));
      if (!alive) {
        off();
        return;
      }
      unlisteners.push(off);
    };

    void (async () => {
      try {
        const boot = await api.bootstrap();
        if (!alive) return;

        setState(boot.state);
        setRclone(boot.rclone);
        setFuse(boot.fuse);
        setAutostart(boot.autostart);
        setPlatform(boot.platform);
        setVersion(boot.version);
        setPaths({
          configDir: boot.configDir,
          rcloneConf: boot.rcloneConf,
          stateFile: boot.stateFile,
        });
        setRuntimes(
          Object.fromEntries(boot.runningMounts.map((runtime) => [runtime.mountId, runtime])),
        );
        setDangling(boot.dangling);
        if (boot.warning) {
          setNotice({ kind: "error", text: boot.warning });
        }

        await register<JobLine>("job:line", (payload) => {
          setLogs((log) => appendLine(log, payload.jobId, payload.line));
        });

        await register<JobFinished>("job:finished", (payload) => {
          setRuns((current) => {
            const next = { ...current };
            for (const [runId, handle] of Object.entries(next)) {
              if (handle.jobId === payload.jobId) delete next[runId];
            }
            return next;
          });
          const { summary } = payload;
          // The backend writes the summary to the state file before it announces
          // the end of the run, so the store can update the job in place. Asking
          // for the whole state again would re-read the file and run rclone.
          setState((current) =>
            current
              ? {
                  ...current,
                  jobs: current.jobs.map((job) =>
                    job.id === payload.jobId ? { ...job, lastRun: summary } : job,
                  ),
                }
              : current,
          );
          const text =
            summary.outcome === "success"
              ? "Run finished successfully."
              : (summary.message ?? `Run ended as ${summary.outcome}.`);
          setNotice({
            kind: summary.outcome === "success" ? "success" : "error",
            text,
          });
        });

        await register<MountChanged>("mount:changed", (payload) => {
          setRuntimes((current) => ({ ...current, [payload.mountId]: payload.runtime }));
        });

        await register<MountLine>("mount:line", (payload) => {
          setLogs((log) => appendLine(log, `mount:${payload.mountId}`, payload.line));
        });

        await register<string>("startup:warning", (payload) => {
          setNotice({ kind: "error", text: payload });
        });
      } catch (error) {
        if (alive) setFatal(describeError(error));
      }
    })();

    return () => {
      alive = false;
      for (const off of unlisteners) off();
    };
  }, []);

  const trackRun = useCallback((handle: RunHandle) => {
    setRuns((current) => ({ ...current, [handle.runId]: handle }));
  }, []);

  const clearLog = useCallback((key: string) => {
    setLogs((current) => {
      const next = { ...current };
      delete next[key];
      return next;
    });
  }, []);

  const replaceState = useCallback((next: AppState) => setState(next), []);
  const replaceRclone = useCallback((info: RcloneInfo) => setRclone(info), []);
  const replaceAutostart = useCallback((enabled: boolean) => setAutostart(enabled), []);
  const replaceFuse = useCallback((status: FuseStatus) => setFuse(status), []);

  const value = useMemo<AppContextValue | null>(() => {
    if (!state || !rclone || !fuse || !platform) return null;
    return {
      state,
      rclone,
      fuse,
      autostart,
      platform,
      version,
      configDir: paths.configDir,
      rcloneConf: paths.rcloneConf,
      stateFile: paths.stateFile,
      logs,
      runs,
      runtimes,
      dangling,
      notice,
      setNotice,
      replaceState,
      replaceRclone,
      replaceAutostart,
      replaceFuse,
      trackRun,
      clearLog,
    };
  }, [
    state,
    rclone,
    fuse,
    autostart,
    platform,
    version,
    paths,
    logs,
    runs,
    runtimes,
    dangling,
    notice,
    replaceState,
    replaceRclone,
    replaceAutostart,
    replaceFuse,
    trackRun,
    clearLog,
  ]);

  if (fatal) {
    return (
      <main className="boot boot--error">
        <h1>S3Dive could not start</h1>
        <p>{fatal}</p>
      </main>
    );
  }

  if (!value) {
    return (
      <main className="boot">
        <span className="boot__mark">S3Dive</span>
        <span className="boot__hint">Checking rclone and your saved buckets…</span>
      </main>
    );
  }

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}
