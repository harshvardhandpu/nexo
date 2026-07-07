import { useCallback, useEffect, useState } from "react";
import { Check, Fingerprint, Pencil, ShieldCheck, Trash2 } from "lucide-react";
import {
  type TrustedDevice,
  listTrustedDevices,
  renameTrustedDevice,
  untrustDevice,
} from "../api/desktop";
import {
  Empty,
  GlassPanel,
  NeonButton,
  PanelHead,
  StatusPill,
} from "../components/ui";
import { formatTimestamp, initials, timeAgo } from "../utils";

function TrustedCard({
  device,
  onChanged,
}: {
  device: TrustedDevice;
  onChanged: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(device.displayName);
  const [busy, setBusy] = useState(false);

  const save = async () => {
    setBusy(true);
    try {
      await renameTrustedDevice(device.id, name.trim() || device.displayName);
      setEditing(false);
      onChanged();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="glass trusted-card">
      <div className="row row--between">
        <div className="row" style={{ gap: 12 }}>
          <div className="peer__avatar">{initials(device.displayName)}</div>
          <div>
            {editing ? (
              <div className="row" style={{ gap: 8 }}>
                <input
                  className="input"
                  value={name}
                  autoFocus
                  onChange={(event) => setName(event.target.value)}
                  onKeyDown={(event) => event.key === "Enter" && void save()}
                  style={{ height: 34, padding: "6px 10px" }}
                />
                <button className="icon-btn" onClick={save} disabled={busy}>
                  <Check size={15} />
                </button>
              </div>
            ) : (
              <strong style={{ fontSize: 15 }}>{device.displayName}</strong>
            )}
            <div className="text-faint mono" style={{ fontSize: 12 }}>
              {device.address}
            </div>
          </div>
        </div>
        <StatusPill variant="ok">
          <ShieldCheck size={13} /> trusted
        </StatusPill>
      </div>

      <div className="kv">
        <span className="kv__k row" style={{ gap: 6 }}>
          <Fingerprint size={13} /> Fingerprint
        </span>
        <span className="kv__v mono">{device.fingerprint}</span>
      </div>
      <div className="kv">
        <span className="kv__k">Trusted since</span>
        <span className="kv__v">{formatTimestamp(device.firstTrusted)}</span>
      </div>
      <div className="kv">
        <span className="kv__k">Last seen</span>
        <span className="kv__v">{timeAgo(device.lastSeen)}</span>
      </div>

      <div className="row row--between" style={{ marginTop: 12 }}>
        <NeonButton
          variant="ghost"
          icon={Pencil}
          onClick={() => setEditing((value) => !value)}
        >
          Rename
        </NeonButton>
        <NeonButton
          variant="danger"
          icon={Trash2}
          onClick={async () => {
            await untrustDevice(device.id);
            onChanged();
          }}
        >
          Remove trust
        </NeonButton>
      </div>
    </div>
  );
}

export function TrustedScreen() {
  const [devices, setDevices] = useState<TrustedDevice[]>([]);

  const load = useCallback(async () => {
    try {
      setDevices(await listTrustedDevices());
    } catch {
      /* ignore */
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <div className="page" key="trusted">
      <GlassPanel strong>
        <PanelHead icon={ShieldCheck} title={`Trusted devices · ${devices.length}`} />
        {devices.length === 0 ? (
          <Empty icon={ShieldCheck}>
            No trusted devices yet. Trust a device from the Devices screen after a
            first transfer establishes its certificate.
          </Empty>
        ) : (
          <div className="grid grid--2">
            {devices.map((device) => (
              <TrustedCard key={device.id} device={device} onChanged={load} />
            ))}
          </div>
        )}
      </GlassPanel>
    </div>
  );
}
