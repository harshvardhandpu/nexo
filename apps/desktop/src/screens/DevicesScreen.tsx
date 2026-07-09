import { useCallback, useEffect, useState } from "react";
import {
  MonitorSmartphone,
  RefreshCw,
  ShieldCheck,
  ShieldPlus,
} from "lucide-react";
import {
  type PairingInfo,
  type PeerDevice,
  cancelPairing,
  confirmPairing,
  listDevices,
  startPairing,
} from "../api/desktop";
import { PairingDialog } from "../components/PairingDialog";
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
            {busy ? "Pairing…" : "Trust"}
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
  // The device whose fingerprint is awaiting confirmation, if any.
  const [pairing, setPairing] = useState<PairingInfo | null>(null);
  const [confirming, setConfirming] = useState(false);

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

  // Step 1: pairing — connect to the discovered device and fetch its advertised
  // certificate fingerprint. This does NOT trust the device; it opens the
  // confirmation modal so the user can verify the fingerprint first.
  const onTrust = async (device: PeerDevice) => {
    setBusyId(device.id);
    setError(null);
    try {
      const info = await startPairing(device.id, device.address);
      setPairing(info);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusyId(null);
    }
  };

  // Step 2: the user verified the fingerprint and approved — store the trust.
  const onConfirm = async () => {
    if (!pairing) return;
    setConfirming(true);
    setError(null);
    try {
      await confirmPairing(pairing.peerId, pairing.displayName);
      setPairing(null);
      await scan();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setConfirming(false);
    }
  };

  // Step 2 (rejected): drop the pending pairing on the backend; store nothing.
  const onReject = async () => {
    if (!pairing) return;
    const peerId = pairing.peerId;
    setPairing(null);
    try {
      await cancelPairing(peerId);
    } catch {
      // Cancellation is best-effort; the pending pairing expires with the app.
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

      {pairing ? (
        <PairingDialog
          pairing={pairing}
          busy={confirming}
          onConfirm={onConfirm}
          onReject={onReject}
        />
      ) : null}
    </div>
  );
}
