export function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) {
    return "0 B";
  }

  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }

  return `${size >= 10 || unit === 0 ? size.toFixed(0) : size.toFixed(1)} ${units[unit]}`;
}

export function formatPercent(ratio: number, digits = 0) {
  if (!Number.isFinite(ratio)) {
    return "0%";
  }
  return `${(Math.max(0, Math.min(1, ratio)) * 100).toFixed(digits)}%`;
}

export function formatCount(value: number) {
  return Number.isFinite(value) ? value.toLocaleString("en-US") : "0";
}

export function fileName(path: string) {
  const trimmed = path.replace(/[\\/]+$/, "");
  const parts = trimmed.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** Deterministic 2-letter avatar seed from an id/name. */
export function initials(value: string) {
  const cleaned = value.replace(/[^a-zA-Z0-9]/g, "");
  return (cleaned.slice(0, 2) || "??").toUpperCase();
}

/** Compact "time ago" from a Unix-seconds timestamp. */
export function timeAgo(unixSeconds: number) {
  if (!unixSeconds) {
    return "never";
  }
  const seconds = Math.max(0, Math.floor(Date.now() / 1000) - unixSeconds);
  if (seconds < 5) return "just now";
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

/** Local date-time from Unix seconds. */
export function formatTimestamp(unixSeconds: number) {
  if (!unixSeconds) {
    return "—";
  }
  return new Date(unixSeconds * 1000).toLocaleString();
}

/** Human duration from milliseconds. */
export function formatDuration(ms: number) {
  if (!ms || ms < 0) return "—";
  if (ms < 1000) return `${ms} ms`;
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 1 : 0)} s`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes}m ${Math.floor(seconds % 60)}s`;
}

