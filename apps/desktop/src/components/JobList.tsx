import { CheckCircle2, Loader2, XCircle } from "lucide-react";
import type { TransferJob } from "../api/desktop";

type JobListProps = {
  jobs: TransferJob[];
};

export function JobList({ jobs }: JobListProps) {
  if (jobs.length === 0) {
    return <div className="empty">No active transfer jobs.</div>;
  }

  return (
    <div className="jobList">
      {jobs.map((job) => (
        <article className="job" key={job.jobId}>
          <div className="job__head">
            {job.state === "running" ? (
              <Loader2 className="spin" size={18} />
            ) : job.state === "completed" ? (
              <CheckCircle2 size={18} />
            ) : (
              <XCircle size={18} />
            )}
            <strong>
              #{job.jobId} {job.kind}
            </strong>
            <span className={`pill pill--${job.state}`}>{job.state}</span>
          </div>
          {job.error ? <p className="job__error">{job.error}</p> : null}
          {job.output.length > 0 ? (
            <pre className="job__output">{job.output.slice(-8).join("\n")}</pre>
          ) : null}
        </article>
      ))}
    </div>
  );
}
