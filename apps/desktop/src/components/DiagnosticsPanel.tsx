import { useEffect, useState } from "react";
import { Activity, RefreshCw, Terminal } from "lucide-react";
import { type Diagnostics, getDiagnostics } from "../api/desktop";
import { GlassPanel, NeonButton, PanelHead, StatusPill } from "./ui";
import { formatBytes } from "../utils";

function Line({ k, v, mono = true }: { k: string; v: string; mono?: boolean }) {
  return (
    <div className="kv">
      <span className="kv__k">{k}</span>
      <span className={`kv__v ${mono ? "mono" : ""}`}>{v || "—"}</span>
    </div>
  );
}

/**
 * Task 5: hidden developer diagnostics (Settings → Advanced). Read-only view of
 * device identity, mDNS/receiver status, storage locations, and transfer stats.
 */
export function DiagnosticsPanel() {
  const [diag, setDiag] = useState<Diagnostics | null>(null);
  const [open, setOpen] = useState(false);

  const load = () => void getDiagnostics().then(setDiag).catch(() => {});
  useEffect(() => {
    if (!open) return;
    load();
    const timer = window.setInterval(load, 3000);
    return () => window.clearInterval(timer);
  }, [open]);

  return (
    <GlassPanel>
      <PanelHead
        icon={Terminal}
        title="Advanced · Diagnostics"
        action={
          <NeonButton
            variant="ghost"
            icon={open ? RefreshCw : Activity}
            onClick={() => (open ? load() : setOpen(true))}
          >
            {open ? "Refresh" : "Show diagnostics"}
          </NeonButton>
        }
      />
      {!open ? (
        <p className="text-faint" style={{ margin: 0, fontSize: 13 }}>
          Developer view: device ID, certificate fingerprint, mDNS + receiver
          status, storage locations, and transfer statistics.
        </p>
      ) : !diag ? (
        <p className="text-faint" style={{ margin: 0 }}>
          Loading diagnostics…
        </p>
      ) : (
        <div className="stack--sm">
          <div className="row row--wrap" style={{ gap: 8, marginBottom: 6 }}>
            <StatusPill variant={diag.receiverRunning ? "live" : "idle"}>
              Receiver {diag.receiverRunning ? "running" : "stopped"}
            </StatusPill>
            <StatusPill variant={diag.mdnsDiscoverable ? "ok" : "idle"}>
              mDNS {diag.mdnsDiscoverable ? "advertising" : "off"}
            </StatusPill>
          </div>
          <Line k="Device ID" v={diag.deviceId} />
          <Line k="Certificate fingerprint" v={diag.certificateFingerprint} />
          <Line k="Endpoint" v={diag.endpoint ?? "not advertised"} />
          <Line k="Storage location" v={diag.stateDir} />
          <Line k="Download folder" v={diag.downloadDir} />
          <Line k="Last transfer" v={diag.lastTransfer ?? "none"} mono={false} />
          <Line
            k="Transfers"
            v={`${diag.completedTransfers} ok / ${diag.failedTransfers} failed / ${diag.totalTransfers} total`}
            mono={false}
          />
          <Line
            k="Data moved"
            v={`↑ ${formatBytes(diag.bytesSent)} · ↓ ${formatBytes(diag.bytesReceived)}`}
            mono={false}
          />
        </div>
      )}
    </GlassPanel>
  );
}
