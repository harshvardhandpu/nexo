import {
  ArrowDownToLine,
  CheckCircle2,
  Send,
  XCircle,
} from "lucide-react";
import type { TransferJob } from "../api/desktop";
import { latestProgress } from "../lib/progress";
import { formatBytes, formatCount } from "../utils";
import { ChunkGrid, LiquidProgress, StatusPill } from "./ui";

function stateVariant(state: TransferJob["state"]) {
  if (state === "completed") return "ok" as const;
  if (state === "failed") return "danger" as const;
  return "live" as const;
}

/** One transfer job rendered with live liquid progress + chunk grid + log. */
export function JobCard({
  job,
  showChunks = true,
  showLog = false,
}: {
  job: TransferJob;
  showChunks?: boolean;
  showLog?: boolean;
}) {
  const progress = latestProgress(job.output);
  const isSend = job.kind === "send";
  const KindIcon = isSend ? Send : ArrowDownToLine;
  const stateClass =
    job.state === "completed"
      ? "is-success"
      : job.state === "failed"
        ? "is-error"
        : "";

  const ratio = progress?.ratio ?? (job.state === "completed" ? 1 : 0);

  return (
    <article className={`job ${stateClass}`}>
      <div className="job__head">
        <div className="job__title">
          <KindIcon size={17} className="icon" />
          <span>{isSend ? "Sending" : "Receiving"}</span>
          <span className="text-faint mono">#{job.jobId}</span>
        </div>
        <StatusPill variant={stateVariant(job.state)}>
          {job.state === "completed" ? (
            <>
              <CheckCircle2 size={13} /> Complete
            </>
          ) : job.state === "failed" ? (
            <>
              <XCircle size={13} /> Failed
            </>
          ) : (
            "Live"
          )}
        </StatusPill>
      </div>

      <LiquidProgress ratio={ratio} label="" />

      {progress ? (
        <div className="row row--between text-muted" style={{ fontSize: 12.5 }}>
          <span className="mono">
            {formatCount(progress.completedChunks)} /{" "}
            {formatCount(progress.totalChunks)} chunks
          </span>
          <span className="mono">
            {formatBytes(progress.completedBytes)} /{" "}
            {formatBytes(progress.totalBytes)}
          </span>
        </div>
      ) : null}

      {showChunks && progress && progress.totalChunks > 0 ? (
        <ChunkGrid
          total={progress.totalChunks}
          completed={progress.completedChunks}
          active={job.state === "running"}
        />
      ) : null}

      {job.error ? <div className="banner banner--error">{job.error}</div> : null}

      {showLog && job.output.length > 0 ? (
        <div className="log">
          {job.output.slice(-6).map((line, index, all) => (
            <div
              key={index}
              className={`log__line ${index === all.length - 1 ? "log__line--last" : ""}`}
            >
              {line}
            </div>
          ))}
        </div>
      ) : null}
    </article>
  );
}
