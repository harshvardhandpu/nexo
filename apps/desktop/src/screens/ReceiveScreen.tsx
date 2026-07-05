import { useState } from "react";
import {
  ArrowDownToLine,
  RefreshCw,
  Radio,
  Users,
  Wifi,
} from "lucide-react";
import type { DesktopData } from "../lib/useDesktopData";
import { JobCard } from "../components/JobCard";
import {
  Banner,
  Empty,
  GlassPanel,
  NeonButton,
  PanelHead,
  StatusPill,
} from "../components/ui";
import { initials } from "../utils";

export function ReceiveScreen({ data }: { data: DesktopData }) {
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const receiveJobs = data.jobs
    .filter((job) => job.kind === "receive")
    .sort((a, b) => b.jobId - a.jobId);
  const listening = receiveJobs.some((job) => job.state === "running");

  const startReceiving = async () => {
    setError(null);
    setStarting(true);
    try {
      await data.receive();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setStarting(false);
    }
  };

  return (
    <div className="page" key="receive">
      <GlassPanel strong>
        <PanelHead
          icon={Radio}
          title="Receive mode"
          action={
            <StatusPill variant={listening ? "live" : data.receiver ? "ok" : "idle"}>
              {listening ? "Listening" : data.receiver ? "Advertised" : "Offline"}
            </StatusPill>
          }
        />
        <div className="stack">
          <p className="text-muted" style={{ margin: 0 }}>
            Start receive mode to accept one incoming transfer. Nexo advertises a
            stable address and certificate so an interrupted sender can reconnect
            and resume.
          </p>
          <div className="file-chip">
            <Wifi size={15} className="icon" />
            <span className="text-faint">Advertised endpoint</span>
            <span className="file-chip__name" style={{ marginLeft: "auto" }}>
              {data.receiver?.address ?? "not advertised yet"}
            </span>
          </div>
          {error ? <Banner variant="error">{error}</Banner> : null}
          <NeonButton
            icon={ArrowDownToLine}
            onClick={startReceiving}
            loading={starting}
          >
            Start receiving
          </NeonButton>
        </div>
      </GlassPanel>

      <GlassPanel>
        <PanelHead
          icon={Users}
          title="Peer discovery"
          action={
            <NeonButton
              variant="ghost"
              icon={RefreshCw}
              onClick={data.refreshPeers}
              loading={data.peersLoading}
            >
              Scan LAN
            </NeonButton>
          }
        />
        {data.peersError ? <Banner variant="error">{data.peersError}</Banner> : null}
        {data.peers.length === 0 ? (
          <Empty icon={Users}>
            {data.peersLoading
              ? "Scanning the local network…"
              : "No peers discovered yet. Run a scan to find devices."}
          </Empty>
        ) : (
          <div className="peer-list">
            {data.peers.map((peer) => (
              <div className="peer" key={peer.peerId}>
                <div className="peer__avatar">{initials(peer.displayName)}</div>
                <div className="peer__meta">
                  <strong>{peer.displayName}</strong>
                  <span>
                    {peer.addresses.join(", ") || peer.peerId}
                    {peer.port ? `:${peer.port}` : ""}
                  </span>
                </div>
                <StatusPill variant="ok">online</StatusPill>
              </div>
            ))}
          </div>
        )}
      </GlassPanel>

      <GlassPanel>
        <PanelHead icon={ArrowDownToLine} title="Receive activity" />
        {receiveJobs.length === 0 ? (
          <Empty icon={Radio}>Incoming transfers will appear here.</Empty>
        ) : (
          <div className="stack">
            {receiveJobs.map((job) => (
              <JobCard key={job.jobId} job={job} showLog />
            ))}
          </div>
        )}
      </GlassPanel>
    </div>
  );
}
