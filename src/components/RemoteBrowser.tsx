import { useCallback, useEffect, useState } from "react";
import { api, describeError } from "../api";
import type { RemoteEntry } from "../types";

/**
 * Walk the folder structure of a bucket so the user can pick the prefix a job or
 * mount should target, without typing paths by hand.
 */
export function RemoteBrowser({
  remoteId,
  remoteName,
  initialPath,
  onPick,
  onCancel,
}: {
  remoteId: string;
  remoteName: string;
  initialPath: string;
  onPick: (path: string) => void;
  onCancel: () => void;
}) {
  const [path, setPath] = useState(initialPath.replace(/^\/+|\/+$/g, ""));
  const [entries, setEntries] = useState<RemoteEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(
    async (target: string) => {
      setLoading(true);
      setError(null);
      try {
        const result = await api.browseRemote(remoteId, target);
        setEntries(result);
      } catch (caught) {
        setEntries([]);
        setError(describeError(caught));
      } finally {
        setLoading(false);
      }
    },
    [remoteId],
  );

  useEffect(() => {
    void load(path);
  }, [load, path]);

  const segments = path === "" ? [] : path.split("/");

  return (
    <div className="browser">
      <div className="browser__crumb">
        <button type="button" className="crumb" onClick={() => setPath("")}>
          {remoteName}:
        </button>
        {segments.map((segment, index) => {
          const target = segments.slice(0, index + 1).join("/");
          return (
            <span key={target} className="browser__crumb-part">
              <span className="browser__sep">/</span>
              <button type="button" className="crumb" onClick={() => setPath(target)}>
                {segment}
              </button>
            </span>
          );
        })}
      </div>

      <div className="browser__list">
        {path !== "" ? (
          <button
            type="button"
            className="browser__row browser__row--up"
            onClick={() => setPath(segments.slice(0, -1).join("/"))}
          >
            <span className="browser__icon">↑</span>
            <span>Parent folder</span>
          </button>
        ) : null}

        {loading ? <p className="browser__status">Listing…</p> : null}
        {error ? <p className="browser__status browser__status--error">{error}</p> : null}
        {!loading && !error && entries.length === 0 ? (
          <p className="browser__status">No subfolders here. You can still use this level.</p>
        ) : null}

        {entries.map((entry) => (
          <button
            key={entry.path}
            type="button"
            className="browser__row"
            onClick={() => setPath(entry.path)}
          >
            <span className="browser__icon">▸</span>
            <span className="mono">{entry.name}</span>
          </button>
        ))}
      </div>

      <footer className="browser__footer">
        <span className="browser__target mono">
          Selected: {path === "" ? `${remoteName}:` : `${remoteName}:${path}`}
        </span>
        <div className="browser__actions">
          <button type="button" className="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="button button--primary" onClick={() => onPick(path)}>
            Use this folder
          </button>
        </div>
      </footer>
    </div>
  );
}
