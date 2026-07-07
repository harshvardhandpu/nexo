import { useEffect, useState } from "react";
import {
  ArrowDownToLine,
  Boxes,
  Gauge,
  Radio,
  Send,
  Signal,
} from "lucide-react";
import type { DesktopData } from "../lib/useDesktopData";
import type { Screen } from "./nav";
import {
  type ReceiverStatus,
  getReceiverStatus,
} from "../api/desktop";
import { JobCard } from "../components/JobCard";
import { NodeNetwork } from "../components/NodeNetwork";
import {
  Empty,
  GlassPanel,
  LiquidProgress,
  NeonButton,
  PanelHead,
  StatCard,
  StatusPill,
} from "../components/ui";
import { formatBytes, formatCount, formatPercent } from "../utils";

function ReceiverStatusPanel() {
  const [status, setStatus] = useState<ReceiverStatus | null>(null);

  useEffect(() => {
    const load = () => void getReceiverStatus().then(setStatus).catch(() => {});
    load();
    const timer = window.setInterval(load, 2000);
    return () => window.clearInterval(timer);
  }, []);

  const available = status?.receiving && status?.discoverable;

  return (
    <GlassPanel>
      <div className="row row--between row--wrap">
        <div className="row" style={{ gap: 12 }}>
          <span
            className={`presence-dot ${available ? "presence-dot--online" : ""}`}
            style={{ position: "relative", inset: 0, width: 14, height: 14 }}
          />
          <div>
            <strong style={{ fontSize: 15 }}>
              Receiver status ·{" "}
              <span className={available ? "gradient-text" : "text-muted"}>
                {available ? "Available" : status?.receiving ? "Starting…" : "Offline"}
              </span>
            </strong>
            <div className="text-faint" style={{ fontSize: 12.5 }}>
              {available
                ? "Device discoverable — nearby peers can send to you"
                : status?.backgroundEnabled
                  ? "Background mode on — starting receiver…"
                  : "Background mode off — open Receive to accept transfers"}
            </div>
          </div>
        </div>
        <StatusPill variant={available ? "live" : "idle"}>
          <Radio size={13} /> {status?.endpoint ?? "not advertised"}
        </StatusPill>
      </div>
    </GlassPanel>
  );
}

export function Dashboard({
  data,
  onNavigate,
}: {
  data: DesktopData;
  onNavigate: (screen: Screen) => void;
}) {
  const latest = data.status?.latest ?? null;
  const activeJobs = data.jobs.filter((job) => job.state === "running");
  const receiverReady = Boolean(data.receiver);
  const latestRatio =
    latest && latest.totalBytes > 0
      ? latest.completedBytes / latest.totalBytes
      : latest && latest.totalChunks > 0
        ? latest.completedChunks / latest.totalChunks
        : 0;

  return (
    <div className="page" key="dashboard">
      <ReceiverStatusPanel />
      <div className="hero">
        <GlassPanel strong className="hero__panel">
          <StatusPill variant={receiverReady ? "ok" : "idle"}>
            {receiverReady ? "Receiver ready" : "Idle"}
          </StatusPill>
          <h2>
            Move large files at <span className="gradient-text">light speed</span>,
            crash-safe by design.
          </h2>
          <p>
            Encrypted QUIC transport, chunk-level integrity, and resume that
            survives interruptions — now with a premium desktop cockpit.
          </p>
          <div className="row row--wrap">
            <NeonButton icon={Send} onClick={() => onNavigate("send")}>
              Send a file
            </NeonButton>
            <NeonButton
              variant="ghost"
              icon={Radio}
              onClick={() => onNavigate("receive")}
            >
              Receive
            </NeonButton>
          </div>
        </GlassPanel>

        <GlassPanel className="hero__viz">
          <NodeNetwork live={receiverReady || activeJobs.length > 0} />
        </GlassPanel>
      </div>

      <div className="grid grid--3">
        <StatCard
          label="Active transfers"
          value={formatCount(activeJobs.length)}
          sub={activeJobs.length ? "in progress" : "nothing running"}
          icon={Signal}
        />
        <StatCard
          label="Latest state"
          value={latest?.state ?? "—"}
          sub={latest?.fileName ?? "no transfers yet"}
          icon={Gauge}
        />
        <StatCard
          label="Data moved (latest)"
          value={formatBytes(latest?.completedBytes ?? 0)}
          sub={
            latest ? `of ${formatBytes(latest.totalBytes)}` : "waiting for data"
          }
          icon={Boxes}
        />
      </div>

      {latest ? (
        <GlassPanel>
          <PanelHead
            icon={Gauge}
            title="Latest transfer"
            action={
              <StatusPill
                variant={
                  latest.state === "Completed"
                    ? "ok"
                    : latest.state === "Failed"
                      ? "danger"
                      : "live"
                }
              >
                {latest.state ?? "unknown"}
              </StatusPill>
            }
          />
          <div className="stack">
            <LiquidProgress
              ratio={latestRatio}
              label={formatPercent(latestRatio)}
              tall
            />
            <div className="row row--between text-muted" style={{ fontSize: 13 }}>
              <span className="mono">{latest.fileName ?? latest.transferId}</span>
              <span className="mono">
                {formatCount(latest.completedChunks)} /{" "}
                {formatCount(latest.totalChunks)} chunks
              </span>
            </div>
          </div>
        </GlassPanel>
      ) : null}

      <GlassPanel>
        <PanelHead icon={ArrowDownToLine} title="Active jobs" />
        {activeJobs.length === 0 ? (
          <Empty icon={Signal}>No transfers are running right now.</Empty>
        ) : (
          <div className="stack">
            {activeJobs.map((job) => (
              <JobCard key={job.jobId} job={job} showChunks={false} />
            ))}
          </div>
        )}
      </GlassPanel>
    </div>
  );
}
