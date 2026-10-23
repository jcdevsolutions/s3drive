import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, describeError } from "../api";
import { useApp } from "../store";
import type { Direction, Job, JobKind } from "../types";
import {
  Badge,
  Banner,
  ConfirmDialog,
  EmptyState,
  Field,
  LogPanel,
  Modal,
  Toggle,
} from "../components/ui";
import { RemoteBrowser } from "../components/RemoteBrowser";
import { duration, joinRemotePath, relativeTime } from "../format";

interface Draft {
  id: string;
  createdAtMs: number;
  lastRun: Job["lastRun"];
  name: string;
  kind: JobKind;
  direction: Direction;
  remote: string;
  remotePath: string;
  localPath: string;
  dryRun: boolean;
  include: string;
  exclude: string;
}

const KIND_INFO: Record<JobKind, { label: string; blurb: string; tone: "accent" | "neutral" | "warning" }> = {
  sync: {
    label: "Mirror (sync)",
    blurb: "Makes the destination identical to the source. Files that exist only at the destination are deleted.",
    tone: "warning",
  },
  copy: {
    label: "Copy",
    blurb: "Adds and updates files at the destination. Nothing is ever deleted.",
    tone: "accent",
  },
  bisync: {
    label: "Two-way (bisync)",
    blurb:
      "Changes travel both directions, and deletions propagate too. The first run records the baseline with --resync; re-run it if the two sides drift apart.",
    tone: "neutral",
  },
};

function draftFrom(job: Job): Draft {
  return {
    id: job.id,
    createdAtMs: job.createdAtMs,
    lastRun: job.lastRun,
    name: job.name,
    kind: job.kind,
    direction: job.direction,
    remote: job.remote,
    remotePath: job.remotePath,
    localPath: job.localPath,
    dryRun: job.dryRun,
    include: job.include.join("\n"),
    exclude: job.exclude.join("\n"),
  };
}

function emptyDraft(remote: string): Draft {
  return {
    id: "",
    createdAtMs: 0,
    lastRun: null,
    name: "",
    kind: "copy",
    direction: "push",
    remote,
    remotePath: "",
    localPath: "",
    dryRun: false,
    include: "",
    exclude: "",
  };
}

function splitPatterns(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((pattern) => pattern.trim())
    .filter((pattern) => pattern !== "");
}

export default function JobsView() {
  const { state, replaceState, setNotice, logs, runs, trackRun, clearLog } = useApp();
  const [draft, setDraft] = useState<Draft | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [browsing, setBrowsing] = useState(false);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [pendingRun, setPendingRun] = useState<{ job: Job; resync: boolean } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<Job | null>(null);

  const update = (patch: Partial<Draft>) =>
    setDraft((current) => (current ? { ...current, ...patch } : current));

  const remoteOptions = state.remotes;
  const selectedRemote = draft ? remoteOptions.find((remote) => remote.name === draft.remote) : undefined;

  const pickFolder = async () => {
    const picked = await open({
      directory: true,
      multiple: false,
      title: "Choose the local folder to sync",
    });
    if (typeof picked === "string") update({ localPath: picked });
  };

  const save = async () => {
    if (!draft) return;
    setSaving(true);
    setFormError(null);
    try {
      const job: Job = {
        id: draft.id,
        createdAtMs: draft.createdAtMs,
        lastRun: draft.lastRun,
        name: draft.name,
        kind: draft.kind,
        direction: draft.direction,
        remote: draft.remote,
        remotePath: draft.remotePath,
        localPath: draft.localPath,
        dryRun: draft.dryRun,
        include: splitPatterns(draft.include),
        exclude: splitPatterns(draft.exclude),
      };
      const next = await api.saveJob(job);
      replaceState(next);
      setDraft(null);
      setNotice({ kind: "success", text: `Saved job '${job.name}'.` });
    } catch (error) {
      setFormError(describeError(error));
    } finally {
      setSaving(false);
    }
  };

  const startRun = async (job: Job, resync: boolean) => {
    try {
      const summary = await api.runJob(job.id, resync);
      trackRun({ runId: summary.runId, jobId: job.id });
      setNotice({ kind: "info", text: `Running '${job.name}'…` });
      setExpanded((current) => ({ ...current, [job.id]: true }));
    } catch (error) {
      setNotice({ kind: "error", text: describeError(error) });
    }
  };

  const requestRun = (job: Job) => {
    const destructive = job.kind === "sync" && !job.dryRun;
    // A baseline only counts when it actually completed; a failed or dry first run
    // must still be able to establish one later.
    const needsResync = job.kind === "bisync" && job.lastRun?.outcome !== "success";
    if (destructive || needsResync) {
      setPendingRun({ job, resync: needsResync });
      return;
    }
    void startRun(job, false);
  };

  const runForJob = (jobId: string) =>
    Object.values(runs).find((handle) => handle.jobId === jobId);

  const stop = async (jobId: string) => {
    const handle = runForJob(jobId);
    if (!handle) return;
    try {
      await api.cancelJob(handle.runId);
    } catch (error) {
      setNotice({ kind: "error", text: describeError(error) });
    }
  };

  const remove = async () => {
    if (!pendingDelete) return;
    try {
      const next = await api.deleteJob(pendingDelete.id);
      replaceState(next);
      setNotice({ kind: "success", text: `Removed '${pendingDelete.name}'.` });
    } catch (error) {
      setNotice({ kind: "error", text: describeError(error) });
    } finally {
      setPendingDelete(null);
    }
  };

  return (
    <section className="view">
      <header className="view__header">
        <div>
          <h1>Sync jobs</h1>
          <p className="view__subtitle">
            One folder, one bucket path. Run on demand and watch rclone's log live.
          </p>
        </div>
        <button
          type="button"
          className="button button--primary"
          disabled={remoteOptions.length === 0}
          onClick={() => setDraft(emptyDraft(remoteOptions[0]?.name ?? ""))}
        >
          New job
        </button>
      </header>

      {remoteOptions.length === 0 ? (
        <Banner tone="warning" title="Add a bucket first">
          A job needs an S3 remote. Create one in the Buckets tab, then come back here.
        </Banner>
      ) : null}

      {state.jobs.length === 0 ? (
        <EmptyState
          title="No jobs yet"
          message="Pick a local folder and a bucket path, then choose mirror, copy or two-way."
        />
      ) : (
        <div className="stack">
          {state.jobs.map((job) => {
            const running = runForJob(job.id);
            const lines = logs[job.id] ?? [];
            const isOpen = expanded[job.id] ?? false;
            const info = KIND_INFO[job.kind];
            return (
              <article className="card card--wide" key={job.id}>
                <header className="card__header">
                  <div>
                    <div className="card__title-row">
                      <h2>{job.name}</h2>
                      <Badge tone={info.tone}>{info.label}</Badge>
                      {job.dryRun ? <Badge tone="accent">dry run</Badge> : null}
                      {running ? <Badge tone="success">running</Badge> : null}
                    </div>
                    <p className="card__meta mono">
                      {job.kind === "bisync"
                        ? `${job.localPath} ⇄ ${joinRemotePath(job.remote, job.remotePath)}`
                        : job.direction === "push"
                          ? `${job.localPath} → ${joinRemotePath(job.remote, job.remotePath)}`
                          : `${joinRemotePath(job.remote, job.remotePath)} → ${job.localPath}`}
                    </p>
                  </div>
                </header>

                <div className="job-status">
                  <span>
                    Last run:{" "}
                    <strong>{job.lastRun ? job.lastRun.outcome : "never"}</strong>
                    {job.lastRun ? ` · ${relativeTime(job.lastRun.startedAtMs)}` : ""}
                    {job.lastRun?.finishedAtMs
                      ? ` · took ${duration(job.lastRun.startedAtMs, job.lastRun.finishedAtMs)}`
                      : ""}
                  </span>
                  {job.lastRun?.message ? (
                    <span className="job-status__message">{job.lastRun.message}</span>
                  ) : null}
                </div>

                <footer className="card__actions">
                  {running ? (
                    <button type="button" className="button button--danger" onClick={() => void stop(job.id)}>
                      Stop
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="button button--primary"
                      onClick={() => requestRun(job)}
                    >
                      Run now
                    </button>
                  )}
                  <button
                    type="button"
                    className="button"
                    onClick={() =>
                      setExpanded((current) => ({ ...current, [job.id]: !isOpen }))
                    }
                  >
                    {isOpen ? "Hide log" : `Log (${lines.length})`}
                  </button>
                  {job.kind === "bisync" ? (
                    <button
                      type="button"
                      className="button"
                      disabled={Boolean(running)}
                      onClick={() => setPendingRun({ job, resync: true })}
                      title="Re-record the bisync baseline with --resync"
                    >
                      Re-sync baseline
                    </button>
                  ) : null}
                  <button type="button" className="button" onClick={() => setDraft(draftFrom(job))}>
                    Edit
                  </button>
                  <button type="button" className="button button--quiet" onClick={() => setPendingDelete(job)}>
                    Delete
                  </button>
                </footer>

                {isOpen ? (
                  <LogPanel
                    lines={lines}
                    running={Boolean(running)}
                    onClear={() => clearLog(job.id)}
                    placeholder="Run the job to see rclone output here."
                  />
                ) : null}
              </article>
            );
          })}
        </div>
      )}

      {draft ? (
        <Modal
          wide
          title={draft.id ? `Edit job '${draft.name}'` : "New sync job"}
          subtitle="Nothing runs in the background: a job runs when you press Run."
          onClose={() => setDraft(null)}
          footer={
            <>
              <button type="button" className="button" onClick={() => setDraft(null)}>
                Cancel
              </button>
              <button
                type="button"
                className="button button--primary"
                disabled={saving || draft.name.trim() === "" || draft.localPath === ""}
                onClick={() => void save()}
              >
                {saving ? "Saving…" : "Save job"}
              </button>
            </>
          }
        >
          {formError ? <Banner tone="danger" title="Could not save">{formError}</Banner> : null}

          <div className="form-grid">
            <Field label="Job name">
              <input
                className="input"
                value={draft.name}
                placeholder="Photos to S3"
                onChange={(event) => update({ name: event.target.value })}
              />
            </Field>

            <Field label="Bucket">
              <select
                className="input"
                value={draft.remote}
                onChange={(event) => update({ remote: event.target.value, remotePath: "" })}
              >
                {remoteOptions.map((remote) => (
                  <option key={remote.id} value={remote.name}>
                    {remote.name}
                  </option>
                ))}
              </select>
            </Field>
          </div>

          <div className="kind-picker">
            {(Object.keys(KIND_INFO) as JobKind[]).map((kind) => (
              <button
                key={kind}
                type="button"
                className={`kind${draft.kind === kind ? " kind--active" : ""}`}
                onClick={() => update({ kind })}
              >
                <span className="kind__label">{KIND_INFO[kind].label}</span>
                <span className="kind__blurb">{KIND_INFO[kind].blurb}</span>
              </button>
            ))}
          </div>

          {draft.kind !== "bisync" ? (
            <Field label="Direction">
              <div className="segmented">
                <button
                  type="button"
                  className={`segmented__option${draft.direction === "push" ? " segmented__option--active" : ""}`}
                  onClick={() => update({ direction: "push" })}
                >
                  Local folder → bucket
                </button>
                <button
                  type="button"
                  className={`segmented__option${draft.direction === "pull" ? " segmented__option--active" : ""}`}
                  onClick={() => update({ direction: "pull" })}
                >
                  Bucket → local folder
                </button>
              </div>
            </Field>
          ) : null}

          <Field label="Bucket folder" hint={selectedRemote ? `${draft.remote}:${draft.remotePath}` : undefined}>
            <div className="input-row">
              <input
                className="input mono"
                value={draft.remotePath}
                placeholder="path/inside/bucket (empty = whole bucket)"
                onChange={(event) => update({ remotePath: event.target.value })}
              />
              <button
                type="button"
                className="button"
                disabled={!selectedRemote}
                onClick={() => setBrowsing(true)}
              >
                Browse…
              </button>
            </div>
          </Field>

          <Field label="Local folder">
            <div className="input-row">
              <input
                className="input mono"
                value={draft.localPath}
                placeholder="/home/you/Pictures"
                onChange={(event) => update({ localPath: event.target.value })}
              />
              <button type="button" className="button" onClick={() => void pickFolder()}>
                Choose…
              </button>
            </div>
          </Field>

          <div className="form-grid">
            <Field label="Include patterns" hint="One per line, rclone glob syntax. Empty means everything.">
              <textarea
                className="input input--area mono"
                value={draft.include}
                rows={3}
                placeholder="*.jpg&#10;*.png"
                onChange={(event) => update({ include: event.target.value })}
              />
            </Field>
            <Field label="Exclude patterns" hint="One per line. Applied after includes.">
              <textarea
                className="input input--area mono"
                value={draft.exclude}
                rows={3}
                placeholder="*.tmp&#10;.DS_Store"
                onChange={(event) => update({ exclude: event.target.value })}
              />
            </Field>
          </div>

          <Toggle
            checked={draft.dryRun}
            onChange={(dryRun) => update({ dryRun })}
            label="Dry run"
            description="Report what would change without touching either side."
          />
        </Modal>
      ) : null}

      {browsing && selectedRemote ? (
        <Modal
          wide
          title="Choose a bucket folder"
          onClose={() => setBrowsing(false)}
          footer={null}
        >
          <RemoteBrowser
            remoteId={selectedRemote.id}
            remoteName={selectedRemote.name}
            initialPath={draft?.remotePath ?? ""}
            onPick={(path) => {
              update({ remotePath: path });
              setBrowsing(false);
            }}
            onCancel={() => setBrowsing(false)}
          />
        </Modal>
      ) : null}

      {pendingRun ? (
        <ConfirmDialog
          title={pendingRun.job.kind === "sync" ? "Mirroring can delete files" : "First two-way run"}
          message={
            pendingRun.job.kind === "sync"
              ? "rclone sync makes the destination identical to the source. Files that exist only at the destination will be deleted."
              : pendingRun.resync
                ? "This run records the bisync baseline with --resync. Deletions propagate in both directions on later runs, so make sure both sides hold what you expect."
                : "rclone bisync moves changes both ways and propagates deletions. Files deleted on one side are removed on the other."
          }
          detail={`${pendingRun.job.localPath}\n${joinRemotePath(pendingRun.job.remote, pendingRun.job.remotePath)}`}
          confirmLabel={
            pendingRun.job.kind === "sync"
              ? "Run mirror anyway"
              : pendingRun.resync
                ? "Start baseline run"
                : "Run two-way sync"
          }
          tone="danger"
          onConfirm={() => {
            const { job, resync } = pendingRun;
            setPendingRun(null);
            void startRun(job, resync);
          }}
          onCancel={() => setPendingRun(null)}
        />
      ) : null}

      {pendingDelete ? (
        <ConfirmDialog
          title={`Delete job '${pendingDelete.name}'?`}
          message="The job definition is removed. Files at either end are untouched."
          confirmLabel="Delete job"
          onConfirm={() => void remove()}
          onCancel={() => setPendingDelete(null)}
        />
      ) : null}
    </section>
  );
}
