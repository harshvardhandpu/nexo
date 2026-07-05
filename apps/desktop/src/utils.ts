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

