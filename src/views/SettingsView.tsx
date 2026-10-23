import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, describeError } from "../api";
import { useApp } from "../store";
import { Badge, Banner, Field, StatusDot } from "../components/ui";

export default function SettingsView() {
  const { state, rclone, platform, configDir, rcloneConf, stateFile, replaceState, replaceRclone, fuse, setNotice } =
    useApp();
  const [rclonePath, setRclonePath] = useState(state.settings.rclonePath ?? "");
  const [bandwidth, setBandwidth] = useState(state.settings.bandwidthLimit ?? "");
  const [flags, setFlags] = useState(state.settings.defaultFlags.join("\n"));
  const [saving, setSaving] = useState(false);
  const [checking, setChecking] = useState(false);

  const pickBinary = async () => {
    const picked = await open({
      multiple: false,
      title: "Select the rclone executable",
    });
    if (typeof picked === "string") setRclonePath(picked);
  };

  const recheck = async () => {
    setChecking(true);
    try {
      const info = await api.refreshRclone();
      replaceRclone(info);
      setNotice({
        kind: info.available ? "success" : "error",
        text: info.available
          ? `rclone ${info.version ?? ""} at ${info.program ?? ""}`
          : (info.message ?? "rclone is not available."),
      });
    } catch (error) {
      setNotice({ kind: "error", text: describeError(error) });
    } finally {
      setChecking(false);
    }
  };

  const save = async () => {
    setSaving(true);
    try {
      const next = await api.saveSettings({
        rclonePath: rclonePath.trim() === "" ? null : rclonePath.trim(),
        bandwidthLimit: bandwidth.trim() === "" ? null : bandwidth.trim(),
        defaultFlags: flags
          .split("\n")
          .map((flag) => flag.trim())
          .filter((flag) => flag !== ""),
      });
      replaceState(next);
      replaceRclone(await api.refreshRclone());
      setNotice({ kind: "success", text: "Settings saved." });
    } catch (error) {
      setNotice({ kind: "error", text: describeError(error) });
    } finally {
      setSaving(false);
    }
  };

  const reveal = async (path: string) => {
    try {
      await revealItemInDir(path);
    } catch (error) {
      setNotice({ kind: "error", text: describeError(error) });
    }
  };

  return (
    <section className="view">
      <header className="view__header">
        <div>
          <h1>Settings</h1>
          <p className="view__subtitle">The rclone binary S3Dive drives, and how it runs it.</p>
        </div>
      </header>

      <article className="card card--inline">
        <div className="card__title-row">
          <StatusDot state={rclone.available ? "ok" : "warn"} />
          <h2>rclone</h2>
          <Badge tone={rclone.available ? "success" : "danger"}>
            {rclone.available ? (rclone.version ?? "detected") : "not found"}
          </Badge>
        </div>
        <dl className="facts facts--wide">
          <div>
            <dt>Binary</dt>
            <dd className="mono">{rclone.program ?? "—"}</dd>
          </div>
          <div>
            <dt>Source</dt>
            <dd>{rclone.source === "configured" ? "Settings override" : rclone.source === "path" ? "PATH" : "missing"}</dd>
          </div>
        </dl>
        {rclone.message ? <p className="card__meta">{rclone.message}</p> : null}
        <div className="card__actions">
          <button type="button" className="button" disabled={checking} onClick={() => void recheck()}>
            {checking ? "Checking…" : "Re-check"}
          </button>
        </div>
      </article>

      <article className="card card--inline">
        <h2>rclone location</h2>
        <Field label="Binary path" hint="Leave empty to use the rclone found on PATH.">
          <div className="input-row">
            <input
              className="input mono"
              value={rclonePath}
              placeholder="/usr/local/bin/rclone"
              onChange={(event) => setRclonePath(event.target.value)}
            />
            <button type="button" className="button" onClick={() => void pickBinary()}>
              Choose…
            </button>
          </div>
        </Field>

        <div className="form-grid">
          <Field label="Bandwidth limit" hint="Passed as --bwlimit, for example 8M or 500k.">
            <input
              className="input mono"
              value={bandwidth}
              placeholder="unlimited"
              onChange={(event) => setBandwidth(event.target.value)}
            />
          </Field>
          <Field
            label="Extra flags"
            hint="One flag per line. A value may follow on the same line. Applied to every sync, copy, bisync and mount."
          >
            <textarea
              className="input input--area mono"
              rows={3}
              value={flags}
              placeholder="--transfers 8&#10;--checkers 16"
              onChange={(event) => setFlags(event.target.value)}
            />
          </Field>
        </div>

        <div className="card__actions">
          <button type="button" className="button button--primary" disabled={saving} onClick={() => void save()}>
            {saving ? "Saving…" : "Save settings"}
          </button>
        </div>
      </article>

      <article className="card card--inline">
        <h2>Files</h2>
        <p className="card__meta">
          S3Dive keeps its own rclone.conf so it never edits the config your CLI uses.
        </p>
        <dl className="facts facts--wide">
          <div>
            <dt>Config folder</dt>
            <dd className="mono">{configDir}</dd>
          </div>
          <div>
            <dt>rclone.conf</dt>
            <dd className="mono">{rcloneConf}</dd>
          </div>
          <div>
            <dt>state.json</dt>
            <dd className="mono">{stateFile}</dd>
          </div>
        </dl>
        <div className="card__actions">
          <button type="button" className="button" onClick={() => void reveal(configDir)}>
            Reveal config folder
          </button>
        </div>
      </article>

      <article className="card card--inline">
        <h2>Environment</h2>
        <dl className="facts facts--wide">
          <div>
            <dt>Platform</dt>
            <dd>{platform}</dd>
          </div>
          <div>
            <dt>Mount support</dt>
            <dd>
              <StatusDot state={fuse.available ? "ok" : "warn"} /> {fuse.summary}
            </dd>
          </div>
          <div>
            <dt>Schema</dt>
            <dd className="mono">v{state.schemaVersion}</dd>
          </div>
        </dl>
        {!fuse.available ? (
          <Banner tone="warning" title={fuse.summary}>
            <p>{fuse.detail}</p>
            {fuse.hint ? <p>{fuse.hint}</p> : null}
          </Banner>
        ) : null}
      </article>
    </section>
  );
}
