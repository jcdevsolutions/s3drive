export function relativeTime(timestampMs: number | null): string {
  if (!timestampMs) return "never";
  const delta = Date.now() - timestampMs;
  const minute = 60_000;
  const hour = 60 * minute;
  const day = 24 * hour;

  if (delta < minute) return "just now";
  if (delta < hour) return `${Math.floor(delta / minute)}m ago`;
  if (delta < day) return `${Math.floor(delta / hour)}h ago`;
  if (delta < 30 * day) return `${Math.floor(delta / day)}d ago`;
  return new Date(timestampMs).toLocaleDateString();
}

export function duration(startMs: number, endMs: number | null): string {
  const end = endMs ?? Date.now();
  const seconds = Math.max(0, Math.round((end - startMs) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  if (minutes < 60) return `${minutes}m ${rest}s`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

export function joinRemotePath(remote: string, path: string): string {
  const trimmed = path.replace(/^\/+|\/+$/g, "");
  return trimmed === "" ? `${remote}:` : `${remote}:${trimmed}`;
}
