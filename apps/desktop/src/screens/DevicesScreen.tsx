import { useCallback, useEffect, useState } from "react";
import {
  MonitorSmartphone,
  RefreshCw,
  ShieldCheck,
  ShieldPlus,
} from "lucide-react";
import {
  type PeerDevice,
  listDevices,
  trustDevice,
} from "../api/desktop";
import {
  Banner,
  Empty,
  GlassPanel,
  NeonButton,
  PanelHead,
  StatusPill,
} from "../components/ui";
import { initials, timeAgo } from "../utils";

const SCAN_MS = 5000;

function DeviceCard({
  device,
  onTrust,
  busy,
}: {
  device: PeerDevice;
  onTrust: (device: PeerDevice) => void;
  busy: boolean;
}) {
  return (
    <div className="peer">
      <div className="peer__avatar" style={{ position: "relative" }}>
        {initials(device.displayName)}
        <span
          className={`presence-dot ${device.online ? "presence-dot--online" : ""}`}
          title={device.online ? "online" : "offline"}
        />
      </div>
      <div className="peer__meta">
        <strong>{device.displayName}</strong>
        <span>
          {device.address || "no address"} · {device.platform}
        </span>
      </div>
      <div className="row" style={{ gap: 8 }}>
        <StatusPill variant={device.online ? "live" : "idle"}>
          {device.online ? "online" : timeAgo(device.lastSeen)}
        </StatusPill>
        {device.trusted ? (
          <StatusPill variant="ok">
            <ShieldCheck size={13} /> trusted
          </StatusPill>
        ) : (
          <NeonButton
            variant="ghost"
            icon={ShieldPlus}
            onClick={() => onTrust(device)}
            loading={busy}
          >
            Trust
          </NeonButton>
        )}
      </div>
    </div>
  );
}

export function DevicesScreen() {
  const [devices, setDevices] = useState<PeerDevice[]>([]);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  const scan = useCallback(async () => {
    setScanning(true);
    try {
      setDevices(await listDevices());
      setError(null);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setScanning(false);
    }
  }, []);

  useEffect(() => {
    void scan();
    const timer = window.setInterval(scan, SCAN_MS);
    return () => window.clearInterval(timer);
  }, [scan]);

  const onTrust = async (device: PeerDevice) => {
    setBusyId(device.id);
    setError(null);
    try {
      await trustDevice(
        device.id,
        device.displayName,
        device.address,
        device.platform,
      );
      await scan();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusyId(null);
    }
  };

  const online = devices.filter((device) => device.online).length;

  return (
    <div className="page" key="devices">
      <GlassPanel strong>
        <PanelHead
          icon={MonitorSmartphone}
          title="Nearby devices"
          action={
            <div className="row" style={{ gap: 10 }}>
              <StatusPill variant={online > 0 ? "live" : "idle"}>
                {online} online
              </StatusPill>
              <NeonButton
                variant="ghost"
                icon={RefreshCw}
                onClick={scan}
                loading={scanning}
              >
                Rescan
              </NeonButton>
            </div>
          }
        />
        {error ? <Banner variant="error">{error}</Banner> : null}
        {devices.length === 0 ? (
          <Empty icon={MonitorSmartphone}>
            {scanning
              ? "Scanning the local network…"
              : "No devices found yet. Start Receive on another device so it advertises."}
          </Empty>
        ) : (
          <div className="peer-list">
            {devices.map((device) => (
              <DeviceCard
                key={device.id}
                device={device}
                onTrust={onTrust}
                busy={busyId === device.id}
              />
            ))}
          </div>
        )}
      </GlassPanel>
    </div>
  );
}
