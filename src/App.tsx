import { useEffect, useState } from "react";
import { S3DiveProvider, useApp } from "./store";
import { Banner } from "./components/ui";
import BucketsView from "./views/BucketsView";
import JobsView from "./views/JobsView";
import MountsView from "./views/MountsView";
import SettingsView from "./views/SettingsView";

type ViewKey = "buckets" | "jobs" | "mounts" | "settings";

const NAV: Array<{ key: ViewKey; label: string; hint: string }> = [
  { key: "buckets", label: "Buckets", hint: "S3 remotes and credentials" },
  { key: "jobs", label: "Jobs", hint: "Sync, copy and two-way runs" },
  { key: "mounts", label: "Mounts", hint: "Buckets as local folders" },
  { key: "settings", label: "Settings", hint: "rclone and app options" },
];

function NoticeBar() {
  const { notice, setNotice } = useApp();

  useEffect(() => {
    if (!notice || notice.kind === "error") return;
    const timer = window.setTimeout(() => setNotice(null), 6000);
    return () => window.clearTimeout(timer);
  }, [notice, setNotice]);

  if (!notice) return null;

  return (
    <div className={`notice notice--${notice.kind}`} role="status">
      <span>{notice.text}</span>
      <button type="button" className="icon-button" onClick={() => setNotice(null)} aria-label="Dismiss">
        ×
      </button>
    </div>
  );
}

function Shell() {
  const { state, rclone, fuse, runtimes, runs, dangling } = useApp();
  const [view, setView] = useState<ViewKey>("buckets");

  const runningMounts = Object.values(runtimes).filter((runtime) => runtime.running).length;
  const runningJobs = Object.keys(runs).length;

  const counts: Record<ViewKey, string> = {
    buckets: String(state.remotes.length),
    jobs: runningJobs > 0 ? `${runningJobs} running` : String(state.jobs.length),
    mounts: runningMounts > 0 ? `${runningMounts} live` : String(state.mounts.length),
    settings: "",
  };

  return (
    <div className="shell">
      <aside className="rail">
        <div className="rail__brand">
          <span className="rail__mark" aria-hidden="true">
            ⌄
          </span>
          <div>
            <strong>S3Dive</strong>
            <span className="rail__tag">rclone navigator</span>
          </div>
        </div>

        <nav className="rail__nav" aria-label="Sections">
          {NAV.map((item) => (
            <button
              key={item.key}
              type="button"
              className={`rail__link${view === item.key ? " rail__link--active" : ""}`}
              onClick={() => setView(item.key)}
              aria-current={view === item.key ? "page" : undefined}
            >
              <span className="rail__link-label">{item.label}</span>
              {counts[item.key] ? <span className="rail__count">{counts[item.key]}</span> : null}
              <span className="rail__link-hint">{item.hint}</span>
            </button>
          ))}
        </nav>

        <footer className="rail__footer">
          <div className="rail__stat">
            <span className={`dot ${rclone.available ? "dot--ok" : "dot--warn"}`} />
            <span>{rclone.available ? `rclone ${rclone.version ?? ""}` : "rclone missing"}</span>
          </div>
          <div className="rail__stat">
            <span className={`dot ${fuse.available ? "dot--ok" : "dot--idle"}`} />
            <span>{fuse.available ? "mounting ready" : "mounting unavailable"}</span>
          </div>
        </footer>
      </aside>

      <main className="main">
        <NoticeBar />
        {dangling.length > 0 ? (
          <Banner tone="warning" title="Some jobs or mounts lost their bucket">
            <p>
              These entries point at a bucket that no longer exists. They are left
              untouched: pick another bucket and save them to use them again.
            </p>
            <ul>
              {dangling.map((entry) => (
                <li key={entry}>{entry}</li>
              ))}
            </ul>
          </Banner>
        ) : null}
        {view === "buckets" ? <BucketsView /> : null}
        {view === "jobs" ? <JobsView /> : null}
        {view === "mounts" ? <MountsView /> : null}
        {view === "settings" ? <SettingsView /> : null}
      </main>
    </div>
  );
}

export default function App() {
  return (
    <S3DiveProvider>
      <Shell />
    </S3DiveProvider>
  );
}
