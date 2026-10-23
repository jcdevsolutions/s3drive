import { useState } from "react";
import { api, describeError } from "../api";
import { useApp } from "../store";
import type { Remote, RemoteTest, S3Provider } from "../types";
import { Badge, Banner, ConfirmDialog, EmptyState, Field, Modal, Toggle } from "../components/ui";

interface Draft {
  id: string;
  createdAtMs: number;
  name: string;
  provider: S3Provider;
  endpoint: string;
  region: string;
  accessKeyId: string;
  secret: string;
  forcePathStyle: boolean;
}

function newDraft(): Draft {
  return {
    id: "",
    createdAtMs: 0,
    name: "",
    provider: "aws",
    endpoint: "",
    region: "us-east-1",
    accessKeyId: "",
    secret: "",
    forcePathStyle: false,
  };
}

function draftFrom(remote: Remote): Draft {
  return {
    id: remote.id,
    createdAtMs: remote.createdAtMs,
    name: remote.name,
    provider: remote.provider,
    endpoint: remote.endpoint ?? "",
    region: remote.region,
    accessKeyId: remote.accessKeyId,
    secret: "",
    forcePathStyle: remote.forcePathStyle,
  };
}

export default function BucketsView() {
  const { state, replaceState, setNotice } = useApp();
  const [draft, setDraft] = useState<Draft | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState<string | null>(null);
  const [results, setResults] = useState<Record<string, RemoteTest>>({});
  const [pendingDelete, setPendingDelete] = useState<Remote | null>(null);

  const update = (patch: Partial<Draft>) =>
    setDraft((current) => (current ? { ...current, ...patch } : current));

  const save = async () => {
    if (!draft) return;
    setSaving(true);
    setFormError(null);
    try {
      const remote: Remote = {
        id: draft.id,
        createdAtMs: draft.createdAtMs,
        name: draft.name,
        provider: draft.provider,
        endpoint: draft.endpoint.trim() === "" ? null : draft.endpoint.trim(),
        region: draft.region,
        accessKeyId: draft.accessKeyId,
        forcePathStyle: draft.forcePathStyle,
      };
      const next = await api.saveRemote(remote, draft.secret.trim() === "" ? null : draft.secret);
      replaceState(next);
      setDraft(null);
      setNotice({ kind: "success", text: `Saved bucket '${remote.name}'.` });
    } catch (error) {
      setFormError(describeError(error));
    } finally {
      setSaving(false);
    }
  };

  const test = async (remote: Remote) => {
    setTesting(remote.id);
    try {
      const result = await api.testRemote(remote.id);
      setResults((current) => ({ ...current, [remote.id]: result }));
      setNotice({
        kind: result.ok ? "success" : "error",
        text: `${remote.name}: ${result.message}`,
      });
    } catch (error) {
      setNotice({ kind: "error", text: describeError(error) });
    } finally {
      setTesting(null);
    }
  };

  const remove = async () => {
    if (!pendingDelete) return;
    try {
      const next = await api.deleteRemote(pendingDelete.id);
      replaceState(next);
      setNotice({ kind: "success", text: `Removed '${pendingDelete.name}'.` });
    } catch (error) {
      setNotice({ kind: "error", text: describeError(error) });
    } finally {
      setPendingDelete(null);
    }
  };

  const isCompatible = draft?.provider === "other";

  return (
    <section className="view">
      <header className="view__header">
        <div>
          <h1>Buckets</h1>
          <p className="view__subtitle">
            S3 remotes written to S3Dive's own rclone.conf. Your global rclone config is
            never touched.
          </p>
        </div>
        <button type="button" className="button button--primary" onClick={() => setDraft(newDraft())}>
          Add bucket
        </button>
      </header>

      {state.remotes.length === 0 ? (
        <EmptyState
          title="No buckets yet"
          message="Add an S3 bucket with its access key. Then you can point jobs and mounts at it."
          action={
            <button type="button" className="button button--primary" onClick={() => setDraft(newDraft())}>
              Add your first bucket
            </button>
          }
        />
      ) : (
        <div className="grid">
          {state.remotes.map((remote) => {
            const result = results[remote.id];
            return (
              <article className="card" key={remote.id}>
                <header className="card__header">
                  <div>
                    <h2>{remote.name}</h2>
                    <p className="card__meta">
                      {remote.endpoint
                        ? remote.endpoint
                        : remote.provider === "aws"
                          ? `AWS · ${remote.region}`
                          : `S3-compatible · ${remote.region}`}
                    </p>
                  </div>
                  <Badge tone={remote.provider === "aws" ? "accent" : "neutral"}>
                    {remote.provider === "aws" ? "AWS" : "S3-compatible"}
                  </Badge>
                </header>

                <dl className="facts">
                  <div>
                    <dt>Access key</dt>
                    <dd className="mono">{maskKey(remote.accessKeyId)}</dd>
                  </div>
                  <div>
                    <dt>Region</dt>
                    <dd className="mono">{remote.region}</dd>
                  </div>
                  <div>
                    <dt>Path style</dt>
                    <dd>{remote.forcePathStyle ? "Path-style" : "Virtual-hosted"}</dd>
                  </div>
                </dl>

                {result ? (
                  <div className={`test-result test-result--${result.ok ? "ok" : "bad"}`}>
                    <strong>{result.ok ? "Connection OK" : "Connection failed"}</strong>
                    <p>{result.message}</p>
                    {result.entries.length > 0 ? (
                      <p className="mono test-result__entries">
                        {result.entries.slice(0, 6).join(" · ")}
                        {result.entries.length > 6 ? " …" : ""}
                      </p>
                    ) : null}
                  </div>
                ) : null}

                <footer className="card__actions">
                  <button
                    type="button"
                    className="button"
                    disabled={testing === remote.id}
                    onClick={() => void test(remote)}
                  >
                    {testing === remote.id ? "Testing…" : "Test connection"}
                  </button>
                  <button type="button" className="button" onClick={() => setDraft(draftFrom(remote))}>
                    Edit
                  </button>
                  <button type="button" className="button button--quiet" onClick={() => setPendingDelete(remote)}>
                    Remove
                  </button>
                </footer>
              </article>
            );
          })}
        </div>
      )}

      {draft ? (
        <Modal
          title={draft.id ? `Edit bucket '${draft.name}'` : "Add an S3 bucket"}
          subtitle="Secrets stay in S3Dive's own rclone.conf (mode 0600), never in your global config."
          onClose={() => setDraft(null)}
          footer={
            <>
              <button type="button" className="button" onClick={() => setDraft(null)}>
                Cancel
              </button>
              <button
                type="button"
                className="button button--primary"
                disabled={saving || draft.name.trim() === "" || draft.accessKeyId.trim() === ""}
                onClick={() => void save()}
              >
                {saving ? "Saving…" : "Save bucket"}
              </button>
            </>
          }
        >
          {formError ? <Banner tone="danger" title="Could not save">{formError}</Banner> : null}

          <div className="form-grid">
            <Field label="Name" hint="Used as the rclone remote name: letters, digits, - _ . +">
              <input
                className="input"
                value={draft.name}
                placeholder="media"
                onChange={(event) => update({ name: event.target.value })}
              />
            </Field>

            <Field label="Provider">
              <select
                className="input"
                value={draft.provider}
                onChange={(event) => {
                  const provider = event.target.value as S3Provider;
                  update({
                    provider,
                    forcePathStyle: provider === "other" ? true : draft.forcePathStyle,
                  });
                }}
              >
                <option value="aws">Amazon S3</option>
                <option value="other">S3-compatible (MinIO, R2, Wasabi…)</option>
              </select>
            </Field>

            {isCompatible ? (
              <Field label="Endpoint" hint="For example https://minio.local:9000">
                <input
                  className="input"
                  value={draft.endpoint}
                  placeholder="https://s3.example.com"
                  onChange={(event) => update({ endpoint: event.target.value })}
                />
              </Field>
            ) : null}

            <Field label="Region">
              <input
                className="input"
                value={draft.region}
                placeholder="us-east-1"
                onChange={(event) => update({ region: event.target.value })}
              />
            </Field>

            <Field label="Access key ID">
              <input
                className="input mono"
                value={draft.accessKeyId}
                placeholder="AKIA…"
                autoComplete="off"
                spellCheck={false}
                onChange={(event) => update({ accessKeyId: event.target.value })}
              />
            </Field>

            <Field
              label="Secret access key"
              hint={draft.id ? "Leave empty to keep the stored secret." : "Written to S3Dive's own rclone.conf."}
            >
              <input
                className="input mono"
                type="password"
                value={draft.secret}
                placeholder={draft.id ? "unchanged" : "your secret key"}
                autoComplete="new-password"
                spellCheck={false}
                onChange={(event) => update({ secret: event.target.value })}
              />
            </Field>
          </div>

          {isCompatible ? (
            <Toggle
              checked={draft.forcePathStyle}
              onChange={(forcePathStyle) => update({ forcePathStyle })}
              label="Force path-style addressing"
              description="Required by most self-hosted endpoints, such as MinIO."
            />
          ) : null}
        </Modal>
      ) : null}

      {pendingDelete ? (
        <ConfirmDialog
          title={`Remove '${pendingDelete.name}'?`}
          message="The remote is deleted and its section is removed from rclone.conf. Jobs and mounts using it must be removed first."
          confirmLabel="Remove bucket"
          onConfirm={() => void remove()}
          onCancel={() => setPendingDelete(null)}
        />
      ) : null}
    </section>
  );
}

function maskKey(key: string): string {
  if (key.length <= 8) return key;
  return `${key.slice(0, 4)}…${key.slice(-4)}`;
}
