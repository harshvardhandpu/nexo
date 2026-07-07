import { useEffect, useState } from "react";
import {
  Bell,
  Cpu,
  Database,
  Eye,
  FolderDown,
  HardDrive,
  Monitor,
  Radio,
  ShieldCheck,
  UserCog,
} from "lucide-react";
import type { DesktopData } from "../lib/useDesktopData";
import {
  type AppPreferences,
  type BackgroundSettings,
  getBackgroundSettings,
  getPreferences,
  setBackgroundSettings,
  setPreferences,
} from "../api/desktop";
import { GlassPanel, PanelHead, Toggle } from "../components/ui";
import { formatBytes } from "../utils";

function Row({ k, v }: { k: string; v: string }) {
  return (
    <div className="kv">
      <span className="kv__k">{k}</span>
      <span className="kv__v">{v}</span>
    </div>
  );
}

/**
 * Feature 4: sectioned settings (General / Transfer / Privacy) backed by
 * persisted AppPreferences + BackgroundSettings. Background/start-on-login write
 * through to the OS autostart layer via the bridge.
 */
export function SettingsScreen({ data }: { data: DesktopData }) {
  const { settings, paths } = data;
  const [prefs, setPrefs] = useState<AppPreferences | null>(null);
  const [bg, setBg] = useState<BackgroundSettings | null>(null);

  useEffect(() => {
    void getPreferences().then(setPrefs).catch(() => {});
    void getBackgroundSettings().then(setBg).catch(() => {});
  }, []);

  const savePrefs = async (next: AppPreferences) => {
    setPrefs(next);
    try {
      await setPreferences(next);
    } catch {
      void getPreferences().then(setPrefs).catch(() => {});
    }
  };

  const saveBg = async (next: BackgroundSettings) => {
    setBg(next);
    try {
      setBg(
        await setBackgroundSettings(next.backgroundReceiving, next.startOnLogin),
      );
    } catch {
      void getBackgroundSettings().then(setBg).catch(() => {});
    }
  };

  return (
    <div className="page" key="settings">
      {/* --- General --- */}
      <GlassPanel strong>
        <PanelHead icon={UserCog} title="General" />
        <div className="stack">
          <label className="field">
            <span>Device name</span>
            <input
              className="input"
              placeholder="e.g. Harsh Laptop"
              value={prefs?.deviceName ?? ""}
              onChange={(event) =>
                prefs && setPrefs({ ...prefs, deviceName: event.target.value })
              }
              onBlur={() => prefs && void savePrefs(prefs)}
            />
          </label>
          <div className="divider" />
          <Toggle
            label="Keep Nexo available when closed"
            hint="Stay discoverable and accept transfers after the window is closed."
            checked={bg?.backgroundReceiving ?? true}
            onChange={(value) =>
              saveBg({
                backgroundReceiving: value,
                startOnLogin: bg?.startOnLogin ?? false,
              })
            }
          />
          <div className="divider" />
          <Toggle
            label="Start Nexo on login"
            hint="Register an OS autostart entry so Nexo launches when you sign in."
            checked={bg?.startOnLogin ?? false}
            onChange={(value) =>
              saveBg({
                backgroundReceiving: bg?.backgroundReceiving ?? true,
                startOnLogin: value,
              })
            }
          />
          <div className="divider" />
          <div className="row row--between">
            <span className="toggle-row__text">
              <strong>Theme</strong>
              <span className="text-faint">Midnight Flow (dark)</span>
            </span>
            <span className="pill">
              <Monitor size={13} /> Midnight
            </span>
          </div>
        </div>
      </GlassPanel>

      {/* --- Transfer --- */}
      <GlassPanel>
        <PanelHead icon={FolderDown} title="Transfer" />
        <div className="stack">
          <label className="field">
            <span>Default download folder</span>
            <input
              className="input"
              placeholder={settings?.receiveDir ?? "~/Downloads"}
              value={prefs?.downloadDir ?? ""}
              onChange={(event) =>
                prefs && setPrefs({ ...prefs, downloadDir: event.target.value })
              }
              onBlur={() => prefs && void savePrefs(prefs)}
            />
          </label>
          <div className="divider" />
          <Toggle
            label="Auto-accept from trusted devices"
            hint="Skip the approval prompt for devices you already trust."
            checked={prefs?.autoAcceptTrusted ?? false}
            onChange={(value) =>
              prefs && savePrefs({ ...prefs, autoAcceptTrusted: value })
            }
          />
          <div className="divider" />
          <Toggle
            label="Notifications"
            hint="Show a desktop notification for incoming transfers when hidden."
            checked={prefs?.notificationsEnabled ?? true}
            onChange={(value) =>
              prefs && savePrefs({ ...prefs, notificationsEnabled: value })
            }
          />
        </div>
      </GlassPanel>

      {/* --- Privacy --- */}
      <GlassPanel>
        <PanelHead icon={Eye} title="Privacy" />
        <div className="stack">
          <Toggle
            label="Discoverable"
            hint="Advertise this device to nearby peers over the local network."
            checked={prefs?.discoverable ?? true}
            onChange={(value) =>
              prefs && savePrefs({ ...prefs, discoverable: value })
            }
          />
          <div className="divider" />
          <div className="row row--between row--wrap">
            <span className="toggle-row__text">
              <strong>Trusted devices</strong>
              <span className="text-faint">
                Manage which devices you’ve trusted in the Trusted screen.
              </span>
            </span>
            <span className="pill">
              <ShieldCheck size={13} /> Certificate-pinned
            </span>
          </div>
        </div>
      </GlassPanel>

      {/* --- Storage / identity (reference) --- */}
      <div className="grid grid--2">
        <GlassPanel>
          <PanelHead icon={HardDrive} title="Storage" />
          <div>
            <Row k="Chunk size" v={settings ? formatBytes(settings.chunkSize) : "—"} />
            <Row k="State directory" v={settings?.stateDir ?? "—"} />
            <Row k="Receive directory" v={settings?.receiveDir ?? "—"} />
          </div>
        </GlassPanel>
        <GlassPanel>
          <PanelHead icon={Database} title="State files" />
          <div>
            <Row k="Database" v={paths?.database ?? "—"} />
            <Row k="Receiver advert" v={paths?.receiverPeer ?? "—"} />
            <Row k="Peer id" v={paths?.peerId ?? "—"} />
          </div>
        </GlassPanel>
      </div>

      <GlassPanel>
        <PanelHead icon={Cpu} title="About Nexo" />
        <p className="text-muted" style={{ margin: 0 }}>
          Nexo is a premium, local-first file transfer app: encrypted QUIC
          transport, SHA-256 chunk + whole-file verification, and crash-safe
          incremental resume. The desktop layer only calls the unchanged core.
        </p>
        <div className="row row--wrap" style={{ marginTop: 14, gap: 10 }}>
          <span className="pill">
            <Radio size={13} /> Local-first
          </span>
          <span className="pill">
            <ShieldCheck size={13} /> End-to-end encrypted
          </span>
          <span className="pill">
            <Bell size={13} /> Background receiver
          </span>
        </div>
      </GlassPanel>
    </div>
  );
}
