import { useEffect, useRef, useState } from "react";
import { Activity, Gauge, Trash2 } from "lucide-react";
import type { TransferJob } from "../api/desktop";
import type { DesktopData } from "../lib/useDesktopData";
import { latestProgress } from "../lib/progress";
import { JobCard } from "../components/JobCard";
import {
  Empty,
  GlassPanel,
  NeonButton,
  PanelHead,
  StatCard,
} from "../components/ui";
import { formatBytes } from "../utils";

/** Instantaneous per-job throughput from successive poll snapshots. */
function useThroughput(jobs: TransferJob[]): Map<number, number> {
  const previous = useRef<Map<number, { bytes: number; time: number }>>(
    new Map(),
  );
  const [rates, setRates] = useState<Map<number, number>>(new Map());

  useEffect(() => {
    const now = performance.now();
    const nextRates = new Map<number, number>();
    const nextPrev = new Map<number, { bytes: number; time: number }>();

    for (const job of jobs) {
      const bytes = latestProgress(job.output)?.completedBytes ?? 0;
      nextPrev.set(job.jobId, { bytes, time: now });
      const before = previous.current.get(job.jobId);
      if (before && now > before.time && job.state === "running") {
        const seconds = (now - before.time) / 1000;
        const delta = bytes - before.bytes;
        if (seconds > 0 && delta > 0) {
          nextRates.set(job.jobId, delta / seconds);
        }
      }
    }

    previous.current = nextPrev;
    setRates(nextRates);
  }, [jobs]);

  return rates;
}

export function MonitorScreen({ data }: { data: DesktopData }) {
  const rates = useThroughput(data.jobs);
  const jobs = [...data.jobs].sort((a, b) => b.jobId - a.jobId);
  const running = jobs.filter((job) => job.state === "running");
  const completed = jobs.filter((job) => job.state === "completed").length;
  const failed = jobs.filter((job) => job.state === "failed").length;
  const totalRate = Array.from(rates.values()).reduce((sum, r) => sum + r, 0);

  return (
    <div className="page" key="monitor">
      <div className="grid grid--3">
        <StatCard label="Running" value={running.length} icon={Activity} />
        <StatCard
          label="Throughput"
          value={`${formatBytes(totalRate)}/s`}
          icon={Gauge}
        />
        <StatCard
          label="Done / failed"
          value={`${completed} / ${failed}`}
          icon={Gauge}
        />
      </div>

      <GlassPanel>
        <PanelHead
          icon={Activity}
          title="Live transfers"
          action={
            <NeonButton
              variant="ghost"
              icon={Trash2}
              onClick={data.clearJobs}
              disabled={jobs.every((job) => job.state === "running")}
            >
              Clear finished
            </NeonButton>
          }
        />
        {jobs.length === 0 ? (
          <Empty icon={Activity}>
            No transfers yet. Start a send or receive to watch chunks fill in real
            time.
          </Empty>
        ) : (
          <div className="stack">
            {jobs.map((job) => {
              const rate = rates.get(job.jobId);
              return (
                <div className="stack--sm" key={job.jobId}>
                  <JobCard job={job} showChunks showLog={false} />
                  {rate ? (
                    <span
                      className="text-faint mono"
                      style={{ fontSize: 12, textAlign: "right" }}
                    >
                      {formatBytes(rate)}/s
                    </span>
                  ) : null}
                </div>
              );
            })}
          </div>
        )}
      </GlassPanel>
    </div>
  );
}
