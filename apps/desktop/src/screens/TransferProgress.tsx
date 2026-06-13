import { RotateCcw } from "lucide-react";
import type { TransferJob, TransferStatusSnapshot } from "../api/desktop";
import { JobList } from "../components/JobList";
import { ProgressBar } from "../components/ProgressBar";
import { formatBytes } from "../utils";

type TransferProgressProps = {
  status: TransferStatusSnapshot | null;
  jobs: TransferJob[];
  onClearJobs: () => void;
};

export function TransferProgress({
  status,
  jobs,
  onClearJobs,
}: TransferProgressProps) {
  const latest = status?.latest ?? null;

  return (
    <section className="screen">
      <div className="screenHeader">
        <div>
          <h1>Transfer Progress</h1>
        </div>
        <button onClick={onClearJobs} title="Clear completed jobs">
          <RotateCcw size={18} />
          Clear
        </button>
      </div>

      <div className="panel">
        <div className="transferSummary">
          <strong>{latest?.fileName ?? "No transfer"}</strong>
          <span>{latest?.state ?? "Idle"}</span>
        </div>
        <ProgressBar
          value={latest?.completedBytes ?? 0}
          max={latest?.totalBytes ?? 0}
        />
        <div className="splitDetail">
          <span>
            {latest
              ? `${latest.completedChunks}/${latest.totalChunks} chunks`
              : "0/0 chunks"}
          </span>
          <span>
            {latest
              ? `${formatBytes(latest.completedBytes)} / ${formatBytes(latest.totalBytes)}`
              : "0 B / 0 B"}
          </span>
        </div>
      </div>

      <JobList jobs={jobs} />
    </section>
  );
}
