/**
 * Parses the human-readable progress lines the core CLI emits (captured by the
 * Rust bridge into each job's `output`) into structured transfer state. The UI
 * never reaches into the engine; it only reads what the existing core already
 * prints, e.g. "sent: 12/1280 chunks, 50331648/5368709120 bytes".
 */

export type TransferProgress = {
  label: string;
  completedChunks: number;
  totalChunks: number;
  completedBytes: number;
  totalBytes: number;
  ratio: number;
};

const PROGRESS_LINE =
  /^(\w+):\s+(\d+)\/(\d+)\s+chunks,\s+(\d+)\/(\d+)\s+bytes$/;

export function parseProgressLine(line: string): TransferProgress | null {
  const match = PROGRESS_LINE.exec(line.trim());
  if (!match) {
    return null;
  }

  const completedChunks = Number(match[2]);
  const totalChunks = Number(match[3]);
  const completedBytes = Number(match[4]);
  const totalBytes = Number(match[5]);
  const ratio =
    totalBytes > 0
      ? completedBytes / totalBytes
      : totalChunks > 0
        ? completedChunks / totalChunks
        : 0;

  return {
    label: match[1],
    completedChunks,
    totalChunks,
    completedBytes,
    totalBytes,
    ratio: Math.max(0, Math.min(1, ratio)),
  };
}

/** Most recent parseable progress line in a job's output, if any. */
export function latestProgress(lines: string[]): TransferProgress | null {
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const parsed = parseProgressLine(lines[index]);
    if (parsed) {
      return parsed;
    }
  }
  return null;
}

/** The address a receive job advertised, from its "receiving on <addr>" line. */
export function receivingAddress(lines: string[]): string | null {
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const match = /^receiving on (.+)$/.exec(lines[index].trim());
    if (match) {
      return match[1];
    }
  }
  return null;
}
