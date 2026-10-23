import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, describeError } from "../api";
import { useApp } from "../store";
import type { Mount, VfsCacheMode } from "../types";
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
import { joinRemotePath } from "../format";

interface Draft {
  id: string;
  createdAtMs: number;
  name: string;
  remote: string;
  remotePath: string;
  mountPoint: string;
  autoMount: boolean;
  readOnly: boolean;
  allowOther: boolean;
  vfsCacheMode: VfsCacheMode;
}

const CACHE_MODES: Array<{ value: VfsCacheMode; label: string; blurb: string }> = [
  { value: "off", label: "Off", blurb: "No caching. Slowest, streams only." },
  { value: "minimal", label: "Minimal", blurb: "Caches open files only." },
  { value: "writes", label: "Writes", blurb: "Caches writes locally. Recommended default." },
  { value: "full", label: "Full", blurb: "Caches reads and writes. Most disk, most speed." },
];

function draftFrom(mount: Mount): Draft {
  return { ...mount };
}

function emptyDraft(remote: string): Draft {
  return {
    id: "",
    createdAtMs: 0,
    name: "",
    remote,
    remotePath: "",
    mountPoint: "",
    autoMount: true,
    readOnly: false,
    allowOther: false,
    vfsCacheMode: "writes",
  };
}

export default function MountsView() {
  const { state, fuse, platform, autostart, replaceState, replaceAutostart, setNotice, logs, runtimes, clearLog } =
    useApp();
  const [draft, setDraft] = useState<Draft | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [browsing, setBrowsing] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [pendingDelete, setPendingDelete] = useState<Mount | null>(null);

  const isWindows = platform === "Windows";
  const update = (patch: Partial<Draft>) =>
    setDraft((current) => (current ? { ...current, ...patch } : current));

  const selectedRemote = draft ? state.remotes.find((remote) => remote.name === draft.remote) : undefined;

  const pickMountPoint = async () => {
    const picked = await open({
      directory: true,
      multiple: false,
      title: "Choose the folder to mount into",
    });
    if (typeof picked === "string") update({ mountPoint: picked });
  };

  const save = async () => {
    if (!draft) return;
    setSaving(true);
    setFormError(null);
    try {
      const mount: Mount = { ...draft };
      const next = await api.saveMount(mount);
      replaceState(next);
      setDraft(null);
      setNotice({ kind: "success", text: `Saved mount '${mount.name}'.` });
    } catch (error) {
      setFormError(describeError(error));
    } finally {
      setSaving(false);
    }
  };

  const start = async (mount: Mount) => {
    setBusy(mount.id);
    try {
      const runtime = await api.startMount(mount.id);
      setNotice({ kind: "info", text: runtime.detail });
    } catch (error) {
      setNotice({ kind: "error", text: describeError(error) });
    } finally {
      setBusy(null);
    }
  };

  const stop = async (mount: Mount) => {
    setBusy(mount.id);
    try {
      await api.stopMount(mount.id);
      setNotice({ kind: "info", text: `Unmounting '${mount.name}'…` });
    } catch (error) {
      setNotice({ kind: "error", text: describeError(error) });
    } finally {
      setBusy(null);
    }
  };

  const toggleAutostart = async (enabled: boolean) => {
    try {
      const actual = await api.setAutostart(enabled);
      replaceAutostart(actual);
      setNotice({
        kind: "success",
        text: actual
          ? "S3Dive will start at login, and mounts marked auto-mount come up with it."
          : "S3Dive will no longer start at login.",
      });
    } catch (error) {
      setNotice({ kind: "error", text: describeError(error) });
    }
  };

  const remove = async () => {
    if (!pendingDelete) return;
    try {
      const next = await api.deleteMount(pendingDelete.id);
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
          <h1>Mounts</h1>
          <p className="view__subtitle">
            Expose a bucket as a normal folder. Mounts marked auto-mount start whenever
            S3Dive starts.
          </p>
        </div>
        <button
          type="button"
          className="button button--primary"
          disabled={state.remotes.length === 0}
          onClick={() => setDraft(emptyDraft(state.remotes[0]?.name ?? ""))}
        >
          New mount
        </button>
      </header>

      {!fuse.available ? (
        <Banner
          tone="warning"
          title={fuse.summary}
          action={
            fuse.installUrl ? (
              <button
                type="button"
                className="button"
                onClick={() => void openUrl(fuse.installUrl ?? "")}
              >
                Install guide
              </button>
            ) : undefined
          }
        >
          <p>{fuse.detail}</p>
          {fuse.hint ? <p>{fuse.hint}</p> : null}
        </Banner>
      ) : null}

      <div className="card card--inline">
        <Toggle
          checked={autostart}
          onChange={(next) => void toggleAutostart(next)}
          label="Start S3Dive at login"
          description="Registers the app with the operating system. Both the app and its auto-mounts come up after a reboot."
        />
      </div>

      {state.mounts.length === 0 ? (
        <EmptyState
          title="Nothing mounted yet"
          message="Create a mount, point it at a bucket folder and an empty local directory."
        />
      ) : (
        <div className="stack">
          {state.mounts.map((mount) => {
            const runtime = runtimes[mount.id];
            const running = runtime?.running ?? false;
            const lines = logs[`mount:${mount.id}`] ?? [];
            const isOpen = expanded[mount.id] ?? false;
            return (
              <article className="card card--wide" key={mount.id}>
                <header className="card__header">
                  <div>
                    <div className="card__title-row">
                      <h2>{mount.name}</h2>
                      <Badge tone={running ? "success" : "neutral"}>
                        {running ? "mounted" : "stopped"}
                      </Badge>
                      {mount.autoMount ? <Badge tone="accent">auto-mount</Badge> : null}
                      {mount.readOnly ? <Badge tone="neutral">read-only</Badge> : null}
                    </div>
                    <p className="card__meta mono">
                      {joinRemotePath(mount.remote, mount.remotePath)} → {mount.mountPoint}
                    </p>
                  </div>
                </header>

                <div className="job-status">
                  <span>
                    Cache: <strong>{mount.vfsCacheMode}</strong>
                    {runtime?.pid ? ` · pid ${runtime.pid}` : ""}
                  </span>
                  {runtime?.detail ? <span className="job-status__message">{runtime.detail}</span> : null}
                </div>

                <footer className="card__actions">
                  {running ? (
                    <button
                      type="button"
                      className="button button--danger"
                      disabled={busy === mount.id}
                      onClick={() => void stop(mount)}
                    >
                      Unmount
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="button button--primary"
                      disabled={busy === mount.id || !fuse.available}
                      onClick={() => void start(mount)}
                    >
                      Mount
                    </button>
                  )}
                  <button
                    type="button"
                    className="button"
                    onClick={() => setExpanded((current) => ({ ...current, [mount.id]: !isOpen }))}
                  >
                    {isOpen ? "Hide log" : "Log"}
                  </button>
                  <button type="button" className="button" onClick={() => setDraft(draftFrom(mount))}>
                    Edit
                  </button>
                  <button type="button" className="button button--quiet" onClick={() => setPendingDelete(mount)}>
                    Remove
                  </button>
                </footer>

                {isOpen ? (
                  <LogPanel
                    lines={lines}
                    running={running}
                    onClear={() => clearLog(`mount:${mount.id}`)}
                    placeholder="rclone mount messages appear here while it runs."
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
          title={draft.id ? `Edit mount '${draft.name}'` : "New mount"}
          subtitle="The mount point must exist and be empty. It cannot overlap a job folder."
          onClose={() => setDraft(null)}
          footer={
            <>
              <button type="button" className="button" onClick={() => setDraft(null)}>
                Cancel
              </button>
              <button
                type="button"
                className="button button--primary"
                disabled={saving || draft.name.trim() === "" || draft.mountPoint === ""}
                onClick={() => void save()}
              >
                {saving ? "Saving…" : "Save mount"}
              </button>
            </>
          }
        >
          {formError ? <Banner tone="danger" title="Could not save">{formError}</Banner> : null}

          <div className="form-grid">
            <Field label="Mount name">
              <input
                className="input"
                value={draft.name}
                placeholder="Media drive"
                onChange={(event) => update({ name: event.target.value })}
              />
            </Field>

            <Field label="Bucket">
              <select
                className="input"
                value={draft.remote}
                onChange={(event) => update({ remote: event.target.value, remotePath: "" })}
              >
                {state.remotes.map((remote) => (
                  <option key={remote.id} value={remote.name}>
                    {remote.name}
                  </option>
                ))}
              </select>
            </Field>
          </div>

          <Field label="Bucket and folder" hint={selectedRemote ? `${draft.remote}:${draft.remotePath}` : undefined}>
            <div className="input-row">
              <input
                className="input mono"
                value={draft.remotePath}
                placeholder="bucket/optional/folder (empty = all buckets)"
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

          <Field label="Mount point" hint="An existing, empty local directory.">
            <div className="input-row">
              <input
                className="input mono"
                value={draft.mountPoint}
                placeholder="/home/you/mnt/media"
                onChange={(event) => update({ mountPoint: event.target.value })}
              />
              <button type="button" className="button" onClick={() => void pickMountPoint()}>
                Choose…
              </button>
            </div>
          </Field>

          <Field label="Cache mode">
            <select
              className="input"
              value={draft.vfsCacheMode}
              onChange={(event) => update({ vfsCacheMode: event.target.value as VfsCacheMode })}
            >
              {CACHE_MODES.map((mode) => (
                <option key={mode.value} value={mode.value}>
                  {mode.label} — {mode.blurb}
                </option>
              ))}
            </select>
          </Field>

          <div className="toggle-stack">
            <Toggle
              checked={draft.autoMount}
              onChange={(autoMount) => update({ autoMount })}
              label="Auto-mount when S3Dive starts"
              description="Combined with 'Start S3Dive at login', this survives a reboot."
            />
            <Toggle
              checked={draft.readOnly}
              onChange={(readOnly) => update({ readOnly })}
              label="Read-only"
              description="Blocks writes through the mounted folder."
            />
            {!isWindows ? (
              <Toggle
                checked={draft.allowOther}
                onChange={(allowOther) => update({ allowOther })}
                label="Allow other users"
                description="Passes --allow-other. Requires the FUSE allow_other option."
              />
            ) : null}
          </div>
        </Modal>
      ) : null}

      {browsing && selectedRemote ? (
        <Modal wide title="Choose a bucket folder" onClose={() => setBrowsing(false)} footer={null}>
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

      {pendingDelete ? (
        <ConfirmDialog
          title={`Remove mount '${pendingDelete.name}'?`}
          message="The mount definition is removed. If it is running, it is unmounted first."
          confirmLabel="Remove mount"
          onConfirm={() => void remove()}
          onCancel={() => setPendingDelete(null)}
        />
      ) : null}
    </section>
  );
}
