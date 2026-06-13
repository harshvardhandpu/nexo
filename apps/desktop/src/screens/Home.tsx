import { HardDrive, Radio, Send, ShieldCheck } from "lucide-react";
import type { DesktopSettings, TransferStatusSnapshot } from "../api/desktop";
import { Metric } from "../components/Metric";
import { ProgressBar } from "../components/ProgressBar";
import { formatBytes } from "../utils";

type HomeProps = {
  settings: DesktopSettings | null;
  status: TransferStatusSnapshot | null;
  onNavigate: (screen: string) => void;
};

export function Home({ settings, status, onNavigate }: HomeProps) {
  const latest = status?.latest ?? null;

  return (
    <section className="screen">
      <div className="screenHeader">
        <div>
          <h1>Nexo</h1>
        </div>
        <div className="quickActions">
          <button onClick={() => onNavigate("send")} title="Send File">
            <Send size={18} />
            Send
          </button>
          <button onClick={() => onNavigate("receive")} title="Receive Files">
            <Radio size={18} />
            Receive
          </button>
        </div>
      </div>

      <div className="metricsGrid">
        <Metric
          label="Latest state"
          value={latest?.state ?? "Idle"}
          detail={latest?.fileName ?? "No transfer selected"}
        />
        <Metric
          label="Completed"
          value={
            latest
              ? `${latest.completedChunks}/${latest.totalChunks} chunks`
              : "0/0 chunks"
          }
          detail={
            latest
              ? `${formatBytes(latest.completedBytes)} / ${formatBytes(latest.totalBytes)}`
              : "No bytes transferred"
          }
        />
        <Metric
          label="Chunk size"
          value={settings ? formatBytes(settings.chunkSize) : "Loading"}
          detail="Engine default"
        />
        <Metric
          label="Storage"
          value="SQLite"
          detail={settings?.stateDir ?? "Loading state path"}
        />
      </div>

      <div className="panel">
        <div className="panelHeader">
          <ShieldCheck size={19} />
          <h2>Transfer Progress</h2>
        </div>
        <ProgressBar
          value={latest?.completedBytes ?? 0}
          max={latest?.totalBytes ?? 0}
        />
        <div className="splitDetail">
          <span>{latest?.transferId ?? "No transfer recorded"}</span>
          <span>{latest?.sessionId ?? ""}</span>
        </div>
      </div>

      <div className="panel">
        <div className="panelHeader">
          <HardDrive size={19} />
          <h2>Receive Directory</h2>
        </div>
        <code className="pathLine">{settings?.receiveDir ?? "Loading"}</code>
      </div>
    </section>
  );
}
